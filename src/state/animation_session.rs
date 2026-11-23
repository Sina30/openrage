use super::*;

impl AppState {
    /// Pick a skeleton for the selected clip, unless the user has picked one.
    ///
    /// ⚠ The candidates are the skeletons **open documents** carry, so an
    /// asset whose skeleton is not open reports that rather than being
    /// guessed at.
    pub fn resolve_skeleton_automatically(&mut self) {
        if self.preview_scene.skeleton_pinned || self.preview_scene.skeleton_source.is_some() {
            return;
        }
        let Some(clip) = self.preview_scene.clip else {
            return;
        };
        let Some(crate::state::DocPayload::Resource(or_io::Asset::Clips(dictionary))) =
            self.document(clip.document).map(|d| &d.payload)
        else {
            return;
        };
        let tags = or_anim::clip_tags(dictionary, clip.clip_key);
        if tags.is_empty() {
            return;
        }

        let candidates: Vec<(
            crate::state::preview_scene::SkeletonSource,
            &or_io::gta5::CrSkeletonData,
        )> = self
            .documents
            .iter()
            .flat_map(|doc| {
                crate::state::preview_scene::skeleton_sources_in(doc.id, &doc.payload)
                    .into_iter()
                    .filter_map(|source| {
                        crate::state::preview_scene::skeleton_data_from(&doc.payload, source)
                            .map(|data| (source, data))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let suggestion = crate::state::preview_scene::suggest_skeleton(&tags, &candidates);
        if self.preview_scene.adopt_suggested_skeleton(suggestion) {
            self.anim.invalidate();
        }
    }

    pub fn tick_animation(&mut self) {
        let open: Vec<DocumentId> = self.documents.iter().map(|d| d.id).collect();
        let stale: Vec<DocumentId> = (self.preview_scene.documents())
            .filter(|id| !open.contains(id))
            .collect();
        for id in stale {
            if self.preview_scene.forget_document(id) {
                self.anim.invalidate();
            }
        }

        // ⚠ Before the rebuild, not after. A session built with no skeleton
        // reports a binding failure, and adopting one afterwards would show
        // that failure for a frame on an asset that was never wrong.
        self.resolve_skeleton_automatically();

        if self
            .anim
            .needs_rebuild(&self.preview_scene, self.skeleton_epoch())
        {
            self.rebuild_anim_session();
        }

        let dt = self.sim.step_real_dt();
        let _ = self.anim_transport.take_seek();
        self.anim_transport.tick(dt);
        let time = self.anim_transport.time;
        let apply_root_motion = self.preview_scene.apply_root_motion;
        let edits = std::mem::take(&mut self.anim_edits);
        self.anim.evaluate(time, apply_root_motion, &edits);
        self.anim_edits = edits;
    }

    /// Record the pose layer as it stood `before` an edit, as a step of the clip's document.
    pub fn push_anim_edit(&mut self, label: impl Into<String>, before: or_anim::PoseEdits) {
        let Some(document) = self
            .preview_scene
            .clip
            .map(|c| c.document)
            .or(self.active_document)
        else {
            return;
        };
        if before == self.anim_edits {
            return;
        }
        self.record(document, label, Change::Pose(before));
    }

    pub fn apply_clip_fields(
        &mut self,
        document: DocumentId,
        clip_key: u32,
        snapshot: &ClipFieldSnapshot,
    ) -> bool {
        let Some(doc) = self.document_mut(document) else {
            return false;
        };
        if let DocPayload::Gta6Anim(_) = doc.payload {
            return self.apply_gta6_clip_fields(document, clip_key, snapshot);
        }
        let DocPayload::Resource(or_io::Asset::Clips(dictionary)) = &mut doc.payload else {
            return false;
        };
        let Some(clip) = or_anim::clip::clip_mut(dictionary, clip_key) else {
            return false;
        };
        or_anim::clip::header_mut(clip).name.data = snapshot.name.clone();
        or_anim::clip::set_looped(clip, snapshot.looped);
        if let or_io::gta5::CrClipAny::CrClipAnimation(entry) = clip {
            entry.start_time = snapshot.start_time;
            entry.end_time = snapshot.end_time;
            entry.rate = snapshot.rate;
        }
        true
    }

    pub fn clip_field_snapshot(
        &self,
        document: DocumentId,
        clip_key: u32,
    ) -> Option<ClipFieldSnapshot> {
        let doc = self.document(document)?;
        if let DocPayload::Gta6Anim(_) = doc.payload {
            return self.gta6_clip_field_snapshot(document, clip_key);
        }
        let DocPayload::Resource(or_io::Asset::Clips(dictionary)) = &doc.payload else {
            return None;
        };
        let clip = or_anim::clip::clip(dictionary, clip_key)?;
        let (start_time, end_time, rate) = match clip {
            or_io::gta5::CrClipAny::CrClipAnimation(entry) => {
                (entry.start_time, entry.end_time, entry.rate)
            }
            or_io::gta5::CrClipAny::CrClipAnimations(list) => (0.0, list.total_duration, 1.0),
            or_io::gta5::CrClipAny::Base(_) => (0.0, 0.0, 1.0),
        };
        Some(ClipFieldSnapshot {
            name: or_anim::clip::header(clip).name.data.clone(),
            start_time,
            end_time,
            rate,
            looped: or_anim::clip::is_looped(clip),
        })
    }

    pub fn bake_anim_edits(&mut self) -> Result<Option<String>, String> {
        if self.anim_edits.is_empty() {
            return Ok(None);
        }
        let Some(selection) = self.preview_scene.clip else {
            return Ok(None);
        };
        let Some(binding) = self.anim.binding.as_ref() else {
            return Err("no skeleton is bound, so edits cannot be baked".to_string());
        };
        let skeleton = binding.skeleton.clone();
        let edits = std::mem::take(&mut self.anim_edits);
        let edit_count = edits.len();

        // ⚠ Before the bake changes the clips: `mark_edited` after it would read them over.
        self.catch_up(selection.document);
        let baked = Self::bake_into_document(
            self.documents
                .iter_mut()
                .find(|d| d.id == selection.document),
            selection.clip_key,
            &skeleton,
            &edits,
        );

        let baked = match baked {
            Ok(baked) => baked,
            Err(reason) => {
                self.anim_edits = edits;
                return Err(reason);
            }
        };

        let mut summary = format!(
            "baked {edit_count} edited value{} into {} DOF{}",
            if edit_count == 1 { "" } else { "s" },
            baked.edited_dofs,
            if baked.edited_dofs == 1 { "" } else { "s" }
        );
        if baked.is_lossy() {
            summary.push_str(&format!(
                "; {} channel(s) could not be decoded and were replaced with the bind pose",
                baked.undecodable_dofs
            ));
        }

        // ⚠ The layer the pose steps swap is now in the animation, so undoing one would apply it twice.
        if let Some(doc) = self.document_mut(selection.document) {
            doc.mark_edited();
        }
        self.anim.invalidate();
        Ok(Some(summary))
    }

    fn bake_into_document(
        doc: Option<&mut Document>,
        clip_key: u32,
        skeleton: &or_anim::Skeleton,
        edits: &or_anim::PoseEdits,
    ) -> Result<or_anim::BakedAnimation, String> {
        let Some(doc) = doc else {
            return Err("the clip's document is no longer open".to_string());
        };
        let DocPayload::Resource(or_io::Asset::Clips(dictionary)) = &mut doc.payload else {
            return Err("the clip's document is not a clip dictionary".to_string());
        };
        let Some(clip) = or_anim::clip::clip(dictionary, clip_key) else {
            return Err("the clip is no longer in the dictionary".to_string());
        };
        let key = match clip {
            or_io::gta5::CrClipAny::CrClipAnimation(entry) => entry.animation.key,
            or_io::gta5::CrClipAny::CrClipAnimations(_) => {
                return Err(
                    "baking a multi-animation clip is not supported; edit its animations \
                     individually"
                        .to_string(),
                )
            }
            or_io::gta5::CrClipAny::Base(_) => None,
        };
        // ⚠ The animation is the dictionary's, so every clip playing it plays the edit.
        let Some(animation) = key.and_then(|key| or_anim::clip::animation_mut(dictionary, key))
        else {
            return Err("this clip references no animation".to_string());
        };

        let clip_binding = or_anim::ClipBinding::build(skeleton, animation);
        or_anim::bake_into(
            animation,
            skeleton,
            &clip_binding,
            edits,
            or_io::codec::anim::EncodePolicy::Lossless,
        )
        .map_err(|e| format!("re-encode failed: {e}"))
    }

    /// How often the skeleton source's document has changed, which the bones a session binds are read from.
    pub(super) fn skeleton_epoch(&self) -> Option<u64> {
        let source = self.preview_scene.skeleton_source?;
        Some(self.document(source.document())?.payload_epoch())
    }

    pub fn rebuild_anim_session(&mut self) {
        let scene = self.preview_scene.clone();
        let epoch = self.skeleton_epoch();
        // ⚠ Read before the install, which sets it: only another clip or skeleton source is a new choice.
        // Whatever else asks for a rebuild (a key, an edit, a bake, a toggle) leaves the user's working range and Loop.
        let same_choice = self.anim.holds_the_transport_for(&scene);
        let Some((clip, skeleton_source)) = scene.clip.zip(scene.skeleton_source) else {
            self.anim.clear(&scene, epoch);
            return;
        };

        let bound = {
            let clip_payload = self.document(clip.document).map(|d| &d.payload);
            let skeleton_payload = self
                .document(skeleton_source.document())
                .map(|d| &d.payload);
            match (clip_payload, skeleton_payload) {
                (Some(DocPayload::Resource(or_io::Asset::Clips(dictionary))), Some(payload)) => {
                    preview_scene::skeleton_data_from(payload, skeleton_source)
                        .and_then(|data| or_anim::bind(dictionary, clip.clip_key, data))
                }
                _ => None,
            }
        };

        match bound {
            Some((player, binding)) => {
                // Only play a clip whose tags actually fit, unless the user asked
                // for the mismatch. Silently posing a figure with 3 of 84 bones
                // driven looks like a decoder bug; refusing and explaining does
                // not. The escape hatch preserves CodeWalker's deliberate
                // "any dictionary on any ped" behaviour.
                if !binding.looks_compatible() && !scene.allow_mismatched_skeleton {
                    tracing::info!(
                        clip = %binding.clip_name,
                        resolved = binding.report.resolved,
                        tags = binding.report.clip_tags(),
                        "clip does not match the selected skeleton; not playing"
                    );
                    self.anim.clear(&scene, epoch);
                    self.anim.binding = Some(binding);
                    return;
                }
                if same_choice {
                    let looped_changed = self.anim.looped_changed(&player);
                    follow_the_clip(looped_changed, &player, &mut self.anim_transport);
                } else {
                    player.configure_transport(&mut self.anim_transport);
                }
                self.anim.install(&scene, epoch, player, binding);
            }
            None => self.anim.clear(&scene, epoch),
        }
    }
}

/// The clip's length and frame rate follow the clip; the working range and Loop stay as the user left them,
/// unless the clip's own Looped flag changed since the transport took it (`looped_changed`), which is the newer word.
///
/// ⚠ `set_duration` hands the range back, so a range that was never trimmed is left to follow the new length.
fn follow_the_clip(
    looped_changed: bool,
    player: &or_anim::ClipPlayer,
    transport: &mut or_transport::Transport,
) {
    let (range, looping) = (transport.range, transport.is_looping());
    let trimmed = range != transport.bounds;
    player.configure_transport(transport);
    if trimmed {
        transport.set_range(range);
    }
    if !looped_changed {
        transport.set_looping(looping);
    }
}

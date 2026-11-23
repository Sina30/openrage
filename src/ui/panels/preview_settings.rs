use std::collections::BTreeSet;

use crate::state::preview_scene::{self, MeshCandidate, SkeletonSource, SkeletonSuggestion};
use crate::state::{AppState, DocPayload, MeshSource};
use crate::ui::panels::drag_value;
use crate::ui::theme;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    // A GTA VI animation picks its model in its timeline's bar; only how the preview draws is set here.
    if matches!(
        state.active_document().map(|d| &d.payload),
        Some(DocPayload::Gta6Anim(_))
    ) {
        section_header(ui, "DISPLAY");
        draw_bones_display(ui, &mut state.preview_scene);
        return;
    }
    if !matches!(
        state.active_document().map(|d| &d.payload),
        Some(DocPayload::Resource(or_io::Asset::Clips(_)))
    ) {
        theme::empty_message(ui, "Open a clip dictionary (.ycd) to configure a preview");
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_clip_section(ui, state);
            ui.separator();
            draw_clip_fields(ui, state);
            ui.separator();
            draw_skeleton_section(ui, state);
            ui.separator();
            draw_binding_report(ui, state);
            ui.separator();
            draw_edit_section(ui, state);
            ui.separator();
            draw_mesh_section(ui, state);
            ui.separator();
            draw_display_section(ui, state);
        });
}

fn draw_clip_fields(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "CLIP FIELDS");
    let Some(selection) = state.preview_scene.clip else {
        ui.label(
            egui::RichText::new("No clip selected.")
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    };
    let Some(before) = state.clip_field_snapshot(selection.document, selection.clip_key) else {
        ui.label(
            egui::RichText::new("The selected clip is no longer in the dictionary.")
                .small()
                .color(theme::WARNING),
        );
        return;
    };

    let mut edited = before.clone();
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Name");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut edited.name)
                    .desired_width(ui.available_width() - 8.0),
            )
            .on_hover_text(
                "The clip's own name. Its atMap key is the hash of the *original* name and \
                 does not change, so renaming does not move the clip.",
            )
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label("Window");
        let start = drag_value(ui, &mut edited.start_time, |d| {
            d.speed(0.01).range(0.0..=f32::MAX).suffix("s")
        });
        start
            .response
            .on_hover_text("m_StartTime — where in the animation this clip begins");
        changed |= start.inner;
        ui.label("→");
        let end = drag_value(ui, &mut edited.end_time, |d| {
            d.speed(0.01).range(0.0..=f32::MAX).suffix("s")
        });
        end.response.on_hover_text("m_EndTime — where it ends");
        changed |= end.inner;
    });

    ui.horizontal(|ui| {
        ui.label("Rate");
        let rate = drag_value(ui, &mut edited.rate, |d| {
            d.speed(0.01).range(0.01..=16.0).suffix("×")
        });
        rate.response
            .on_hover_text("m_Rate — playback multiplier over the window");
        changed |= rate.inner;
        changed |= ui
            .checkbox(&mut edited.looped, "Loop")
            .on_hover_text("The clip's IS_LOOPED flag")
            .changed();
    });

    if edited.end_time < edited.start_time {
        edited.end_time = edited.start_time;
    }

    if changed
        && edited != before
        && state.apply_clip_fields(selection.document, selection.clip_key, &edited)
    {
        let change = crate::state::Change::ClipFields {
            clip_key: selection.clip_key,
            other: before,
        };
        state.record(selection.document, "Edit clip fields", change);
        state.anim.invalidate();
    }
}

fn draw_edit_section(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "EDITS");
    let count = state.anim_edits.len();
    if count == 0 {
        ui.label(
            egui::RichText::new(
                "No pose edits. Edits are additive over the source clip and are baked into \
                 channel data only when you save.",
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }

    let bones = state.anim_edits.edited_bones();
    ui.label(
        egui::RichText::new(format!(
            "{count} edited value{} across {} bone{}",
            if count == 1 { "" } else { "s" },
            bones.len(),
            if bones.len() == 1 { "" } else { "s" }
        ))
        .small()
        .color(theme::ACCENT_TEXT),
    );

    if let Some(skeleton) = state.anim.skeleton() {
        let names: Vec<String> = bones
            .iter()
            .take(10)
            .map(|bone| {
                skeleton
                    .bone(*bone)
                    .map(|b| b.name.clone())
                    .unwrap_or_else(|| format!("bone {bone}"))
            })
            .collect();
        ui.label(
            egui::RichText::new(names.join(", "))
                .small()
                .monospace()
                .color(theme::TEXT_SECONDARY),
        );
    }

    ui.horizontal(|ui| {
        if ui
            .button("Discard edits")
            .on_hover_text("Revert to the source clip. Undoable.")
            .clicked()
        {
            let before = state.anim_edits.clone();
            state.anim_edits.clear();
            state.push_anim_edit("discard edits", before);
            state.anim.invalidate();
        }
    });
}

fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(text)
            .strong()
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

fn draw_clip_section(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "CLIP");
    match state.anim.binding.as_ref() {
        Some(binding) => {
            ui.label(egui::RichText::new(&binding.clip_name).monospace());
            ui.label(
                egui::RichText::new(format!(
                    "{:.3}s{}",
                    binding.duration,
                    match binding.frame_rate {
                        Some(fps) => format!(" · {fps:.1} fps"),
                        None => String::new(),
                    }
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
        }
        None => {
            ui.label(
                egui::RichText::new("No clip selected")
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    }
}

fn draw_skeleton_section(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "SKELETON");

    let candidates: Vec<(SkeletonSource, String, usize)> = state
        .documents
        .iter()
        .flat_map(|doc| {
            preview_scene::skeleton_sources_in(doc.id, &doc.payload)
                .into_iter()
                .map(|source| {
                    let bones = preview_scene::skeleton_data_from(&doc.payload, source)
                        .map(|s| s.bones.len())
                        .unwrap_or(0);
                    let label = match source {
                        SkeletonSource::DictionaryEntry(_, index) => {
                            format!("{} [{index}]", doc.name)
                        }
                        _ => doc.name.clone(),
                    };
                    (source, label, bones)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    if candidates.is_empty() {
        ui.label(
            egui::RichText::new(
                "No skeleton available. Open a .yft, .ydr or .ydd that carries one.",
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }

    let current = state.preview_scene.skeleton_source;
    let current_label = current
        .and_then(|source| {
            candidates
                .iter()
                .find(|(candidate, _, _)| *candidate == source)
                .map(|(_, label, bones)| format!("{label} ({bones} bones)"))
        })
        .unwrap_or_else(|| "(none)".to_string());

    let mut chosen = current;
    egui::ComboBox::from_id_salt("anim_skeleton_source")
        .selected_text(current_label)
        .width(ui.available_width() - 8.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, "(none)");
            for (source, label, bones) in &candidates {
                ui.selectable_value(
                    &mut chosen,
                    Some(*source),
                    format!("{label} · {bones} bones · {}", source.kind_label()),
                );
            }
        });
    if chosen != current {
        // Choosing from the list is a decision, including choosing none, so
        // the automatic resolver stops overriding it.
        state.preview_scene.pin_skeleton(chosen);
        state.anim.invalidate();
    }

    if state.preview_scene.skeleton_pinned {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Chosen by you")
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            if ui
                .small_button("Choose automatically")
                .on_hover_text(
                    "Rank the open skeletons against this clip's bone tags and\n\
                     take the best, the way an unpicked preview does.",
                )
                .clicked()
            {
                state.preview_scene.unpin_skeleton();
                state.anim.invalidate();
            }
        });
    } else if let Some(suggestion) = suggest(state, &candidates) {
        let label = candidates
            .iter()
            .find(|(source, _, _)| *source == suggestion.source)
            .map(|(_, label, _)| label.clone())
            .unwrap_or_default();
        ui.label(
            egui::RichText::new(format!(
                "Chosen automatically: {label} ({:.0}% of tags)",
                suggestion.coverage * 100.0
            ))
            .small()
            .color(theme::ACCENT_TEXT),
        );
    }

    let mut allow = state.preview_scene.allow_mismatched_skeleton;
    if ui
        .checkbox(&mut allow, "Allow mismatched skeleton")
        .on_hover_text(
            "Play a clip whose bone tags barely match this skeleton.\n\
             Off, a mismatch is reported instead of rendered — which is usually\n\
             what you want. On, you get CodeWalker's \"any dictionary on any ped\"\n\
             behaviour, peculiar results included.",
        )
        .changed()
    {
        state.preview_scene.allow_mismatched_skeleton = allow;
        state.anim.invalidate();
    }

    let mut model = state.preview_scene.draw_skeleton_model;
    if ui
        .checkbox(&mut model, "Draw the skeleton file's own model")
        .on_hover_text(
            "Draw the skeleton file's own geometry while no mesh is added, or the meshes added draw nothing.\n\
             A ped's .yft holds only a 25 metre plane, which this hides.\n\
             The skeleton and every mesh added are drawn either way.",
        )
        .changed()
    {
        state.set_draw_skeleton_model(model);
    }
}

fn suggest(
    state: &AppState,
    candidates: &[(SkeletonSource, String, usize)],
) -> Option<SkeletonSuggestion> {
    let clip = state.preview_scene.clip?;
    let dictionary = match state.document(clip.document).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::Clips(dictionary))) => dictionary,
        _ => return None,
    };
    let tags: BTreeSet<u16> = or_anim::clip_tags(dictionary, clip.clip_key);
    if tags.is_empty() {
        return None;
    }

    let resolved: Vec<(SkeletonSource, &or_io::gta5::CrSkeletonData)> = candidates
        .iter()
        .filter_map(|(source, _, _)| {
            let doc = state.document(source.document())?;
            let data = preview_scene::skeleton_data_from(&doc.payload, *source)?;
            Some((*source, data))
        })
        .collect();
    preview_scene::suggest_skeleton(&tags, &resolved)
}

fn draw_binding_report(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "BINDING");
    let Some(binding) = state.anim.binding.as_ref() else {
        ui.label(
            egui::RichText::new("Nothing bound yet.")
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    };

    let report = &binding.report;
    let compatible = report.looks_compatible();
    let colour = if compatible {
        theme::SUCCESS
    } else {
        theme::WARNING
    };
    ui.label(egui::RichText::new(report.summary()).small().color(colour));

    let ratio = report.match_ratio();
    let (bar, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width() - 8.0, 6.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(bar, 2.0, theme::BG_L2);
    if ratio > 0.0 {
        let filled = egui::Rect::from_min_size(
            bar.min,
            egui::vec2(bar.width() * ratio.clamp(0.0, 1.0), bar.height()),
        );
        ui.painter().rect_filled(filled, 2.0, colour);
    }

    if !report.unresolved_tags.is_empty() {
        egui::CollapsingHeader::new(format!(
            "{} unresolved bone tags",
            report.unresolved_tags.len()
        ))
        .id_salt("anim_unresolved_tags")
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "The clip drives these bone tags and the skeleton has no bone with them. \
                     A long list means the clip was authored for a different rig.",
                )
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            let shown: Vec<String> = report
                .unresolved_tags
                .iter()
                .take(48)
                .map(|tag| format!("0x{tag:04X}"))
                .collect();
            ui.label(egui::RichText::new(shown.join("  ")).monospace().small());
            if report.unresolved_tags.len() > shown.len() {
                ui.label(
                    egui::RichText::new(format!(
                        "… and {} more",
                        report.unresolved_tags.len() - shown.len()
                    ))
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
            }
        });
    }

    if !report.untouched_bones.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} skeleton bones hold their bind pose",
                report.untouched_bones.len()
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        )
        .on_hover_text(
            "Normal for a partial clip — a facial or hand-only animation drives \
             only part of the rig.",
        );
    }

    if report.compact {
        ui.label(
            egui::RichText::new(
                "Compact (compressed) animation blocks: readable and writable, but not \
                 decodable without crAnimCache expansion. Playback shows the bind pose.",
            )
            .small()
            .color(theme::WARNING),
        );
    }

    let errors = state.anim.last_errors();
    if !errors.is_empty() {
        egui::CollapsingHeader::new(format!("{} decode problems", errors.len()))
            .id_salt("anim_decode_errors")
            .show(ui, |ui| {
                for error in errors.iter().take(8) {
                    ui.label(egui::RichText::new(error).small().color(theme::ERROR));
                }
            });
    }
}

fn draw_mesh_section(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "MESHES");
    ui.label(
        egui::RichText::new(
            "Drawables to skin. A dressed ped needs several — head, upper, lower, feet.",
        )
        .small()
        .color(theme::TEXT_SECONDARY),
    );

    let mut remove: Option<MeshSource> = None;
    for source in state.preview_scene.mesh_sources.clone() {
        ui.horizontal(|ui| {
            let name = state
                .document(source.document)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("(closed #{})", source.document));
            let label = match source.entry {
                Some(index) => format!("{name} [{index}]"),
                None => name,
            };
            ui.label(egui::RichText::new(label).monospace().small());
            if ui
                .small_button(egui_phosphor::regular::X)
                .on_hover_text("Remove")
                .clicked()
            {
                remove = Some(source);
            }
        });
    }
    if let Some(source) = remove {
        state.preview_scene.remove_mesh(source);
        state.anim.invalidate();
    }

    let candidates: Vec<MeshCandidate> = state
        .documents
        .iter()
        .flat_map(|doc| preview_scene::mesh_sources_in(doc.id, &doc.payload, &doc.name))
        .filter(|candidate| !state.preview_scene.mesh_sources.contains(&candidate.source))
        .collect();

    if !candidates.is_empty() {
        let mut to_add: Option<MeshSource> = None;
        egui::ComboBox::from_id_salt("anim_add_mesh")
            .selected_text("Add mesh…")
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for candidate in &candidates {
                    if ui.selectable_label(false, &candidate.label).clicked() {
                        to_add = Some(candidate.source);
                    }
                }
            });
        if let Some(source) = to_add {
            state.preview_scene.add_mesh(source);
            state.anim.invalidate();
        }
    }

    if state.preview_scene.mesh_sources.is_empty() {
        // The box that hides the skeleton file's model is drawn only where a skeleton is offered
        let a_skeleton_is_offered = (state.documents.iter())
            .any(|doc| !preview_scene::skeleton_sources_in(doc.id, &doc.payload).is_empty());
        let shown = if !a_skeleton_is_offered {
            "None added."
        } else if state.preview_scene.draw_skeleton_model {
            "None added — the skeleton's own drawable is shown instead."
        } else {
            "None added — the skeleton file's own model is hidden."
        };
        ui.label(
            egui::RichText::new(shown)
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
}

fn draw_display_section(ui: &mut egui::Ui, state: &mut AppState) {
    section_header(ui, "DISPLAY");
    draw_bones_display(ui, &mut state.preview_scene);
    let scene = &mut state.preview_scene;

    if ui
        .checkbox(&mut scene.apply_root_motion, "Apply root motion")
        .on_hover_text(
            "Let the clip's mover tracks translate the figure.\n\
             Off, a walk cycle plays on the spot — usually what you want for \
             inspecting it.",
        )
        .changed()
    {
        state.anim.invalidate();
    }
}

fn draw_bones_display(ui: &mut egui::Ui, scene: &mut preview_scene::PreviewScene) {
    ui.checkbox(&mut scene.show_bones, "Show bones")
        .on_hover_text(
            "Draw the skeleton as lines over the mesh: at rest until a clip poses it,\n\
             and a click on a line selects its bone",
        );

    ui.add_enabled_ui(scene.show_bones, |ui| {
        ui.checkbox(&mut scene.show_bind_pose_ghost, "Bind-pose ghost")
            .on_hover_text(
                "Draw the bind pose alongside the animated one, in grey.\n\
                 The fastest way to see whether a clip is doing anything, and what.",
            );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_ycd() -> AppState {
        let mut state = AppState::new();
        state.documents.clear();
        let doc = crate::state::Document::new(
            1,
            Some(std::path::PathBuf::from("anim.ycd")),
            DocPayload::Resource(or_io::Asset::Clips(Default::default())),
        );
        state.documents.push(doc);
        state.active_document = Some(1);
        state
    }

    #[test]
    fn no_clip_means_no_suggestion() {
        let state = state_with_ycd();
        assert!(suggest(&state, &[]).is_none());
    }

    #[test]
    fn a_gta_vi_animation_turns_its_bones_and_bind_pose_ghost_on_here() {
        use or_io::gta6::anim::{AnimResource, RootKind};
        let mut state = state_with_ycd();
        state.documents[0].payload = DocPayload::Gta6Anim(std::sync::Arc::new(AnimResource {
            root: RootKind::ClipDictionary,
            root_word: 2,
            name: "dict_a".into(),
            key: 0,
            block_map: None,
            word_at_60: 0,
            word_at_98: None,
            sets: Vec::new(),
            external_sets: Vec::new(),
            animation_keys: Vec::new(),
            clips: Vec::new(),
            motions: Vec::new(),
            matchers: Vec::new(),
            dictionaries: Vec::new(),
            end_region: None,
            sections: None,
            edited: false,
        }));
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui, &mut state));
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                texts.push(text.galley.text().to_string());
            }
        }
        assert!(texts.iter().any(|t| t == "Bind-pose ghost"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains(".ycd")), "{texts:?}");
    }
}

use crate::skeleton::Skeleton;
use or_io::codec::anim::track;
use or_io::gta5::{CrAnimation, CrClipAny, CrClipDictionary};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoneBinding {
    pub bone: u16,
    pub tag: u16,
    pub translation: Option<u32>,
    pub rotation: Option<u32>,
    pub scale: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingReport {
    pub resolved: usize,
    pub unresolved_tags: Vec<u16>,
    pub untouched_bones: Vec<u16>,
    pub skeleton_bones: usize,
    pub compact: bool,
    pub has_root_motion: bool,
}

impl BindingReport {
    pub fn clip_tags(&self) -> usize {
        self.resolved + self.unresolved_tags.len()
    }

    pub fn match_ratio(&self) -> f32 {
        let total = self.clip_tags();
        if total == 0 {
            0.0
        } else {
            self.resolved as f32 / total as f32
        }
    }

    pub fn looks_compatible(&self) -> bool {
        self.resolved > 0 && self.match_ratio() >= 0.5
    }

    pub fn summary(&self) -> String {
        let total = self.clip_tags();
        if total == 0 {
            return "clip drives no bones".to_string();
        }
        let mut text = format!(
            "{}/{} bone tags resolved ({} of {} skeleton bones driven)",
            self.resolved,
            total,
            self.skeleton_bones - self.untouched_bones.len(),
            self.skeleton_bones
        );
        if self.compact {
            text.push_str(" · compact blocks cannot be decoded");
        }
        if self.has_root_motion {
            text.push_str(" · has root motion");
        }
        text
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClipBinding {
    pub bones: Vec<BoneBinding>,
    pub mover_translation: Option<u32>,
    pub mover_rotation: Option<u32>,
    pub report: BindingReport,
}

impl ClipBinding {
    pub fn build(skeleton: &Skeleton, animation: &CrAnimation) -> Self {
        let mut per_tag: std::collections::HashMap<u16, [Option<u32>; 3]> =
            std::collections::HashMap::new();
        let mut mover_translation = None;
        let mut mover_rotation = None;

        for (index, dof) in animation.dofs.iter().enumerate() {
            let index = index as u32;
            let slot = match dof.track {
                track::BONE_TRANSLATION => 0,
                track::BONE_ROTATION => 1,
                track::BONE_SCALE => 2,
                track::MOVER_TRANSLATION => {
                    mover_translation = mover_translation.or(Some(index));
                    continue;
                }
                track::MOVER_ROTATION => {
                    mover_rotation = mover_rotation.or(Some(index));
                    continue;
                }
                _ => continue,
            };
            let entry = per_tag.entry(dof.id).or_insert([None; 3]);
            if entry[slot].is_none() {
                entry[slot] = Some(index);
            }
        }

        let mut bones = Vec::new();
        let mut unresolved: BTreeSet<u16> = BTreeSet::new();
        let mut touched: BTreeSet<u16> = BTreeSet::new();

        for (&tag, tracks) in per_tag.iter() {
            match skeleton.index_for_tag(tag) {
                Some(bone) => {
                    touched.insert(bone);
                    bones.push(BoneBinding {
                        bone,
                        tag,
                        translation: tracks[0],
                        rotation: tracks[1],
                        scale: tracks[2],
                    });
                }
                None => {
                    unresolved.insert(tag);
                }
            }
        }

        bones.sort_unstable_by_key(|b| b.bone);

        let untouched_bones = (0..skeleton.bone_count() as u16)
            .filter(|b| !touched.contains(b))
            .collect();

        let report = BindingReport {
            resolved: bones.len(),
            unresolved_tags: unresolved.into_iter().collect(),
            untouched_bones,
            skeleton_bones: skeleton.bone_count(),
            compact: crate::clip::is_compact(animation),
            has_root_motion: mover_translation.is_some() || mover_rotation.is_some(),
        };

        Self {
            bones,
            mover_translation,
            mover_rotation,
            report,
        }
    }

    pub fn for_clip(
        skeleton: &Skeleton,
        dictionary: &CrClipDictionary,
        clip: &CrClipAny,
    ) -> Vec<Self> {
        crate::clip::played(dictionary, clip)
            .into_iter()
            .map(|played| Self::build(skeleton, played.animation))
            .collect()
    }

    pub fn merged_report(bindings: &[Self]) -> BindingReport {
        if bindings.is_empty() {
            return BindingReport::default();
        }
        if bindings.len() == 1 {
            return bindings[0].report.clone();
        }

        let skeleton_bones = bindings[0].report.skeleton_bones;
        let mut resolved_tags: BTreeSet<u16> = BTreeSet::new();
        let mut unresolved: BTreeSet<u16> = BTreeSet::new();
        let mut touched: BTreeSet<u16> = BTreeSet::new();
        let mut compact = false;
        let mut has_root_motion = false;

        for binding in bindings {
            for bone in &binding.bones {
                resolved_tags.insert(bone.tag);
                touched.insert(bone.bone);
            }
            unresolved.extend(binding.report.unresolved_tags.iter().copied());
            compact |= binding.report.compact;
            has_root_motion |= binding.report.has_root_motion;
        }
        for tag in &resolved_tags {
            unresolved.remove(tag);
        }

        BindingReport {
            resolved: resolved_tags.len(),
            unresolved_tags: unresolved.into_iter().collect(),
            untouched_bones: (0..skeleton_bones as u16)
                .filter(|b| !touched.contains(b))
                .collect(),
            skeleton_bones,
            compact,
            has_root_motion,
        }
    }

    pub fn animated_bones(&self) -> impl Iterator<Item = u16> + '_ {
        self.bones.iter().map(|b| b.bone)
    }

    pub fn binding_for_bone(&self, bone: u16) -> Option<&BoneBinding> {
        self.bones
            .binary_search_by_key(&bone, |b| b.bone)
            .ok()
            .map(|i| &self.bones[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dof, v4};
    use or_io::codec::anim::format_type;
    use or_io::gta5::{CrBoneData, CrSkeletonData, Dof};
    fn bone(index: u16, tag: u16, parent: i16) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(0.0, 0.0, 0.0, 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("BONE_{index}"),
            dofs: 0,
            index,
            bone_id: tag,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    fn skeleton() -> Skeleton {
        Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1, 0, 1],
            bones: vec![bone(0, 100, -1), bone(1, 200, 0), bone(2, 300, 1)],
            ..CrSkeletonData::default()
        })
    }

    fn animation(dofs: Vec<Dof>) -> CrAnimation {
        CrAnimation {
            num_frames: 2,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs,
            ..CrAnimation::default()
        }
    }

    #[test]
    fn resolves_tags_to_bone_indices() {
        let animation = animation(vec![
            dof(200, format_type::VECTOR3, track::BONE_TRANSLATION),
            dof(200, format_type::QUATERNION, track::BONE_ROTATION),
            dof(100, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let binding = ClipBinding::build(&skeleton(), &animation);

        assert_eq!(binding.bones.len(), 2);
        assert_eq!(binding.bones[0].bone, 0);
        assert_eq!(binding.bones[0].rotation, Some(2));
        assert_eq!(binding.bones[0].translation, None);
        assert_eq!(binding.bones[1].bone, 1);
        assert_eq!(binding.bones[1].translation, Some(0));
        assert_eq!(binding.bones[1].rotation, Some(1));
    }

    #[test]
    fn report_counts_resolved_unresolved_and_untouched() {
        let animation = animation(vec![
            dof(200, format_type::QUATERNION, track::BONE_ROTATION),
            dof(999, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let report = ClipBinding::build(&skeleton(), &animation).report;

        assert_eq!(report.resolved, 1);
        assert_eq!(report.unresolved_tags, vec![999]);
        assert_eq!(report.untouched_bones, vec![0, 2]);
        assert_eq!(report.skeleton_bones, 3);
        assert_eq!(report.clip_tags(), 2);
        assert!((report.match_ratio() - 0.5).abs() < 1e-6);
        assert!(report.looks_compatible());
    }

    #[test]
    fn a_clip_for_a_different_rig_reads_as_incompatible() {
        let animation = animation(vec![
            dof(7001, format_type::QUATERNION, track::BONE_ROTATION),
            dof(7002, format_type::QUATERNION, track::BONE_ROTATION),
            dof(7003, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let report = ClipBinding::build(&skeleton(), &animation).report;
        assert_eq!(report.resolved, 0);
        assert_eq!(report.match_ratio(), 0.0);
        assert!(!report.looks_compatible());
        assert_eq!(report.untouched_bones, vec![0, 1, 2]);
    }

    #[test]
    fn mover_tracks_are_split_out_as_root_motion() {
        let animation = animation(vec![
            dof(0, format_type::VECTOR3, track::MOVER_TRANSLATION),
            dof(0, format_type::QUATERNION, track::MOVER_ROTATION),
            dof(100, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let binding = ClipBinding::build(&skeleton(), &animation);
        assert_eq!(binding.mover_translation, Some(0));
        assert_eq!(binding.mover_rotation, Some(1));
        assert!(binding.report.has_root_motion);
        assert_eq!(binding.bones.len(), 1);
        assert_eq!(binding.bones[0].bone, 0);
    }

    #[test]
    fn unknown_track_ids_are_ignored() {
        let animation = animation(vec![
            dof(100, format_type::FLOAT, track::VISIBILITY),
            dof(100, format_type::FLOAT, 3),
        ]);
        let binding = ClipBinding::build(&skeleton(), &animation);
        assert!(binding.bones.is_empty());
        assert_eq!(binding.report.resolved, 0);
    }

    #[test]
    fn duplicate_dofs_for_one_track_keep_the_first() {
        let animation = animation(vec![
            dof(100, format_type::QUATERNION, track::BONE_ROTATION),
            dof(100, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let binding = ClipBinding::build(&skeleton(), &animation);
        assert_eq!(binding.bones[0].rotation, Some(0));
    }

    #[test]
    fn binding_lookup_by_bone_is_a_binary_search() {
        let animation = animation(vec![
            dof(300, format_type::QUATERNION, track::BONE_ROTATION),
            dof(100, format_type::QUATERNION, track::BONE_ROTATION),
        ]);
        let binding = ClipBinding::build(&skeleton(), &animation);
        assert_eq!(binding.binding_for_bone(0).map(|b| b.tag), Some(100));
        assert_eq!(binding.binding_for_bone(2).map(|b| b.tag), Some(300));
        assert!(binding.binding_for_bone(1).is_none());
        assert_eq!(binding.animated_bones().collect::<Vec<_>>(), vec![0, 2]);
    }

    #[test]
    fn merged_report_unions_touched_bones_across_animations() {
        let skeleton = skeleton();
        let first = ClipBinding::build(
            &skeleton,
            &animation(vec![dof(
                100,
                format_type::QUATERNION,
                track::BONE_ROTATION,
            )]),
        );
        let second = ClipBinding::build(
            &skeleton,
            &animation(vec![
                dof(300, format_type::QUATERNION, track::BONE_ROTATION),
                dof(999, format_type::QUATERNION, track::BONE_ROTATION),
            ]),
        );
        let merged = ClipBinding::merged_report(&[first, second]);
        assert_eq!(merged.resolved, 2);
        assert_eq!(merged.unresolved_tags, vec![999]);
        assert_eq!(merged.untouched_bones, vec![1]);
    }

    #[test]
    fn merged_report_forgives_a_tag_resolved_elsewhere() {
        let skeleton = skeleton();
        let a = ClipBinding::build(
            &skeleton,
            &animation(vec![dof(
                100,
                format_type::QUATERNION,
                track::BONE_ROTATION,
            )]),
        );
        let b = a.clone();
        let merged = ClipBinding::merged_report(&[a, b]);
        assert_eq!(merged.resolved, 1);
        assert!(merged.unresolved_tags.is_empty());
    }

    #[test]
    fn merged_report_of_nothing_is_empty() {
        let report = ClipBinding::merged_report(&[]);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.match_ratio(), 0.0);
        assert!(!report.looks_compatible());
        assert_eq!(report.summary(), "clip drives no bones");
    }

    #[test]
    fn summary_mentions_compact_blocks_and_root_motion() {
        let mut report = BindingReport {
            resolved: 2,
            skeleton_bones: 3,
            untouched_bones: vec![2],
            compact: true,
            has_root_motion: true,
            ..BindingReport::default()
        };
        let text = report.summary();
        assert!(text.contains("2/2 bone tags resolved"), "{text}");
        assert!(text.contains("2 of 3 skeleton bones driven"), "{text}");
        assert!(text.contains("compact"), "{text}");
        assert!(text.contains("root motion"), "{text}");

        report.compact = false;
        report.has_root_motion = false;
        assert!(!report.summary().contains("compact"));
    }

    #[test]
    fn binding_a_clip_covers_every_referenced_animation() {
        let clip = crate::testing::list_clip(&[(1, 0.0, 0.0, 0.0), (2, 0.0, 0.0, 0.0)], 1.0, false);
        let rotating = |tag| {
            animation(vec![dof(
                tag,
                format_type::QUATERNION,
                track::BONE_ROTATION,
            )])
        };
        let dictionary =
            crate::testing::dictionary(Vec::new(), vec![(1, rotating(100)), (2, rotating(200))]);

        let bindings = ClipBinding::for_clip(&skeleton(), &dictionary, &clip);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].bones[0].bone, 0);
        assert_eq!(bindings[1].bones[0].bone, 1);
    }

    #[test]
    fn a_clip_without_animations_binds_to_nothing() {
        let bindings = ClipBinding::for_clip(
            &skeleton(),
            &CrClipDictionary::default(),
            &CrClipAny::Base(Default::default()),
        );
        assert!(bindings.is_empty());
    }
}

//! Which skeleton a clip wants, and what binding it to one produced.
//!
//! Pure functions over a clip dictionary and a `crSkeletonData` — the half of
//! preview that has an answer before anything is on screen.

use std::collections::BTreeSet;

use or_io::gta5::{CrClipDictionary, CrSkeletonData};

use crate::binding::BindingReport;
use crate::player::ClipPlayer;
use crate::skeleton::Skeleton;

/// A clip resolved against a skeleton: everything a caller needs to describe
/// the result without asking the player again.
#[derive(Debug, Clone)]
pub struct BoundClip {
    pub skeleton: Skeleton,
    pub report: BindingReport,
    pub clip_name: String,
    pub duration: f32,
    pub frame_rate: Option<f32>,
}

impl BoundClip {
    pub fn looks_compatible(&self) -> bool {
        self.report.looks_compatible()
    }
}

/// Bind a clip to a skeleton, producing the player and the report.
///
/// Returns `None` only when the clip key is not in the dictionary. A clip that
/// resolves *nothing* still produces a player and a report, because "0 of 84 tags
/// resolved" is the answer the user needs.
pub fn bind(
    dictionary: &CrClipDictionary,
    clip_key: u32,
    skeleton_data: &CrSkeletonData,
) -> Option<(ClipPlayer, BoundClip)> {
    let clip = crate::clip::clip(dictionary, clip_key)?;
    let skeleton = Skeleton::build(skeleton_data);
    let player = ClipPlayer::bind(&skeleton, dictionary, clip);
    let name = &crate::clip::header(clip).name.data;
    let bound = BoundClip {
        report: player.report().clone(),
        clip_name: if name.is_empty() {
            format!("(unnamed 0x{clip_key:08X})")
        } else {
            name.clone()
        },
        duration: player.duration(),
        frame_rate: player.frame_rate(),
        skeleton,
    };
    Some((player, bound))
}

/// Every bone tag a clip drives.
pub fn clip_tags(dictionary: &CrClipDictionary, clip_key: u32) -> BTreeSet<u16> {
    let mut tags = BTreeSet::new();
    if let Some(clip) = crate::clip::clip(dictionary, clip_key) {
        for played in crate::clip::played(dictionary, clip) {
            tags.extend(crate::clip::bone_ids(played.animation));
        }
    }
    tags
}

/// The fraction of `clip_tags` a skeleton can supply.
pub fn coverage_score(clip_tags: &BTreeSet<u16>, skeleton_data: &CrSkeletonData) -> f32 {
    if clip_tags.is_empty() {
        return 0.0;
    }
    let available: BTreeSet<u16> = skeleton_data.bones.iter().map(|b| b.bone_id).collect();
    let matched = clip_tags.intersection(&available).count();
    matched as f32 / clip_tags.len() as f32
}

/// One candidate skeleton's standing, identified by its position in the slice
/// the caller passed.
///
/// ⚠ **An index, not a handle.** What a candidate *is* — a document, a
/// dictionary entry, a file on disk — is the caller's vocabulary, and taking
/// it here would put the application's document model into the animation
/// runtime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkeletonRanking {
    pub index: usize,
    pub coverage: f32,
    pub bone_count: usize,
}

/// Candidates that cover at least one tag, best coverage first, and the
/// smaller skeleton first when two cover equally.
pub fn rank_skeletons(
    clip_tags: &BTreeSet<u16>,
    candidates: &[&CrSkeletonData],
) -> Vec<SkeletonRanking> {
    let mut out: Vec<SkeletonRanking> = candidates
        .iter()
        .enumerate()
        .map(|(index, data)| SkeletonRanking {
            index,
            coverage: coverage_score(clip_tags, data),
            bone_count: data.bones.len(),
        })
        .filter(|s| s.coverage > 0.0)
        .collect();
    out.sort_by(|a, b| {
        b.coverage
            .partial_cmp(&a.coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.bone_count.cmp(&b.bone_count))
    });
    out
}

/// The best-ranked candidate, if any covers a tag.
pub fn best_skeleton(
    clip_tags: &BTreeSet<u16>,
    candidates: &[&CrSkeletonData],
) -> Option<SkeletonRanking> {
    rank_skeletons(clip_tags, candidates).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dictionary, dof, single_clip, v4};
    use or_io::codec::anim::{format_type, track};
    use or_io::gta5::{CrAnimation, CrBoneData};

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

    fn skeleton_with(tags: &[u16]) -> CrSkeletonData {
        let bones: Vec<CrBoneData> = tags
            .iter()
            .enumerate()
            .map(|(i, tag)| bone(i as u16, *tag, if i == 0 { -1 } else { i as i16 - 1 }))
            .collect();
        let parent_indices = (0..tags.len())
            .map(|i| if i == 0 { -1 } else { i as i16 - 1 })
            .collect();
        CrSkeletonData {
            parent_indices,
            bones,
            ..CrSkeletonData::default()
        }
    }

    fn dictionary_driving(tags: &[u16]) -> CrClipDictionary {
        let dofs = tags
            .iter()
            .map(|tag| dof(*tag, format_type::QUATERNION, track::BONE_ROTATION))
            .collect();
        let animation = CrAnimation {
            num_frames: 2,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs,
            ..CrAnimation::default()
        };
        dictionary(
            vec![(0x1234, single_clip("idle", 0x1000, 0.0, 1.0, 1.0))],
            vec![(0x1000, animation)],
        )
    }

    #[test]
    fn binding_a_matching_clip_and_skeleton_reports_full_coverage() {
        let dict = dictionary_driving(&[100, 200, 300]);
        let skeleton = skeleton_with(&[100, 200, 300]);
        let (player, bound) = bind(&dict, 0x1234, &skeleton).expect("bound");

        assert_eq!(bound.clip_name, "idle");
        assert!((bound.duration - 1.0).abs() < 1e-6);
        assert_eq!(bound.report.resolved, 3);
        assert!(bound.report.unresolved_tags.is_empty());
        assert!(bound.looks_compatible());
        assert_eq!(player.report().resolved, 3);
    }

    #[test]
    fn binding_an_unrelated_skeleton_reports_the_mismatch_rather_than_failing() {
        let dict = dictionary_driving(&[100, 200, 300]);
        let skeleton = skeleton_with(&[7001, 7002]);
        let (_, bound) = bind(&dict, 0x1234, &skeleton).expect("still binds");

        assert_eq!(bound.report.resolved, 0);
        assert_eq!(bound.report.unresolved_tags, vec![100, 200, 300]);
        assert!(!bound.looks_compatible());
        assert!(bound.report.summary().contains("0/3"));
    }

    #[test]
    fn an_unknown_clip_key_does_not_bind() {
        let dict = dictionary_driving(&[100]);
        assert!(bind(&dict, 0xDEAD, &skeleton_with(&[100])).is_none());
    }

    #[test]
    fn an_unnamed_clip_falls_back_to_its_key() {
        let mut dict = dictionary_driving(&[100]);
        if let Some(clip) = crate::clip::clip_mut(&mut dict, 0x1234) {
            crate::clip::header_mut(clip).name.data.clear();
        }
        let (_, bound) = bind(&dict, 0x1234, &skeleton_with(&[100])).expect("bound");
        assert_eq!(bound.clip_name, "(unnamed 0x00001234)");
    }

    #[test]
    fn clip_tags_collects_every_driven_bone() {
        let dict = dictionary_driving(&[300, 100, 200]);
        let tags = clip_tags(&dict, 0x1234);
        assert_eq!(tags.into_iter().collect::<Vec<_>>(), vec![100, 200, 300]);
        assert!(clip_tags(&dict, 0xDEAD).is_empty());
    }

    #[test]
    fn coverage_scores_the_fraction_of_tags_present() {
        let tags: BTreeSet<u16> = [100, 200, 300, 400].into_iter().collect();
        assert!((coverage_score(&tags, &skeleton_with(&[100, 200, 300, 400])) - 1.0).abs() < 1e-6);
        assert!((coverage_score(&tags, &skeleton_with(&[100, 200])) - 0.5).abs() < 1e-6);
        assert_eq!(coverage_score(&tags, &skeleton_with(&[9999])), 0.0);
        assert_eq!(
            coverage_score(&BTreeSet::new(), &skeleton_with(&[100])),
            0.0
        );
    }

    #[test]
    fn rankings_are_by_coverage_and_skip_unrelated_skeletons() {
        let tags: BTreeSet<u16> = [100, 200, 300, 400].into_iter().collect();
        let half = skeleton_with(&[100, 200]);
        let full = skeleton_with(&[100, 200, 300, 400]);
        let none = skeleton_with(&[8888]);
        let candidates = [&half, &full, &none];

        let ranked = rank_skeletons(&tags, &candidates);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].index, 1, "the index is into the caller's slice");
        assert!((ranked[0].coverage - 1.0).abs() < 1e-6);
        assert_eq!(best_skeleton(&tags, &candidates).map(|s| s.index), Some(1));
    }

    #[test]
    fn equal_coverage_prefers_the_smaller_skeleton() {
        let tags: BTreeSet<u16> = [100, 200].into_iter().collect();
        let superset = skeleton_with(&[100, 200, 300, 400, 500]);
        let exact = skeleton_with(&[100, 200]);
        let candidates = [&superset, &exact];
        let ranked = rank_skeletons(&tags, &candidates);
        assert_eq!(ranked[0].index, 1);
        assert_eq!(ranked[0].bone_count, 2);
    }

    #[test]
    fn no_candidates_means_no_ranking() {
        let tags: BTreeSet<u16> = [100].into_iter().collect();
        assert!(best_skeleton(&tags, &[]).is_none());
    }
}

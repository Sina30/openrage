//! Resolving a geometry's matrix palette against a skeleton that is not
//! necessarily the one it was authored against: palette slot → the authoring
//! skeleton's bone **tag** → the target skeleton's index.
//!
//! ⚠ **The palette is index-based in the file and has to stay that way.** It
//! is the GPU's matrix palette: a geometry's `Binding` channel holds four
//! slots per vertex and the palette maps each slot to one bone, so the
//! indirection exists to keep the per-vertex cost at one byte. Measured over
//! the staged `.ydr` corpus by
//! `crates/runtime/or-anim/tests/what_a_matrix_palette_addresses.rs`: **1,802
//! of 1,802** entries sit below their own skeleton's bone count, and the
//! widest entry is 107 against a widest shipped bone tag of 62,169. Storing
//! tags there would be a different file format, not a better one.
//!
//! ⚠ **What breaks is not the palette but the pairing.** An index means
//! something only relative to the skeleton it was authored against, and the
//! preview picks its mesh source and its skeleton source separately. Over 595
//! pairs of shipped skeletons, **8,231 of 10,159** shared bone tags sit at a
//! *different index* in the two — so taking a palette entry as an index into
//! somebody else's skeleton is wrong four times in five, and it is wrong
//! silently: every index is in range and the mesh simply deforms about the
//! wrong joints.

use std::collections::BTreeMap;

use or_io::codec::mesh::VertexSource;

use crate::skeleton::Skeleton;
use crate::skin::{SkinnedMesh, VertexInfluences};

/// One palette slot's bone, as both numbers: the index the file stated and the
/// tag that identifies it.
///
/// ⚠ **This is what has to be captured while the authoring skeleton is still
/// in hand.** A decoded [`SkinnedMesh`] holds authoring *indices* on every
/// vertex, and by the time a caller knows which skeleton it will pose against,
/// the drawable that supplied the palette may be a different document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteBone {
    pub index: u16,
    pub tag: u16,
}

/// The palette's bones, paired with the tags its own skeleton gives them.
///
/// An entry the authoring skeleton has no bone for is dropped, because there
/// is no tag to record for it and carrying the index alone would be the thing
/// this exists to replace.
pub fn palette_bones(palette: &[u16], authoring: &Skeleton) -> Vec<PaletteBone> {
    palette
        .iter()
        .filter_map(|entry| {
            authoring.bone(*entry).map(|bone| PaletteBone {
                index: *entry,
                tag: bone.tag,
            })
        })
        .collect()
}

/// Move a decoded mesh's influences onto `target`, using tags captured at
/// build time.
///
/// ⚠ **One remap per distinct bone, not per vertex.** A mesh has tens of
/// thousands of vertices and a palette has tens of slots, so the mapping is
/// built once and applied by lookup.
pub fn retarget_mesh(
    mesh: &mut SkinnedMesh,
    bones: &[PaletteBone],
    target: &Skeleton,
) -> PaletteRetarget {
    let mut out = PaletteRetarget {
        slots: Vec::with_capacity(bones.len()),
        ..Default::default()
    };
    let mut remap: BTreeMap<u16, u16> = BTreeMap::new();
    for bone in bones {
        match target.index_for_tag(bone.tag) {
            Some(index) => {
                out.by_tag += 1;
                out.slots.push(index);
                remap.insert(bone.index, index);
            }
            None => {
                out.unresolved.push(bone.index);
                out.slots.push(bone.index);
            }
        }
    }

    for influences in mesh.influences.iter_mut() {
        remap_influences(influences, &remap);
    }
    mesh.bones_used = mesh
        .bones_used
        .iter()
        .map(|b| remap.get(b).copied().unwrap_or(*b))
        .collect();
    mesh.bones_used.sort_unstable();
    mesh.bones_used.dedup();

    out
}

/// A palette resolved against a target skeleton, and how each slot got there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteRetarget {
    /// One target-skeleton bone index per palette slot, in slot order.
    pub slots: Vec<u16>,
    /// Slots resolved through a bone tag.
    pub by_tag: usize,
    /// Slots the target skeleton has no bone for.
    ///
    /// ⚠ An unresolved slot keeps the index it held, so a retarget is never
    /// worse than not retargeting.
    pub unresolved: Vec<u16>,
}

impl PaletteRetarget {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Whether every slot found its bone by tag.
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty() && !self.slots.is_empty()
    }

    /// One line, for a panel that has to say what happened.
    pub fn summary(&self) -> String {
        format!(
            "{} of {} palette slot(s) resolved by tag, {} left at their file index",
            self.by_tag,
            self.slots.len(),
            self.unresolved.len()
        )
    }
}

/// Map a geometry's palette onto `target`, using `authoring` to say what each
/// entry means.
///
/// `authoring` is the skeleton the geometry's palette indexes — the one in the
/// same drawable, where there is one. Pass the same skeleton for both to get
/// the identity.
///
/// ⚠ **An absent authoring skeleton is not an error and is not a retarget.**
/// **28 of 42** skinned clothing components in the staged corpus carry no
/// skeleton at all, so the file's own index is the only statement available.
/// [`retarget_palette_or_identity`] handles that case.
pub fn retarget_palette(
    palette: &[u16],
    authoring: &Skeleton,
    target: &Skeleton,
) -> PaletteRetarget {
    let mut out = PaletteRetarget {
        slots: Vec::with_capacity(palette.len()),
        ..Default::default()
    };
    for entry in palette {
        let resolved = authoring
            .bone(*entry)
            .and_then(|bone| target.index_for_tag(bone.tag));
        match resolved {
            Some(index) => {
                out.by_tag += 1;
                out.slots.push(index);
            }
            None => {
                out.unresolved.push(*entry);
                out.slots.push(*entry);
            }
        }
    }
    out
}

/// The same, where the authoring skeleton may be missing.
///
/// With no authoring skeleton every slot is carried through unchanged and
/// reported as unresolved, which is the truth: the index is all the file says.
pub fn retarget_palette_or_identity(
    palette: &[u16],
    authoring: Option<&Skeleton>,
    target: &Skeleton,
) -> PaletteRetarget {
    match authoring {
        Some(authoring) => retarget_palette(palette, authoring, target),
        None => PaletteRetarget {
            slots: palette.to_vec(),
            by_tag: 0,
            unresolved: palette.to_vec(),
        },
    }
}

/// Decode a geometry's skinning with its influences already in `target`'s
/// indices.
///
/// The half of [`crate::skin::decode_skinning`] that knows the palette may
/// have been authored elsewhere. ⚠ Skinning consumes bone numbers as indices
/// into [`crate::pose::Pose::skin_matrices`], so the retarget has to happen
/// *before* the mesh is built — rewriting it afterwards means walking every
/// vertex again.
pub fn decode_skinning_against<S: VertexSource + ?Sized>(
    vertices: &S,
    palette: &[u16],
    authoring: Option<&Skeleton>,
    target: &Skeleton,
) -> Option<(SkinnedMesh, PaletteRetarget)> {
    let retarget = retarget_palette_or_identity(palette, authoring, target);
    let mut mesh = crate::skin::decode_skinning(vertices, palette)?;

    // `decode_skinning` resolved each vertex through the file's own palette,
    // so the influences hold authoring indices.
    let mut remap: BTreeMap<u16, u16> = BTreeMap::new();
    for (slot, entry) in palette.iter().enumerate() {
        if let Some(mapped) = retarget.slots.get(slot) {
            remap.entry(*entry).or_insert(*mapped);
        }
    }

    for influences in mesh.influences.iter_mut() {
        remap_influences(influences, &remap);
    }
    mesh.bones_used = mesh
        .bones_used
        .iter()
        .map(|b| remap.get(b).copied().unwrap_or(*b))
        .collect();
    mesh.bones_used.sort_unstable();
    mesh.bones_used.dedup();

    Some((mesh, retarget))
}

fn remap_influences(influences: &mut VertexInfluences, remap: &BTreeMap<u16, u16>) {
    for lane in 0..influences.count as usize {
        let bone = influences.bones[lane];
        if let Some(mapped) = remap.get(&bone) {
            influences.bones[lane] = *mapped;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::v4;
    use or_io::gta5::{CrBoneData, CrSkeletonData};

    fn bone(index: u16, tag: u16, parent: i16) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(0.0, 0.0, 0.0, 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("BONE_{tag}"),
            dofs: 0,
            index,
            bone_id: tag,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    /// A skeleton whose bone `i` carries `tags[i]`.
    fn skeleton_of(tags: &[u16]) -> Skeleton {
        let bones: Vec<CrBoneData> = tags
            .iter()
            .enumerate()
            .map(|(i, tag)| bone(i as u16, *tag, if i == 0 { -1 } else { 0 }))
            .collect();
        let parent_indices: Vec<i16> = (0..tags.len())
            .map(|i| if i == 0 { -1 } else { 0 })
            .collect();
        Skeleton::build(&CrSkeletonData {
            parent_indices,
            bones,
            ..CrSkeletonData::default()
        })
    }

    /// ⚠ A tag that moved index has to follow, and an index taken at face
    /// value would not.
    #[test]
    fn a_tag_that_moved_index_is_followed() {
        let authoring = skeleton_of(&[100, 200, 300]);
        let target = skeleton_of(&[300, 100, 200]);

        let out = retarget_palette(&[0, 1, 2], &authoring, &target);
        assert_eq!(out.slots, vec![1, 2, 0], "tag 100 is index 1 in the target");
        assert_eq!(out.by_tag, 3);
        assert!(out.is_complete());

        // What a caller without this module would have used, and it is wrong
        // for every slot.
        assert_ne!(out.slots, vec![0, 1, 2]);
    }

    #[test]
    fn the_same_skeleton_retargets_to_itself() {
        let skeleton = skeleton_of(&[100, 200, 300]);
        let out = retarget_palette(&[2, 0, 1], &skeleton, &skeleton);
        assert_eq!(out.slots, vec![2, 0, 1]);
        assert!(out.is_complete());
    }

    /// ⚠ A slot the target cannot supply keeps its file index rather than
    /// becoming zero: a retarget must never be worse than not retargeting.
    #[test]
    fn a_slot_the_target_lacks_keeps_its_file_index() {
        let authoring = skeleton_of(&[100, 200, 300]);
        let target = skeleton_of(&[300, 100]);

        let out = retarget_palette(&[0, 1, 2], &authoring, &target);
        assert_eq!(out.slots, vec![1, 1, 0], "tag 200 is absent, slot 1 stands");
        assert_eq!(out.unresolved, vec![1]);
        assert_eq!(out.by_tag, 2);
        assert!(!out.is_complete());
    }

    /// ⚠ **An absent authoring skeleton is reported, not guessed at.** 28 of
    /// 42 skinned clothing components are in this case.
    #[test]
    fn with_no_authoring_skeleton_every_slot_is_carried_and_reported() {
        let target = skeleton_of(&[100, 200]);
        let out = retarget_palette_or_identity(&[0, 1, 7], None, &target);
        assert_eq!(
            out.slots,
            vec![0, 1, 7],
            "the file's own indices, unchanged"
        );
        assert_eq!(out.by_tag, 0);
        assert_eq!(out.unresolved.len(), 3);
        assert!(!out.is_complete());
        assert!(out.summary().contains("0 of 3"));
    }

    #[test]
    fn an_entry_past_the_authoring_skeleton_is_unresolved() {
        let authoring = skeleton_of(&[100, 200]);
        let target = skeleton_of(&[100, 200]);
        let out = retarget_palette(&[0, 9], &authoring, &target);
        assert_eq!(out.slots, vec![0, 9]);
        assert_eq!(out.unresolved, vec![9]);
    }

    #[test]
    fn an_empty_palette_retargets_to_nothing() {
        let skeleton = skeleton_of(&[100]);
        let out = retarget_palette(&[], &skeleton, &skeleton);
        assert!(out.is_empty());
        assert!(!out.is_complete(), "nothing resolved is not complete");
    }

    /// ⚠ The deferred form: tags captured while the authoring skeleton was in
    /// hand, applied to a mesh once the target is known. Every influence has
    /// to move with its slot.
    #[test]
    fn a_mesh_follows_the_tags_captured_at_build_time() {
        let authoring = skeleton_of(&[100, 200, 300]);
        let target = skeleton_of(&[300, 100, 200]);
        let palette = [0u16, 1, 2];

        let bones = palette_bones(&palette, &authoring);
        assert_eq!(bones.len(), 3);
        assert_eq!(bones[0].tag, 100);

        let mut mesh = SkinnedMesh {
            influences: vec![
                VertexInfluences {
                    bones: [0, 2, 0, 0, 0, 0, 0, 0],
                    weights: [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    count: 2,
                },
                VertexInfluences::rigid(1),
            ],
            bones_used: vec![0, 1, 2],
            ..SkinnedMesh::default()
        };

        let out = retarget_mesh(&mut mesh, &bones, &target);
        assert!(out.is_complete());
        assert_eq!(mesh.influences[0].bones[0], 1, "tag 100 moved to index 1");
        assert_eq!(mesh.influences[0].bones[1], 0, "tag 300 moved to index 0");
        assert_eq!(mesh.influences[1].bones[0], 2, "tag 200 moved to index 2");
        assert_eq!(mesh.bones_used, vec![0, 1, 2]);
    }

    /// ⚠ A slot whose tag the target lacks leaves its influences where they
    /// were, which is what a caller doing nothing would have had.
    #[test]
    fn an_unresolved_slot_leaves_its_influences_alone() {
        let authoring = skeleton_of(&[100, 200]);
        let target = skeleton_of(&[100]);
        let bones = palette_bones(&[0, 1], &authoring);

        let mut mesh = SkinnedMesh {
            influences: vec![VertexInfluences::rigid(1)],
            bones_used: vec![1],
            ..SkinnedMesh::default()
        };
        let out = retarget_mesh(&mut mesh, &bones, &target);
        assert_eq!(out.unresolved, vec![1]);
        assert_eq!(mesh.influences[0].bones[0], 1);
    }

    /// An entry the authoring skeleton has no bone for has no tag to record,
    /// so it does not become a slot.
    #[test]
    fn a_palette_entry_past_the_authoring_skeleton_yields_no_bone() {
        let authoring = skeleton_of(&[100, 200]);
        assert_eq!(palette_bones(&[0, 9, 1], &authoring).len(), 2);
    }
}

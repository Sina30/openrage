//! A skeleton's rest pose is what its bones derive: over every staged drawable, drawable dictionary and
//! fragment of both targets, deriving the default and inverse transforms and the comprehensive signature
//! again from the bone data reproduces what the file holds.

use or_io::gta5::skeleton::bucket_bone_tags;
use or_io::gta5::{CrBoneData, CrSkeletonData, Mat34V};
use or_io::Asset;
use or_io_verify::gates::files;

const WHAT: &str = "a_skeletons_rest_pose_is_what_its_bones_derive";

/// Every skeleton an asset holds, whichever target and whichever drawable holds it.
fn skeletons(asset: &Asset) -> Vec<&CrSkeletonData> {
    match asset {
        Asset::Drawable(d) => d.base.skeleton.as_deref().into_iter().collect(),
        Asset::DrawableGen9(d) => d.base.skeleton.as_deref().into_iter().collect(),
        Asset::DrawableDict(dict) => (dict.entries.iter().flatten())
            .filter_map(|d| d.base.skeleton.as_deref())
            .collect(),
        Asset::DrawableDictGen9(dict) => (dict.entries.iter().flatten())
            .filter_map(|d| d.base.skeleton.as_deref())
            .collect(),
        Asset::Fragment(f) => (f.drawables().into_iter())
            .filter_map(|d| d.base.skeleton.as_deref())
            .collect(),
        Asset::FragmentGen9(f) => (f.drawables().into_iter())
            .filter_map(|d| d.base.skeleton.as_deref())
            .collect(),
        _ => Vec::new(),
    }
}

/// The first three lanes of every column of `m`.
fn lanes(m: &Mat34V) -> [f32; 12] {
    let columns = [&m.col0, &m.col1, &m.col2, &m.col3];
    std::array::from_fn(|i| {
        let column = columns[i / 3];
        [column.x, column.y, column.z][i % 3]
    })
}

#[test]
fn a_skeletons_rest_pose_is_what_its_bones_derive() {
    let mut paths = Vec::new();
    for ext in ["ydr", "ydd", "yft"] {
        paths.extend(files(ext, None, WHAT).unwrap_or_default());
    }
    if paths.is_empty() {
        return;
    }

    let (mut compared, mut bones, mut scaled, mut worst) =
        (0usize, 0usize, 0usize, (0.0f32, String::new()));
    let mut per_target = std::collections::BTreeMap::<String, usize>::new();
    let mut wrong_signatures = Vec::new();
    for path in &paths {
        let asset = or_io::open(path).expect("a corpus file opens");
        for (which, shipped) in skeletons(&asset).into_iter().enumerate() {
            let count = shipped.bones.len();
            if count == 0
                || shipped.default_transforms.len() != count
                || shipped.inverse_transforms.len() != count
            {
                continue;
            }
            // Wiped, so that a derivation that does nothing cannot pass.
            let mut derived = shipped.clone();
            for m in derived
                .default_transforms
                .iter_mut()
                .chain(&mut derived.inverse_transforms)
            {
                (m.col0.x, m.col1.y, m.col2.z, m.col3.x) = (99.0, 99.0, 99.0, 99.0);
            }
            derived.signature_comprehensive = 0;
            let roots: Vec<usize> = (0..count)
                .filter(|&bone| shipped.bones[bone].parent_index == -1)
                .collect();
            for root in roots {
                derived
                    .rebuild_rest_pose(root)
                    .expect("a shipped skeleton derives");
            }

            let name = format!("{} #{which}", path.display());
            compared += 1;
            bones += count;
            *per_target
                .entry(format!("{:?}", asset.target()))
                .or_default() += 1;
            scaled += (shipped.bones.iter())
                .filter(|b| (b.scale.x, b.scale.y, b.scale.z) != (1.0, 1.0, 1.0))
                .count();
            let pairs = (shipped
                .default_transforms
                .iter()
                .zip(&derived.default_transforms))
            .chain(
                shipped
                    .inverse_transforms
                    .iter()
                    .zip(&derived.inverse_transforms),
            );
            for (have, got) in pairs {
                let off = (lanes(have).iter().zip(lanes(got)))
                    .fold(0.0f32, |a, (x, y)| a.max((x - y).abs()));
                if off > worst.0 {
                    worst = (off, name.clone());
                }
            }
            if shipped.signature_comprehensive != derived.signature_comprehensive {
                wrong_signatures.push(name);
            }
        }
    }

    eprintln!(
        "{compared} skeleton(s) {per_target:?}, {bones} bone(s), {scaled} of them not unit scale: \
         worst transform deviation {} in {}; {} comprehensive signature(s) differ",
        worst.0,
        worst.1,
        wrong_signatures.len()
    );
    assert!(compared > 0, "no staged file holds a skeleton");
    assert!(
        worst.0 < 1e-4,
        "a derived transform is {} from the file's, in {}",
        worst.0,
        worst.1
    );
    assert!(
        wrong_signatures.is_empty(),
        "the comprehensive signature derived differs from the file's in {wrong_signatures:?}"
    );
}

/// CRC-32 chained over `words`, each as eight little-endian bytes.
fn chained(words: impl IntoIterator<Item = u64>) -> u32 {
    let mut crc = flate2::Crc::new();
    for word in words {
        crc.update(&word.to_le_bytes());
    }
    crc.sum()
}

/// The signature and the non-chiral signature a skeleton's bones state, as the engine computes them:
/// the first hashes each bone's id and degrees of freedom in order; the second hashes each bone's id
/// with whether it can translate, rotate and scale at all, in the order the bone-id table lists the
/// bones when any bone's id is not its index, else in the order the bones are in.
fn signatures_of(skeleton: &CrSkeletonData) -> (u32, u32) {
    const TRANSLATION: u16 = 0x70;
    const ROTATION: u16 = 0x07;
    const SCALE: u16 = 0x700;
    let id_and_dofs = |bone: &CrBoneData| (u64::from(bone.bone_id) << 32) | u64::from(bone.dofs);
    let non_chiral = |bone: &CrBoneData| {
        let can = |mask: u16| u64::from(bone.dofs & mask != 0);
        (u64::from(bone.bone_id) << 32)
            | can(TRANSLATION)
            | (can(ROTATION) << 1)
            | (can(SCALE) << 2)
    };
    let bones = &skeleton.bones;
    let has_ids = (bones.iter().enumerate()).any(|(at, bone)| usize::from(bone.bone_id) != at);
    let order: Vec<usize> = if has_ids {
        let slots = usize::from(skeleton.bone_tag_slot_count);
        (bucket_bone_tags(slots, bones.iter().map(|bone| bone.bone_id))
            .into_iter()
            .flatten())
        .map(|(_, index)| index as usize)
        .collect()
    } else {
        (0..bones.len()).collect()
    };
    (
        chained(bones.iter().map(id_and_dofs)),
        chained(order.into_iter().map(|at| non_chiral(&bones[at]))),
    )
}

#[test]
fn a_skeletons_other_two_signatures_are_what_its_bones_state() {
    let mut paths = Vec::new();
    for ext in ["ydr", "ydd", "yft"] {
        paths.extend(files(ext, None, WHAT).unwrap_or_default());
    }
    if paths.is_empty() {
        return;
    }
    let (mut compared, mut wrong) = (0usize, Vec::new());
    for path in &paths {
        let asset = or_io::open(path).expect("a corpus file opens");
        for (which, shipped) in skeletons(&asset).into_iter().enumerate() {
            if shipped.bones.is_empty() {
                continue;
            }
            compared += 1;
            let (signature, non_chiral) = signatures_of(shipped);
            if (signature, non_chiral) != (shipped.signature, shipped.signature_non_chiral) {
                wrong.push(format!(
                    "{} #{which}: signature {signature:#x} against {:#x}, non-chiral {non_chiral:#x} against {:#x}",
                    path.display(),
                    shipped.signature,
                    shipped.signature_non_chiral
                ));
            }
        }
    }
    eprintln!(
        "{compared} skeleton(s), {} with a signature that differs",
        wrong.len()
    );
    assert!(compared > 0, "no staged file holds a skeleton");
    assert!(
        wrong.is_empty(),
        "the signatures derived differ from the file's in {} skeleton(s):\n  {}",
        wrong.len(),
        wrong
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

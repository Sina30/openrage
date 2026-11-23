use super::*;
use crate::gta6::hash::name_hash32;

pub(super) struct BuiltBone {
    pub(super) name: &'static str,
    pub(super) parent: i16,
    pub(super) rotation: [f32; 4],
    pub(super) translation: [f32; 3],
}

pub(super) const BONES: [BuiltBone; 3] = [
    BuiltBone {
        name: "SKEL_ROOT",
        parent: -1,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.0, 0.0, 1.0],
    },
    BuiltBone {
        name: "SKEL_Spine0",
        parent: 0,
        rotation: [0.0, 0.0, 0.70710677, 0.70710677],
        translation: [0.5, 0.0, 0.0],
    },
    BuiltBone {
        name: "SKEL_Head",
        parent: 1,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.25, 0.0, 0.0],
    },
];

/// The lane `row` of column `column` of bone `bone`'s built inverse bind: a value no other lane holds.
fn inverse_lane(bone: usize, column: usize, row: usize) -> f32 {
    (100 * bone + 10 * column + row) as f32 + 0.5
}

/// A skeleton record of `bones` in `img`; where it lies and where its bones lie.
pub(super) fn skeleton_into(img: &mut Image, bones: &[BuiltBone]) -> (usize, usize) {
    let skeleton = img.alloc(0x70);
    let records = img.alloc(0x50 * bones.len());
    img.ptr(skeleton + 0x28, records);
    let inverses = img.alloc(0x40 * bones.len());
    img.ptr(skeleton + 0x30, inverses);
    for i in 0..bones.len() {
        for c in 0..4 {
            for r in 0..4 {
                // The w lanes hold words that are not the matrix's.
                let x = if r == 3 {
                    f32::NAN
                } else {
                    inverse_lane(i, c, r)
                };
                img.put(inverses + 0x40 * i + 16 * c + 4 * r, &x.to_le_bytes());
            }
        }
    }
    img.u16(skeleton + 0x6C, bones.len() as u16);
    img.u16(skeleton + 0x6E, bones.len() as u16);
    for (i, bone) in bones.iter().enumerate() {
        let at = records + 0x50 * i;
        for (k, x) in bone.rotation.iter().enumerate() {
            img.put(at + 4 * k, &x.to_le_bytes());
        }
        img.vec3(at + 0x10, bone.translation);
        img.u32(at + 0x1C, name_hash32(bone.name.as_bytes()));
        img.vec3(at + 0x20, [1.0; 3]);
        let name = img.alloc(bone.name.len() + 1);
        img.put(name, bone.name.as_bytes());
        img.ptr(at + 0x30, name);
        img.u16(at + 0x44, 0xFFFF);
        img.put(at + 0x46, &bone.parent.to_le_bytes());
        img.u16(at + 0x48, i as u16);
    }
    (skeleton, records)
}

/// A drawable root whose 0x18 points at a skeleton of `bones`.
fn drawable_with(bones: &[BuiltBone]) -> (Image, usize, usize) {
    let mut img = Image::default();
    let root = img.alloc(DRAWABLE_ROOT);
    let (skeleton, records) = skeleton_into(&mut img, bones);
    img.ptr(root + 0x18, skeleton);
    (img, skeleton, records)
}

#[test]
fn a_drawables_skeleton_reads_each_bones_name_hash_parent_and_local_bind() {
    let (img, _, _) = drawable_with(&BONES);
    let skeleton = read_skeleton(&img.bytes, false).unwrap().unwrap();
    assert_eq!(skeleton.bones.len(), 3);
    for (read, built) in skeleton.bones.iter().zip(&BONES) {
        assert_eq!(read.name, built.name);
        assert_eq!(read.hash, name_hash32(built.name.as_bytes()));
        assert_eq!(read.parent, u16::try_from(built.parent).ok());
        assert_eq!(read.rotation, built.rotation);
        assert_eq!(read.translation, built.translation);
        assert_eq!(read.scale, [1.0; 3]);
    }
}

#[test]
fn each_bone_reads_its_inverse_bind_column_by_column_from_the_array_at_0x30() {
    let (img, _, _) = drawable_with(&BONES);
    let skeleton = read_skeleton(&img.bytes, false).unwrap().unwrap();
    for (i, bone) in skeleton.bones.iter().enumerate() {
        let built: [[f32; 3]; 4] =
            std::array::from_fn(|c| std::array::from_fn(|r| inverse_lane(i, c, r)));
        assert_eq!(bone.inverse_bind, built, "bone {i}");
    }
}

#[test]
fn a_skeleton_with_bones_and_no_inverse_binds_is_refused() {
    let (mut img, skeleton, _) = drawable_with(&BONES);
    img.u64(skeleton + 0x30, 0);
    assert!(matches!(
        read_skeleton(&img.bytes, false),
        Err(HeaderlessError::Null {
            what: "inverse binds",
            ..
        })
    ));
}

#[test]
fn a_fragments_skeleton_is_found_through_the_drawable_its_0x18_points_at() {
    let mut img = Image::default();
    let fragment = img.alloc(0x100);
    let drawable = img.alloc(DRAWABLE_ROOT);
    img.ptr(fragment + 0x18, drawable);
    let (skeleton, _) = skeleton_into(&mut img, &BONES[..1]);
    img.ptr(drawable + 0x18, skeleton);
    let read = read_skeleton(&img.bytes, true).unwrap().unwrap();
    assert_eq!(read.bones.len(), 1);
    assert_eq!(read.bones[0].name, "SKEL_ROOT");
    assert_eq!(read.bones[0].parent, None);
}

#[test]
fn a_drawable_whose_0x18_is_null_has_no_skeleton() {
    let mut img = Image::default();
    img.alloc(DRAWABLE_ROOT);
    assert_eq!(read_skeleton(&img.bytes, false).unwrap(), None);
}

#[test]
fn a_bone_whose_stated_hash_is_not_its_names_reads_both_as_a_renamed_bone_holds_them() {
    let (mut img, _, records) = drawable_with(&BONES);
    img.u32(records + 0x50 + 0x1C, name_hash32(b"SKEL_Spine1"));
    let read = read_skeleton(&img.bytes, false).unwrap().unwrap();
    assert_eq!(read.bones[1].name, "SKEL_Spine0");
    assert_eq!(read.bones[1].hash, name_hash32(b"SKEL_Spine1"));
}

#[test]
fn a_bone_whose_parent_does_not_come_before_it_is_refused() {
    let (mut img, _, records) = drawable_with(&BONES);
    img.put(records + 0x50 + 0x46, &2i16.to_le_bytes());
    assert!(matches!(
        read_skeleton(&img.bytes, false),
        Err(HeaderlessError::IndexPastCount {
            index: 2,
            count: 1,
            ..
        })
    ));
}

#[test]
fn a_bone_whose_index_is_not_its_place_is_refused() {
    let (mut img, _, records) = drawable_with(&BONES);
    img.u16(records + 2 * 0x50 + 0x48, 7);
    assert!(matches!(
        read_skeleton(&img.bytes, false),
        Err(HeaderlessError::Disagree {
            first: 7,
            second: 2,
            ..
        })
    ));
}

#[test]
fn two_bone_counts_that_differ_are_refused() {
    let (mut img, skeleton, _) = drawable_with(&BONES);
    img.u16(skeleton + 0x6E, 2);
    assert!(matches!(
        read_skeleton(&img.bytes, false),
        Err(HeaderlessError::Disagree {
            first: 3,
            second: 2,
            ..
        })
    ));
}

#[test]
fn a_bone_count_past_the_section_is_refused_before_it_sizes_anything() {
    let (mut img, skeleton, _) = drawable_with(&BONES);
    img.u16(skeleton + 0x6C, u16::MAX);
    img.u16(skeleton + 0x6E, u16::MAX);
    assert!(matches!(
        read_skeleton(&img.bytes, false),
        Err(HeaderlessError::PastEnd { what: "bones", .. })
    ));
}

#[test]
fn a_dictionary_reads_the_skeleton_of_each_drawable_entry_and_passes_over_any_other() {
    let mut img = Image::default();
    let root = img.alloc(0x40);
    let codes = img.alloc(8);
    let entries = img.alloc(16);
    let (first, second) = (name_hash32(b"head_a"), name_hash32(b"head_b"));
    img.u32(codes, first);
    img.u32(codes + 4, second);
    img.ptr(root + 0x20, codes);
    img.u16(root + 0x28, 2);
    img.ptr(root + 0x30, entries);
    img.u16(root + 0x38, 2);
    // The first entry is no drawable root: its shader group and level pointers are null.
    let other = img.alloc(DRAWABLE_ROOT);
    img.ptr(entries, other);
    let drawable = img.alloc(DRAWABLE_ROOT);
    let group = img.alloc(0x20);
    let level = img.alloc(0x20);
    img.ptr(drawable + 0x10, group);
    img.ptr(drawable + 0x50, level);
    img.put(drawable + 0x7C, &10.0f32.to_le_bytes());
    let (skeleton, _) = skeleton_into(&mut img, &BONES);
    img.ptr(drawable + 0x18, skeleton);
    img.ptr(entries + 8, drawable);
    let read = read_dictionary_skeletons(&img.bytes).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].0, second);
    let names: Vec<_> = read[0].1.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, BONES.map(|b| b.name));
}

use super::super::bytes::f32_at;
use super::skeleton::{skeleton_into, BONES};
use super::*;

/// A drawable whose skeleton is [`BONES`], with local matrices at 0x38 whose w lanes hold words
/// of their own; the image, and where the bones, their inverse binds and their locals lie.
fn drawable() -> (Image, usize, usize, usize) {
    let mut img = Image::default();
    let root = img.alloc(DRAWABLE_ROOT);
    let (skeleton, records) = skeleton_into(&mut img, &BONES);
    img.ptr(root + 0x18, skeleton);
    let locals = img.alloc(0x40 * BONES.len());
    for i in 0..BONES.len() {
        for c in 0..4 {
            img.u32(
                locals + 0x40 * i + 16 * c + 12,
                0xD00D_0000 + (4 * i + c) as u32,
            );
        }
    }
    img.ptr(skeleton + 0x38, locals);
    let inverses = u64_at(&img.bytes, skeleton + 0x30) as usize - 0x5000_0000;
    (img, records, inverses, locals)
}

/// `m`, four columns with the translation last, applied to the point `p`.
fn transformed(m: &[[f32; 3]; 4], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
}

fn floats(bytes: &[u8], at: usize) -> [f32; 3] {
    std::array::from_fn(|k| f32_at(bytes, at + 4 * k))
}

#[test]
fn a_moved_bone_writes_its_numbers_its_local_and_the_inverse_binds_under_it() {
    let (mut img, records, inverses, locals) = drawable();
    let before = img.bytes.clone();
    let mut edited = read_skeleton(&img.bytes, false).unwrap().unwrap();
    edited.bones[1].translation = [2.0, 3.0, 4.0];
    edited.bones[1].scale = [1.0, 2.0, 0.5];
    rebind_under(&mut edited, 1).unwrap();
    write_skeleton(&mut img.bytes, false, &edited).unwrap();
    assert_eq!(read_skeleton(&img.bytes, false).unwrap().unwrap(), edited);

    // Bone 1 turns a quarter about z from the root at (0, 0, 1): its origin is (2, 3, 5), and bone
    // 2's, 0.25 along its x scaled by 1, is a quarter turn on, at (2, 3.25, 5).
    let (one, two) = ([2.0, 3.0, 5.0], [2.0, 3.25, 5.0]);
    let inverse = |i: usize| edited.bones[i].inverse_bind;
    assert!(close(transformed(&inverse(1), one), [0.0; 3]));
    assert!(close(transformed(&inverse(2), two), [0.0; 3]));
    assert!(close(transformed(&inverse(1), two), BONES[2].translation));
    // Bone 1's local: its turned axes scaled by (1, 2, 0.5), then its translation.
    let local = locals + 0x40;
    assert!(close(floats(&img.bytes, local), [0.0, 1.0, 0.0]));
    assert!(close(floats(&img.bytes, local + 16), [-2.0, 0.0, 0.0]));
    assert!(close(floats(&img.bytes, local + 32), [0.0, 0.0, 0.5]));
    assert_eq!(floats(&img.bytes, local + 48), [2.0, 3.0, 4.0]);

    // Nothing else is written: not the root, not a hash, not a w lane, not the other locals.
    let mut written = vec![false; before.len()];
    let mut mark = |at: usize, len: usize| written[at..at + len].fill(true);
    mark(records + 0x50, 0x1C);
    mark(records + 0x50 + 0x20, 12);
    for c in 0..4 {
        mark(locals + 0x40 + 16 * c, 12);
        for i in [1, 2] {
            mark(inverses + 0x40 * i + 16 * c, 12);
        }
    }
    for (at, (was, now)) in before.iter().zip(&img.bytes).enumerate() {
        if !written[at] {
            assert_eq!(was, now, "byte {at:#X}");
        }
    }
}

#[test]
fn an_unchanged_skeleton_writes_nothing_and_a_refused_edit_changes_nothing() {
    let (mut img, ..) = drawable();
    let before = img.bytes.clone();
    let read = read_skeleton(&img.bytes, false).unwrap().unwrap();
    write_skeleton(&mut img.bytes, false, &read).unwrap();
    assert_eq!(img.bytes, before);

    let edit = |change: &dyn Fn(&mut Skeleton)| {
        let mut edited = read.clone();
        change(&mut edited);
        edited
    };
    let refused = [
        edit(&|s: &mut Skeleton| s.bones[2].name = "SKEL_Neck".into()),
        edit(&|s: &mut Skeleton| s.bones[2].parent = Some(0)),
        edit(&|s: &mut Skeleton| {
            s.bones.pop();
        }),
        edit(&|s: &mut Skeleton| s.bones[1].scale = [1.0, 0.0, 1.0]),
        edit(&|s: &mut Skeleton| s.bones[0].translation = [f32::NAN, 0.0, 0.0]),
    ];
    for (i, edited) in refused.iter().enumerate() {
        assert!(
            write_skeleton(&mut img.bytes, false, edited).is_err(),
            "edit {i}"
        );
        assert_eq!(img.bytes, before, "edit {i}");
    }
}

#[test]
fn a_fragments_skeleton_with_no_locals_is_written_through_its_drawable() {
    let mut img = Image::default();
    let fragment = img.alloc(0x100);
    let drawable = img.alloc(DRAWABLE_ROOT);
    img.ptr(fragment + 0x18, drawable);
    let (skeleton, _) = skeleton_into(&mut img, &BONES);
    img.ptr(drawable + 0x18, skeleton);
    let mut edited = read_skeleton(&img.bytes, true).unwrap().unwrap();
    edited.bones[0].rotation = [0.0, 0.6, 0.0, 0.8];
    rebind_under(&mut edited, 0).unwrap();
    write_skeleton(&mut img.bytes, true, &edited).unwrap();
    assert_eq!(read_skeleton(&img.bytes, true).unwrap().unwrap(), edited);
    // Read as a drawable's, the fragment's 0x18 names no skeleton of three bones: refused.
    assert!(write_skeleton(&mut img.bytes, false, &edited).is_err());
}

#[test]
fn a_rebind_derives_the_bone_and_those_under_it_and_leaves_the_others() {
    let (img, ..) = drawable();
    let read = read_skeleton(&img.bytes, false).unwrap().unwrap();
    let mut rebound = read.clone();
    rebind_under(&mut rebound, 2).unwrap();
    assert_eq!(rebound.bones[..2], read.bones[..2]);
    // Bone 2's origin: the root's (0, 0, 1), 0.5 along x, then 0.25 along x turned a quarter about z.
    let origin = [0.5, 0.25, 1.0];
    assert!(close(
        transformed(&rebound.bones[2].inverse_bind, origin),
        [0.0; 3]
    ));
    assert!(rebind_under(&mut rebound, 3).is_err());
}

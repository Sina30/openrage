use super::skeleton::{skeleton_into, BONES};
use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

/// A drawable whose skeleton is [`BONES`], its block map last; the sections and where the bones lie.
fn mapped() -> (Rpf9Sections, usize) {
    let mut img = Image::default();
    let root = img.alloc(DRAWABLE_ROOT);
    let (skeleton, records) = skeleton_into(&mut img, &BONES);
    img.ptr(root + 0x18, skeleton);
    let map = img.alloc(0x20);
    img.bytes[map + 8] = 1;
    img.bytes[map + 9] = 1;
    img.ptr(root + 0x08, map);
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes,
        physical: vec![0; 16],
    };
    (sections, records)
}

fn skeleton(s: &Rpf9Sections) -> Skeleton {
    read_skeleton(&s.virtual_section, false)
        .expect("it reads")
        .expect("a skeleton")
}

#[test]
fn renamed_bones_read_back_by_their_new_names_and_keep_the_hashes_they_are_found_by() {
    let (s, records) = mapped();
    let read = skeleton(&s);
    let mut edited = read.clone();
    let (longer, shorter) = ("SKEL_Spine0_Renamed_Longer", "H");
    edited.bones[1].name = longer.into();
    edited.bones[2].name = shorter.into();
    let mut written = s.clone();
    rename_bones(&mut written, false, &edited).expect("renamed");
    let back = skeleton(&written);
    assert_eq!(back, edited);
    for (b, r) in back.bones.iter().zip(&read.bones) {
        assert_eq!(b.hash, r.hash);
    }

    // The names lie at the old end one after the other, and below it only their pointers changed.
    let old = s.virtual_section.len();
    let v = &written.virtual_section;
    let text = [longer, "\0", shorter, "\0"].concat().into_bytes();
    assert_eq!(&v[old..old + text.len()], text.as_slice());
    assert_eq!(v.len(), old + text.len().next_multiple_of(16));
    let pointer = |bone: usize| records + 0x50 * bone + 0x30;
    assert_eq!(u64_at(v, pointer(1)), 0x5000_0000 + old as u64);
    let second = old + longer.len() + 1;
    assert_eq!(u64_at(v, pointer(2)), 0x5000_0000 + second as u64);
    for (at, (was, now)) in s.virtual_section.iter().zip(v).enumerate() {
        let moved = [pointer(1), pointer(2)]
            .iter()
            .any(|p| (*p..*p + 8).contains(&at));
        if !moved {
            assert_eq!(was, now, "byte {at:#X}");
        }
    }

    // Names as read write as nothing.
    let mut again = written.clone();
    rename_bones(&mut again, false, &back).expect("nothing to rename");
    assert!(again == written);
}

#[test]
fn a_rename_the_record_cannot_hold_is_refused_and_changes_nothing() {
    let (s, _) = mapped();
    let read = skeleton(&s);
    let edit = |change: &dyn Fn(&mut Skeleton)| {
        let mut edited = read.clone();
        change(&mut edited);
        edited
    };
    let refused = [
        edit(&|k: &mut Skeleton| k.bones[1].name = String::new()),
        edit(&|k: &mut Skeleton| k.bones[1].name = "SKEL\0Neck".into()),
        edit(&|k: &mut Skeleton| {
            k.bones[1].name = "SKEL_Neck".into();
            k.bones[1].hash ^= 1;
        }),
        edit(&|k: &mut Skeleton| {
            k.bones.pop();
        }),
    ];
    for (i, edited) in refused.iter().enumerate() {
        let mut written = s.clone();
        assert!(
            rename_bones(&mut written, false, edited).is_err(),
            "edit {i}"
        );
        assert!(written == s, "edit {i} changed bytes");
    }

    // A stored RSC8 has no block map to grow its virtual section by.
    let mut stored = s.clone();
    stored.form = Rpf9Form::StoredRsc8 { config: 0 };
    let renamed = edit(&|k: &mut Skeleton| k.bones[2].name = "SKEL_Neck".into());
    let before = stored.clone();
    assert!(matches!(
        rename_bones(&mut stored, false, &renamed),
        Err(BoneNameError::Grow(_))
    ));
    assert!(stored == before);
}

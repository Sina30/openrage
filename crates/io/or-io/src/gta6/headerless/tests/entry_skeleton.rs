use super::skeleton::{skeleton_into, BONES};
use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};
use crate::gta6::hash::name_hash32;

/// A dictionary of two entries, the first no drawable root, the second a drawable whose skeleton
/// is [`BONES`], its block map last; the sections, the two codes and where the second's bones lie.
fn dictionary() -> (Rpf9Sections, [u32; 2], usize) {
    let mut img = Image::default();
    let root = img.alloc(0x40);
    let codes = img.alloc(8);
    let entries = img.alloc(16);
    let keyed = [name_hash32(b"head_a"), name_hash32(b"head_b")];
    img.u32(codes, keyed[0]);
    img.u32(codes + 4, keyed[1]);
    img.ptr(root + 0x20, codes);
    img.u16(root + 0x28, 2);
    img.ptr(root + 0x30, entries);
    img.u16(root + 0x38, 2);
    let other = img.alloc(DRAWABLE_ROOT);
    img.ptr(entries, other);
    let drawable = img.alloc(DRAWABLE_ROOT);
    let group = img.alloc(0x20);
    let level = img.alloc(0x20);
    img.ptr(drawable + 0x10, group);
    img.ptr(drawable + 0x50, level);
    img.put(drawable + 0x7C, &10.0f32.to_le_bytes());
    let (skeleton, records) = skeleton_into(&mut img, &BONES);
    img.ptr(drawable + 0x18, skeleton);
    img.ptr(entries + 8, drawable);
    let map = img.alloc(0x20);
    img.bytes[map + 8] = 1;
    img.bytes[map + 9] = 1;
    img.ptr(root + 0x08, map);
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: img.bytes,
        physical: vec![0; 16],
    };
    (sections, keyed, records)
}

fn entry(s: &Rpf9Sections, code: u32) -> Skeleton {
    read_skeleton_of(&s.virtual_section, SkeletonOwner::Entry(code))
        .expect("it reads")
        .expect("a skeleton")
}

#[test]
fn an_entrys_skeleton_reads_as_the_dictionarys_reader_reads_it() {
    let (s, [first, second], _) = dictionary();
    let all = read_dictionary_skeletons(&s.virtual_section).unwrap();
    assert_eq!(all, vec![(second, entry(&s, second))]);
    assert!(matches!(
        read_skeleton_of(&s.virtual_section, SkeletonOwner::Entry(first)),
        Err(HeaderlessError::NotADrawable { .. })
    ));
    let absent = first ^ second ^ 1;
    assert!(read_skeleton_of(&s.virtual_section, SkeletonOwner::Entry(absent)).is_err());
}

#[test]
fn an_entrys_moved_bone_is_written_in_place_and_reads_back() {
    let (mut s, [first, second], records) = dictionary();
    let before = s.virtual_section.clone();
    let mut edited = entry(&s, second);
    edited.bones[2].translation = [0.75, 0.5, 0.25];
    rebind_under(&mut edited, 2).unwrap();
    write_skeleton(
        &mut s.virtual_section,
        SkeletonOwner::Entry(second),
        &edited,
    )
    .unwrap();
    assert_eq!(entry(&s, second), edited);
    assert_eq!(s.virtual_section.len(), before.len());
    // The first two bones' records are as they were.
    assert_eq!(
        s.virtual_section[records..records + 2 * 0x50],
        before[records..records + 2 * 0x50]
    );
    // Read as the root's or another entry's, the edit is refused and nothing changes.
    let written = s.virtual_section.clone();
    assert!(write_skeleton(&mut s.virtual_section, false, &edited).is_err());
    assert!(write_skeleton(&mut s.virtual_section, SkeletonOwner::Entry(first), &edited).is_err());
    assert_eq!(s.virtual_section, written);
}

#[test]
fn an_entrys_renamed_bone_reads_back_by_its_new_name_and_keeps_its_hash() {
    let (s, [_, second], _) = dictionary();
    let read = entry(&s, second);
    let mut edited = read.clone();
    edited.bones[1].name = "SKEL_Spine0_Renamed".into();
    let mut written = s.clone();
    rename_bones(&mut written, SkeletonOwner::Entry(second), &edited).expect("renamed");
    let back = entry(&written, second);
    assert_eq!(back, edited);
    assert_eq!(back.bones[1].hash, read.bones[1].hash);
    assert!(written.virtual_section.len() > s.virtual_section.len());
    // Names as read write as nothing.
    let mut again = written.clone();
    rename_bones(&mut again, SkeletonOwner::Entry(second), &back).expect("nothing to rename");
    assert!(again == written);
}

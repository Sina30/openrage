use super::write_filter_tracks::mapped;
use super::write_filters::{bones, filters_and_record, tracks};
use super::*;
use crate::container::rpf::Rpf9Sections;
use crate::gta6::hash::name_hash32;

/// Each entry's code, name and element count, in code order.
fn entries(sections: &Rpf9Sections) -> Vec<(u32, String, usize)> {
    let dict = read_dictionary(&sections.virtual_section, None).expect("the dictionary reads");
    (dict.entries.into_iter())
        .map(|(code, e)| match e.expect("each entry reads") {
            Entry::FrameFilter { name, tracks } => (code, name, tracks.len()),
            Entry::AnimationRecord { name, bones } => (code, name, bones.len()),
            other => panic!("{other:?} is no filter or record"),
        })
        .collect()
}

/// The offset of the object of the entry of `code`.
fn object(sections: &Rpf9Sections, code: u32) -> usize {
    let v = &sections.virtual_section;
    let count = usize::from(u16::from_le_bytes([v[0x28], v[0x29]]));
    let codes = (u64_at(v, 0x20) & 0x0FFF_FFFF) as usize;
    let at = (0..count)
        .position(|i| u32_at(v, codes + 4 * i) == code)
        .expect("the entry");
    (u64_at(v, (u64_at(v, 0x30) & 0x0FFF_FFFF) as usize + 8 * at) & 0x0FFF_FFFF) as usize
}

#[test]
fn a_copied_entry_takes_its_key_in_order_and_carries_its_source_s_bytes() {
    let (tracks, bones) = (tracks(), bones());
    let mut sections = mapped(filters_and_record(&tracks, &bones));
    let (filter, record) = (name_hash32(b"edit_filter"), name_hash32(b"edit_set"));
    // The filter is the entry after the 0x40-byte root.
    sections.virtual_section[0x4C..0x50].copy_from_slice(&0x1357_u32.to_le_bytes());

    let mut copied = sections.clone();
    let key = copy_dictionary_entry(&mut copied, filter, "copy_filter").expect("copied");
    assert_eq!(key, name_hash32(b"copy_filter"));
    let long = "a_record_named_at_length";
    let long_key = copy_dictionary_entry(&mut copied, record, long).expect("copied");
    let short_key = copy_dictionary_entry(&mut copied, record, "short").expect("copied");
    assert_eq!(
        (long_key, short_key),
        (name_hash32(long.as_bytes()), name_hash32(b"short"))
    );
    let mut wanted = vec![
        (filter, "pack:/edit_filter.iff".to_string(), 3),
        (record, "edit_set".to_string(), 2),
        (key, "pack:/copy_filter.iff".to_string(), 3),
        (long_key, long.to_string(), 2),
        (short_key, "short".to_string(), 2),
    ];
    wanted.sort_unstable();
    assert_eq!(entries(&copied), wanted);

    // The copy's signature and every byte of its elements are its source's.
    let (source, copy) = (object(&copied, filter), object(&copied, key));
    let v = &copied.virtual_section;
    assert_eq!(u32_at(v, copy + 0x0C), 0x1357);
    let elements = |at: usize| (u64_at(v, at + 0x28) & 0x0FFF_FFFF) as usize;
    let bytes = |at: usize| v[elements(at)..elements(at) + 3 * 48].to_vec();
    assert_ne!(elements(source), elements(copy));
    assert_eq!(bytes(source), bytes(copy));

    // The arrays it replaced hold no stray pointer: a growth of the copy goes through.
    let mut grown = copied.clone();
    write_filter_tracks(&mut grown, key, &tracks[..1]).expect("written");
    let counts: Vec<usize> = entries(&grown).iter().map(|e| e.2).collect();
    let at = wanted.iter().position(|e| e.0 == key).expect("the copy");
    assert_eq!(counts[at], 1);

    // A key the dictionary holds, an entry it lacks and a name with a space are refused.
    let mut refused = sections.clone();
    assert!(copy_dictionary_entry(&mut refused, record, "edit_filter").is_err());
    assert!(copy_dictionary_entry(&mut refused, 0xDEAD, "new").is_err());
    assert!(copy_dictionary_entry(&mut refused, filter, "a b").is_err());
    assert_eq!(refused, sections);
}

#[test]
fn a_removed_entry_leaves_the_others_reading_and_a_dictionary_keeps_one() {
    let (tracks, bones) = (tracks(), bones());
    let sections = mapped(filters_and_record(&tracks, &bones));
    let (filter, record) = (name_hash32(b"edit_filter"), name_hash32(b"edit_set"));
    let mut copied = sections.clone();
    let key = copy_dictionary_entry(&mut copied, filter, "copy_filter").expect("copied");

    let mut removed = copied.clone();
    remove_dictionary_entry(&mut removed, filter).expect("removed");
    let codes: Vec<u32> = entries(&removed).iter().map(|e| e.0).collect();
    let mut wanted = vec![record, key];
    wanted.sort_unstable();
    assert_eq!(codes, wanted);
    // What is left still grows.
    write_filter_tracks(&mut removed, key, &tracks[1..]).expect("written");

    let mut one = sections.clone();
    remove_dictionary_entry(&mut one, record).expect("removed");
    assert_eq!(entries(&one).len(), 1);
    assert!(remove_dictionary_entry(&mut one, filter).is_err());
    assert!(remove_dictionary_entry(&mut one, record).is_err());
}

#[test]
fn a_filter_s_key_hashes_its_stem_and_a_record_s_its_name() {
    let key = |name: &str, filter: bool| entry_key(name, filter);
    assert_eq!(
        key("walk_filter", true),
        Some((name_hash32(b"walk_filter"), "pack:/walk_filter.iff".into()))
    );
    assert_eq!(
        key("pack:/f/walk.iff", true),
        Some((name_hash32(b"walk"), "pack:/f/walk.iff".into()))
    );
    assert_eq!(key("dog", false), Some((name_hash32(b"dog"), "dog".into())));
    assert_eq!(key("", false), None);
    assert_eq!(key("pack:/.iff", true), None);
}

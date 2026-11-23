//! Names placed by counting the entries a nested archive's header states.

use super::*;

/// A top archive and a nested one of `entries` entries, the zlib runs `listed` of them, then
/// `between`, the names and a top-level run where no header follows them.
fn counted(entries: u32, listed: usize, between: &[u8]) -> (Layout, u64) {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(entries, TABLE, 2);
    for i in 0..listed {
        l.zlib(Rpf9Content::Xml, &xml(0x700 + 0x40 * i), &[], true);
    }
    l.unclaimed(between);
    let names = l.at();
    l.unclaimed(&noise(NAMES as usize, 109));
    l.zlib(Rpf9Content::Xml, &xml(0x500), &[], true);
    (l, names)
}

/// What the count places over what a scan of `bytes` lists.
fn placed(bytes: &[u8]) -> Vec<Rpf9ScanFind> {
    let mut count = Rpf9NamesByCount::default();
    scan_all(bytes).0.iter().for_each(|find| count.push(find));
    count.place(Cursor::new(bytes)).expect("placed")
}

#[test]
fn names_packed_before_a_top_level_run_are_placed_after_the_entries_the_header_counts() {
    let (l, names) = counted(3, 3, &[]);
    assert_eq!(scan_all(&l.bytes).0, l.finds, "the scan places no names");
    let nested = l.finds[1].offset;
    let want = names_find(names, u64::from(NAMES), nested);
    assert_eq!(placed(&l.bytes), [want]);
    let alone = rpf9_names_by_count_at(Cursor::new(&l.bytes), nested).expect("counted");
    assert_eq!(alone, Some(want));
}

#[test]
fn an_entry_with_no_bytes_leaves_the_names_after_the_entry_before_it() {
    let (l, names) = counted(3, 2, &[]);
    let nested = l.finds[1].offset;
    assert_eq!(
        placed(&l.bytes),
        [names_find(names, u64::from(NAMES), nested)]
    );
}

#[test]
fn names_cut_off_from_the_next_find_by_an_unlisted_body_start_where_the_last_entry_ends() {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(1, TABLE, 2);
    let nested = l.finds[1].offset;
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    let names = l.at();
    l.unclaimed(&noise(NAMES as usize, 109));
    l.page();
    l.unclaimed(&noise(0x880, 137));
    l.zlib(Rpf9Content::Xml, &xml(0x500), &[], true);
    assert_eq!(scan_all(&l.bytes).0, l.finds, "the scan places no names");
    assert_eq!(
        placed(&l.bytes),
        [names_find(names, u64::from(NAMES), nested)]
    );
}

#[test]
fn a_header_whose_entries_hold_a_first_entry_of_no_stated_end_is_not_counted() {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(2, TABLE, 2);
    let entry = noise(0x300, 113);
    let head = <[u8; 4]>::try_from(&entry[..4]).expect("four bytes");
    let find = Rpf9ScanFind {
        offset: l.at(),
        kind: Rpf9BodyKind::FirstEntry { head },
        span: RPF9_SLOT,
        stored_len: None,
        inflated_len: None,
        container: None,
    };
    l.claim(find, &entry[..16]);
    l.unclaimed(&entry[16..]);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    l.unclaimed(&noise(NAMES as usize, 109));
    l.zlib(Rpf9Content::Xml, &xml(0x500), &[], true);
    assert_eq!(scan_all(&l.bytes).0, l.finds);
    assert!(placed(&l.bytes).is_empty());
}

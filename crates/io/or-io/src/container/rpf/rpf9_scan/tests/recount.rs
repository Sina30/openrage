//! Where a nested archive's names are placed, a movie's run named and a stored RSC8 refused.

use super::*;

/// A top archive and a nested one of `entries` entries whose first is the empty-archive
/// placeholder, its names right after it, and the next nested archive on the page after them.
fn placeholder(entries: u32) -> Layout {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(entries, TABLE, 2);
    let nested = l.finds[1].offset;
    let stated = (entries == 1).then_some(RPF9_SLOT);
    let find = Rpf9ScanFind {
        offset: l.at(),
        kind: Rpf9BodyKind::FirstEntry { head: *b"FORC" },
        span: RPF9_SLOT,
        stored_len: stated,
        inflated_len: stated,
        container: None,
    };
    l.claim(find, EMPTY_ARCHIVE_PLACEHOLDER);
    l.names(NAMES as usize, nested);
    l.page();
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    l
}

#[test]
fn an_empty_archive_placeholder_states_its_end_and_its_names_follow_it() {
    // In an archive of two entries the same bytes state no end, and the names are found at the
    // slot after them all the same.
    for entries in [1, 2] {
        let l = placeholder(entries);
        assert_eq!(scan_all(&l.bytes).0, l.finds, "{entries} entries");
        assert_eq!(
            scan_in_slices(&l.bytes, 0x800).0,
            l.finds,
            "{entries} entries"
        );
    }
}

/// A nested archive whose first entry, a stored RSC8, ends at `entry_end`; `gap` zero bytes, the
/// names, placed or not, `pad` zero bytes and the next nested archive.
fn behind_rsc8(entry_end: usize, gap: usize, pad: usize, placed: bool) -> Layout {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(2, TABLE, 2);
    let nested = l.finds[1].offset;
    l.rsc8((entry_end - l.bytes.len() - 16) as u32);
    l.unclaimed(&vec![0; gap]);
    if placed {
        l.names(NAMES as usize, nested);
    } else {
        l.unclaimed(&noise(NAMES as usize, 109));
    }
    l.unclaimed(&vec![0; pad]);
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    l
}

#[test]
fn the_next_header_follows_the_names_at_once_or_at_the_page_strictly_after_their_end() {
    let page = PAGE as usize;
    let names = NAMES as usize;
    for (case, l) in [
        // Names that end on a page, then a whole page of zeros.
        ("a page after", behind_rsc8(2 * page - names, 0, page, true)),
        // An entry that ends on a page, a page of zeros, and the names on the page after it.
        (
            "names a page on",
            behind_rsc8(2 * page, page, page - names, true),
        ),
        // Zero bytes after the names that stop short of a page.
        (
            "short of a page",
            behind_rsc8(2 * page - names, 0, 0x20, false),
        ),
    ] {
        assert_eq!(scan_all(&l.bytes).0, l.finds, "{case}");
    }
}

#[test]
fn names_behind_a_body_of_unstated_end_are_found_at_the_slot_they_start() {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(2, TABLE, 2);
    let nested = l.finds[1].offset;
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
    l.names(NAMES as usize, nested);
    l.page();
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    assert_eq!(scan_all(&l.bytes).0, l.finds);
    assert_eq!(scan_in_slices(&l.bytes, 0x800).0, l.finds);
}

#[test]
fn names_behind_a_short_run_of_unstated_end_and_unclaimed_bytes_are_found_at_their_slot() {
    let mut l = Layout::default();
    l.archive(2, TABLE, 1);
    l.page();
    l.archive(2, TABLE, 2);
    let nested = l.finds[1].offset;
    // One short chunk states no end, and bytes no body claims follow it before the names.
    l.zlib(Rpf9Content::Other, &noise(3000, 11), &[], false);
    l.unclaimed(&noise(0x200, 131));
    l.names(NAMES as usize, nested);
    l.page();
    l.archive(2, TABLE, 3);
    l.zlib(Rpf9Content::Xml, &xml(0x700), &[], true);
    assert_eq!(scan_all(&l.bytes).0, l.finds);
    assert_eq!(scan_in_slices(&l.bytes, 0x800).0, l.finds);
}

#[test]
fn a_movie_of_several_chunks_is_named_by_the_whole_run_s_length() {
    let len = PIECE as usize + 0x500;
    let movie = |stated: u32| {
        let mut body = noise(len, 127);
        body[..4].copy_from_slice(b"GFX\x0A");
        LE::write_u32(&mut body[4..], stated);
        body
    };
    let gfx = Rpf9Content::Identified(crate::gta6::other_bodies::OtherBody::Gfx);
    for (stated, content) in [(len as u32, gfx), (0x123, Rpf9Content::Other)] {
        let mut l = Layout::default();
        l.zlib(content, &movie(stated), &[], false);
        assert_eq!(scan_all(&l.bytes).0, l.finds, "{stated:#X} stated");
    }
}

#[test]
fn a_stored_rsc8_of_config_1_read_at_its_offset_alone_is_refused_by_name() {
    let mut l = Layout::default();
    l.unclaimed(&[0; 16]);
    let at = l.at();
    l.rsc8(0x100);
    l.bytes[at as usize + 7] = 1;
    assert!(matches!(
        read_rpf9_body(Cursor::new(&l.bytes), at, u64::MAX),
        Err(Rpf9ScanError::ConfigOneOutsideContainer { offset }) if offset == at
    ));
}

#[test]
fn the_placeholder_read_at_its_offset_alone_is_the_row_the_scan_lists_and_extracts_as_itself() {
    let l = placeholder(1);
    let first = |f: &&Rpf9ScanFind| matches!(f.kind, Rpf9BodyKind::FirstEntry { .. });
    let want = *l.finds.iter().find(first).expect("listed");
    let read = |bytes: &[u8], at| read_rpf9_body(Cursor::new(bytes), at, u64::MAX);
    let (find, bytes) = read(&l.bytes, want.offset).expect("read");
    assert_eq!(find, want);
    assert_eq!(bytes, EMPTY_ARCHIVE_PLACEHOLDER.to_vec());
    let extracted = extract_rpf9_body(Cursor::new(&l.bytes), want.offset, u64::MAX);
    let (_, file) = extracted.expect("extracted");
    assert_eq!(file, bytes);
    let (loose, _) = read(&file, 0).expect("read loose");
    assert_eq!(loose, Rpf9ScanFind { offset: 0, ..want });
    let off_slot = read(&l.bytes, want.offset + 8);
    assert!(!off_slot.is_ok_and(|(f, _)| first(&&f)), "off a slot");
}

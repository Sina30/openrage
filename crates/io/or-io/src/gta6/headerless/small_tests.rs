use super::super::{read, read_dictionary, Resource};
use super::*;
use crate::container::rpf::{Rpf9Role, Rpf9Signature};

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn word(v: &mut [u8], at: usize, w: u64) {
    put(v, at, &w.to_le_bytes());
}

fn half(v: &mut [u8], at: usize, w: u32) {
    put(v, at, &w.to_le_bytes());
}

fn ptr(v: &mut [u8], at: usize, to: usize) {
    word(v, at, 0x5000_0000 + to as u64);
}

fn signature(v: &[u8]) -> Rpf9Signature {
    Rpf9Signature::of(v, v.len() as u64).expect("a signature")
}

fn is_layout<T>(read: Result<T>) -> bool {
    matches!(read, Err(HeaderlessError::Layout { .. }))
}

const KEY: u64 = 0x0F1E_2D3C_4B5A_6978;
const WORDS: [u32; 6] = [0x11, 0x2345, 0x67, 0x70, 0x1A0, 0x1B0];

/// A descriptor whose stream is `len` bytes and whose u16 pair is 7 and `second`: its root, the
/// stream, the block map and 16 zero bytes.
fn descriptor(len: u32, second: u16) -> Vec<u8> {
    let map = (0x60 + len as usize).next_multiple_of(16);
    let mut v = vec![0u8; map + 0x20];
    ptr(&mut v, 0x08, map);
    half(&mut v, 0x10, len);
    half(&mut v, 0x14, len);
    ptr(&mut v, 0x18, 0x60);
    for (k, w) in WORDS.iter().enumerate() {
        half(&mut v, 0x30 + 4 * k, *w);
    }
    word(&mut v, 0x48, KEY);
    put(&mut v, 0x50, &7u16.to_le_bytes());
    put(&mut v, 0x52, &second.to_le_bytes());
    for (i, b) in v[0x60..0x60 + len as usize].iter_mut().enumerate() {
        *b = (i % 251) as u8 + 1;
    }
    v[map + 8] = 1;
    v
}

#[test]
fn a_descriptor_reads_its_key_words_and_stream_and_refuses_a_field_off_its_rules() {
    let v = descriptor(76, 8);
    let s = signature(&v);
    assert_eq!(s.to_string(), "0PhP00hh");
    assert_eq!(s.role(), Some(Rpf9Role::KeyedBlobDescriptor));
    let Ok(Resource::KeyedBlobDescriptor(d)) = read(s, &v, Some(&[])) else {
        panic!("{s} reads as a keyed-blob descriptor");
    };
    assert_eq!(d.key, KEY);
    assert_eq!(d.words, WORDS);
    assert_eq!(d.pair, [7, 8]);
    assert_eq!(d.stream, v[0x60..0x60 + 76]);

    assert!(read_keyed_blob_descriptor(&descriptor(28, 7), None).is_ok());
    // The pair 7 and 8 is held only over a stream of 76 bytes.
    assert!(is_layout(read_keyed_blob_descriptor(
        &descriptor(28, 8),
        None
    )));
    let refused = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut v = descriptor(28, 7);
        edit(&mut v);
        is_layout(read_keyed_blob_descriptor(&v, None))
    };
    assert!(refused(&|v| half(v, 0x44, 0x190)), "0x44 below 0x40");
    assert!(refused(&|v| half(v, 0x44, 0x1A8)), "0x44 not 16-aligned");
    assert!(
        refused(&|v| half(v, 0x3C, 0x80)),
        "0x3C not 0x38 rounded up"
    );
    assert!(refused(&|v| word(v, 0x48, 0)), "no key");
    assert!(
        refused(&|v| half(v, 0x14, 29)),
        "a capacity other than the length"
    );
    assert!(
        refused(&|v| put(v, 0x50, &6u16.to_le_bytes())),
        "a pair not 7 first"
    );
    assert!(refused(&|v| v[0x25] = 1), "a byte set at 0x20 to 0x2F");
    assert!(refused(&|v| v[0x5F] = 1), "a byte set at 0x54 to 0x5F");
    assert!(
        refused(&|v| v.extend([0; 16])),
        "a section past the block map's 32 bytes"
    );
    let v = descriptor(28, 7);
    assert!(is_layout(read_keyed_blob_descriptor(&v, Some(&[0; 16]))));
}

/// A key placed in a table of two groups by its `home` group and its control byte `tag`.
const fn key(high: u64, home: u64, tag: u64) -> u64 {
    (high << 8) | (home << 7) | tag
}

const ASSETS: [u64; 3] = [
    key(0x0000_A1B2_C3D4_E5F6, 0, 0x11),
    key(0x0000_1728_394A_5B6C, 1, 0x22),
    key(0x0000_3344_5566_7788, 0, 0x33),
];
const GROUP_KEY: u64 = key(0x0000_9988_7766_5544, 1, 0x44);

/// Key `k` in slot `slot` of group `group` of the table at `table` of `size`-byte slots; where its
/// slot lies.
fn put_slot(v: &mut [u8], table: usize, size: usize, group: usize, slot: usize, k: u64) -> usize {
    let g = table + (16 + 16 * size) * group;
    v[g + slot] = (k & 0x7F) as u8;
    let at = g + 16 + size * slot;
    word(v, at, k);
    at
}

/// An asset index of [`ASSETS`] in table 0 at 0x60 and [`GROUP_KEY`] in table 1 at 0x480, its
/// list at 0x7A0, the value records at 0x7B0 to 0x7DF and the block map at 0x7E0.
fn asset_index() -> Vec<u8> {
    let mut v = vec![0u8; 0x7F0];
    ptr(&mut v, 0x08, 0x7E0);
    v[0x7E8] = 1;
    half(&mut v, 0x10, 2);
    half(&mut v, 0x14, 3);
    ptr(&mut v, 0x18, 0x60);
    half(&mut v, 0x20, 2);
    half(&mut v, 0x24, 1);
    ptr(&mut v, 0x28, 0x480);
    for g in 0..2 {
        v[0x60 + 0x210 * g..][..16].fill(EMPTY);
        v[0x480 + 0x190 * g..][..16].fill(EMPTY);
    }
    let placed = [(ASSETS[0], 0, 0), (ASSETS[1], 1, 3), (ASSETS[2], 0, 5)];
    for (i, (k, group, slot)) in placed.into_iter().enumerate() {
        let at = put_slot(&mut v, 0x60, ASSET_SLOT, group, slot, k);
        ptr(&mut v, at + 0x18, 0x7B0 + 0x10 * i);
        v[0x7B0 + 0x10 * i] = 0x5A ^ i as u8;
    }
    let at = put_slot(&mut v, 0x480, GROUP_SLOT, 1, 7, GROUP_KEY);
    half(&mut v, at + 0x08, 2);
    half(&mut v, at + 0x0C, 2);
    ptr(&mut v, at + 0x10, 0x7A0);
    word(&mut v, 0x7A0, ASSETS[2]);
    word(&mut v, 0x7A8, ASSETS[0]);
    v
}

#[test]
fn an_asset_index_reads_each_table_s_keys_and_lists_and_refuses_a_slot_off_its_rules() {
    let v = asset_index();
    let s = signature(&v);
    assert_eq!(s.to_string(), "0PhPhP00");
    assert_eq!(s.role(), Some(Rpf9Role::AssetIndex));
    let Ok(Resource::AssetIndex(index)) = read(s, &v, Some(&[])) else {
        panic!("{s} reads as an asset index");
    };
    assert_eq!(
        index.assets,
        [(ASSETS[0], 0x7B0), (ASSETS[2], 0x7D0), (ASSETS[1], 0x7C0)]
    );
    assert_eq!(index.groups[0], [(GROUP_KEY, vec![ASSETS[2], ASSETS[0]])]);
    assert!(index.groups[1..].iter().all(Vec::is_empty));

    // With table 1 unused, its bytes are not read.
    let mut only = v.clone();
    only[0x20..0x30].fill(0);
    let s = signature(&only);
    assert_eq!(s.to_string(), "0PhP0000");
    let Ok(Resource::AssetIndex(index)) = read(s, &only, Some(&[])) else {
        panic!("{s} reads as an asset index");
    };
    assert_eq!(index.assets.len(), 3);
    assert!(index.groups.iter().all(Vec::is_empty));

    // Asset 0's slot is at 0x70; table 1's slot of the group key at 0x6C8.
    let refused = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut v = asset_index();
        edit(&mut v);
        is_layout(read_asset_index(&v, None))
    };
    assert!(refused(&|v| v[0x60] ^= 1), "a control byte off its key");
    assert!(
        refused(&|v| v[0x60 + 16 + 0x20] = 1),
        "an empty slot not zero"
    );
    assert!(
        refused(&|v| half(v, 0x10, 4)),
        "a group count off its key count"
    );
    assert!(
        refused(&|v| half(v, 0x14, 2)),
        "a key count off the full slots"
    );
    assert!(refused(&|v| ptr(v, 0x18, 0x70)), "table 0 not at 0x60");
    assert!(
        refused(&|v| ptr(v, 0x88, 0x7B8)),
        "a value record unaligned"
    );
    assert!(
        refused(&|v| ptr(v, 0x88, 0x100)),
        "a value record in table 0"
    );
    assert!(
        refused(&|v| ptr(v, 0x88, 0x7E0)),
        "a value record on the block map"
    );
    assert!(
        refused(&|v| ptr(v, 0x88, 0x7C0)),
        "a value record two slots share"
    );
    assert!(
        refused(&|v| word(v, 0x7A8, GROUP_KEY)),
        "a list naming no asset"
    );
    assert!(
        refused(&|v| half(v, 0x6C8 + 0x0C, 3)),
        "a list's capacity off its count"
    );
    assert!(refused(&|v| v[0x7E9] = 1), "a physical block");
}

#[test]
fn a_key_sits_in_its_home_group_or_past_full_groups_only() {
    // Seventeen keys of home 0 in two groups: sixteen fill group 0 and one is in group 1.
    let mut v = vec![0u8; 0x60 + 2 * 0x210];
    half(&mut v, 0x10, 2);
    half(&mut v, 0x14, 17);
    ptr(&mut v, 0x18, 0x60);
    v[0x60 + 0x210..][..16].fill(EMPTY);
    for s in 0..16 {
        put_slot(
            &mut v,
            0x60,
            ASSET_SLOT,
            0,
            s,
            key(0x10 + s as u64, 0, s as u64 + 1),
        );
    }
    put_slot(&mut v, 0x60, ASSET_SLOT, 1, 4, key(0x77, 0, 0x55));
    let probed = sections(&v, None, |b, _| table(b, 0x10, ASSET_SLOT)).expect("a table");
    let (slots, span) = probed.expect("in use");
    assert_eq!((slots.len(), span), (17, (0x60, 0x60 + 2 * 0x210)));

    // With room in group 0, the key in group 1 lies past it.
    v[0x60 + 15] = EMPTY;
    v[0x60 + 16 + ASSET_SLOT * 15..][..ASSET_SLOT].fill(0);
    half(&mut v, 0x14, 16);
    let past = sections(&v, None, |b, _| table(b, 0x10, ASSET_SLOT));
    assert!(is_layout(past));
    assert_eq!(group_count(28), 2);
    assert_eq!(group_count(29), 4);
    assert_eq!(group_count(0), 2);
}

/// A neighbour table of `elements` in its raw buffer at 0x20, and its physical section.
fn neighbours(elements: &[[u16; 4]]) -> (Vec<u8>, Vec<u8>) {
    let mut v = vec![0u8; 0xA0];
    ptr(&mut v, 0x08, 0x80);
    (v[0x88], v[0x89]) = (1, 1);
    ptr(&mut v, 0x10, 0x20);
    half(&mut v, 0x28, 2 * elements.len() as u32);
    put(&mut v, 0x2C, &4u16.to_le_bytes());
    put(&mut v, 0x2E, &9u16.to_le_bytes());
    half(&mut v, 0x30, 0x1601);
    word(&mut v, 0x38, 0x6000_0000);
    ptr(&mut v, 0x48, 0x60);
    let mut p: Vec<u8> = (elements.iter().flatten())
        .flat_map(|n| n.to_le_bytes())
        .collect();
    p.resize(p.len().next_multiple_of(16), 0);
    (v, p)
}

const ELEMENTS: [[u16; 4]; 3] = [[1, 2, 1, 2], [2, 0, 0, 2], [0, 1, 1, 0]];

#[test]
fn a_neighbour_table_reads_its_elements_and_refuses_one_that_names_itself_or_none() {
    let (v, p) = neighbours(&ELEMENTS);
    let s = signature(&v);
    assert_eq!(s.to_string(), "0PP00hhG");
    assert_eq!(s.role(), Some(Rpf9Role::NeighbourTable));
    let Ok(Resource::NeighbourTable(table)) = read(s, &v, Some(&p)) else {
        panic!("{s} reads as a neighbour table");
    };
    assert_eq!(table.elements, ELEMENTS);

    let (v, p) = neighbours(&[[1, 2, 1, 2], [1, 0, 0, 2], [0, 1, 1, 0]]);
    assert!(
        is_layout(read_neighbour_table(&v, Some(&p))),
        "element 1 names itself"
    );
    let (v, p) = neighbours(&[[1, 2, 1, 3], [2, 0, 0, 2], [0, 1, 1, 0]]);
    assert!(matches!(
        read_neighbour_table(&v, Some(&p)),
        Err(HeaderlessError::IndexPastCount {
            index: 3,
            count: 3,
            ..
        })
    ));
    let (mut v, p) = neighbours(&ELEMENTS);
    half(&mut v, 0x28, 5);
    assert!(
        is_layout(read_neighbour_table(&v, Some(&p))),
        "an odd word count"
    );
    let (v, mut p) = neighbours(&ELEMENTS);
    p.extend([0; 16]);
    assert!(
        is_layout(read_neighbour_table(&v, Some(&p))),
        "a longer physical section"
    );
    let (mut v, p) = neighbours(&ELEMENTS);
    word(&mut v, 0x60, 7);
    assert!(matches!(
        read_neighbour_table(&v, Some(&p)),
        Err(HeaderlessError::Constant { at: 0x60, .. })
    ));
}

#[test]
fn an_empty_dictionary_reads_as_one_of_no_entries_and_refuses_an_array_named_for_none() {
    let mut v = vec![0u8; 0x60];
    ptr(&mut v, 0x08, 0x40);
    v[0x48] = 1;
    half(&mut v, 0x18, 1);
    let s = signature(&v);
    assert_eq!(s.to_string(), "0P0h0000");
    assert_eq!(s.role(), Some(Rpf9Role::Dictionary));
    let Ok(Resource::Dictionary(d)) = read(s, &v, Some(&[])) else {
        panic!("{s} reads as a dictionary");
    };
    assert!(d.entries.is_empty());
    for (at, value) in [
        (0x10usize, 0x5000_0040u64),
        (0x18, 2),
        (0x20, 0x5000_0050),
        (0x30, 0x5000_0050),
    ] {
        let mut broken = v.clone();
        word(&mut broken, at, value);
        assert!(read_dictionary(&broken, None).is_err(), "{at:#X}");
    }
}

#[test]
fn a_drawable_with_no_skeleton_sphere_or_box_is_placed_as_a_drawable_and_a_morph_target_is_not() {
    let mut v = vec![0u8; 0x100];
    ptr(&mut v, 0x08, 0xF0);
    ptr(&mut v, 0x10, 0xE0);
    let s = signature(&v);
    assert_eq!(s.to_string(), "0PP00000");
    assert_eq!(s.role(), None, "no level and no name");
    ptr(&mut v, 0x50, 0xC0);
    ptr(&mut v, 0xA8, 0xD0);
    assert_eq!(signature(&v).role(), Some(Rpf9Role::Drawable));
    ptr(&mut v, 0x10, 0x70);
    half(&mut v, 0x80, 0x1601);
    assert_eq!(signature(&v).role(), Some(Rpf9Role::MorphTarget));
}

/// A body of `len` bytes whose offset table holds `offsets`.
fn offset_table(offsets: &[u64], len: usize) -> Vec<u8> {
    let mut body = vec![0u8; len];
    half(&mut body, 0x14, offsets.len() as u32);
    for (i, o) in offsets.iter().enumerate() {
        word(&mut body, 0x28 + 8 * i, *o);
    }
    body
}

#[test]
fn an_offset_table_reads_its_items_offsets_and_refuses_ones_out_of_order_or_past_the_body() {
    let table = read_offset_table(&offset_table(&[0x40, 0x60, 0x90], 0xA0)).expect("a table");
    assert_eq!(table.offsets, [0x40, 0x60, 0x90]);
    assert_eq!(table.len, 0xA0);
    for offsets in [
        &[0x50, 0x60, 0x90][..],
        &[0x40, 0x90, 0x60][..],
        &[0x40, 0x68, 0x90][..],
        &[0x40, 0x60, 0xA0][..],
        &[][..],
    ] {
        let body = offset_table(offsets, 0xA0);
        assert!(is_layout(read_offset_table(&body)), "{offsets:X?}");
    }
    let mut stray = offset_table(&[0x40, 0x60, 0x90], 0xA0);
    stray[0x20] = 1;
    assert!(is_layout(read_offset_table(&stray)));
}

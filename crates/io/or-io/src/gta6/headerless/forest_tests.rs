use super::*;

const TAG: u64 = (OBJECT_POINTER_TAG as u64) << 32;

/// Five items: root 0 over 1 over 2, then root 3 over 4.
const AT: [usize; 5] = [0x50, 0x70, 0xA0, 0xC0, 0xE0];
const LEN: usize = 0x100;
const KEYS: [u32; 2] = [0x1357_9BDF, 0x2468_ACE0];
const TYPES: [u32; 5] = [
    0xA1A1_0001,
    0xB2B2_0002,
    0xC3C3_0003,
    0xA1A1_0001,
    0xD4D4_0004,
];

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn word(v: &mut [u8], at: usize, w: u64) {
    put(v, at, &w.to_le_bytes());
}

fn half(v: &mut [u8], at: usize, w: u32) {
    put(v, at, &w.to_le_bytes());
}

/// The pointer word to item `to`.
fn to(item: usize) -> u64 {
    TAG | (AT[item] as u64 + 8)
}

fn body() -> Vec<u8> {
    let mut v = vec![0u8; LEN];
    half(&mut v, 0x14, AT.len() as u32);
    for (i, at) in AT.iter().enumerate() {
        word(&mut v, 0x28 + 8 * i, *at as u64);
        half(&mut v, at + 8, TYPES[i]);
    }
    half(&mut v, AT[0], KEYS[0]);
    half(&mut v, AT[0] + 4, 0xFFFF_0000);
    word(&mut v, AT[0] + 0x10, to(1));
    word(&mut v, AT[1] + 0x18, to(2));
    word(&mut v, AT[1] + 0x20, 0x1234_5678);
    half(&mut v, AT[3], KEYS[1]);
    half(&mut v, AT[3] + 4, 0x0001_0000);
    word(&mut v, AT[3] + 0x10, to(4));
    v
}

fn rule(body: &[u8]) -> &'static str {
    match read_object_trees(body) {
        Err(HeaderlessError::Layout { why, .. }) => &why[..2],
        other => panic!("not refused by a rule: {other:?}"),
    }
}

#[test]
fn an_object_tree_table_reads_its_roots_children_and_types() {
    let trees = read_object_trees(&body()).expect("the trees");
    assert_eq!(trees.roots, [0, 3]);
    let children: Vec<&[usize]> = trees.items.iter().map(|i| &i.children[..]).collect();
    assert_eq!(children, [&[1][..], &[2], &[], &[4], &[]]);
    let types: Vec<u32> = trees.items.iter().map(|i| i.type_tag).collect();
    assert_eq!(types, TYPES);
    let sizes: Vec<u64> = trees.items.iter().map(|i| i.size).collect();
    let ends = AT.iter().skip(1).copied().chain([LEN]);
    let expected: Vec<u64> = AT.iter().zip(ends).map(|(a, z)| (z - a) as u64).collect();
    assert_eq!(sizes, expected);
    assert_eq!((trees.items[0].key, trees.items[3].key), (KEYS[0], KEYS[1]));
    assert_eq!(
        (trees.items[0].aux, trees.items[3].aux),
        (0xFFFF_0000, 0x0001_0000)
    );
    let per_tree: Vec<Vec<u64>> = (trees.trees())
        .map(|t| t.iter().map(|i| i.offset).collect())
        .collect();
    let at = AT.map(|a| a as u64);
    assert_eq!(per_tree, [at[..3].to_vec(), at[3..].to_vec()]);
}

#[test]
fn a_scan_names_the_table_by_its_head() {
    use crate::gta6::other_bodies::OtherBody;
    let v = body();
    assert_eq!(OtherBody::of(&v, LEN as u64), Some(OtherBody::ObjectTrees));
    let mut moved = body();
    word(&mut moved, 0x28, AT[0] as u64 + 16);
    assert_eq!(OtherBody::of(&moved, LEN as u64), None);
    let mut stray = body();
    stray[0x20] = 1;
    assert_eq!(OtherBody::of(&stray, LEN as u64), None);
    assert_eq!(OtherBody::of(&v[..0x2F], 0x2F), None);
}

#[test]
fn each_broken_rule_refuses_the_table_by_its_name() {
    let mut short = body();
    short.truncate(LEN - 8);
    assert_eq!(rule(&short), "T3");

    let mut v = body();
    half(&mut v, AT[1] + 0x0C, 1);
    assert_eq!(rule(&v), "T4");

    let mut v = body();
    word(&mut v, AT[2] + 0x10, u64::from(OBJECT_POINTER_TAG));
    assert_eq!(rule(&v), "T5");

    let mut v = body();
    word(&mut v, AT[2] + 0x10, TAG | (AT[2] as u64 + 0x10));
    assert_eq!(rule(&v), "T5");

    let mut v = body();
    word(&mut v, AT[2] + 0x10, to(1));
    assert_eq!(rule(&v), "T6");

    let mut v = body();
    word(&mut v, AT[1] + 0x20, to(2));
    assert_eq!(rule(&v), "T6");

    let mut v = body();
    half(&mut v, AT[4], 0x0BAD_0001);
    assert_eq!(rule(&v), "T7");

    let mut v = body();
    half(&mut v, AT[3], 0);
    half(&mut v, AT[3] + 4, 0);
    word(&mut v, AT[3] + 0x10, 0);
    word(&mut v, AT[1] + 0x20, to(4));
    // Item 3 is now neither keyed nor pointed at.
    assert_eq!(rule(&v), "T7");

    let mut v = body();
    half(&mut v, AT[4] + 4, 1);
    assert_eq!(rule(&v), "T7");

    let mut v = body();
    word(&mut v, AT[3] + 0x10, 0);
    word(&mut v, AT[1] + 0x20, to(4));
    assert_eq!(rule(&v), "T8");

    let mut v = body();
    half(&mut v, AT[3], KEYS[0]);
    assert_eq!(rule(&v), "T8");
}

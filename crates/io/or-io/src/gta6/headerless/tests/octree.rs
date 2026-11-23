use super::*;
use crate::gta6::hash::fmix64;

const CELL: u32 = 160 + 512 * 179;

/// An octree of one group: the root, an open leaf and a surface leaf under water, by `keys` and
/// `words` in node order; the offset of its group.
fn octree(keys: [u64; 3], words: [u32; 3]) -> (Image, usize) {
    let mut img = Image::default();
    let root = img.alloc(0x60);
    let group = img.alloc(0x110);
    let array = img.alloc(12);
    let map = img.alloc(16);
    img.bytes[map + 8] = 1;
    img.ptr(root + 0x08, map);
    img.u32(root + 0x10, 1);
    img.u32(root + 0x14, 3);
    img.ptr(root + 0x18, group);
    img.ptr(root + 0x20, array);
    for at in [0x28, 0x2C, 0x30] {
        img.u32(root + at, 128);
    }
    img.u32(root + 0x34, CELL);
    img.vec3(root + 0x40, [-12288.0, -9856.0, 0.0]);
    img.u64(root + 0x50, 3);
    img.bytes[group..group + 16].fill(0x80);
    for (node, key) in keys.iter().enumerate() {
        img.bytes[group + node] = (fmix64(*key) & 0x7F) as u8;
        img.u64(group + 16 + 16 * node, *key);
        img.u64(group + 24 + 16 * node, node as u64);
        img.u32(array + 4 * node, words[node]);
    }
    (img, group)
}

const KEYS: [u64; 3] = [1, 8, 9];
const WORDS: [u32; 3] = [0xFF00, 0x0001, 0x0103];

#[test]
fn an_octree_reads_through_its_role_to_its_nodes_by_kind() {
    let (img, _) = octree(KEYS, WORDS);
    let signature = Rpf9Signature::of(&img.bytes, img.bytes.len() as u64).expect("a signature");
    assert_eq!(signature.role(), Some(Rpf9Role::NavOctree), "{signature}");
    let Ok(Resource::NavOctree(tree)) = read(signature, &img.bytes, Some(&[])) else {
        panic!("an octree");
    };
    assert_eq!(
        (tree.cell, tree.edge, tree.groups, tree.nodes),
        (CELL, 128, 1, 3)
    );
    assert_eq!(
        (
            tree.depth,
            tree.internal,
            tree.placeholder,
            tree.open,
            tree.surface
        ),
        (1, 1, 0, 1, 1)
    );
}

#[test]
fn a_slot_whose_tag_is_not_its_key_s_hash_is_refused() {
    let (mut img, group) = octree(KEYS, WORDS);
    img.bytes[group + 1] ^= 1;
    assert!(matches!(
        read_octree(&img.bytes),
        Err(HeaderlessError::HashTag { key: 8, .. })
    ));
}

#[test]
fn a_key_that_is_not_a_location_code_or_has_no_parent_is_refused() {
    for bad in [2, 64] {
        let (img, _) = octree([1, bad, 9], WORDS);
        assert!(matches!(
            read_octree(&img.bytes),
            Err(HeaderlessError::OctreeKey { key }) if key == bad
        ));
    }
}

#[test]
fn a_leaf_of_no_measured_class_is_refused() {
    let (img, _) = octree(KEYS, [0xFF00, 0x0002, 0x0103]);
    assert!(matches!(
        read_octree(&img.bytes),
        Err(HeaderlessError::NodeWord { node: 1, word: 2 })
    ));
}

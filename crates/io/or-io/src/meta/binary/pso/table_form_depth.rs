//! A table-form tree nested twice the old fixed cap reads whole, one nested past what a save
//! writes is refused, and one that contains itself is refused.

use super::table_form::tests::{body, member, open, pointer, Class};
use super::table_layout::DEPTH;
use crate::meta::binary::hash_index::default_hash_index;
use crate::meta::instance::value::{MetaInstance, Value};

const NODE: u32 = 0x1111_0010;
const NEXT: u32 = 0x2222_0100;
const POINTER64: u8 = 0x07;

/// `depth` blocks of one 8-byte structure, each pointing at the next and the last holding `last`.
fn chain(depth: u64, last: u64) -> Vec<u8> {
    let node = Class {
        name: NODE,
        size: 8,
        members: vec![member(NEXT, 0, POINTER64, 0, 0)],
    };
    let blocks: Vec<(u32, Vec<u8>)> = (1..=depth)
        .map(|id| {
            let next = if id == depth {
                last
            } else {
                pointer(id + 1, 0)
            };
            (NODE, next.to_le_bytes().to_vec())
        })
        .collect();
    body(&[node], &blocks, 1)
}

#[test]
fn a_tree_nested_as_deep_as_a_save_writes_reads_whole_and_one_deeper_is_refused() {
    let depth = DEPTH as u64;
    let (instance, _) = open(&chain(depth, 0)).expect("it reads");
    let index = default_hash_index();
    let node = |next: Value| {
        let mut node = MetaInstance::new(index.name_or_literal(NODE));
        node.fields.push((index.name_or_literal(NEXT), next));
        node
    };
    let mut value = Value::Pointer(None);
    for _ in 1..depth {
        value = Value::Pointer(Some(Box::new(node(value))));
    }
    assert_eq!(instance, node(value));

    // Some 7,000 levels fit the budget of a 56 KB block; dropping them would overflow the stack.
    let refused = open(&chain(depth + 1, 0))
        .expect_err("it nests too deep")
        .to_string();
    assert!(refused.contains("nests deeper than 64"), "{refused}");
}

#[test]
fn a_tree_whose_last_structure_points_back_at_the_first_is_refused() {
    let refused = open(&chain(40, pointer(1, 0)))
        .expect_err("it contains itself")
        .to_string();
    assert!(refused.contains("contains itself"), "{refused}");
}

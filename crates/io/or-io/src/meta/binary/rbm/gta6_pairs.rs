//! A GTA VI headerless RBM tree whose maps gained or lost pairs, such as an anim scene's entity
//! or scene: the body is laid out anew, each table rebuilt from its pairs with an added one last
//! and every region after it moved, where the unchanged tree lays out anew as the body stores it.
//!
//! ⚠ Where a body's blocks do not lay out again as stored, where a new pair's regions go and
//! which order its blocks take are not known, so a pair is neither added nor removed there.
//! A table is refilled in the order its pairs are read, slot order from slot 0; a table whose
//! probe chain wraps was filled from the slot after its first empty slot (a slot map) or after
//! its first group that is not full (a group map), so it comes back valid but differs from the
//! stored table inside its own bytes.

use crate::meta::binary::layout::RbmLayout;
use crate::meta::binary::pso::table_form;
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;
use crate::meta::instance::MetaInstance;

/// Whether `instance` adds a pair to or takes one from a map of the tree `layout`'s body stores.
pub(super) fn pairs_changed(layout: &RbmLayout, instance: &MetaInstance) -> Result<bool> {
    let stored = table_form::decode(&layout.model)?;
    if !instance_keys_differ(&stored, instance) {
        return Ok(false);
    }
    let split = layout.virtual_page_size.min(layout.decompressed.len());
    let (v, p) = layout.decompressed.split_at(split);
    let again = super::gta6_relayout::write(layout, &stored)?;
    if again.0 != v || again.1 != p {
        return Err(MetaError::Other(
            "GTA VI RBM: a map's pairs changed, and this body's blocks do not lay out again as it stores them, so where an added or removed pair's regions go is not known".into(),
        ));
    }
    Ok(true)
}

fn instance_keys_differ(stored: &MetaInstance, edited: &MetaInstance) -> bool {
    (stored.fields.iter()).any(|(name, s)| edited.get(name).is_some_and(|e| keys_differ(s, e)))
}

/// Whether a map in `stored` holds other keys than the same map in `edited`.
// ponytail: a map inside an array whose length changed is not compared; match the elements as
// the array grower does if such a map's pairs must change in the same save.
fn keys_differ(stored: &Value, edited: &Value) -> bool {
    match (stored, edited) {
        (Value::Struct(s), Value::Struct(e)) => instance_keys_differ(s, e),
        (Value::Pointer(Some(s)), Value::Pointer(Some(e))) => instance_keys_differ(s, e),
        (Value::Array(s), Value::Array(e)) => {
            s.len() == e.len() && s.iter().zip(e).any(|(s, e)| keys_differ(s, e))
        }
        (Value::Map(s), Value::Map(e)) => {
            !(s.iter().map(|(k, _)| k)).eq(e.iter().map(|(k, _)| k))
                || (s.iter().zip(e)).any(|((_, s), (_, e))| keys_differ(s, e))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::gta6::{open, read_model};
    use super::super::gta6_grow::tests::{body, member, pointer};
    use super::super::gta6_grow_tree::write_grown;
    use super::*;
    use crate::container::rpf::Rpf9Sections;
    use crate::meta::binary::hash_index::default_hash_index;
    use crate::meta::binary::layout::{ContainerLayout, MetaLayout};
    use crate::meta::instance::value::HashedString;

    const ROOT: u32 = 0x4848_0001;
    const PAIR: u32 = 0x4848_0002;
    const SCENES: u32 = 0x4848_0010;
    const ARRAY_INFO: u32 = 0x100;
    const SLOTS: usize = 32;
    /// Keys of home slots 5 and 19, and one more of home slot 5.
    const KEYS: [u32; 3] = [0x1234_0005, 0x1234_0013, 0x1234_0025];

    fn name(hash: u32) -> String {
        default_hash_index().name_or_literal(hash)
    }

    /// A root whose one member is a slot map of `{hash, u32}` pairs holding the first two keys.
    fn slot_map() -> Rpf9Sections {
        let root = (
            ROOT,
            0x10,
            vec![
                member(SCENES, 0, 0x73, 0, 2 << 16 | 1),
                member(ARRAY_INFO, 0, 0x4A, 0, 0),
                member(ARRAY_INFO, 0, 0x15, 0, 0),
            ],
        );
        let pair = (
            PAIR,
            8,
            vec![
                member(0x4848_0020, 0, 0x4A, 0, 0),
                member(0x4848_0021, 4, 0x15, 0, 0),
            ],
        );
        let mut head = vec![0u8; 0x10];
        head[..8].copy_from_slice(&[SLOTS as u8, 0, 0, 0, 2, 0, 0, 0]);
        head[8..].copy_from_slice(&pointer(2, 0).to_le_bytes());
        let mut table = vec![0u8; 8 * SLOTS];
        for (key, value) in [(KEYS[0], 50u32), (KEYS[1], 60)] {
            let at = 8 * (key as usize % SLOTS);
            table[at..at + 4].copy_from_slice(&key.to_le_bytes());
            table[at + 4..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        body(&[root, pair], &[(ROOT, head), (PAIR, table)], 1)
    }

    fn opened(v: &[u8], p: &[u8]) -> (MetaInstance, RbmLayout) {
        let (instance, MetaLayout { container, .. }) = open(v, p).expect("it opens");
        let ContainerLayout::Rbm(layout) = container else {
            panic!("a headerless RBM layout");
        };
        (instance, *layout)
    }

    fn pairs(tree: &mut MetaInstance) -> &mut Vec<(Value, Value)> {
        match tree.get_mut(&name(SCENES)) {
            Some(Value::Map(pairs)) => pairs,
            _ => panic!("a map"),
        }
    }

    /// Each pair's key hash and value, by key.
    fn held(tree: &mut MetaInstance) -> Vec<(u32, u64)> {
        let mut held: Vec<(u32, u64)> = (pairs(tree).iter())
            .map(|pair| match pair {
                (Value::Hash(h), Value::UInt(v)) => (h.hash, *v),
                other => panic!("a hash and a u32: {other:?}"),
            })
            .collect();
        held.sort_unstable();
        held
    }

    /// The slot map laid out anew once, which lays out again as it is.
    fn canonical() -> (Vec<u8>, Vec<u8>) {
        let s = slot_map();
        let (tree, layout) = opened(&s.virtual_section, &s.physical);
        super::super::gta6_relayout::write(&layout, &tree).expect("laid out anew")
    }

    #[test]
    fn a_pair_added_last_takes_the_slot_after_its_home_and_one_removed_leaves_the_others() {
        let (v, p) = canonical();
        let (mut tree, layout) = opened(&v, &p);
        assert_eq!(held(&mut tree), [(KEYS[0], 50), (KEYS[1], 60)]);
        assert!(!pairs_changed(&layout, &tree).expect("unchanged"));

        let key = Value::Hash(HashedString::from_hash(KEYS[2]));
        pairs(&mut tree).push((key, Value::UInt(70)));
        assert!(pairs_changed(&layout, &tree).expect("added"));
        let (v2, p2) = write_grown(&layout, &tree).expect("laid out anew");
        let (mut again, again_layout) = opened(&v2, &p2);
        assert_eq!(
            held(&mut again),
            [(KEYS[0], 50), (KEYS[1], 60), (KEYS[2], 70)]
        );
        let saved = write_grown(&again_layout, &again).expect("saved again");
        assert_eq!(saved, (v2.clone(), p2.clone()));
        // Home slot 5 is taken, so the probe goes on to slot 6.
        let model = read_model(&v2, &p2).expect("it reads");
        let table = (model.data_blocks.iter())
            .find(|b| b.data_type == PAIR)
            .expect("the table's block");
        assert_eq!(table.data[8 * 6..8 * 6 + 4], KEYS[2].to_le_bytes());

        let (mut tree, layout) = opened(&v, &p);
        pairs(&mut tree).remove(0);
        let (v3, p3) = write_grown(&layout, &tree).expect("laid out anew");
        let (mut again, _) = opened(&v3, &p3);
        assert_eq!(held(&mut again), [(KEYS[1], 60)]);
    }

    #[test]
    fn a_pair_is_not_added_to_a_body_whose_blocks_do_not_lay_out_again_as_stored() {
        let (mut v, p) = canonical();
        v.extend([0u8; 16]);
        let (mut tree, layout) = opened(&v, &p);
        assert!(write_grown(&layout, &tree).is_ok(), "an unchanged save");
        let key = Value::Hash(HashedString::from_hash(KEYS[2]));
        pairs(&mut tree).push((key, Value::UInt(70)));
        let err = write_grown(&layout, &tree)
            .expect_err("refused")
            .to_string();
        assert!(err.contains("is not known"), "{err}");
    }
}

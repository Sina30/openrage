//! A GTA VI headerless RBM tree whose maps gained or lost pairs, such as an anim scene's entity
//! or scene: the body is laid out anew, each table rebuilt from its pairs with an added one last
//! and every region after it moved, where the unchanged tree lays out anew as the body stores it.
//!
//! ⚠ Where a body's blocks do not lay out again as stored, where a new pair's regions go and
//! which order its blocks take are not known, so a pair is neither added nor removed there; a
//! body whose re-lay differs only inside its hash tables' blocks, each block of its type, length
//! and place, is an equally valid slot layout and takes one. A table is refilled in the order its pairs are read, slot order from slot 0; a table whose
//! probe chain wraps was filled from the slot after its first empty slot (a slot map) or after
//! its first group that is not full (a group map), so it comes back valid but differs from the
//! stored table inside its own bytes.

use std::collections::HashSet;

use super::schema::{PsoType, RbmClass, RbmClassEntry, RbmDataBlock, RbmModel};
use crate::meta::binary::layout::RbmLayout;
use crate::meta::binary::pso::table_form;
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;
use crate::meta::instance::MetaInstance;

const SLOT_MAP: u8 = 0x73;
const GROUP_MAP: u8 = 0x75;
/// The name a class's array-info member takes, which names no member of a pair.
const ARRAY_INFO: u32 = 0x100;

/// Whether `instance` adds a pair to or takes one from a map of the tree `layout`'s body stores.
pub(super) fn pairs_changed(layout: &RbmLayout, instance: &MetaInstance) -> Result<bool> {
    let stored = table_form::decode(&layout.model)?;
    if !instance_keys_differ(&stored, instance) {
        return Ok(false);
    }
    let split = layout.virtual_page_size.min(layout.decompressed.len());
    let (v, p) = layout.decompressed.split_at(split);
    let again = super::gta6_relayout::write(layout, &stored)?;
    if (again.0 != v || again.1 != p) && !only_tables_differ(&layout.model, (v, p), &again) {
        return Err(MetaError::Other(
            "GTA VI RBM: a map's pairs changed, and this body's blocks do not lay out again as it stores them, so where an added or removed pair's regions go is not known".into(),
        ));
    }
    Ok(true)
}

/// Whether `again`, laid out anew from the unchanged tree of the body `stored` holds, differs
/// from it only inside blocks of a hash table's type, every block of the same type, length and
/// place as `model` states it.
fn only_tables_differ(
    model: &RbmModel,
    stored: (&[u8], &[u8]),
    again: &(Vec<u8>, Vec<u8>),
) -> bool {
    if stored.0.len() != again.0.len() || stored.1.len() != again.1.len() {
        return false;
    }
    let Ok(laid) = super::gta6::read_model(&again.0, &again.1) else {
        return false;
    };
    let (old, new) = (&model.data_blocks, &laid.data_blocks);
    let same = |a: &RbmDataBlock, b: &RbmDataBlock| {
        a.data_type == b.data_type && a.data_addr == b.data_addr && a.data.len() == b.data.len()
    };
    if old.len() != new.len() || !old.iter().zip(new).all(|(a, b)| same(a, b)) {
        return false;
    }
    let tables = table_types(model);
    let in_a_table = |nibble: u64, at: usize| {
        (old.iter()).any(|b| {
            let from = (b.data_addr & 0x0FFF_FFFF) as usize;
            b.data_addr >> 28 == nibble
                && tables.contains(&b.data_type)
                && (from..from + b.data.len()).contains(&at)
        })
    };
    [(5, stored.0, &again.0), (6, stored.1, &again.1)]
        .into_iter()
        .all(|(nibble, s, a)| {
            (s.iter().zip(a.iter()).enumerate())
                .filter(|(_, (x, y))| x != y)
                .all(|(at, _)| in_a_table(nibble, at))
        })
}

/// The types of the blocks a hash table is laid out in: a slot map's pair structure and a group
/// map's group structure, found as the writer finds them where no stored table names one.
fn table_types(model: &RbmModel) -> HashSet<u32> {
    let types = |e: &RbmClassEntry| (e.data_type, e.union_ch);
    let mut out = HashSet::new();
    for owner in model.classes.values() {
        for map in &owner.entries {
            let grouped = match map.data_type {
                PsoType::Unknown(SLOT_MAP) => false,
                PsoType::Unknown(GROUP_MAP) => true,
                _ => continue,
            };
            let pair = [
                owner.entry(map.map_key_index()).map(types),
                owner.entry(map.map_value_index()).map(types),
            ];
            let is_pair = |c: &RbmClass| {
                let named: Vec<_> = (c.entries.iter())
                    .filter(|e| e.name_hash != ARRAY_INFO)
                    .map(|e| Some(types(e)))
                    .collect();
                named == pair
            };
            for c in model.classes.values() {
                let fits = match grouped {
                    false => is_pair(c),
                    true => {
                        c.entries.len() == 4
                            && c.entries[2].data_type == PsoType::Struct
                            && (model.class_for(c.entries[2].referent_hash())).is_some_and(|p| {
                                is_pair(p)
                                    && u64::from(c.data_size) == 16 + 16 * u64::from(p.data_size)
                            })
                    }
                };
                if fits {
                    out.insert(c.name_hash);
                }
            }
        }
    }
    out
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
    fn a_re_lay_that_differs_only_inside_a_table_s_block_is_an_equally_valid_body() {
        let (v, p) = canonical();
        let (_, layout) = opened(&v, &p);
        let model = read_model(&v, &p).expect("it reads");
        let start = |ty: u32| {
            let block = (model.data_blocks.iter())
                .find(|b| b.data_type == ty)
                .expect("the block");
            (block.data_addr & 0x0FFF_FFFF) as usize
        };
        let (table, root) = (start(PAIR), start(ROOT));
        let stored = (v.as_slice(), p.as_slice());
        // Two slots of the table traded, as a rebuild that moves a pair does.
        let mut moved = v.clone();
        let (a, b) = (table + 8 * 5, table + 8 * 19);
        let first = moved[a..a + 8].to_vec();
        moved.copy_within(b..b + 8, a);
        moved[b..b + 8].copy_from_slice(&first);
        assert_ne!(moved, v);
        assert!(only_tables_differ(
            &layout.model,
            stored,
            &(moved, p.clone())
        ));
        let mut outside = v.clone();
        outside[root] ^= 1;
        assert!(!only_tables_differ(
            &layout.model,
            stored,
            &(outside, p.clone())
        ));
        let mut longer = v.clone();
        longer.extend([0u8; 16]);
        assert!(!only_tables_differ(
            &layout.model,
            stored,
            &(longer, p.clone())
        ));
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

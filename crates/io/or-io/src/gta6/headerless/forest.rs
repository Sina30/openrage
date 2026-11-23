//! The object-tree table: an offset table whose items are typed objects that point at later items
//! by tagged offsets, stored tree after tree in pre-order, each root keyed by a model name's hash.

use std::collections::{HashMap, HashSet};

use super::bytes::{u32_at, u64_at};
use super::small::read_offset_table;
use super::{HeaderlessError, Result};

/// The high half of an item's pointer word, whose low half is the target item's offset plus 8.
pub const OBJECT_POINTER_TAG: u32 = 0x7171_7171;

const TREES: &str = "object-tree table";

fn no<T>(why: &'static str) -> Result<T> {
    Err(HeaderlessError::Layout { shape: TREES, why })
}

/// One item: an object of the type its tag names, whose fields are not measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectItem {
    pub offset: u64,
    /// Its bytes, up to the next item or the body's end.
    pub size: u64,
    /// `name_hash32` of a model name in a root, zero in a child.
    pub key: u32,
    /// The word at +0x04, zero in a child; what a root's holds is not known.
    pub aux: u32,
    pub type_tag: u32,
    /// The items its pointer words name, by index, in the order the words lie.
    pub children: Vec<usize>,
}

/// The items of the table, and which of them no pointer names: the root of each tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectTrees {
    pub items: Vec<ObjectItem>,
    /// Ascending; item 0 is the first, and a tree runs to the next root.
    pub roots: Vec<usize>,
}

impl ObjectTrees {
    /// Each tree's items, its root first.
    pub fn trees(&self) -> impl Iterator<Item = &[ObjectItem]> {
        let ends = (self.roots.iter().skip(1).copied()).chain([self.items.len()]);
        (self.roots.iter().zip(ends))
            .map(|(&start, end)| self.items.get(start..end).unwrap_or_default())
    }
}

/// The trees of an offset table's items, refused by the rule (T3 to T8) an item breaks.
pub fn read_object_trees(body: &[u8]) -> Result<ObjectTrees> {
    let table = read_offset_table(body)?;
    let offsets = &table.offsets;
    let index: HashMap<u64, usize> = (offsets.iter().enumerate()).map(|(i, o)| (*o, i)).collect();
    let ends = (offsets.iter().skip(1).copied()).chain([table.len]);
    let mut pointed = vec![false; offsets.len()];
    let mut items = Vec::with_capacity(offsets.len());
    for (i, (&at, end)) in offsets.iter().zip(ends).enumerate() {
        let size = end - at;
        if size < 32 || !size.is_multiple_of(16) {
            return no("T3: an item is not a multiple of 16 bytes of at least 32");
        }
        let item = &body[at as usize..end as usize];
        if u32_at(item, 0x0C) != 0 {
            return no("T4: an item's word at +0x0C is not zero");
        }
        let mut children = Vec::new();
        for k in (0..item.len()).step_by(8) {
            let word = u64_at(item, k);
            if word as u32 == OBJECT_POINTER_TAG {
                return no("T5: the pointer tag lies in a word's low half");
            }
            if (word >> 32) as u32 != OBJECT_POINTER_TAG {
                continue;
            }
            let target = (word as u32).checked_sub(8).map(u64::from);
            let Some(&j) = target.and_then(|t| index.get(&t)) else {
                return no("T5: a tagged word names no item's +0x08");
            };
            if j <= i {
                return no("T6: a pointer names its own item or an earlier one");
            }
            if std::mem::replace(&mut pointed[j], true) {
                return no("T6: two pointers name one item");
            }
            children.push(j);
        }
        items.push(ObjectItem {
            offset: at,
            size,
            key: u32_at(item, 0),
            aux: u32_at(item, 4),
            type_tag: u32_at(item, 8),
            children,
        });
    }
    for (item, pointed) in items.iter().zip(&pointed) {
        if *pointed == (item.key != 0) {
            return no("T7: an item is both pointed at and keyed, or neither");
        }
        if *pointed && item.aux != 0 {
            return no("T7: a pointed-at item's 8-byte prefix is not zero");
        }
    }
    // Item 0 is never pointed at, as a pointer goes forward, so by T7 it is a root.
    let roots: Vec<usize> = (0..items.len()).filter(|i| !pointed[*i]).collect();
    let tree: Vec<usize> = (0..items.len())
        .map(|i| roots.partition_point(|r| *r <= i))
        .collect();
    for (i, item) in items.iter().enumerate() {
        if item.children.iter().any(|c| tree[*c] != tree[i]) {
            return no("T8: a pointer leaves its root's tree");
        }
    }
    let mut keys = HashSet::with_capacity(roots.len());
    if !roots.iter().all(|r| keys.insert(items[*r].key)) {
        return no("T8: two roots share a key");
    }
    Ok(ObjectTrees { items, roots })
}

#[cfg(test)]
#[path = "forest_tests.rs"]
mod tests;

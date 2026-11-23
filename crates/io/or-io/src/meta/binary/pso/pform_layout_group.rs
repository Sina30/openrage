//! A GTA VI P-form group map laid out anew: its table rebuilt from the tree's pairs in their order,
//! each pair's array given a region of its own under the header words of a stored pair's array of
//! the same count.

use std::collections::HashMap;

use crate::meta::binary::layout::PsoLayout;
use crate::meta::error::Result;
use crate::meta::instance::value::Value;

use super::super::pform_group::{group_pairs, pair_array};
use super::super::pform_group_grow::{
    control, control_at, groups_for, hashed_key, key_hash, slots, SLOTS,
};
use super::super::reader::{named_pair, GTA6_COUNT_FIRST_ARRAY, GTA6_GROUP_MAP};
use super::super::schema::{
    element_stride, struct_subtype, PsoDataType, PsoStructEntry, PsoStructInfo,
};
use super::super::table_layout::DEPTH;
use super::{refused, Placer};

/// The control bytes of a group whose slots are all empty.
const EMPTY: [u8; SLOTS] = [0x80; SLOTS];
/// Where a pair's value starts: after the key's 4 bytes and 4 zero bytes.
const VALUE_AT: u32 = 8;
/// What a pair array's pointer word reaches into its block: twelve bits from bit 20.
const POINTER_REACH: usize = 1 << 12;

/// What the stored document's group maps tell a new layout of them.
#[derive(Default)]
pub(super) struct Groups {
    /// The pair structure of each `(owner, member)`'s stored map.
    pairs: HashMap<(u32, u32), u32>,
    /// The second and fourth words of the stored pair arrays' headers by `(pair structure,
    /// count)`, `None` where two stored headers of that count differ.
    values: HashMap<(u32, u32), Option<[u32; 2]>>,
    /// `(block, at, word)` written over the second word of a pointer once pointers are placed.
    words: Vec<(usize, usize, u32)>,
}

impl Groups {
    /// What the group maps of the document `layout` was read from hold.
    pub(super) fn stored(layout: &PsoLayout) -> Self {
        let mut groups = Self::default();
        for block in &layout.pmap.entries {
            let Some(info) = layout.schema.struct_for(block.name_hash) else {
                continue;
            };
            let Ok(start) = usize::try_from(block.offset) else {
                continue;
            };
            let Ok(len) = usize::try_from(block.length) else {
                continue;
            };
            let size = info.data_size as usize;
            if size == 0 {
                continue;
            }
            // Every region of a block is of its type, back to back.
            for k in 0..len / size {
                groups.scan(layout, info, start + k * size, 0);
            }
        }
        groups
    }

    fn scan(&mut self, layout: &PsoLayout, info: &PsoStructInfo, base: usize, depth: usize) {
        if depth > DEPTH {
            return;
        }
        for entry in &info.entries {
            let at = base + entry.offset() as usize;
            match entry.data_type {
                PsoDataType::Map if entry.subtype() == GTA6_GROUP_MAP => {
                    self.record(layout, info, entry, at)
                }
                PsoDataType::Structure if entry.subtype() == struct_subtype::EMBEDDED => {
                    if let Some(inner) = layout.schema.struct_for(entry.referent_hash()) {
                        self.scan(layout, inner, at, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn record(
        &mut self,
        layout: &PsoLayout,
        owner: &PsoStructInfo,
        entry: &PsoStructEntry,
        at: usize,
    ) {
        let Ok(Some(map)) = group_pairs(&layout.psin, &layout.pmap, &layout.schema, at) else {
            return;
        };
        let class = map.pair.name_hash;
        self.pairs.insert((owner.name_hash, entry.name_hash), class);
        if pair_array(owner, entry).is_none() {
            return;
        }
        for &slot in &map.slots {
            let header = slot + map.value.offset() as usize;
            let Some(h) = layout.psin.get(header..header + 16) else {
                continue;
            };
            let word = |k: usize| u32::from_be_bytes([h[k], h[k + 1], h[k + 2], h[k + 3]]);
            let carried = Some([word(4), word(12)]);
            (self.values.entry((class, word(0))))
                .and_modify(|stored| {
                    if *stored != carried {
                        *stored = None;
                    }
                })
                .or_insert(carried);
        }
    }

    /// Every carried word written over the second word of the pointer it follows.
    pub(super) fn overwrite(&self, blocks: &mut [(u32, Vec<u8>)]) -> Result<()> {
        for &(block, at, word) in &self.words {
            let slot = (blocks.get_mut(block))
                .and_then(|(_, data)| data.get_mut(at..at + 4))
                .ok_or_else(|| refused(format!("a write past block {block} at {at:#X}")))?;
            slot.copy_from_slice(&word.to_be_bytes());
        }
        Ok(())
    }
}

impl<'a> Placer<'a> {
    /// `{u32 groups, u32 pairs, pointer}` over a table of the structure the stored map of this
    /// member holds, its pairs inserted in the tree's order.
    pub(super) fn group_map(
        &mut self,
        owner: &'a PsoStructInfo,
        entry: &PsoStructEntry,
        block: usize,
        at: usize,
        pairs: &[(Value, Value)],
        depth: usize,
    ) -> Result<()> {
        let class = (self.groups.pairs.get(&(owner.name_hash, entry.name_hash)).copied())
            .ok_or_else(|| {
                refused(
                    "a group map's pairs are of a structure only the block that holds them names, and no stored map of this member holds any",
                )
            })?;
        let info = self.struct_named(class)?;
        let [key, value] = named_pair(info)?;
        let measured = hashed_key(&key)
            && key.offset() == 0
            && value.data_type == PsoDataType::Array
            && value.subtype() == GTA6_COUNT_FIRST_ARRAY
            && value.offset() == VALUE_AT;
        let (true, Some(element)) = (measured, pair_array(owner, entry)) else {
            return Err(refused(
                "a group map whose pair is not a 32-bit string hash at 0 and a subtype-10 array at 8 is not laid out anew, because no other pair shape is known",
            ));
        };
        let keys = (pairs.iter())
            .map(|(k, _)| key_hash(k))
            .collect::<Result<Vec<u32>>>()?;
        let groups = groups_for(keys.len())?;
        let places = slots(&keys, groups)?;
        let size = info.data_size as usize;
        let too_large = || refused("a group map is larger than a block holds");
        let unit = (size.checked_mul(SLOTS))
            .and_then(|bytes| bytes.checked_add(SLOTS))
            .ok_or_else(too_large)?;
        let len = unit.checked_mul(groups).ok_or_else(too_large)?;
        let (Ok(g), Ok(n)) = (u32::try_from(groups), u32::try_from(pairs.len())) else {
            return Err(too_large());
        };
        let table = self.region(class, len)?;
        self.put(block, at, &[g.to_be_bytes(), n.to_be_bytes()].concat())?;
        self.pointers.push((block, at + 8, table.0, table.1));
        for group in 0..groups {
            self.put(table.0, table.1 + unit * group, &EMPTY)?;
        }
        for ((&hash, (_, v)), &slot) in keys.iter().zip(pairs).zip(&places) {
            let group = table.1 + unit * (slot / SLOTS);
            let j = slot % SLOTS;
            self.put(table.0, group + control_at(j), &[control(hash)])?;
            let pair = group + SLOTS + size * j;
            self.put(table.0, pair + key.offset() as usize, &hash.to_be_bytes())?;
            let value_at = (table.0, pair + value.offset() as usize);
            self.group_value(owner, &element, class, value_at, v, depth)?;
        }
        Ok(())
    }

    /// A pair's `{u32 count, u32, u32 pointer, u32}` at `at` of `block` and its elements in a
    /// region of their own, the second and fourth words carried from a stored pair of that count.
    fn group_value(
        &mut self,
        owner: &'a PsoStructInfo,
        element: &PsoStructEntry,
        class: u32,
        (block, at): (usize, usize),
        value: &Value,
        depth: usize,
    ) -> Result<()> {
        let Value::Array(items) = value else {
            return Err(refused(
                "a group map's pair value is not an array, where its structure states one",
            ));
        };
        let too_many =
            || refused("a group map's pair array holds more elements than a count states");
        let n = u32::try_from(items.len()).map_err(|_| too_many())?;
        let [second, fourth] = (self.groups.values.get(&(class, n)).copied().flatten())
            .ok_or_else(|| {
                refused(format!(
                    "a group map's pair array of {n} elements has no stored pair array of that count to carry its header from, and what the header's second and fourth words hold at another count is not known"
                ))
            })?;
        self.put(block, at, &[n.to_be_bytes(), second.to_be_bytes()].concat())?;
        if items.is_empty() {
            return self.put(block, at + 12, &fourth.to_be_bytes());
        }
        let stride = element_stride(self.schema, element).ok_or_else(|| {
            refused(format!(
                "an array of {:?} subtype {} has no known element size",
                element.data_type,
                element.subtype()
            ))
        })?;
        let len = (stride.checked_mul(items.len()))
            .ok_or_else(|| refused("an array is larger than a block holds"))?;
        let tag = match element.data_type {
            PsoDataType::Structure if element.subtype() == struct_subtype::EMBEDDED => {
                element.referent_hash()
            }
            other => u32::from(other.to_byte()),
        };
        let target = self.region(tag, len)?;
        if target.1 >= POINTER_REACH {
            return Err(refused(format!(
                "a group map's pair array would start {:#X} bytes into its block, and its header's pointer word is known only below 0x1000",
                target.1
            )));
        }
        self.pointers.push((block, at + 8, target.0, target.1));
        self.groups.words.push((block, at + 12, fourth));
        for (i, item) in items.iter().enumerate() {
            let element_at = target.1 + stride * i;
            self.member(owner, element, target.0, element_at, Some(item), depth + 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "pform_layout_group_tests.rs"]
mod tests;

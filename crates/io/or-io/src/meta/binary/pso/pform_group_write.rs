//! A GTA VI P-form group map saved in place, over the pairs it holds.
//!
//! ⚠ A key or a pair count the table does not hold lays the map out anew; what the second and
//! fourth words of a pair's array header hold at another count is not known, so counts are kept.

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;

use super::super::pform_group::{group_pairs, pair_array, value_array};
use super::super::pform_group_grow::pair_slots;
use super::super::schema::{PsoStructEntry, PsoStructInfo};
use super::{kind_of, PsoEncoder};

impl PsoEncoder<'_> {
    /// `pairs` into the full pairs of the group map `entry` of `owner` whose header is at
    /// `offset`, as many as it holds, each with the key it holds.
    pub(super) fn write_group_map(
        &mut self,
        owner: &PsoStructInfo,
        entry: &PsoStructEntry,
        offset: usize,
        pairs: &[(Value, Value)],
    ) -> Result<()> {
        let layout = self.layout;
        let Some(map) = group_pairs(&layout.psin, &layout.pmap, &layout.schema, offset)? else {
            return Ok(());
        };
        let element = pair_array(owner, entry);
        self.emit_allocation(map.table.0, map.table.1)?;
        let width = self.element_stride(&map.key).unwrap_or(0);
        let slots = pair_slots(&map, &layout.psin, pairs)?;
        for (i, ((k, v), at)) in pairs.iter().zip(slots).enumerate() {
            let key_at = at + map.key.offset() as usize;
            self.path.push(format!("[{i}].key"));
            self.write_member(map.pair, &map.key, key_at, k)?;
            self.path.pop();
            let keys = key_at..key_at + width;
            if self.psin.get(keys.clone()) != layout.psin.get(keys) {
                return Err(MetaError::Other(format!(
                    "pair {i} of a GTA VI group map has a key other than the one stored, and where a key goes is known only for a 32-bit string hash"
                )));
            }
            self.path.push(format!("[{i}].value"));
            let value_at = at + map.value.offset() as usize;
            match &element {
                Some(element) => self.write_pair_array(owner, element, value_at, v)?,
                None => self.write_member(map.pair, &map.value, value_at, v)?,
            }
            self.path.pop();
        }
        Ok(())
    }

    /// A pair's subtype-10 array of `element`s of `owner` into the elements its stored header
    /// names, keeping the header.
    fn write_pair_array(
        &mut self,
        owner: &PsoStructInfo,
        element: &PsoStructEntry,
        at: usize,
        written: &Value,
    ) -> Result<()> {
        let Value::Array(items) = written else {
            return Err(MetaError::Other(format!(
                "expected an array, found {}",
                kind_of(written)
            )));
        };
        let stride = self.element_stride(element).ok_or_else(|| {
            MetaError::Other(format!(
                "array of {:?} has no known element size",
                element.data_type
            ))
        })?;
        let layout = self.layout;
        let (count, base) = value_array(&layout.psin, &layout.pmap, at, stride)?;
        self.emit_allocation(at, 16)?;
        if items.len() != count {
            return Err(MetaError::Other(format!(
                "a GTA VI group map's array holds {} elements where {count} are stored, and what the second and fourth words of its header hold for another count is not known",
                items.len()
            )));
        }
        for (i, item) in items.iter().enumerate() {
            self.path.push(format!("[{i}]"));
            self.write_member(owner, element, base + stride * i, item)?;
            self.path.pop();
        }
        Ok(())
    }
}

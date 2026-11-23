//! A GTA VI P-form group map saved in place, over the pairs it holds.
//!
//! ⚠ Where a new key goes was measured on one pair, and what the second and fourth words of a
//! pair's array header hold for another count on one header, so keys and counts are kept.

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;

use super::super::pform_group::{group_pairs, pair_array, value_array};
use super::super::schema::{PsoStructEntry, PsoStructInfo};
use super::{kind_of, PsoEncoder};

impl PsoEncoder<'_> {
    /// `pairs` into the full pairs of the group map whose header is at `offset`, as many as it
    /// holds, each with the key it holds.
    pub(super) fn write_group_map(
        &mut self,
        offset: usize,
        pairs: &[(Value, Value)],
    ) -> Result<()> {
        let layout = self.layout;
        let Some(map) = group_pairs(&layout.psin, &layout.pmap, &layout.schema, offset)? else {
            return Ok(());
        };
        self.emit_allocation(map.table.0, map.table.1)?;
        let width = self.element_stride(&map.key).unwrap_or(0);
        for (i, ((k, v), &at)) in pairs.iter().zip(&map.slots).enumerate() {
            let key_at = at + map.key.offset() as usize;
            self.path.push(format!("[{i}].key"));
            self.write_member(map.pair, &map.key, key_at, k)?;
            self.path.pop();
            let keys = key_at..key_at + width;
            if self.psin.get(keys.clone()) != layout.psin.get(keys) {
                return Err(MetaError::Other(format!(
                    "pair {i} of a GTA VI group map has a key other than the one stored, and where a new key goes in a group map is measured on one pair"
                )));
            }
            self.path.push(format!("[{i}].value"));
            let value_at = at + map.value.offset() as usize;
            self.write_pair_value(map.pair, &map.value, value_at, v)?;
            self.path.pop();
        }
        Ok(())
    }

    /// A pair's value: a subtype-10 array into the elements its stored header names, keeping the
    /// header, else as any member.
    fn write_pair_value(
        &mut self,
        pair: &PsoStructInfo,
        value: &PsoStructEntry,
        at: usize,
        written: &Value,
    ) -> Result<()> {
        let Some(element) = pair_array(pair, value) else {
            return self.write_member(pair, value, at, written);
        };
        let Value::Array(items) = written else {
            return Err(MetaError::Other(format!(
                "expected an array, found {}",
                kind_of(written)
            )));
        };
        let stride = self.element_stride(&element).ok_or_else(|| {
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
            self.write_member(pair, &element, base + stride * i, item)?;
            self.path.pop();
        }
        Ok(())
    }
}

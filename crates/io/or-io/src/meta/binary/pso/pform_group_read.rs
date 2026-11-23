//! A GTA VI P-form group map read as the map of its full pairs, in slot order.

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;

use super::super::pform_group::{group_pairs, pair_array, value_array};
use super::super::schema::{PsoStructEntry, PsoStructInfo};
use super::PsoDecoder;

impl PsoDecoder<'_, '_> {
    /// The group map whose header is at `offset`.
    pub(super) fn decode_group_map(&mut self, offset: usize) -> Result<Value> {
        let Some(map) = group_pairs(self.data, self.pmap, self.schema, offset)? else {
            return Ok(Value::Map(Vec::new()));
        };
        let mark = self.nesting.enter(None)?;
        let mut pairs = Vec::with_capacity(map.slots.len());
        let decoded: Result<()> = map.slots.iter().try_for_each(|&at| {
            let key_at = at + map.key.offset() as usize;
            let k = self.decode_member(map.pair, &map.key, key_at)?;
            let value_at = at + map.value.offset() as usize;
            let v = self.decode_pair_value(map.pair, &map.value, value_at)?;
            pairs.push((k, v));
            Ok(())
        });
        self.nesting.leave(mark)?;
        decoded?;
        Ok(Value::Map(pairs))
    }

    /// A pair's value: a subtype-10 array by the header it has in a pair, else as any member.
    fn decode_pair_value(
        &mut self,
        pair: &PsoStructInfo,
        value: &PsoStructEntry,
        at: usize,
    ) -> Result<Value> {
        let Some(element) = pair_array(pair, value) else {
            return self.decode_member(pair, value, at);
        };
        let stride = self.element_stride(&element).ok_or_else(|| {
            MetaError::Other(format!(
                "pso: a GTA VI group map's array of {:?} has no known element size",
                element.data_type
            ))
        })?;
        let (count, base) = value_array(self.data, self.pmap, at, stride)?;
        self.decode_array_elements(pair, &element, base, count)
    }
}

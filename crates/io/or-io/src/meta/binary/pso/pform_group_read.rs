//! A GTA VI P-form group map read as the map of its full pairs, in slot order.

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;

use super::super::pform_group::{group_pairs, pair_array, value_array};
use super::super::schema::{PsoStructEntry, PsoStructInfo};
use super::PsoDecoder;

impl PsoDecoder<'_, '_> {
    /// The group map `entry` of `owner` whose header is at `offset`.
    pub(super) fn decode_group_map(
        &mut self,
        owner: &PsoStructInfo,
        entry: &PsoStructEntry,
        offset: usize,
    ) -> Result<Value> {
        let Some(map) = group_pairs(self.data, self.pmap, self.schema, offset)? else {
            return Ok(Value::Map(Vec::new()));
        };
        let element = pair_array(owner, entry);
        let mark = self.nesting.enter(None)?;
        let mut pairs = Vec::with_capacity(map.slots.len());
        let decoded: Result<()> = map.slots.iter().try_for_each(|&at| {
            let key_at = at + map.key.offset() as usize;
            let k = self.decode_member(map.pair, &map.key, key_at)?;
            let value_at = at + map.value.offset() as usize;
            let v = match &element {
                Some(element) => self.decode_pair_array(owner, element, value_at)?,
                None => self.decode_member(map.pair, &map.value, value_at)?,
            };
            pairs.push((k, v));
            Ok(())
        });
        self.nesting.leave(mark)?;
        decoded?;
        Ok(Value::Map(pairs))
    }

    /// A pair's subtype-10 array of `element`s of `owner`, by the header it has in a pair.
    fn decode_pair_array(
        &mut self,
        owner: &PsoStructInfo,
        element: &PsoStructEntry,
        at: usize,
    ) -> Result<Value> {
        let stride = self.element_stride(element).ok_or_else(|| {
            MetaError::Other(format!(
                "pso: a GTA VI group map's array of {:?} has no known element size",
                element.data_type
            ))
        })?;
        let (count, base) = value_array(self.data, self.pmap, at, stride)?;
        self.decode_array_elements(owner, element, base, count)
    }
}

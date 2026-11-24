//! The elements of a state's run that are not placed in the world: pairs of a key and a value
//! structure, each read member by member by the declaration in the body's own type table.

use crate::meta::binary::pso::table_form;
use crate::meta::binary::rbm::reader::{read_u32_le, read_u64_le};
use crate::meta::binary::rbm::schema::{PsoType, RbmModel};
use crate::meta::error::MetaError;
use crate::meta::instance::value::MetaInstance;

use super::mission_tables::{Body, StateRecord};

/// The name hash of a pair's key member.
pub const KEY: u32 = 0x6098_A50E;
/// The name hash of a pair's value member.
pub const VALUE: u32 = 0x063F_A3F2;
/// The name hash of a type-table row that names no member: an element or a map's key or value.
const ANONYMOUS: u32 = 0x100;
/// The type code of an eight-byte wide hash.
const WIDE_HASH: u8 = 0x4F;

/// Why the elements of a run do not read.
#[derive(Debug, thiserror::Error)]
pub enum OtherShapeError {
    #[error("the body's type table and blocks do not read: {0}")]
    Model(MetaError),
    #[error("block {block} does not hold pairs of a key and a structure: {why}")]
    NotAPair { block: usize, why: &'static str },
    #[error("the element at {at:#X} of block {block} does not read: {error}")]
    Element {
        block: usize,
        at: usize,
        error: MetaError,
    },
}

/// One element: its key and its value structure, with everything below it.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherElement {
    /// Where the element starts in the body.
    pub at: usize,
    pub key: u64,
    /// Whether the key is an eight-byte wide hash rather than a four-byte one.
    pub wide: bool,
    pub value: MetaInstance,
}

/// A body's type table and blocks, read once for every run of it.
#[derive(Debug, Clone)]
pub struct Declared(RbmModel);

impl Declared {
    /// The declarations `bytes` state, refused where they are not a table-form document.
    pub fn open(bytes: &[u8]) -> Result<Self, OtherShapeError> {
        table_form::model_of(bytes)
            .map(Self)
            .map_err(OtherShapeError::Model)
    }

    /// The elements of `record`'s run, which [`Body::run`] gave as elements of another shape.
    // A run of a state table's records has a map where the value structure is, so it is refused:
    // a caller asks `Body::run` first.
    pub fn elements(
        &self,
        body: &Body,
        record: &StateRecord,
    ) -> Result<Vec<OtherElement>, OtherShapeError> {
        let Some((k, offset)) = record.run else {
            return Ok(Vec::new());
        };
        let not = |why| OtherShapeError::NotAPair { block: k + 1, why };
        let (Some(block), Some(placed)) = (self.0.data_blocks.get(k), body.blocks.get(k)) else {
            return Err(not("the record names a block the body does not have"));
        };
        let pair = (self.0.class_for(block.data_type))
            .ok_or(not("the type table does not declare its type"))?;
        let named: Vec<_> = (pair.entries.iter())
            .filter(|e| e.name_hash != ANONYMOUS)
            .collect();
        let &[key, value] = named.as_slice() else {
            return Err(not("its type does not name exactly two members"));
        };
        let wide = match (key.name_hash, key.offset, key.data_type) {
            (KEY, 0, PsoType::NsStringHash) => false,
            (KEY, 0, PsoType::Unknown(WIDE_HASH)) => true,
            _ => return Err(not("its first member is not a key at +0")),
        };
        if value.name_hash != VALUE || value.data_type != PsoType::Struct {
            return Err(not("its second member is not a value structure"));
        }
        let size = pair.data_size as usize;
        (0..usize::from(record.count))
            .map(|n| -> Result<OtherElement, OtherShapeError> {
                let start = n.checked_mul(size).and_then(|len| len.checked_add(offset));
                let at = start.unwrap_or(usize::MAX);
                let failed = |error: MetaError| OtherShapeError::Element {
                    block: k + 1,
                    at: placed.offset.saturating_add(at),
                    error,
                };
                let key = match wide {
                    true => read_u64_le(&block.data, at),
                    false => read_u32_le(&block.data, at).map(u64::from),
                }
                .map_err(failed)?;
                let value_at = at.saturating_add(value.offset as usize);
                let value = table_form::read_struct_at(&self.0, k, value_at, value.referent_hash())
                    .map_err(failed)?;
                Ok(OtherElement {
                    at: placed.offset.saturating_add(at),
                    key,
                    wide,
                    value,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "mission_other_tests.rs"]
mod tests;

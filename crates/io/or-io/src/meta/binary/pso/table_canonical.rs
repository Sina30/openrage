//! A table-form document whose stored block order the rules do not give back, saved in the
//! canonical form: its blocks in the order the rules give, accepted only where everything else
//! the rules write is what the document stores and the form is a fixed point.

use crate::meta::binary::rbm::schema::RbmModel;
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::MetaInstance;

use super::sections::{split_sections, RawPsoSection};
use super::table_form::TableFormError;
use super::table_layout::{emit, lay_out, lay_out_in_place, where_they_miss, Held, Laid};

/// `instance` in the canonical form, where `laid`, the unedited `original` laid out by the rules,
/// differs from `stored` only in where its blocks are; refused by what the rules miss otherwise.
pub(super) fn write(
    sections: &[RawPsoSection],
    model: &RbmModel,
    stored: &[u8],
    original: &MetaInstance,
    laid: &Laid,
    held: &Held,
    instance: &MetaInstance,
) -> Result<Vec<u8>> {
    let missed = || -> MetaError { TableFormError::RulesMiss(where_they_miss(model, laid)).into() };
    if !only_the_order_differs(sections, model, stored, original, laid) {
        return Err(missed());
    }
    if !is_fixed_point(&emit(sections, laid), Some(original))? {
        return Err(missed());
    }
    let (edited, _) = lay_out(model, instance, Some(held))?;
    let bytes = emit(sections, &edited);
    if !is_fixed_point(&bytes, None)? {
        return Err(TableFormError::RulesMiss(
            "its canonical form, read again, does not lay out the same".into(),
        )
        .into());
    }
    Ok(bytes)
}

/// Whether the rules give back every byte of `stored` but the order of its blocks: the same
/// blocks in another order, and in their stored places every value, header and table written as
/// stored.
fn only_the_order_differs(
    sections: &[RawPsoSection],
    model: &RbmModel,
    stored: &[u8],
    original: &MetaInstance,
    laid: &Laid,
) -> bool {
    let types: Vec<u32> = model.data_blocks.iter().map(|b| b.data_type).collect();
    let laid_types: Vec<u32> = laid.blocks.iter().map(|(ty, _)| *ty).collect();
    if types == laid_types && model.root_block_id == laid.root {
        return false;
    }
    let mut blocks: Vec<(u32, usize)> = (model.data_blocks.iter())
        .map(|b| (b.data_type, b.data.len()))
        .collect();
    let mut laid_blocks: Vec<(u32, usize)> = (laid.blocks.iter())
        .map(|(ty, data)| (*ty, data.len()))
        .collect();
    blocks.sort_unstable();
    laid_blocks.sort_unstable();
    if blocks != laid_blocks {
        return false;
    }
    lay_out_in_place(model, original, None)
        .is_ok_and(|(in_place, _)| emit(sections, &in_place) == stored)
}

/// Whether `bytes`, read again and laid out by the rules, give themselves back, holding the
/// values of `expected` where one is given.
fn is_fixed_point(bytes: &[u8], expected: Option<&MetaInstance>) -> Result<bool> {
    let sections = split_sections(bytes)?;
    let model = super::table_form::model(&sections)?;
    let read = super::table_form::decode(&model)?;
    if expected.is_some_and(|expected| *expected != read) {
        return Ok(false);
    }
    let (laid, _) = lay_out(&model, &read, None)?;
    Ok(emit(&sections, &laid) == bytes)
}

#[cfg(test)]
#[path = "table_canonical_tests.rs"]
mod tests;

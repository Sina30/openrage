//! GTA VI's P-form group map: `{u32 groups, u32 pairs, pointer}` big-endian, the pointer naming
//! `groups` groups of 16 control bytes and 16 pairs of its block's structure. It is read whole and
//! saved only in place.

use crate::meta::error::{MetaError, Result};

use super::reader::{gta6_pairs, named_pair, GTA6_COUNT_FIRST_ARRAY};
use super::schema::{PsoDataType, PsoSchemaSection, PsoStructEntry, PsoStructInfo};
use super::sections::{PmapSection, PsoPointer};

/// The control byte of an empty slot; a full one holds a byte below it.
const EMPTY: u8 = 0x80;

/// The full pairs of a stored group map.
pub(super) struct GroupPairs<'s> {
    pub(super) pair: &'s PsoStructInfo,
    pub(super) key: PsoStructEntry,
    pub(super) value: PsoStructEntry,
    /// Where its groups start in `PSIN`, and the bytes they take.
    pub(super) table: (usize, usize),
    /// Where each full pair starts in `PSIN`, in slot order.
    pub(super) slots: Vec<usize>,
}

fn refused(why: String) -> MetaError {
    MetaError::Other(format!("pso: a GTA VI group map {why}"))
}

/// The big-endian u32 at `at` of `psin`.
fn word(psin: &[u8], at: usize) -> Result<u32> {
    (at.checked_add(4))
        .and_then(|end| psin.get(at..end))
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| refused(format!("runs past PSIN at {at:#X}")))
}

/// The pairs of the group map whose header is at `at` of `psin`; `None` for an empty one.
// ⚠ Control byte `j` of a group is byte `j ^ 3` of its sixteen, which are big-endian words.
pub(super) fn group_pairs<'s>(
    psin: &[u8],
    pmap: &PmapSection,
    schema: &'s PsoSchemaSection,
    at: usize,
) -> Result<Option<GroupPairs<'s>>> {
    let (groups, count) = (word(psin, at)?, word(psin, at + 4)?);
    let (low, high) = (word(psin, at + 8)?, word(psin, at + 12)?);
    let pointer = PsoPointer {
        raw: u64::from(low) | u64::from(high) << 32,
    };
    if pointer.is_null() {
        if groups != 0 || count != 0 {
            return Err(refused(format!(
                "states {groups} groups and {count} pairs behind a null pointer"
            )));
        }
        return Ok(None);
    }
    if !groups.is_power_of_two() {
        return Err(refused(format!(
            "states {groups} groups, where every measured one states a power of two"
        )));
    }
    let (pair, base) = gta6_pairs(pmap, schema, pointer)?;
    let [key, value] = named_pair(pair)?;
    let size = pair.data_size as usize;
    let unit = (size.checked_mul(16)).and_then(|pairs| pairs.checked_add(16));
    let len = unit.and_then(|unit| unit.checked_mul(groups as usize));
    let (Some(unit), Some(len)) = (unit, len) else {
        return Err(refused(format!("of {groups} groups of {size}-byte pairs")));
    };
    if base.checked_add(len).is_none_or(|end| end > psin.len()) {
        return Err(refused(format!(
            "of {groups} groups of {unit} bytes at {base:#X} runs past PSIN"
        )));
    }
    let mut slots = Vec::new();
    for group in (0..groups as usize).map(|g| base + unit * g) {
        for j in 0..16 {
            match psin[group + (j ^ 3)] {
                EMPTY => {}
                control if control < EMPTY => slots.push(group + 16 + size * j),
                control => {
                    return Err(refused(format!(
                        "holds control byte {control:#04X}, which is neither 0x80 nor below it"
                    )))
                }
            }
        }
    }
    if slots.len() != count as usize {
        return Err(refused(format!(
            "holds {} full pairs, where its header states {count}",
            slots.len()
        )));
    }
    Ok(Some(GroupPairs {
        pair,
        key,
        value,
        table: (base, len),
        slots,
    }))
}

/// The element member of a pair's `value` where it is a subtype-10 array, whose header in a group
/// map's pair is not the one an array has elsewhere.
pub(super) fn pair_array(pair: &PsoStructInfo, value: &PsoStructEntry) -> Option<PsoStructEntry> {
    let array = value.data_type == PsoDataType::Array
        && value.subtype() == GTA6_COUNT_FIRST_ARRAY
        && value.count() == 0;
    array
        .then(|| value.member_index().and_then(|i| pair.entry(i)).copied())
        .flatten()
}

/// The count of the subtype-10 array whose header is at `at` of a group map's pair, and where its
/// `stride`-byte elements start: `{u32 count, u32, u32 pointer, u32}` big-endian, its second and
/// fourth words carried as stored.
// ⚠ The pointer is the third word alone: block id in bits 0 to 11, offset from bit 20. Its words
// were measured on one header, so only an unchanged count is saved.
pub(super) fn value_array(
    psin: &[u8],
    pmap: &PmapSection,
    at: usize,
    stride: usize,
) -> Result<(usize, usize)> {
    let (count, pointer) = (word(psin, at)? as usize, word(psin, at + 8)?);
    if pointer == 0 {
        if count != 0 {
            return Err(refused(format!(
                "pair counts {count} elements behind a null pointer"
            )));
        }
        return Ok((0, 0));
    }
    if (pointer >> 12) & 0xFF != 0 {
        return Err(refused(format!(
            "pair's array pointer {pointer:#010X} sets bits 12 to 19, whose meaning is not known"
        )));
    }
    let id = pointer & 0xFFF;
    let block = pmap.block(id as i32).ok_or_else(|| {
        refused(format!(
            "pair's array names block {id}, which PMAP does not hold"
        ))
    })?;
    let base = (usize::try_from(block.offset).ok())
        .and_then(|offset| offset.checked_add((pointer >> 20) as usize));
    let end = base.and_then(|base| base.checked_add(stride.checked_mul(count)?));
    match (base, end) {
        (Some(base), Some(end)) if stride > 0 && end <= psin.len() => Ok((count, base)),
        _ => Err(refused(format!(
            "pair's array of {count} elements of {stride} bytes runs past PSIN"
        ))),
    }
}

#[cfg(test)]
#[path = "pform_group_tests.rs"]
mod tests;

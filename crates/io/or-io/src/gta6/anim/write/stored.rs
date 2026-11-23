//! A grown block placed in a stored resource, whose section ends with the region its root 0x08
//! points at: the block goes before that region, which moves past it, and every pointer into the
//! region is moved with it.
//!
//! ⚠ Only the fields known to point into the region are moved: the root's region, set list, name
//! and metadata region, each map's buckets and entries' next, each set's name, each animation's
//! name, block list, DOF list and owner, and each clip's name. A word anywhere else that reads as
//! an address in the region refuses the write, since a pointer left behind would name the block's
//! bytes.

use std::collections::HashSet;

use super::super::{AnimError, AnimResource, RootKind, Section};
use super::VIRTUAL_BASE;

const ALIGN: usize = 16;

fn address(out: &[u8], at: usize) -> Option<u64> {
    (out.get(at..at + 8))
        .map(|w| u64::from_le_bytes([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]))
}

/// Where the region root 0x08 points at starts in `out`.
fn region_start(out: &[u8]) -> Result<usize, AnimError> {
    (address(out, 0x08))
        .and_then(|v| v.checked_sub(VIRTUAL_BASE))
        .map(|v| v as usize)
        .filter(|at| at % ALIGN == 0 && *at <= out.len())
        .ok_or_else(|| {
            AnimError::Unwritten(
                "the stored resource's root 0x08 names no region in its section".into(),
            )
        })
}

/// Where the byte at `at` of `s` lies in `out`, whose last region an earlier insert of this edit
/// may have moved.
pub(super) fn now_at(s: &Section, out: &[u8], at: usize) -> Result<usize, AnimError> {
    let read = s.target("end region", 0x08)?;
    let shift = (region_start(out)?.checked_sub(read)).ok_or_else(|| {
        AnimError::Unwritten("the stored resource's last region moved back".into())
    })?;
    Ok(if at >= read { at + shift } else { at })
}

/// Where `bytes` now lie in `out`, placed before the region root 0x08 points at.
pub(super) fn insert_before_end(
    s: &Section,
    resource: &AnimResource,
    out: &mut Vec<u8>,
    bytes: &[u8],
) -> Result<usize, AnimError> {
    let start = region_start(out)?;
    let end = out.len();
    let region = VIRTUAL_BASE + start as u64..VIRTUAL_BASE + end as u64;
    let mut fields = Vec::new();
    for at in pointer_fields(s, resource)? {
        fields.push(now_at(s, out, at)?);
    }
    let known: HashSet<usize> = fields.iter().copied().collect();
    for at in (0..end.saturating_sub(7)).step_by(8) {
        if !known.contains(&at) && address(out, at).is_some_and(|v| region.contains(&v)) {
            return Err(AnimError::Unwritten(format!(
                "a grown block goes before the stored resource's last region, which moves, and the word at {at:#X} reads as an address in that region but is no pointer known to follow it"
            )));
        }
    }
    let len = bytes.len().next_multiple_of(ALIGN);
    let moved: Vec<(usize, u64)> = (fields.into_iter())
        .filter_map(|at| Some((at, address(out, at).filter(|v| region.contains(v))?)))
        .collect();
    let region_bytes = out.split_off(start);
    out.extend_from_slice(bytes);
    out.resize(start + len, 0);
    out.extend(region_bytes);
    for (at, v) in moved {
        // A field inside the region moved with it.
        let at = if at >= start { at + len } else { at };
        out[at..at + 8].copy_from_slice(&(v + len as u64).to_le_bytes());
    }
    Ok(start)
}

/// The words of the map at `at` that may point at its entries: its buckets' pointer, each bucket
/// and each entry's next at `next_at`.
fn map_fields(s: &Section, at: usize, next_at: usize, wide: bool) -> Result<Vec<usize>, AnimError> {
    let entries = s.map("map", at, next_at, wide)?;
    let mut fields = vec![at];
    if let Some(buckets) = s.pointer("map", at)? {
        // The map read checked that its buckets lie in the section.
        let slots = s.u32("map", at + 8)? as usize;
        fields.extend((0..slots).map(|b| buckets + 8 * b));
    }
    fields.extend(entries.into_iter().map(|(_, entry)| entry + next_at));
    Ok(fields)
}

/// The offsets in `s` of every field of `resource` that may point into its last region.
fn pointer_fields(s: &Section, resource: &AnimResource) -> Result<Vec<usize>, AnimError> {
    let pointer_name =
        |at: usize| -> Result<bool, AnimError> { Ok(s.bytes("name", at, 16)?[15] & 0x80 == 0) };
    let mut fields = vec![0x08, 0x70, 0xA0];
    if pointer_name(0x38)? {
        fields.push(0x38);
    }
    if resource.root == RootKind::ClipDictionary {
        fields.extend(map_fields(s, 0x80, 8, false)?);
        fields.extend(map_fields(s, 0x48, 0x10, true)?);
    }
    for set in s.set_offsets(resource)? {
        if pointer_name(set + 0x38)? {
            fields.push(set + 0x38);
        }
        fields.extend(map_fields(s, set + 0x48, 0x10, false)?);
        for a in s.animation_offsets(set)? {
            fields.extend([a + 0x18, a + 0x30, a + 0x50, a + 0x60]);
        }
    }
    for clip in s.clip_offsets(resource)? {
        if pointer_name(clip + 0x28)? {
            fields.push(clip + 0x28);
        }
    }
    Ok(fields)
}

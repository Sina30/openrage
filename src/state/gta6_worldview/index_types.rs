//! What a map body states that the index keeps and the map readers do not give: the group a
//! script-managed map is switched with, and the hours a time archetype is on.
//!
//! ⚠ Each member is checked against the body's own class table first, as the map readers check
//! theirs: a class that puts it elsewhere would read other bytes.

use or_io::gta6::map;
use or_io::hash::literal_string_hash as lit;
use or_io::meta::binary::rbm::schema::{RbmDataBlock, RbmModel};

/// A member read here: its name's hash, its offset and its type code.
type Member = (u32, u32, u8);

const TIME_ARCHETYPE: u32 = lit(b"CTimeArchetypeDef");
/// The hours a time archetype is on, one bit an hour from bit 0.
const TIME_FLAGS: Member = (lit(b"timeFlags"), 0xB8, 0x15);
/// The 32-bit name of the group a script-managed map is switched with.
const GROUP: Member = (0x4C0A_CB96, 0x7C, 0x4A);
/// Where a map-types body's array of archetype pointers lies in its root.
const ARCHETYPES: usize = 0x18;

/// Every hour of the day: what an archetype that is no time archetype is on.
pub const ALL_HOURS: u32 = 0x00FF_FFFF;

fn states(model: &RbmModel, class: u32, (member, offset, code): Member) -> bool {
    model.classes.get(&class).is_some_and(|class| {
        (class.entries.iter())
            .any(|e| e.name_hash == member && e.offset == offset && e.data_type.to_byte() == code)
    })
}

fn bytes<const N: usize>(from: &[u8], at: usize) -> Option<[u8; N]> {
    from.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// The block a pointer inside data names, and the offset in it: the block's 1-based id in bits 0
/// to 11, bits 12 to 19 clear, the offset from bit 20.
fn target(blocks: &[RbmDataBlock], raw: u64) -> Option<(&RbmDataBlock, usize)> {
    if (raw >> 12) & 0xFF != 0 {
        return None;
    }
    let block = usize::try_from(raw & 0xFFF).ok()?.checked_sub(1)?;
    Some((blocks.get(block)?, usize::try_from(raw >> 20).ok()?))
}

fn root(blocks: &[RbmDataBlock], id: u32) -> Option<&[u8]> {
    let id = usize::try_from(id).ok()?.checked_sub(1)?;
    Some(&blocks.get(id)?.data)
}

/// The name of the group the map of `model` is switched with, 0 where it states none; `None` where the body's class table does not put the member where every measured body has it.
pub fn group_of(model: &RbmModel) -> Option<u32> {
    if !states(model, map::C_MAP_DATA, GROUP) {
        return None;
    }
    group_in(&model.data_blocks, model.root_block_id)
}

fn group_in(blocks: &[RbmDataBlock], root_id: u32) -> Option<u32> {
    bytes(root(blocks, root_id)?, GROUP.1 as usize).map(u32::from_le_bytes)
}

/// The hours each archetype of the map-types body `model` is on, in the order the body states them: every hour for one that is no time archetype; `None` where the body does not read.
pub fn hours_of(model: &RbmModel) -> Option<Vec<u32>> {
    let timed = states(model, TIME_ARCHETYPE, TIME_FLAGS);
    hours_in(&model.data_blocks, model.root_block_id, timed)
}

/// The same from the blocks, `timed` saying whether the class table puts the hours where they are read.
fn hours_in(blocks: &[RbmDataBlock], root_id: u32, timed: bool) -> Option<Vec<u32>> {
    let root = root(blocks, root_id)?;
    let raw = u64::from_le_bytes(bytes(root, ARCHETYPES)?);
    let count = usize::from(u16::from_le_bytes(bytes(root, ARCHETYPES + 8)?));
    if count == 0 {
        return Some(Vec::new());
    }
    let (list, from) = target(blocks, raw)?;
    (0..count)
        .map(|k| {
            let pointer = u64::from_le_bytes(bytes(&list.data, from + 8 * k)?);
            let (block, at) = target(blocks, pointer)?;
            if block.data_type != TIME_ARCHETYPE {
                return Some(ALL_HOURS);
            }
            let flags = bytes(&block.data, at + TIME_FLAGS.1 as usize)?;
            timed.then_some(u32::from_le_bytes(flags) & ALL_HOURS)
        })
        .collect()
}

#[cfg(test)]
#[path = "index_types_tests.rs"]
mod tests;

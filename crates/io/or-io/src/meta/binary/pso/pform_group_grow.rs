//! Where the pairs of a GTA VI P-form group map go: its groups, each key's slot and control byte,
//! and the stored slot a tree's pair is saved into.
//!
//! ⚠ A slot's control byte is byte `j ^ 3` of its group's sixteen: seen only for slot 0, and
//! taken for later slots from the table form, whose words are not byte-swapped.

use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;

use super::pform_group::GroupPairs;
use super::schema::{string_subtype, PsoDataType, PsoStructEntry};

/// The slots of a group.
pub(super) const SLOTS: usize = 16;
/// A table doubles its groups before it holds this many pairs per group.
const LOAD: usize = 14;

fn refused(why: impl std::fmt::Display) -> MetaError {
    MetaError::Other(format!("pso: a GTA VI group map {why}"))
}

/// Whether `key` is the 32-bit string hash whose place in a group map is known.
pub(super) fn hashed_key(key: &PsoStructEntry) -> bool {
    key.data_type == PsoDataType::String && key.subtype() == string_subtype::HASH
}

/// The 32-bit string hash a key's value states, which is also its hash in the table.
pub(super) fn key_hash(key: &Value) -> Result<u32> {
    match key {
        Value::Hash(h) => Ok(h.hash),
        Value::UInt(n) => u32::try_from(*n)
            .map_err(|_| refused(format!("key {n} does not fit a 32-bit string hash"))),
        Value::String(s) => Ok(crate::string_hash(s.as_bytes())),
        _ => Err(refused("key is not a string hash")),
    }
}

/// The groups a table of `pairs` takes: the least power of two from 2 with `pairs < 14 x groups`.
pub(super) fn groups_for(pairs: usize) -> Result<usize> {
    let too_many = || refused("holds more pairs than a table counts");
    let mut groups = 2usize;
    while LOAD.checked_mul(groups).is_some_and(|room| pairs >= room) {
        groups = groups.checked_mul(2).ok_or_else(too_many)?;
    }
    Ok(groups)
}

/// The control byte of a full slot holding `key`; an empty slot holds 0x80.
pub(super) fn control(key: u32) -> u8 {
    (key & 0x7F) as u8
}

/// Where slot `j`'s control byte sits among its group's sixteen.
pub(super) fn control_at(j: usize) -> usize {
    j ^ 3
}

/// Each of `keys`' slot, `group * 16 + j`, inserted in order into `groups` empty groups: from the
/// home group `(key >> 7) & (groups - 1)` on to the first that is not full, its next free slot.
pub(super) fn slots(keys: &[u32], groups: usize) -> Result<Vec<usize>> {
    if !groups.is_power_of_two() {
        return Err(refused(format!(
            "of {groups} groups, which is not a power of two"
        )));
    }
    let mut full = vec![0usize; groups];
    let mut seen = std::collections::HashSet::with_capacity(keys.len());
    let mut out = Vec::with_capacity(keys.len());
    for &key in keys {
        if !seen.insert(key) {
            return Err(refused(format!("holds key {key:#010X} twice")));
        }
        let mut group = ((key >> 7) as usize) & (groups - 1);
        let mut tried = 1;
        while full[group] == SLOTS {
            if tried == groups {
                return Err(refused(format!("of {groups} groups is full")));
            }
            group = (group + 1) & (groups - 1);
            tried += 1;
        }
        out.push(group * SLOTS + full[group]);
        full[group] += 1;
    }
    Ok(out)
}

/// Where each of `pairs` is saved in the stored `map`: the slot holding its key where that key
/// is a string hash, else the slot at its place.
// ⚠ A table laid out anew holds its pairs in slot order, which need not be the tree's; a key the
// table does not hold lays the map out anew.
pub(super) fn pair_slots(
    map: &GroupPairs<'_>,
    psin: &[u8],
    pairs: &[(Value, Value)],
) -> Result<Vec<usize>> {
    let hashed = hashed_key(&map.key);
    let offset = map.key.offset() as usize;
    let mut taken = vec![false; map.slots.len()];
    let mut out = Vec::with_capacity(pairs.len());
    for (i, (key, _)) in pairs.iter().enumerate() {
        let slot = if hashed {
            let hash = key_hash(key)?;
            let want = hash.to_be_bytes();
            (map.slots.iter())
                .position(|&at| psin.get(at + offset..at + offset + 4) == Some(&want[..]))
                .ok_or_else(|| {
                    MetaError::OutgrewItsLayout(format!(
                        "a GTA VI group map gained key {hash:#010X}, which its table does not hold"
                    ))
                })?
        } else if i < map.slots.len() {
            i
        } else {
            return Err(refused(format!("holds {} pairs", map.slots.len())));
        };
        if std::mem::replace(&mut taken[slot], true) {
            return Err(refused(format!(
                "is given pair {i} for a slot already given one"
            )));
        }
        out.push(map.slots[slot]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_doubles_its_groups_once_it_holds_fourteen_pairs_a_group() {
        for (pairs, groups) in [(0, 2), (1, 2), (27, 2), (28, 4), (55, 4), (56, 8), (111, 8)] {
            assert_eq!(groups_for(pairs).expect("a count"), groups, "{pairs} pairs");
        }
    }

    #[test]
    fn a_key_takes_its_home_group_then_the_next_one_with_a_free_slot() {
        let home = |key: u32, groups: usize| ((key >> 7) as usize) & (groups - 1);
        // Seventeen keys of home group 1 of 2: sixteen fill it, the last wraps to group 0.
        let keys: Vec<u32> = (0..17).map(|i| (i << 8) | 0x80 | i).collect();
        assert!(keys.iter().all(|&k| home(k, 2) == 1));
        let placed = slots(&keys, 2).expect("placed");
        let expected: Vec<usize> = (SLOTS..2 * SLOTS).chain([0]).collect();
        assert_eq!(placed, expected);
        assert!(slots(&[5, 9, 5], 2)
            .expect_err("a repeat")
            .to_string()
            .contains("twice"));
        assert_eq!(control(0xC318_70ED), 0x6D);
        assert_eq!((0..4).map(control_at).collect::<Vec<_>>(), [3, 2, 1, 0]);
    }
}

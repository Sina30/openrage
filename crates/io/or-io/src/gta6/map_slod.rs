//! Which archetype each far stand-in id names. No map body states it: each list of far stand-ins
//! states the ids its instances bear and, as a set, the archetypes it stands in for, and the one
//! table that agrees with every list is solved by intersection and elimination over all of them.
//!
//! ⚠ Two ids that always occur together with the same two archetypes cannot be told apart by any
//! list: each takes one of the two, so every list still draws its own archetypes.

use std::collections::{BTreeMap, BTreeSet};

use super::StreamGrassList;

/// What one far stand-in list says of its ids: the distinct ids its instances bear and the words of the archetypes it stands in for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlodClaim {
    /// Ascending, each once.
    pub ids: Vec<u16>,
    /// Ascending, each once.
    pub words: Vec<u64>,
}

impl SlodClaim {
    /// The claim of `list`; `None` for a list that is no far stand-in.
    pub fn of(list: &StreamGrassList) -> Option<Self> {
        if !list.slod {
            return None;
        }
        let ids: BTreeSet<u16> = list.instances.iter().map(|i| i.guid).collect();
        // ⚠ The 32-bit names come in another order than the words in many lists: only the
        // words, as a set, are taken.
        let words: BTreeSet<u64> = list.archetypes.iter().map(|(_, word)| *word).collect();
        Some(Self {
            ids: ids.into_iter().collect(),
            words: words.into_iter().collect(),
        })
    }
}

/// The archetype word each far stand-in id names, by the id: 0 for an id no list settles.
pub fn solve(claims: &[SlodClaim]) -> Vec<u64> {
    // Each id can only name a word every list that holds it states.
    let mut can: BTreeMap<u16, BTreeSet<u64>> = BTreeMap::new();
    for claim in claims {
        let words: BTreeSet<u64> = claim.words.iter().copied().collect();
        for id in &claim.ids {
            (can.entry(*id))
                .and_modify(|held| held.retain(|w| words.contains(w)))
                .or_insert_with(|| words.clone());
        }
    }
    let mut fixed: BTreeMap<u16, u64> = BTreeMap::new();
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    loop {
        let mut found: Vec<(u16, u64)> = (can.iter())
            .filter(|(id, words)| words.len() == 1 && !fixed.contains_key(*id))
            .filter_map(|(id, words)| Some((*id, *words.first()?)))
            .collect();
        // Inside one list, a word only one of its open ids can still take is that id's.
        for claim in claims {
            for word in claim.words.iter().filter(|w| !taken.contains(*w)) {
                let mut open = (claim.ids.iter())
                    .filter(|id| !fixed.contains_key(*id))
                    .filter(|id| can.get(*id).is_some_and(|words| words.contains(word)));
                if let (Some(id), None) = (open.next(), open.next()) {
                    found.push((*id, *word));
                }
            }
        }
        let mut changed = false;
        for (id, word) in found {
            if fixed.contains_key(&id) || taken.contains(&word) {
                continue;
            }
            settle(&mut can, &mut fixed, &mut taken, id, word);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    // What stays open is ids that always occur together with as many words: each takes the
    // least of them no other id took.
    let open: Vec<u16> = (can.keys().copied())
        .filter(|id| !fixed.contains_key(id))
        .collect();
    for id in open {
        let word = (can.get(&id).into_iter().flatten()).find(|w| !taken.contains(*w));
        if let Some(word) = word.copied() {
            settle(&mut can, &mut fixed, &mut taken, id, word);
        }
    }
    let len = fixed.keys().next_back().map_or(0, |id| usize::from(*id) + 1);
    let mut table = vec![0; len];
    for (id, word) in fixed {
        table[usize::from(id)] = word;
    }
    tracing::debug!(
        ids = can.len(),
        settled = table.iter().filter(|w| **w != 0).count(),
        "far stand-in ids solved"
    );
    table
}

/// Fix `id` to `word`: no other id can name it now.
fn settle(
    can: &mut BTreeMap<u16, BTreeSet<u64>>,
    fixed: &mut BTreeMap<u16, u64>,
    taken: &mut BTreeSet<u64>,
    id: u16,
    word: u64,
) {
    fixed.insert(id, word);
    taken.insert(word);
    for (other, words) in can.iter_mut() {
        match *other == id {
            true => *words = BTreeSet::from([word]),
            false => {
                words.remove(&word);
            }
        }
    }
}

#[cfg(test)]
#[path = "map_slod_tests.rs"]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use super::super::{GrassInstance, StreamGrassList};
use super::*;
use crate::gta6::map::Aabb;

const A: u64 = 0xA0A0_0000_0000_0001;
const B: u64 = 0xB0B0_0000_0000_0002;
const C: u64 = 0xC0C0_0000_0000_0003;
const D: u64 = 0xD0D0_0000_0000_0004;

fn claim(ids: &[u16], words: &[u64]) -> SlodClaim {
    SlodClaim {
        ids: ids.to_vec(),
        words: words.to_vec(),
    }
}

/// The words `table` gives the ids of `claim`, as a set.
fn drawn(table: &[u64], claim: &SlodClaim) -> BTreeSet<u64> {
    (claim.ids.iter())
        .map(|id| table.get(usize::from(*id)).copied().unwrap_or(0))
        .collect()
}

/// The words `claim` states, as a set.
fn own(claim: &SlodClaim) -> BTreeSet<u64> {
    claim.words.iter().copied().collect()
}

fn list(slod: bool, guids: &[u16], archetypes: &[(u32, u64)]) -> StreamGrassList {
    let instance = |guid| GrassInstance {
        position: [0; 3],
        normal: [128; 2],
        colour: [0; 3],
        scale: 0,
        scale_z: 0,
        rotation: 0,
        guid,
    };
    StreamGrassList {
        archetype_word: 1,
        archetype_name: 1,
        position: [0.0; 3],
        lod_dist: 100.0,
        lod_level: 2,
        batch_aabb: Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        },
        scale_range: [1.0, 1.0],
        lod_fade_start_dist: 100.0,
        instance_lod_dist: 100.0,
        orient_to_terrain: 1.0,
        slod,
        archetypes: archetypes.to_vec(),
        instances: guids.iter().map(|g| instance(*g)).collect(),
    }
}

#[test]
fn a_claim_takes_each_id_and_each_word_once_and_only_from_a_far_stand_in_list() {
    // The names come in another order than the words: only the words are taken.
    let far = list(true, &[9, 3, 9, 3, 7], &[(0x22, C), (0x11, A), (0x33, C)]);
    assert_eq!(SlodClaim::of(&far), Some(claim(&[3, 7, 9], &[A, C])));
    assert_eq!(SlodClaim::of(&list(false, &[0, 0], &[])), None);
}

#[test]
fn an_id_is_fixed_by_intersection_and_a_fixed_word_leaves_every_other_id() {
    // 1 is A by the two lists that hold it; then 2 is B and 3 is C.
    let claims = [claim(&[1, 2], &[A, B]), claim(&[1, 3], &[A, C])];
    assert_eq!(solve(&claims), vec![0, A, B, C]);
}

#[test]
fn a_word_only_one_open_id_of_a_list_can_take_is_that_ids() {
    // 3 can be A, B or C by intersection alone, but A and B are all 1 and 2 can be.
    let claims = [claim(&[1, 2, 3], &[A, B, C]), claim(&[1, 2], &[A, B])];
    let table = solve(&claims);
    assert_eq!(table.get(3), Some(&C));
    for c in &claims {
        assert_eq!(drawn(&table, c), own(c));
    }
}

#[test]
fn a_swap_pair_no_list_splits_takes_its_two_words_one_each() {
    // 4 and 5 always occur together with D and C: no list tells which is which.
    let claims = [
        claim(&[1, 4, 5], &[A, C, D]),
        claim(&[2, 4, 5], &[B, C, D]),
        claim(&[1, 2], &[A, B]),
        claim(&[1], &[A]),
    ];
    let table = solve(&claims);
    assert_eq!((table[1], table[2]), (A, B));
    let pair = BTreeSet::from([table[4], table[5]]);
    assert_eq!(pair, BTreeSet::from([C, D]));
    // Either order draws every list's own archetypes.
    for c in &claims {
        assert_eq!(drawn(&table, c), own(c));
    }
    // And the choice does not hang on the order the lists come in.
    let mut reversed = claims.to_vec();
    reversed.reverse();
    assert_eq!(solve(&reversed), table);
}

#[test]
fn an_id_two_lists_contradict_names_nothing() {
    let table = solve(&[claim(&[1], &[A]), claim(&[1], &[B]), claim(&[2], &[C])]);
    assert_eq!(table, vec![0, 0, C]);
    assert_eq!(solve(&[]), Vec::<u64>::new());
}

/// The next number of a xorshift sequence.
fn next(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

#[test]
fn a_made_up_world_is_solved_to_its_truth_but_for_ids_no_list_tells_apart() {
    const IDS: u64 = 40;
    let truth = |id: u16| 0x5000_0000_0000_0000 | (u64::from(id) * 0x9E37_79B9 + 1);
    let mut x = 0x2545_F491_4F6C_DD1D_u64;
    let mut claims = Vec::new();
    for _ in 0..120 {
        let size = 1 + next(&mut x) % 6;
        let ids: BTreeSet<u16> = (0..size).map(|_| (next(&mut x) % IDS) as u16).collect();
        let words: BTreeSet<u64> = ids.iter().map(|id| truth(*id)).collect();
        claims.push(SlodClaim {
            ids: ids.into_iter().collect(),
            words: words.into_iter().collect(),
        });
    }
    let table = solve(&claims);
    // Ids that occur in the same lists, and only those, cannot be told apart.
    let mut lists_of: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (at, c) in claims.iter().enumerate() {
        for id in &c.ids {
            lists_of.entry(*id).or_default().push(at);
        }
    }
    for (id, lists) in &lists_of {
        let twin = (lists_of.iter()).any(|(other, theirs)| other != id && theirs == lists);
        let got = table.get(usize::from(*id)).copied().unwrap_or(0);
        assert_ne!(got, 0, "id {id} is settled");
        assert!(
            twin || got == truth(*id),
            "id {id} has no twin and is wrong"
        );
    }
    for c in &claims {
        assert_eq!(drawn(&table, c), own(c));
    }
    // A control: the same lists with each list's ids moved to another list contradict it.
    let mut ids: Vec<Vec<u16>> = claims.iter().map(|c| c.ids.clone()).collect();
    ids.rotate_left(1);
    let moved: Vec<SlodClaim> = (claims.iter().zip(ids))
        .map(|(c, ids)| SlodClaim {
            ids,
            words: c.words.clone(),
        })
        .collect();
    let wrong = solve(&moved);
    let exact = (moved.iter())
        .filter(|c| drawn(&wrong, c) == own(c))
        .count();
    assert!(
        exact < claims.len() / 2,
        "{exact} of {} lists",
        claims.len()
    );
}

//! The allocator: where each garment sits among the drawables of its slot.
//!
//! A garment keeps the position somebody pinned it at, whatever happens around it, and one with no
//! pin takes the lowest place no pin holds, in the order of the pack's list. So a pack of pinned
//! garments never renumbers, and a pack of unpinned ones renumbers when a garment before another
//! leaves, which is what the build's diff reports with a migration map.
//!
//! A place is a number among every drawable of a slot of one ped. A collection holds as many of
//! them as the cap of the pack (128 unless the project says another, 255 at the most), so a slot
//! past the cap is written as several collections: place 130 of a cap of 128 is drawable 2 of the
//! second. Both are in [`Placed`], and a pin states the place.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Wardrobe;
use crate::names::{Ped, Slot};

/// The last drawable a collection holds of a slot: a selection set states 255 for none, so a drawable is one of 255.
pub const MAX_POSITION: u32 = 254;

/// The most collections one ped's slot is cut into. A bound OpenRage sets to keep a pin from asking for millions; the engine states none.
pub const MAX_PARTS: u32 = 64;

/// A garment at its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Its index in the pack's list.
    pub garment: usize,
    pub ped: Ped,
    pub slot: Slot,
    /// Its drawable number in its collection, which is the number the files and the metadata give it.
    pub position: u32,
    /// Which collection of the ped holds it: 0 is the pack's own and n is `<collection>_<n>`.
    pub part: u32,
    /// Its place among every drawable of the slot, the number a pin states.
    pub place: u32,
}

/// Why a garment has no place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Another garment is pinned at the same place.
    Taken { by: usize },
    /// The place is past the last one a slot holds.
    PastTheEnd,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allocation {
    /// Every garment that has a place, by ped, slot and place.
    pub placed: Vec<Placed>,
    /// Every garment that could not have one, with the reason.
    pub refused: Vec<(usize, Refused)>,
}

impl Allocation {
    pub fn of(&self, garment: usize) -> Option<&Placed> {
        self.placed.iter().find(|p| p.garment == garment)
    }

    /// The garments of one slot of one ped, by place, whichever collection holds them.
    pub fn in_slot(&self, ped: Ped, slot: Slot) -> impl Iterator<Item = &Placed> {
        self.placed
            .iter()
            .filter(move |p| p.ped == ped && p.slot == slot)
    }

    /// The garments of one slot of one collection of a ped, by position.
    pub fn in_part(&self, ped: Ped, part: u32, slot: Slot) -> impl Iterator<Item = &Placed> {
        self.in_slot(ped, slot).filter(move |p| p.part == part)
    }

    /// The collections of `ped` that hold a garment, the pack's own first.
    pub fn parts_of(&self, ped: Ped) -> Vec<u32> {
        let parts: BTreeSet<u32> = (self.placed.iter())
            .filter(|p| p.ped == ped)
            .map(|p| p.part)
            .collect();
        parts.into_iter().collect()
    }
}

/// How many drawables a collection of `ped` holds of a slot, and how many collections a slot is cut into.
fn room(wardrobe: &Wardrobe, ped: Ped) -> (u32, u32) {
    match wardrobe.replaces(ped) {
        // ⚠ The files of a game collection are the game's numbers: there is no `_1` to cut into.
        Some(_) => (MAX_POSITION + 1, 1),
        None => (wardrobe.rules.slot_cap(), MAX_PARTS),
    }
}

/// How many places one slot of `ped` holds in all: the cap of a collection, times the collections the slot may be cut into.
pub fn places_of(wardrobe: &Wardrobe, ped: Ped) -> u32 {
    let (cap, parts) = room(wardrobe, ped);
    cap * parts
}

/// Place the garments of `wardrobe` that have a ped and a slot.
pub fn allocate(wardrobe: &Wardrobe) -> Allocation {
    let mut groups: BTreeMap<(Ped, Slot), Vec<usize>> = BTreeMap::new();
    for (index, garment) in wardrobe.garments.iter().enumerate() {
        if let (Some(ped), Some(slot)) = (garment.ped, garment.slot) {
            groups.entry((ped, slot)).or_default().push(index);
        }
    }
    let mut found = Allocation::default();
    for ((ped, slot), members) in groups {
        let (cap, _) = room(wardrobe, ped);
        let end = places_of(wardrobe, ped);
        let mut taken: BTreeMap<u32, usize> = BTreeMap::new();
        for &index in &members {
            let Some(position) = wardrobe.garments[index].position else {
                continue;
            };
            match taken.get(&position) {
                Some(&by) => found.refused.push((index, Refused::Taken { by })),
                None if position >= end => found.refused.push((index, Refused::PastTheEnd)),
                None => {
                    taken.insert(position, index);
                }
            }
        }
        let pinned: BTreeSet<u32> = taken.keys().copied().collect();
        let mut free = (0..end).filter(|p| !pinned.contains(p));
        for &index in &members {
            if wardrobe.garments[index].position.is_some() {
                continue;
            }
            match free.next() {
                Some(position) => {
                    taken.insert(position, index);
                }
                None => found.refused.push((index, Refused::PastTheEnd)),
            }
        }
        found
            .placed
            .extend(taken.into_iter().map(|(place, garment)| Placed {
                garment,
                ped,
                slot,
                position: place % cap,
                part: place / cap,
                place,
            }));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Garment;
    use crate::names::Component;

    const JBIB: Slot = Slot::Component(Component::Jbib);

    fn garment(ped: Ped, slot: Slot, position: Option<u32>) -> Garment {
        let mut g = Garment::new();
        g.ped = Some(ped);
        g.slot = Some(slot);
        g.position = position;
        g
    }

    fn positions(allocation: &Allocation) -> Vec<(usize, u32)> {
        allocation
            .placed
            .iter()
            .map(|p| (p.garment, p.position))
            .collect()
    }

    #[test]
    fn a_garment_with_no_pin_takes_the_lowest_place_no_pin_holds() {
        let mut w = Wardrobe::new("p");
        w.garments = vec![
            garment(Ped::Male, JBIB, None),
            garment(Ped::Male, JBIB, Some(0)),
            garment(Ped::Male, JBIB, None),
            garment(Ped::Male, JBIB, Some(2)),
        ];
        let found = allocate(&w);
        assert_eq!(positions(&found), [(1, 0), (0, 1), (3, 2), (2, 3)]);
        assert!(found.refused.is_empty());
    }

    #[test]
    fn a_pinned_garment_does_not_move_when_garments_come_and_go_around_it() {
        let mut w = Wardrobe::new("p");
        let mut pinned = garment(Ped::Female, JBIB, Some(5));
        pinned.label = Some("pinned".into());
        w.garments = vec![
            garment(Ped::Female, JBIB, None),
            pinned.clone(),
            garment(Ped::Female, JBIB, None),
        ];
        let before = allocate(&w);
        assert_eq!(before.of(1).unwrap().position, 5);

        // One before it leaves and another comes in: the pin stays, the unpinned shift.
        w.garments.remove(0);
        w.garments.insert(0, garment(Ped::Female, JBIB, None));
        w.garments.insert(0, garment(Ped::Female, JBIB, None));
        let after = allocate(&w);
        let at = w.garments.iter().position(|g| g.id == pinned.id).unwrap();
        assert_eq!(after.of(at).unwrap().position, 5);
    }

    #[test]
    fn an_unpinned_garment_moves_when_one_before_it_leaves() {
        let mut w = Wardrobe::new("p");
        w.garments = (0..3).map(|_| garment(Ped::Male, JBIB, None)).collect();
        let last = w.garments[2].id.clone();
        assert_eq!(allocate(&w).of(2).unwrap().position, 2);
        w.garments.remove(0);
        let at = w.garments.iter().position(|g| g.id == last).unwrap();
        assert_eq!(
            allocate(&w).of(at).unwrap().position,
            1,
            "the renumbering a diff reports"
        );
    }

    #[test]
    fn two_garments_pinned_at_one_place_leave_the_second_without_one() {
        let mut w = Wardrobe::new("p");
        w.garments = vec![
            garment(Ped::Male, JBIB, Some(3)),
            garment(Ped::Male, JBIB, Some(3)),
            garment(Ped::Male, JBIB, None),
        ];
        let found = allocate(&w);
        assert_eq!(found.refused, [(1, Refused::Taken { by: 0 })]);
        assert_eq!(positions(&found), [(2, 0), (0, 3)]);
    }

    fn with_cap(w: &mut Wardrobe, cap: u64) {
        w.rules.set("slot-cap", |s| s.threshold = Some(cap));
    }

    #[test]
    fn a_slot_past_the_cap_goes_on_in_the_next_collection_and_a_pin_states_the_place_in_the_slot() {
        let mut w = Wardrobe::new("p");
        w.garments = (0..130).map(|_| garment(Ped::Male, JBIB, None)).collect();
        w.garments.push(garment(Ped::Male, JBIB, Some(300)));
        let found = allocate(&w);
        assert!(found.refused.is_empty());
        let at = |i: usize| {
            let p = found.of(i).unwrap();
            (p.part, p.position, p.place)
        };
        assert_eq!(at(127), (0, 127, 127));
        assert_eq!(
            at(128),
            (1, 0, 128),
            "the first drawable of the next collection"
        );
        assert_eq!(at(129), (1, 1, 129));
        assert_eq!(
            at(130),
            (2, 44, 300),
            "300 is drawable 44 of the third collection"
        );
        assert_eq!(found.parts_of(Ped::Male), [0, 1, 2]);
        assert_eq!(found.in_part(Ped::Male, 1, JBIB).count(), 2);
        assert_eq!(found.in_slot(Ped::Male, JBIB).count(), 131);
    }

    #[test]
    fn the_cap_is_the_projects_between_1_and_255_and_a_pack_that_states_none_has_128() {
        let mut w = Wardrobe::new("p");
        w.garments = (0..130).map(|_| garment(Ped::Male, JBIB, None)).collect();
        assert_eq!(allocate(&w).parts_of(Ped::Male), [0, 1]);
        with_cap(&mut w, 255);
        assert_eq!(allocate(&w).parts_of(Ped::Male), [0]);
        with_cap(&mut w, 40);
        assert_eq!(allocate(&w).parts_of(Ped::Male), [0, 1, 2, 3]);
        with_cap(&mut w, 9999);
        assert_eq!(w.rules.slot_cap(), 255, "held at what a collection holds");
        with_cap(&mut w, 0);
        assert_eq!(w.rules.slot_cap(), 1);
    }

    #[test]
    fn a_slot_holds_no_more_than_the_collections_a_pack_may_cut_it_into() {
        let mut w = Wardrobe::new("p");
        with_cap(&mut w, 1);
        w.garments = (0..66).map(|_| garment(Ped::Male, JBIB, None)).collect();
        w.garments.push(garment(Ped::Male, JBIB, Some(300)));
        let found = allocate(&w);
        assert_eq!(found.placed.len(), MAX_PARTS as usize);
        assert_eq!(
            found.refused.len(),
            3,
            "two past the end of the free places, one pinned past it"
        );
        assert!(found
            .refused
            .iter()
            .all(|(_, why)| *why == Refused::PastTheEnd));
    }

    #[test]
    fn the_files_of_a_game_collection_are_cut_nowhere_and_hold_the_games_255_numbers() {
        let mut w = Wardrobe::new("p");
        w.rules
            .replace
            .insert(Ped::Male, crate::settings::Replace::default());
        w.garments = (0..130).map(|_| garment(Ped::Male, JBIB, None)).collect();
        w.garments.push(garment(Ped::Male, JBIB, Some(300)));
        let found = allocate(&w);
        assert_eq!(found.parts_of(Ped::Male), [0]);
        assert_eq!(found.placed.len(), 130);
        assert_eq!(found.refused, [(130, Refused::PastTheEnd)]);
    }

    #[test]
    fn peds_and_slots_are_places_of_their_own_and_a_garment_with_neither_has_none() {
        let mut w = Wardrobe::new("p");
        let mut unplaced = Garment::new();
        unplaced.slot = Some(JBIB);
        w.garments = vec![
            garment(Ped::Male, JBIB, None),
            garment(Ped::Female, JBIB, None),
            garment(Ped::Male, Slot::Prop(crate::names::Prop::Head), None),
            unplaced,
        ];
        let found = allocate(&w);
        assert_eq!(positions(&found), [(0, 0), (2, 0), (1, 0)]);
        assert!(found.of(3).is_none());
    }
}

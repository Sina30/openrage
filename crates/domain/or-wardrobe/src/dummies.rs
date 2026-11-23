//! Dummies: garments that hold a place and ship no model, and the choices that decide where the places go.
//!
//! The game counts a drawable for every number up to the last one a slot holds, so a place nobody fills is still a
//! drawable, and one that disappears moves every garment after it. A dummy keeps a place on purpose. Four choices
//! use them: a garment added to a slot may take a dummy's place, the holes of a slot may be filled with dummies,
//! the dummies of a slot may be moved after its garments, and a garment taken out may leave a dummy behind.
//!
//! Every function here changes a pack that is in memory and nothing else; the screen that calls it keeps what it
//! changed as one step of Undo.

use crate::alloc::{allocate, Allocation, Placed};
use crate::edit::Error;
use crate::model::{Garment, Wardrobe};
use crate::names::{Ped, Slot};

/// The label a dummy made here carries, so the list shows what it is.
pub const LABEL: &str = "Dummy";

/// A dummy pinned at `place` of `slot` of `ped`.
fn dummy(ped: Ped, slot: Slot, place: u32) -> Garment {
    let mut g = Garment::new();
    g.label = Some(LABEL.to_string());
    g.ped = Some(ped);
    g.slot = Some(slot);
    g.position = Some(place);
    g
}

/// The garments of one slot of one ped, by place.
fn in_slot(allocation: &Allocation, ped: Ped, slot: Slot) -> Vec<Placed> {
    allocation.in_slot(ped, slot).copied().collect()
}

/// A garment taken out of a pack, with what Undo needs to put it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Taken {
    /// Where it stood in the pack's list.
    pub at: usize,
    pub garment: Garment,
    /// What it was in the sets of alternate variations.
    pub sets: crate::alternate::Forgotten,
    /// The id of the dummy that holds its place now, none where nothing does.
    pub left: Option<String>,
}

/// Take garment `id` out of the pack; with `leave` on, a dummy takes its place so that the garments after it keep theirs.
///
/// A garment with no place, and a dummy, leave nothing: there is no place to hold.
pub fn take_out(wardrobe: &mut Wardrobe, id: &str, leave: bool) -> Result<Taken, Error> {
    let at = (wardrobe.garments.iter())
        .position(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let place = allocate(wardrobe).of(at).copied();
    let garment = wardrobe.garments.remove(at);
    let sets = crate::alternate::forget(wardrobe, id);
    let mut taken = Taken {
        at,
        garment,
        sets,
        left: None,
    };
    if let Some(place) = place.filter(|_| leave && !taken.garment.is_reserved()) {
        let held = dummy(place.ped, place.slot, place.place);
        taken.left = Some(held.id.clone());
        wardrobe.garments.insert(at, held);
    }
    Ok(taken)
}

/// Give each of the garments `added` the place of a dummy in its slot, and let the dummy go; the dummies it took the places of, with where they stood in the list, so that Undo can put them back.
///
/// A garment with no pin takes the first dummy of its slot, by place. One pinned at a place a dummy holds takes that place, which
/// would otherwise be a clash. A garment with no ped or slot, a dummy, and a slot with no dummy are left as they are.
pub fn replace_on_add(wardrobe: &mut Wardrobe, added: &[String]) -> Vec<(usize, Garment)> {
    let mut taken = Vec::new();
    for id in added {
        let Some(at) = wardrobe.garments.iter().position(|g| &g.id == id) else {
            continue;
        };
        let g = &wardrobe.garments[at];
        let (Some(ped), Some(slot)) = (g.ped, g.slot) else {
            continue;
        };
        if g.is_reserved() {
            continue;
        }
        let allocation = allocate(wardrobe);
        let held: Vec<Placed> = in_slot(&allocation, ped, slot)
            .into_iter()
            .filter(|p| p.garment != at && wardrobe.garments[p.garment].is_reserved())
            .collect();
        let target = match g.position {
            Some(pin) => held.iter().find(|p| p.place == pin),
            None => held.first(),
        };
        let Some(target) = target.copied() else {
            continue;
        };
        wardrobe.garments[at].position = Some(target.place);
        let gone = wardrobe.garments.remove(target.garment);
        taken.push((target.garment, gone));
    }
    taken
}

/// Fill the holes of a slot of `ped` with dummies: every number below the last garment of each collection that nothing holds. Returns how many it made.
///
/// ⚠ The game counts these as drawables already, so filling changes what is written by nothing; it makes the places
/// garments of their own, which nothing added later can take unless the pack replaces a dummy on add.
pub fn fill(wardrobe: &mut Wardrobe, ped: Ped, slot: Slot) -> usize {
    let allocation = allocate(wardrobe);
    let held = in_slot(&allocation, ped, slot);
    let cap = wardrobe.rules.slot_cap().max(1);
    let mut made = Vec::new();
    for part in held
        .iter()
        .map(|p| p.part)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let last = (held.iter().filter(|p| p.part == part))
            .map(|p| p.position)
            .max()
            .unwrap_or(0);
        for position in 0..=last {
            if !held
                .iter()
                .any(|p| p.part == part && p.position == position)
            {
                made.push(dummy(ped, slot, part * cap + position));
            }
        }
    }
    let count = made.len();
    wardrobe.garments.extend(made);
    count
}

/// Put the dummies of a slot of `ped` after its garments: the garments that ship a model take the lowest places in the order they have, and the dummies follow in theirs. Returns how many garments changed place.
///
/// ⚠ Every garment of the slot is pinned at its new place, since a pin is how a place is stated, and a frozen pack
/// refuses the renumbering unless a reason says why.
pub fn resort(wardrobe: &mut Wardrobe, ped: Ped, slot: Slot) -> usize {
    let allocation = allocate(wardrobe);
    let held = in_slot(&allocation, ped, slot);
    let (real, dummies): (Vec<Placed>, Vec<Placed>) = held
        .iter()
        .partition(|p| !wardrobe.garments[p.garment].is_reserved());
    let mut moved = 0;
    for (place, p) in (0u32..).zip(real.iter().chain(&dummies)) {
        if p.place != place {
            moved += 1;
        }
        wardrobe.garments[p.garment].position = Some(place);
    }
    moved
}

/// How many dummies hold a place in each slot, for a screen that offers to fill or sort them.
pub fn count(wardrobe: &Wardrobe, ped: Ped, slot: Slot) -> usize {
    (wardrobe.garments.iter())
        .filter(|g| g.ped == Some(ped) && g.slot == Some(slot) && g.is_reserved())
        .count()
}

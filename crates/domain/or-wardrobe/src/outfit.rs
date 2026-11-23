//! What the game streams for the heaviest outfit a pack can make: one garment in each slot of a ped, the heaviest of each.
//!
//! The memory of the whole pack ([`crate::plan`]'s `clothing_memory`) is what every file of it would take if all were
//! streamed at once, which no ped does. A ped wears one drawable in a slot and one texture of it, so the worst a ped
//! can stream is the heaviest garment of every slot together.
//!
//! ⚠ The pages each file states in its own header are all this counts (the same figure the whole pack's memory is made
//! of), so a file nobody counted adds none, and the cloth streams with the model it belongs to.

use std::collections::BTreeMap;

use crate::alloc::Allocation;
use crate::facts::Facts;
use crate::measure::Measure;
use crate::model::{Garment, Wardrobe};
use crate::names::{Ped, Slot};

/// What the heaviest outfit of one ped streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heaviest {
    pub ped: Ped,
    /// The pages the outfit streams, in bytes.
    pub bytes: u64,
    /// The garment of each slot it is made of, by id.
    pub garments: Vec<(Slot, String)>,
}

fn streaming(facts: &Facts, path: Option<&String>) -> u64 {
    path.map_or(0, |p| {
        Measure::of_fact(facts.get(p)).map_or(0, |m| m.streaming)
    })
}

/// What wearing `garment` streams: its model and its cloth, and the heaviest of its textures, since one is worn at a time.
pub fn weight(garment: &Garment, facts: &Facts) -> u64 {
    let texture = (garment.variations.iter())
        .map(|v| streaming(facts, v.texture.as_ref()))
        .max()
        .unwrap_or(0);
    streaming(facts, garment.model.as_ref()) + streaming(facts, garment.cloth.as_ref()) + texture
}

/// The heaviest outfit of each ped the pack dresses.
pub fn heaviest(wardrobe: &Wardrobe, allocation: &Allocation, facts: &Facts) -> Vec<Heaviest> {
    let mut by_slot: BTreeMap<(Ped, Slot), (u64, &str)> = BTreeMap::new();
    for placed in &allocation.placed {
        let garment = &wardrobe.garments[placed.garment];
        let weight = weight(garment, facts);
        let best = by_slot
            .entry((placed.ped, placed.slot))
            .or_insert((weight, garment.id.as_str()));
        if weight > best.0 {
            *best = (weight, garment.id.as_str());
        }
    }
    Ped::ALL
        .into_iter()
        .filter_map(|ped| {
            let slots: Vec<_> = by_slot.iter().filter(|((p, _), _)| *p == ped).collect();
            (!slots.is_empty()).then(|| Heaviest {
                ped,
                bytes: slots.iter().map(|(_, (bytes, _))| bytes).sum(),
                garments: (slots.iter())
                    .map(|((_, slot), (_, id))| (*slot, (*id).to_string()))
                    .collect(),
            })
        })
        .collect()
}

/// The pages the heaviest outfit of the heaviest ped streams: the figure of the meter.
pub fn heaviest_bytes(wardrobe: &Wardrobe, allocation: &Allocation, facts: &Facts) -> u64 {
    heaviest(wardrobe, allocation, facts)
        .iter()
        .map(|h| h.bytes)
        .max()
        .unwrap_or(0)
}

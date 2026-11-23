//! What a garment is found by: the words a search of the list reads, which a person remembers it by.

use crate::alloc::Allocation;
use crate::layout;
use crate::model::Wardrobe;
use crate::rules::name_of;

/// The file name a build gives the model of garment `index`, none while it has no place or ships no model.
pub fn game_name(wardrobe: &Wardrobe, allocation: &Allocation, index: usize) -> Option<String> {
    let place = allocation.of(index)?;
    let garment = wardrobe.garments.get(index)?;
    garment.model.as_ref()?;
    Some(layout::model_name(
        place.ped,
        &wardrobe.collection_of(place.ped, place.part),
        place.slot,
        place.position,
        garment.skin,
    ))
}

/// The words of garment `index` a search reads, each apart: its place, its slot, its name, the name the game gives its model, its group and its tags.
///
/// ⚠ Apart, because letters matched in order across one long line match nearly anything: a query is read against one word at a time.
pub fn terms(wardrobe: &Wardrobe, allocation: &Allocation, index: usize) -> Vec<String> {
    let garment = &wardrobe.garments[index];
    let mut words: Vec<String> = Vec::new();
    match allocation.of(index) {
        Some(place) => {
            words.push(format!("{:03}", place.place));
            words.push(place.slot.name().to_string());
        }
        None => words.extend(garment.slot.map(|s| s.name().to_string())),
    }
    words.push(name_of(garment));
    words.extend(game_name(wardrobe, allocation, index));
    words.extend(garment.group.clone());
    words.extend(garment.tags.iter().cloned());
    words
}

//! The far stand-ins of the maps streamed in, as the card pass takes them: each list's box, scales
//! and distance with its instances as the body stores them, and the species each id names.

use std::f32::consts::SQRT_2;
use std::sync::Arc;

use glam::Vec3;
use or_io::gta6::map::stream::StreamGrassList;

use super::super::index::{ArchetypeRow, ModelRef, WorldIndex};

/// A list of far stand-ins that draws, as the card pass takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct CardList {
    /// The sphere that holds every card of the list, corners included.
    pub centre: [f32; 3],
    pub radius: f32,
    /// The lowest corner of the box each instance's place is a fraction of.
    pub min: [f32; 3],
    /// The size of that box.
    pub size: [f32; 3],
    /// The least and the most scale of an instance.
    pub scale_range: [f32; 2],
    /// How far from the camera an instance draws to, in metres.
    pub distance: f32,
    /// How opaque the list draws, above 0 and to 1.
    pub fade: f32,
    /// Each instance as the body stores it: the u16 at +0x0E is the id of its species.
    pub records: Arc<[[u8; 16]]>,
    /// The ids of the species its instances take, each once.
    pub ids: Arc<[u16]>,
}

/// What a far stand-in id stands in for: an archetype's name and its model's sphere; a name of 0 for an id no list settles.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Species {
    pub archetype: u32,
    /// The model's sphere in its own space; a radius of 0 draws nothing.
    pub centre: [f32; 3],
    pub radius: f32,
}

impl CardList {
    /// The cards of the far stand-in list `list`, whole and at its own distance, by the species of `index`.
    pub fn of(list: &StreamGrassList, index: &WorldIndex) -> Self {
        let (min, max) = (
            Vec3::from(list.batch_aabb.min),
            Vec3::from(list.batch_aabb.max),
        );
        // A card stands off its place by its species' centre and its corner reaches past it by
        // the radius times √2, both at the instance's scale.
        let mut reach = 0.0f32;
        let mut ids = std::collections::BTreeSet::new();
        for instance in list.instances.iter().filter(|i| ids.insert(i.guid)) {
            if let Some(a) = archetype_of(index, usize::from(instance.guid)) {
                reach = reach.max(Vec3::from(a.centre).length() + a.radius * SQRT_2);
            }
        }
        let most = list.scale_range[1];
        Self {
            centre: ((min + max) / 2.0).to_array(),
            radius: (max - min).length() / 2.0 + reach * most,
            min: min.to_array(),
            size: (max - min).to_array(),
            scale_range: list.scale_range,
            distance: list.instance_lod_dist,
            fade: 1.0,
            records: list.instances.iter().map(|i| i.record()).collect(),
            ids: ids.into_iter().collect(),
        }
    }

    /// The same seen with distances scaled by `scale`, `fade` opaque, less the instances `hidden`
    /// names: those a prop drawn near the camera stands in for.
    pub fn seen(&self, scale: f32, fade: f32, hidden: &[u16]) -> Self {
        let mut records = Arc::clone(&self.records);
        if !hidden.is_empty() {
            let mut less = records.to_vec();
            for at in hidden {
                if let Some(record) = less.get_mut(usize::from(*at)) {
                    record[14..16].copy_from_slice(&HIDDEN.to_le_bytes());
                }
            }
            records = less.into();
        }
        Self {
            distance: self.distance * scale,
            fade,
            records,
            ..self.clone()
        }
    }
}

/// The species id a hidden instance is given: no species table reaches it, so its card draws nothing.
pub const HIDDEN: u16 = u16::MAX;

/// The archetype the far stand-in id `id` names.
fn archetype_of(index: &WorldIndex, id: usize) -> Option<&ArchetypeRow> {
    let word = index.slod.get(id).copied().filter(|w| *w != 0)?;
    index.archetype(word)
}

/// Each far stand-in id's species, indexed by the id.
pub fn species(index: &WorldIndex) -> Arc<[Species]> {
    (0..index.slod.len())
        .map(|id| {
            archetype_of(index, id).map_or_else(Species::default, |a| Species {
                archetype: a.name,
                centre: a.centre,
                radius: a.radius,
            })
        })
        .collect()
}

/// The model that draws the species of id `id`, for baking its atlas; `None` where unsolved or modelless.
pub fn model_of(index: &WorldIndex, id: usize) -> Option<ModelRef> {
    index.model(archetype_of(index, id)?)
}

#[cfg(test)]
#[path = "stream_slod_tests.rs"]
mod tests;

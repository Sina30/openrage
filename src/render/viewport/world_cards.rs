//! The world viewer's far stand-ins on the device: the selection's card lists handed to the
//! renderer when the selection changes, those the camera sees within their distance shown, and a
//! species baked into the atlas once its model and its colour maps are on the device.
//!
//! ⚠ A species whose model the world never drew near is read for its atlas alone, after
//! everything that draws: until it is baked its cards draw nothing.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;
use or_renderer::world::cards::{CardBatch, CardMesh, CardSpecies};
use or_renderer::{Frustum, ViewportRenderer, WorldBucket};

use super::textures::ABSENT;
use super::{Slot, WorldGpu};
use crate::state::gta6_world::worldview::index::ModelRef;
use crate::state::gta6_world::worldview::stream::slod::Species;
use crate::state::gta6_world::worldview::stream::Selected;

/// What of the far stand-ins the renderer holds.
#[derive(Default)]
pub(super) struct Cards {
    /// The selection whose lists the renderer holds.
    version: Option<u64>,
    /// The species table the slots were given for, held so that no other comes to lie where it did.
    table: Option<Arc<[Species]>>,
    /// The slot each species id was baked into, and `None` for one that will never bake.
    slots: HashMap<usize, Option<u32>>,
    /// The next slot to bake into: never one baked before, since a bake draws over what a slot holds.
    next: u32,
    /// The lists shown last, by their place in the selection.
    shown: Vec<u32>,
}

/// Whether the camera at `eye` sees some of a list's sphere within its distance; a list faded out draws nothing.
fn seen(
    frustum: &Frustum,
    eye: Vec3,
    (centre, radius): (Vec3, f32),
    distance: f32,
    fade: f32,
) -> bool {
    fade > 0.0 && eye.distance(centre) - radius <= distance && frustum.sees_sphere(centre, radius)
}

impl WorldGpu {
    /// Draw no card until the next selection is handed over.
    pub(super) fn hide_cards(&mut self, renderer: &mut ViewportRenderer) {
        if !self.cards.shown.is_empty() {
            renderer.show_card_lists(&[]);
        }
        self.cards.version = None;
        self.cards.shown.clear();
    }

    /// The meshes of the least detailed level of `model` whose every colour map is on the device
    /// or known never to come, with the colour maps of its least detailed level that have not
    /// arrived put in `absent`; what blends or lies on a surface is left out of a stand-in.
    fn card_meshes<'a>(
        &'a self,
        model: &ModelRef,
        grey: &'a wgpu::TextureView,
        absent: &mut Vec<u64>,
    ) -> Option<Vec<CardMesh<'a>>> {
        let Some(Slot::Ready { lods, .. }) = self.models.get(model) else {
            return None;
        };
        for (tried, lod) in lods.iter().rev().enumerate() {
            let mut meshes = Vec::with_capacity(lod.meshes.len());
            let mut waits = false;
            for mesh in &lod.meshes {
                let Some(material) = self.materials.get(mesh.material as usize) else {
                    continue;
                };
                let cutout = match material.bucket {
                    WorldBucket::Opaque | WorldBucket::Emissive => false,
                    WorldBucket::Cutout => true,
                    WorldBucket::Decal | WorldBucket::Blend => continue,
                };
                let colour = match material.maps[0].map(|key| (key, self.textures.of(key))) {
                    None => None,
                    Some((_, (_, Some(view)))) => Some(view),
                    // Not arrived yet: the level waits for it.
                    Some((key, (ABSENT, None))) => {
                        waits = true;
                        if tried == 0 {
                            absent.push(key);
                        }
                        continue;
                    }
                    // No pack holds it: the stand-in is the mid grey the world draws it with.
                    Some((_, (_, None))) => Some(grey),
                };
                let Some((vertices, indices)) = self.pages.buffers(mesh.place.page) else {
                    waits = true;
                    continue;
                };
                meshes.push(CardMesh {
                    vertices,
                    indices,
                    base_vertex: mesh.place.base_vertex as i32,
                    first_index: mesh.place.first_index,
                    index_count: mesh.place.indices,
                    colour,
                    cutout,
                });
            }
            if !waits && !meshes.is_empty() {
                return Some(meshes);
            }
        }
        None
    }

    /// Hand the renderer the selection's card lists when they change, show those whose sphere the
    /// camera at `eye` sees within their distance, and bake the species whose model and colour
    /// maps are on the device; `model_of` is the model of the species of an id. Gives what the
    /// species of the shown lists wait for, asked for as the frame's models and textures are.
    pub(super) fn refresh_cards(
        &mut self,
        renderer: &mut ViewportRenderer,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        selected: &Selected,
        (frustum, eye): (&Frustum, Vec3),
        model_of: &dyn Fn(usize) -> Option<ModelRef>,
    ) -> CardAsks {
        let fresh = self.cards.version != Some(selected.version);
        if fresh {
            self.cards.version = Some(selected.version);
            let lists: Vec<CardBatch> = (selected.cards.iter())
                .map(|list| CardBatch {
                    min: list.min,
                    size: list.size,
                    scale_range: list.scale_range,
                    distance: list.distance,
                    fade: list.fade,
                    records: Arc::clone(&list.records),
                })
                .collect();
            renderer.set_card_lists(device, queue, &lists);
        }
        let shown: Vec<u32> = (selected.cards.iter().enumerate())
            .filter(|(_, list)| {
                let sphere = (Vec3::from(list.centre), list.radius);
                seen(frustum, eye, sphere, list.distance, list.fade)
            })
            .map(|(at, _)| at as u32)
            .collect();
        // Setting the lists shows none of them.
        if fresh || shown != self.cards.shown {
            renderer.show_card_lists(&shown);
            self.cards.shown = shown;
        }

        let species = &selected.species;
        let mut changed =
            !(self.cards.table.as_ref()).is_some_and(|table| Arc::ptr_eq(table, species));
        if changed {
            self.cards.table = Some(Arc::clone(species));
            self.cards.slots.clear();
        }
        // The model of each species a shown list names and no slot holds, by the nearest list that names it.
        let mut waits: HashMap<usize, f32> = HashMap::new();
        for list in self
            .cards
            .shown
            .iter()
            .filter_map(|at| selected.cards.get(*at as usize))
        {
            let away = (eye.distance(Vec3::from(list.centre)) - list.radius).max(0.0);
            for id in list.ids.iter().map(|id| usize::from(*id)) {
                if !self.cards.slots.contains_key(&id) {
                    let nearest = waits.entry(id).or_insert(away);
                    *nearest = nearest.min(away);
                }
            }
        }
        // ponytail: a few bakes a frame, by id; nearest first if the bakes are seen to lag.
        let (mut settled, mut absent, mut models) = (Vec::new(), Vec::new(), Vec::new());
        let grey = self.textures.grey(device, queue);
        let mut bakes = BAKES_A_FRAME;
        let mut more = false;
        for (id, kind) in species.iter().enumerate() {
            if kind.radius <= 0.0 || self.cards.slots.contains_key(&id) {
                continue;
            }
            let Some(model) = model_of(id) else {
                settled.push((id, None));
                continue;
            };
            match self.models.get(&model) {
                // A model that will never come bakes nothing.
                Some(Slot::Missing) => {
                    settled.push((id, None));
                    continue;
                }
                // A species never drawn near is read for its atlas alone, after what draws.
                None => {
                    if let Some(away) = waits.get(&id) {
                        models.push((ASKED_LAST + away, model));
                    }
                    continue;
                }
                Some(Slot::Ready { .. }) => {}
            }
            let Some(meshes) = self.card_meshes(&model, &grey, &mut absent) else {
                continue;
            };
            if bakes == 0 {
                more = true;
                continue;
            }
            bakes -= 1;
            let slot = self.cards.next;
            let fits = slot < renderer.card_slots(device)
                && renderer.bake_card_slot(device, queue, slot, kind.centre, kind.radius, &meshes);
            if fits {
                self.cards.next += 1;
            }
            settled.push((id, fits.then_some(slot)));
        }
        for (id, slot) in settled {
            self.cards.slots.insert(id, slot);
            changed = true;
        }
        if changed {
            let table: Vec<CardSpecies> = (species.iter().enumerate())
                .map(|(id, kind)| CardSpecies {
                    centre: kind.centre,
                    radius: kind.radius,
                    slot: self.cards.slots.get(&id).copied().flatten(),
                })
                .collect();
            renderer.set_card_species(device, queue, &table);
        }
        CardAsks {
            // Asked for after everything the camera sees, at the size a frame of the atlas needs.
            textures: (absent.into_iter())
                .map(|key| (ASKED_LAST, key, BAKED_PIXELS))
                .collect(),
            models,
            more,
        }
    }
}

/// What the far stand-ins wait for: the colour maps and the models of species to bake, and whether a species ready to bake was left for the next frame.
pub(super) struct CardAsks {
    pub textures: Vec<(f32, u64, u32)>,
    pub models: Vec<(f32, ModelRef)>,
    pub more: bool,
}

/// The priority a stand-in's colour map and model are asked for at, after everything that draws.
const ASKED_LAST: f32 = 1.0e9;
/// The most species baked in one frame.
const BAKES_A_FRAME: usize = 16;
/// The longest side a stand-in's colour map is asked for at: a frame of the atlas is 32 texels, a model's map a few times its size.
const BAKED_PIXELS: u32 = 128;

#[cfg(test)]
mod tests {
    use super::*;
    use or_renderer::FlyCamera;

    #[test]
    fn a_list_is_seen_while_some_of_it_is_in_view_within_its_distance_and_not_faded_out() {
        // The camera at the origin looks along +Y.
        let frustum = Frustum::of(FlyCamera::default().view_proj(1.0));
        let ahead = (Vec3::new(0.0, 1000.0, 0.0), 50.0);
        assert!(seen(&frustum, Vec3::ZERO, ahead, 1000.0, 1.0));
        // Its nearest point is what is measured, not its centre
        assert!(seen(&frustum, Vec3::ZERO, ahead, 951.0, 1.0));
        assert!(!seen(&frustum, Vec3::ZERO, ahead, 949.0, 1.0));
        assert!(!seen(&frustum, Vec3::ZERO, ahead, 1000.0, 0.0));
        let behind = (Vec3::new(0.0, -1000.0, 0.0), 50.0);
        assert!(!seen(&frustum, Vec3::ZERO, behind, 5000.0, 1.0));
    }
}

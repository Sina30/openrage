//! The world viewer's far stand-ins on the device: the selection's card lists handed to the
//! renderer when the selection changes, those the camera sees within their distance shown, and a
//! species baked into the atlas once its model and its colour maps are on the device.
//!
//! ⚠ A species is baked from the levels of its model that the world itself drew: a species whose
//! model never comes near enough to be read has no slot, and its cards draw nothing.

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
fn seen(frustum: &Frustum, eye: Vec3, (centre, radius): (Vec3, f32), distance: f32, fade: f32) -> bool {
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
    fn card_meshes(&self, model: &ModelRef, absent: &mut Vec<u64>) -> Option<Vec<CardMesh<'_>>> {
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
                    // No pack holds it: the stand-in is the colour of its vertices.
                    Some((_, (_, None))) => None,
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
    /// camera at `eye` sees within their distance, and bake one species whose model is on the
    /// device; `model_of` is the model of the species of an id. Gives the colour maps a species
    /// waits for, asked for as the frame's textures are: `(priority, key, pixels)`.
    pub(super) fn refresh_cards(
        &mut self,
        renderer: &mut ViewportRenderer,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        selected: &Selected,
        (frustum, eye): (&Frustum, Vec3),
        model_of: &dyn Fn(usize) -> Option<ModelRef>,
    ) -> Vec<(f32, u64, u32)> {
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
        let mut changed = !(self.cards.table.as_ref()).is_some_and(|table| Arc::ptr_eq(table, species));
        if changed {
            self.cards.table = Some(Arc::clone(species));
            self.cards.slots.clear();
        }
        // ponytail: one bake a layout, of the first species by id that can be baked; nearest first if the bakes are seen to lag.
        let (mut settled, mut absent) = (Vec::new(), Vec::new());
        let mut baking = true;
        for (id, kind) in species.iter().enumerate() {
            if kind.radius <= 0.0 || self.cards.slots.contains_key(&id) {
                continue;
            }
            let Some(model) = model_of(id) else {
                settled.push((id, None));
                continue;
            };
            let Some(meshes) = self.card_meshes(&model, &mut absent) else {
                continue;
            };
            if !baking {
                continue;
            }
            baking = false;
            let slot = self.cards.next;
            let fits = slot < renderer.card_slots(device)
                && renderer.bake_card_slot(device, queue, slot, kind.centre, kind.radius, &meshes);
            settled.push((id, fits.then_some(slot)));
        }
        for (id, slot) in settled {
            if slot.is_some() {
                self.cards.next += 1;
            }
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
        // Asked for after everything the camera sees, at the size a frame of the atlas needs.
        absent.into_iter().map(|key| (ASKED_LAST, key, BAKED_PIXELS)).collect()
    }
}

/// The priority a stand-in's colour map is asked for at, after every map of what draws.
const ASKED_LAST: f32 = 1.0e9;
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

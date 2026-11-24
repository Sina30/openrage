//! What the world keeps on the device and lets go: a model's meshes in the pages, the materials and palettes they hold, and what goes when the budgets are passed.

use std::collections::HashSet;

use glam::Vec3;

use super::budget::{evicted, remember, Budgets, Idle};
use super::{bucket_of, material, textures, Lod, Material, Mesh, Slot, WorldGpu};
use crate::state::gta6_world::worldview::index::ModelRef;
use crate::state::gta6_world::worldview::{Bucket, CpuMaterial, CpuModel};

impl WorldGpu {
    /// The material that draws one more mesh of this material in this bucket: one for every mesh on the device that names the same.
    pub(super) fn material(&mut self, cpu: &CpuMaterial, bucket: Bucket) -> u32 {
        let found = (self.material_of.get(cpu))
            .and_then(|of| of.iter().find(|(b, _)| *b == bucket))
            .map(|(_, id)| *id);
        if let Some(id) = found {
            self.materials[id as usize].meshes += 1;
            return id;
        }
        let material = Material {
            maps: material::slots_of(cpu),
            colours: std::array::from_fn(|at| material::is_colour(cpu, at)),
            whole: std::array::from_fn(|at| material::read_whole(cpu, at)),
            family: material::family_of(cpu.family),
            constants: material::constants_of(cpu),
            bucket: bucket_of(bucket),
            made: None,
            making: 0,
            touched: 0,
            pixels: 0,
            priority: 0.0,
            at: Vec3::ZERO,
            meshes: 1,
            of: Some((cpu.clone(), bucket)),
        };
        let id = match self.free_materials.pop() {
            Some(id) => {
                self.materials[id as usize] = material;
                id
            }
            None => {
                self.materials.push(material);
                self.materials.len() as u32 - 1
            }
        };
        (self.material_of.entry(cpu.clone()).or_default()).push((bucket, id));
        id
    }

    /// One mesh fewer draws with the material `id`: at none it lets go of what it holds on the device, and its slot is free.
    pub(super) fn unuse_material(&mut self, id: u32) {
        let Some(material) = self.materials.get_mut(id as usize) else {
            return;
        };
        material.meshes = material.meshes.saturating_sub(1);
        if material.meshes > 0 {
            return;
        }
        material.made = None;
        if let Some((cpu, bucket)) = material.of.take() {
            if let Some(of) = self.material_of.get_mut(&cpu) {
                of.retain(|(b, _)| *b != bucket);
                if of.is_empty() {
                    self.material_of.remove(&cpu);
                }
            }
            self.free_materials.push(id);
        }
    }

    /// Write every mesh of every level of `model` to the pages; a mesh of nothing, or one larger
    /// than the device makes a buffer, is left out.
    pub(super) fn put(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        budgets: &Budgets,
        model: &CpuModel,
        (frame, eye): (u64, Vec3),
    ) -> Slot {
        let mut palettes = Vec::with_capacity(model.palettes.len());
        for (key, colours) in &model.palettes {
            if palettes.contains(key) {
                continue;
            }
            (self.palettes).hold((device, queue), *key, colours, budgets.most_side);
            palettes.push(*key);
        }
        let mut bytes = 0;
        let mut lods = Vec::with_capacity(model.lods.len());
        for lod in &model.lods {
            let mut meshes = Vec::with_capacity(lod.meshes.len());
            for mesh in &lod.meshes {
                let place = (self.pages).put(device, queue, budgets, &mesh.vertices, &mesh.indices);
                let Some(place) = place else {
                    continue;
                };
                bytes += place.bytes();
                meshes.push(Mesh {
                    place,
                    material: self.material(&mesh.material, mesh.bucket),
                    density: textures::density(&mesh.vertices, &mesh.indices),
                    drawn: None,
                });
            }
            lods.push(Lod {
                until: lod.until,
                meshes,
            });
        }
        self.model_bytes += bytes;
        Slot::Ready {
            lods,
            bytes,
            drawn: frame,
            held: frame,
            at: eye,
            palettes,
        }
    }

    /// Take `model` off the device and give its room in the pages back, and its materials and palettes where no other model holds them.
    pub(super) fn forget(&mut self, model: &ModelRef) {
        let Some(Slot::Ready {
            lods,
            bytes,
            palettes,
            ..
        }) = self.models.remove(model)
        else {
            return;
        };
        for mesh in lods.iter().flat_map(|lod| &lod.meshes) {
            self.pages.free(mesh.place);
            self.unuse_material(mesh.material);
        }
        self.model_bytes -= bytes;
        for key in palettes {
            self.palettes.release(key);
        }
    }

    /// Take `model` off the device for the budgets, and tell the world's thread it left.
    fn let_go(&mut self, model: ModelRef) {
        self.forget(&model);
        self.cost.evicted.0 += 1;
        remember(&mut self.gone.0, model);
        self.told.push((model, Some(false)));
        self.changes += 1;
    }

    /// Let go of what did not draw in the frame `frame` until the rest fit the budgets: the
    /// models the selection no longer holds first, and of models and of textures the farthest
    /// from `eye` first.
    // ponytail: where what the selection holds is alone over the budget, what of it is out of view is let go and read again; a smaller selection if a card that small shows it.
    pub(super) fn evict(&mut self, budgets: &Budgets, frame: u64, eye: Vec3) {
        if self.model_bytes > budgets.models_kept {
            let idle = (self.models.iter())
                .filter_map(|(model, slot)| match slot {
                    Slot::Ready {
                        bytes,
                        drawn,
                        held,
                        at,
                        ..
                    } if *drawn < frame => Some(Idle {
                        key: *model,
                        bytes: *bytes,
                        held: *held == frame,
                        away: eye.distance(*at),
                    }),
                    _ => None,
                })
                .collect();
            for idle in evicted(idle, self.model_bytes - budgets.models_kept) {
                self.let_go(idle.key);
            }
        }
        // Pages left with holes hold more than the models in them: past the budget and a page, the models that did not draw leave the emptiest page.
        let page = budgets.page.0 + budgets.page.1;
        if let Some(emptiest) = self.pages.emptiest(budgets.models_kept + page) {
            let leaving: Vec<ModelRef> = (self.models.iter())
                .filter(|(_, slot)| match slot {
                    Slot::Ready { lods, drawn, .. } => {
                        let mut meshes = lods.iter().flat_map(|lod| &lod.meshes);
                        *drawn < frame && meshes.any(|mesh| mesh.place.page == emptiest)
                    }
                    Slot::Missing => false,
                })
                .map(|(model, _)| *model)
                .collect();
            for model in leaving {
                self.let_go(model);
            }
        }
        let gone: HashSet<u64> = (self.textures.evict(budgets.textures_kept, frame, eye))
            .into_iter()
            .collect();
        if gone.is_empty() {
            return;
        }
        self.cost.evicted.1 += gone.len() as u64;
        for key in &gone {
            remember(&mut self.gone.1, *key);
        }
        self.changes += 1;
        // ⚠ A material and a mesh made with a texture hold it on the device: both let it go here,
        // or the bytes counted as freed would stay.
        for material in &mut self.materials {
            if material.maps.iter().flatten().any(|key| gone.contains(key)) {
                material.made = None;
            }
        }
        let materials = &self.materials;
        for slot in self.models.values_mut() {
            let Slot::Ready { lods, .. } = slot else {
                continue;
            };
            for mesh in lods.iter_mut().flat_map(|lod| &mut lod.meshes) {
                if materials[mesh.material as usize].made.is_none() {
                    mesh.drawn = None;
                }
            }
        }
    }
}

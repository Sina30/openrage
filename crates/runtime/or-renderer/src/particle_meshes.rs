//! The models of particle batches drawn with the game's own `ptfx_model` passes: each batch's instances are laid out in the world as one indexed mesh and drawn with the technique its rule names.

use std::sync::Arc;

use glam::{Quat, Vec3};

use crate::game_shaders::effect_pass::{EffectCache, EffectKind};
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::model::{ModelPass, ModelVertex as WorldVertex};
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::pipeline::Target;
use crate::particle_models::{ModelBatch, ModelGeometry, ModelInstance};
use crate::particle_slots::{Prepared, Slots};
use crate::particles::ParticleCullMode;

/// Lay out `instances` of `geometry` in the world, a copy of the mesh for each scaled so that its bounding box is the size the instance states, the colour of an instance on every vertex of its copy.
pub fn lay_out(
    geometry: &ModelGeometry,
    instances: &[ModelInstance],
    vertices: &mut Vec<WorldVertex>,
    indices: &mut Vec<u32>,
) {
    vertices.clear();
    indices.clear();
    // A box with no extent along an axis leaves that axis at the size the instance states
    let size = Vec3::from(
        geometry
            .size
            .map(|extent| if extent > 1e-6 { extent } else { 1.0 }),
    );
    for instance in instances {
        let first = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        let [x, y, z, w] = instance.rotation;
        let rotation = Quat::from_xyzw(x, y, z, w);
        let rotation = if rotation.length_squared() > 1e-12 {
            rotation.normalize()
        } else {
            Quat::IDENTITY
        };
        let (scale, position) = (
            Vec3::from(instance.scale) / size,
            Vec3::from(instance.position),
        );
        // A normal follows the inverse of the scale, so that a stretched model stays lit as the surface it is
        let unscale = scale.recip();
        vertices.extend(geometry.vertices.iter().map(|vertex| WorldVertex {
            position: rotation * (Vec3::from(vertex.position) * scale) + position,
            normal: (rotation * (Vec3::from(vertex.normal) * unscale)).normalize_or(Vec3::Z),
            uv: vertex.uv,
            colour: instance.color,
        }));
        indices.extend(geometry.indices.iter().map(|index| first + index));
    }
}

/// The models of a frame's batches, one pass for each, drawn into one kind of target.
pub struct GameModels {
    pack: Arc<GamePack>,
    target: Target,
    cache: EffectCache,
    slots: Slots<ModelPass>,
    /// The mesh of the batch being laid out, kept so that a frame allocates none.
    vertices: Vec<WorldVertex>,
    indices: Vec<u32>,
}

impl GameModels {
    pub fn new(pack: Arc<GamePack>, target: Target) -> Self {
        Self {
            pack,
            target,
            cache: EffectCache::new(),
            slots: Slots::default(),
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Lay out the models of `batches` for a frame with the constants `scene` gives, preparing the pass of each batch that has none or asks for another.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[ModelBatch],
        scene: &EngineConstants,
    ) {
        self.slots.resize(batches.len());
        for (at, batch) in batches.iter().enumerate() {
            let Some(geometry) = batch
                .geometry
                .as_ref()
                .filter(|_| !batch.shader_technique.is_empty())
            else {
                self.slots.clear(at);
                continue;
            };
            // A batch with no instances for a frame keeps its pass and draws none
            if batch.instances.is_empty() {
                if let Some(pass) = self.slots.get_mut(at) {
                    pass.update(device, queue, scene, &[], &[]);
                }
                continue;
            }
            let wanted = Prepared {
                technique: batch.shader_technique.clone(),
                blend: batch.blend,
                cull: ParticleCullMode::None,
                depth_test: true,
                depth_write: true,
                texture: Prepared::texture_of(batch.diffuse_image.as_ref()),
                depth: 0,
            };
            let (cache, pack, target) = (&mut self.cache, &self.pack, &self.target);
            let Some(pass) = self.slots.ensure(at, wanted.clone(), || {
                let kind = EffectKind {
                    technique: wanted.technique.clone(),
                    blend: Some(batch.blend.wgpu_blend()),
                    cull: None,
                    depth_test: true,
                    depth_write: wanted.depth_write,
                    texture: batch.diffuse_image.clone(),
                    values: Vec::new(),
                    depth: None,
                };
                ModelPass::prepare(cache, device, queue, pack, target, &kind)
            }) else {
                continue;
            };
            lay_out(
                geometry,
                &batch.instances,
                &mut self.vertices,
                &mut self.indices,
            );
            pass.update(device, queue, scene, &self.vertices, &self.indices);
        }
    }

    /// The GPU bytes the passes of the last frame hold.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        self.slots.passes().map(|pass| pass.resident_bytes()).sum()
    }

    /// How many batches of the last frame have a pass to draw them.
    pub fn drawn_batches(&self) -> usize {
        self.slots.len()
    }

    /// Draw the batches, each with its own pass, in the order given, into `pass`, which must be a pass of the target this was made for.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[ModelBatch]) {
        for at in 0..batches.len() {
            if let Some(model) = self.slots.get(at) {
                model.record(pass);
            }
        }
    }
}

#[cfg(test)]
mod tests;

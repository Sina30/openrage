//! The trails of particle batches drawn with the game's own `ptfx_trail` passes: each batch's strip is laid out as the vertex stage reads it and drawn with the technique its rule names.

use std::sync::Arc;

use glam::Vec3;

use crate::game_shaders::effect_pass::{EffectCache, EffectKind};
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::pipeline::Target;
use crate::game_shaders::trail::{TrailPass, TrailPoint, MIN_POINTS};
use crate::particle_slots::{Prepared, Slots};
use crate::particles::ParticleCullMode;
use crate::trails::{TrailBatch, TrailVertex};

/// A vertex of a batch as the game's vertex stage reads it, its normal the direction to the eye.
pub fn point_of(vertex: &TrailVertex, eye: Vec3) -> TrailPoint {
    let position = Vec3::from(vertex.position);
    TrailPoint {
        position,
        colour: vertex.color,
        uv: vertex.uv,
        normal: (eye - position).normalize_or(Vec3::Z),
    }
}

/// The trails of a frame's batches, one pass for each, drawn into one kind of target.
pub struct GameTrails {
    pack: Arc<GamePack>,
    target: Target,
    cache: EffectCache,
    slots: Slots<TrailPass>,
    /// The points of the batch being laid out, kept so that a frame allocates none.
    points: Vec<TrailPoint>,
    /// The depth of the picture the trails are drawn into, and which one it is, for a soft one to fade out against.
    depth: Option<(u64, wgpu::TextureView)>,
    depth_serial: u64,
}

impl GameTrails {
    pub fn new(pack: Arc<GamePack>, target: Target) -> Self {
        Self {
            pack,
            target,
            cache: EffectCache::new(),
            slots: Slots::default(),
            points: Vec::new(),
            depth: None,
            depth_serial: 0,
        }
    }

    /// Fade the trails out against `depth`, the depth of the picture they are drawn into, where a surface is behind them; the passes that read the old one are made again.
    pub fn set_depth(&mut self, depth: Option<wgpu::TextureView>) {
        self.depth_serial += 1;
        self.depth = depth.map(|view| (self.depth_serial, view));
    }

    /// Lay out the strips of `batches` for a frame seen from `eye` with the constants `scene` gives, preparing the pass of each batch that has none or asks for another.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[TrailBatch],
        scene: &EngineConstants,
        eye: Vec3,
    ) {
        self.slots.resize(batches.len());
        for (at, batch) in batches.iter().enumerate() {
            if batch.shader_technique.is_empty() {
                self.slots.clear(at);
                continue;
            }
            // A batch with too few vertices for a frame keeps its pass and draws none
            if batch.vertices.len() < MIN_POINTS {
                if let Some(pass) = self.slots.get_mut(at) {
                    pass.update(device, queue, scene, &[]);
                }
                continue;
            }
            let soft = self.depth.as_ref();
            let wanted = Prepared {
                technique: batch.shader_technique.clone(),
                blend: batch.blend,
                cull: ParticleCullMode::None,
                depth_test: true,
                depth_write: false,
                texture: Prepared::texture_of(batch.diffuse_image.as_ref()),
                depth: soft.map_or(0, |(serial, _)| *serial),
            };
            let (cache, pack, target) = (&mut self.cache, &self.pack, &self.target);
            let Some(pass) = self.slots.ensure(at, wanted.clone(), || {
                let kind = EffectKind {
                    technique: wanted.technique.clone(),
                    blend: Some(batch.blend.wgpu_blend()),
                    cull: None,
                    depth_test: true,
                    depth_write: false,
                    texture: batch.diffuse_image.clone(),
                    values: Vec::new(),
                    depth: soft.map(|(_, view)| view.clone()),
                };
                TrailPass::prepare(cache, device, queue, pack, target, &kind)
            }) else {
                continue;
            };
            self.points.clear();
            self.points
                .extend(batch.vertices.iter().map(|vertex| point_of(vertex, eye)));
            pass.set_rule_values(&batch.constants);
            pass.update(device, queue, scene, &self.points);
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

    /// Draw the batches `accept` takes, each with its own pass, in the order given, into `pass`, which must be a pass of the target this was made for.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batches: &[TrailBatch],
        accept: &dyn Fn(&TrailBatch) -> bool,
    ) {
        for (at, batch) in batches.iter().enumerate() {
            if let (true, Some(trail)) = (accept(batch), self.slots.get(at)) {
                trail.record(pass);
            }
        }
    }
}

#[cfg(test)]
mod tests;

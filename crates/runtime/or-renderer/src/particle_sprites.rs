//! The sprites of particle batches drawn with the game's own `ptfx_sprite` passes: each batch's sprites are laid out as the vertex stage reads them and drawn with the technique its rule names.

use std::sync::Arc;

use glam::Vec3;

use crate::game_shaders::effect_pass::{EffectCache, EffectKind};
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::pipeline::Target;
use crate::game_shaders::sprite::{Sprite, SpritePass};
use crate::particle_slots::{Prepared, Slots};
use crate::particles::{ParticleAlignment, ParticleBatch, ParticleCullMode, ParticleInstance};

/// The directions along which a sprite's width and height run, for a sprite at `position` seen from `eye`.
///
/// A camera-facing sprite faces the eye with the world's z up; one aligned to its velocity runs its height along it; the others run their height along the batch's axis.
pub fn basis(
    alignment: ParticleAlignment,
    position: Vec3,
    velocity: Vec3,
    axis: Vec3,
    eye: Vec3,
) -> (Vec3, Vec3) {
    let to_eye = (eye - position).normalize_or(Vec3::Z);
    let facing = |up: Vec3, helper: Vec3| {
        let sideways = up.cross(to_eye);
        let right = if sideways.length() < 1e-4 {
            up.cross(helper)
        } else {
            sideways
        };
        right.normalize_or(Vec3::X)
    };
    match alignment {
        ParticleAlignment::Camera => {
            let mut right = Vec3::Z.cross(to_eye);
            if right.length() < 1e-4 {
                right = Vec3::X;
            }
            let right = right.normalize();
            (right, to_eye.cross(right).normalize())
        }
        ParticleAlignment::Velocity => {
            let speed = velocity.length().max(1e-4);
            let up = velocity / speed;
            (facing(up, Vec3::X), up)
        }
        _ => {
            let up = axis.normalize_or(Vec3::Z);
            let helper = if up.x.abs() > 0.9 { Vec3::Y } else { Vec3::X };
            (facing(up, helper), up)
        }
    }
}

/// The cell of an atlas of `rows` by `cols` that frame number `frame` is, as `[u_min, u_max, v_min, v_max]` with `v_min` the top, moved by `offset`.
pub fn frame_cell(frame: f32, rows: u32, cols: u32, offset: [f32; 2]) -> [f32; 4] {
    let (rows, cols) = (rows.max(1) as f32, cols.max(1) as f32);
    let frame = frame.max(0.0).floor();
    let (column, row) = (frame % cols, (frame / cols).floor());
    [
        column / cols + offset[0],
        (column + 1.0) / cols + offset[0],
        row / rows + offset[1],
        (row + 1.0) / rows + offset[1],
    ]
}

/// One particle as a sprite of its batch, seen from `eye`.
///
/// The particle's four dimensions are its top, bottom, left and right, whose sums are its height and width; a quarter turn of its spin carries its right to its up.
pub fn sprite_of(instance: &ParticleInstance, batch: &ParticleBatch, eye: Vec3) -> Sprite {
    let position = Vec3::from(instance.position);
    let (right, up) = basis(
        batch.alignment,
        position,
        Vec3::from(instance.velocity),
        Vec3::from(batch.align_axis),
        eye,
    );
    let [top, bottom, left, side] = instance.dimensions;
    let width = (left + side).max(1e-4);
    let height = (top + bottom).max(1e-4);
    let (sin, cos) = instance.rotation[0].sin_cos();
    let half_right = right * (0.5 * cos * width) + up * (0.5 * sin * height);
    let half_up = right * (-0.5 * sin * width) + up * (0.5 * cos * height);
    let (rows, cols) = (batch.uv_atlas_rows, batch.uv_atlas_cols);
    Sprite {
        centre: position,
        right: half_right,
        up: half_up,
        colour: instance.color,
        frame_a: frame_cell(instance.tex_frame_a, rows, cols, instance.uv_offset),
        frame_b: frame_cell(instance.tex_frame_b, rows, cols, instance.uv_offset),
        frame_blend: instance.tex_blend.clamp(0.0, 1.0),
        glow: 0.0,
    }
}

fn face(cull: ParticleCullMode) -> Option<wgpu::Face> {
    match cull {
        ParticleCullMode::None => None,
        ParticleCullMode::Front => Some(wgpu::Face::Front),
        ParticleCullMode::Back => Some(wgpu::Face::Back),
    }
}

/// The sprites of a frame's batches, one pass for each, drawn into one kind of target.
pub struct GameSprites {
    pack: Arc<GamePack>,
    target: Target,
    cache: EffectCache,
    slots: Slots<SpritePass>,
    /// The sprites of the batch being laid out, kept so that a frame allocates none.
    sprites: Vec<Sprite>,
    /// The depth of the picture the sprites are drawn into, and which one it is, for a pass to read where its batch does not write depth.
    depth: Option<(u64, wgpu::TextureView)>,
    depth_serial: u64,
}

impl GameSprites {
    pub fn new(pack: Arc<GamePack>, target: Target) -> Self {
        Self {
            pack,
            target,
            cache: EffectCache::new(),
            slots: Slots::default(),
            sprites: Vec::new(),
            depth: None,
            depth_serial: 0,
        }
    }

    /// Fade the sprites that do not write depth out against `depth`, the depth of the picture they are drawn into, where a surface is behind them; the passes that read the old one are made again.
    pub fn set_depth(&mut self, depth: Option<wgpu::TextureView>) {
        self.depth_serial += 1;
        self.depth = depth.map(|view| (self.depth_serial, view));
    }

    pub fn pack(&self) -> &Arc<GamePack> {
        &self.pack
    }

    /// Lay out the sprites of `batches` for a frame seen from `eye` with the constants `scene` gives, preparing the pass of each batch that has none or asks for another.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[ParticleBatch],
        scene: &EngineConstants,
        eye: Vec3,
    ) {
        self.slots.resize(batches.len());
        for (at, batch) in batches.iter().enumerate() {
            if batch.shader_technique.is_empty() {
                self.slots.clear(at);
                continue;
            }
            // A batch with no particles for a frame keeps its pass and draws none
            if batch.instances.is_empty() {
                if let Some(pass) = self.slots.get_mut(at) {
                    pass.update(device, queue, scene, &[]);
                }
                continue;
            }
            // A pass that writes depth cannot read it
            let soft = self.depth.as_ref().filter(|_| !batch.depth_write);
            let wanted = Prepared {
                technique: batch.shader_technique.clone(),
                blend: batch.blend,
                cull: batch.cull,
                depth_test: batch.depth_test,
                depth_write: batch.depth_write,
                texture: Prepared::texture_of(batch.diffuse_image.as_ref()),
                depth: soft.map_or(0, |(serial, _)| *serial),
            };
            let (cache, pack, target) = (&mut self.cache, &self.pack, &self.target);
            let Some(pass) = self.slots.ensure(at, wanted.clone(), || {
                let kind = EffectKind {
                    technique: wanted.technique.clone(),
                    blend: Some(batch.blend.wgpu_blend()),
                    cull: face(batch.cull),
                    depth_test: wanted.depth_test,
                    depth_write: wanted.depth_write,
                    texture: batch.diffuse_image.clone(),
                    values: Vec::new(),
                    depth: soft.map(|(_, view)| view.clone()),
                };
                SpritePass::prepare(cache, device, queue, pack, target, &kind)
            }) else {
                continue;
            };
            self.sprites.clear();
            self.sprites.extend(
                batch
                    .instances
                    .iter()
                    .map(|instance| sprite_of(instance, batch, eye)),
            );
            pass.set_rule_values(&batch.constants);
            pass.update(device, queue, scene, &self.sprites);
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
        batches: &[ParticleBatch],
        accept: &dyn Fn(&ParticleBatch) -> bool,
    ) {
        for (at, batch) in batches.iter().enumerate() {
            if let (true, Some(sprites)) = (accept(batch), self.slots.get(at)) {
                sprites.record(pass);
            }
        }
    }
}

#[cfg(test)]
mod tests;

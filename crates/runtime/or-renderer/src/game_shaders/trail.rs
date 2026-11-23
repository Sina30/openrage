//! Particle trails drawn with a pass of the pack's `ptfx_trail`: one strip of vertices, each a position, a colour and a texture coordinate, and for a lit technique the direction its light is taken from, all read once per vertex.

use glam::Vec3;
use or_io::gta5::shaders::spirv_pack::SignatureElement;

use super::effect_pass::{EffectCache, EffectError, EffectKind, EffectPass, Layout, Shape};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::Target;

/// The effect every trail draws with.
pub const EFFECT: &str = "ptfx_trail";

/// How few vertices make a strip worth drawing: two triangles.
pub const MIN_POINTS: usize = 4;

/// One vertex of a trail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailPoint {
    pub position: Vec3,
    pub colour: [f32; 4],
    pub uv: [f32; 2],
    /// The direction a lit technique takes its light from, which it normalises.
    pub normal: Vec3,
}

/// What an input of the vertex stage is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Position,
    Colour,
    Uv,
    Normal,
}

fn field_of(semantic: &str, index: u32) -> Option<Field> {
    Some(match (semantic, index) {
        ("POSITION", 0) => Field::Position,
        ("COLOR", 0) => Field::Colour,
        ("TEXCOORD", 0) => Field::Uv,
        ("TEXCOORD", 1) => Field::Normal,
        _ => return None,
    })
}

fn field_value(field: Field, point: &TrailPoint) -> [f32; 4] {
    match field {
        Field::Position => [point.position.x, point.position.y, point.position.z, 1.0],
        Field::Colour => point.colour,
        Field::Uv => [point.uv[0], point.uv[1], 0.0, 0.0],
        Field::Normal => [point.normal.x, point.normal.y, point.normal.z, 0.0],
    }
}

/// What the vertex stage reads for an input, by its semantic and index.
pub fn trail_value(semantic: &str, index: u32, point: &TrailPoint) -> [f32; 4] {
    field_of(semantic, index).map_or([0.0; 4], |field| field_value(field, point))
}

fn reads_once_per_trail(_: &SignatureElement) -> bool {
    false
}

const SHAPE: Shape = Shape {
    effect: EFFECT,
    topology: wgpu::PrimitiveTopology::TriangleStrip,
    per_instance: reads_once_per_trail,
};

/// A trail of one kind made ready to draw: the pipeline built and the resources bound once, so that a frame costs a write of the constants and the vertices and a draw into a pass its caller owns.
pub struct TrailPass {
    effect: EffectPass,
    layout: Layout<Field>,
    /// The bytes of the last frame's vertices, written over by the next, so that a frame allocates nothing.
    scratch: Vec<u8>,
    count: u32,
}

impl TrailPass {
    /// Set what the particle rule's shader variables hold, by the hash of each one's name.
    pub fn set_rule_values(&mut self, rule: &[(u32, [f32; 4])]) {
        self.effect.set_rule_values(rule);
    }

    /// The GPU bytes this pass holds.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        self.effect.resident_bytes()
    }

    /// Prepare a trail of `kind` to draw into `target`, a pass of one colour attachment and the depth the target says.
    pub fn prepare(
        cache: &mut EffectCache,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        target: &Target,
        kind: &EffectKind,
    ) -> Result<Self, EffectError> {
        let effect = EffectPass::prepare(cache, device, queue, pack, target, &SHAPE, kind)?;
        let streams = effect.streams();
        let layout = Layout::new(
            &streams.vertex,
            streams.vertex_stride,
            effect.inputs(),
            field_of,
        );
        Ok(Self {
            effect,
            layout,
            scratch: Vec::new(),
            count: 0,
        })
    }

    /// Write the constants of this frame, what the effect declares and the kind's over it and the scene's over those, and the vertices of the strip to draw.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &EngineConstants,
        points: &[TrailPoint],
    ) {
        self.effect.write_constants(queue, scene);
        self.scratch.clear();
        self.layout
            .write(&mut self.scratch, points.len(), |record, field| {
                field_value(field, &points[record])
            });
        self.effect.set_vertices(device, queue, &self.scratch);
        self.count = u32::try_from(points.len()).unwrap_or(u32::MAX);
    }

    /// How many vertices the last update left to draw.
    pub fn drawn(&self) -> u32 {
        self.count
    }

    /// Draw the strip into `pass`, which must have the colour and depth formats this was prepared for; a strip of fewer than [`MIN_POINTS`] draws nothing.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if (self.count as usize) < MIN_POINTS {
            return;
        }
        self.effect.record(pass, 0..self.count, 0..1);
    }
}

#[cfg(test)]
mod tests;

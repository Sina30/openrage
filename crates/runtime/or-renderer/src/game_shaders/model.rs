//! Particle models drawn with a pass of the pack's `ptfx_model`, the game's ordinary mesh effect: one indexed mesh, each vertex a position, a normal, a texture coordinate and a colour, read once per vertex, already in the world.

use glam::Vec3;
use or_io::gta5::shaders::spirv_pack::SignatureElement;

use super::effect_pass::{EffectCache, EffectError, EffectKind, EffectPass, Layout, Shape};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::Target;

/// The effect every particle model draws with.
pub const EFFECT: &str = "ptfx_model";

/// One vertex of a model, in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub colour: [f32; 4],
}

/// What an input of the vertex stage is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Position,
    Normal,
    Uv,
    Colour,
    Tangent,
}

fn field_of(semantic: &str, index: u32) -> Option<Field> {
    Some(match (semantic, index) {
        ("POSITION", 0) => Field::Position,
        ("NORMAL", 0) => Field::Normal,
        // The unlit technique reads the coordinate twice
        ("TEXCOORD", 0 | 1) => Field::Uv,
        ("COLOR", 0) => Field::Colour,
        ("TANGENT", 0) => Field::Tangent,
        _ => return None,
    })
}

fn field_value(field: Field, vertex: &ModelVertex) -> [f32; 4] {
    match field {
        Field::Position => [vertex.position.x, vertex.position.y, vertex.position.z, 1.0],
        Field::Normal => [vertex.normal.x, vertex.normal.y, vertex.normal.z, 0.0],
        Field::Uv => [vertex.uv[0], vertex.uv[1], 0.0, 0.0],
        Field::Colour => vertex.colour,
        // A model has none, and a stage that normalises a zero one writes nothing it can use
        Field::Tangent => [1.0, 0.0, 0.0, 1.0],
    }
}

/// What the vertex stage reads for an input, by its semantic and index.
pub fn model_value(semantic: &str, index: u32, vertex: &ModelVertex) -> [f32; 4] {
    field_of(semantic, index).map_or([0.0; 4], |field| field_value(field, vertex))
}

fn reads_once_per_model(_: &SignatureElement) -> bool {
    false
}

const SHAPE: Shape = Shape {
    effect: EFFECT,
    topology: wgpu::PrimitiveTopology::TriangleList,
    per_instance: reads_once_per_model,
};

/// A model of one kind made ready to draw: the pipeline built and the resources bound once, so that a frame costs a write of the constants, the vertices and the indices and a draw into a pass its caller owns.
pub struct ModelPass {
    effect: EffectPass,
    layout: Layout<Field>,
    /// The bytes of the last frame's vertices, written over by the next, so that a frame allocates nothing.
    scratch: Vec<u8>,
    indices: u32,
}

impl ModelPass {
    /// The GPU bytes this pass holds.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        self.effect.resident_bytes()
    }

    /// Prepare a model of `kind` to draw into `target`, a pass of one colour attachment and the depth the target says.
    pub fn prepare(
        cache: &mut EffectCache,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        target: &Target,
        kind: &EffectKind,
    ) -> Result<Self, EffectError> {
        let effect = EffectPass::prepare(cache, device, queue, pack, target, &SHAPE, kind)?;
        if effect.inputs().iter().any(|i| i.semantic == "BLENDWEIGHT") {
            return Err(EffectError::Skinned(kind.technique.clone()));
        }
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
            indices: 0,
        })
    }

    /// Write the constants of this frame, what the effect declares and the kind's over it and the scene's over those, and the mesh to draw.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &EngineConstants,
        vertices: &[ModelVertex],
        indices: &[u32],
    ) {
        self.effect.write_constants(queue, scene);
        self.scratch.clear();
        self.layout
            .write(&mut self.scratch, vertices.len(), |record, field| {
                field_value(field, &vertices[record])
            });
        self.effect.set_vertices(device, queue, &self.scratch);
        self.effect.set_indices(device, queue, indices);
        self.indices = u32::try_from(indices.len()).unwrap_or(u32::MAX);
    }

    /// How many indices the last update left to draw.
    pub fn drawn(&self) -> u32 {
        self.indices
    }

    /// Draw the mesh into `pass`, which must have the colour and depth formats this was prepared for.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.indices == 0 {
            return;
        }
        self.effect.record_indexed(pass, 0..self.indices);
    }
}

#[cfg(test)]
mod tests;

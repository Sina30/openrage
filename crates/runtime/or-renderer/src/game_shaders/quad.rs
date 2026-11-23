//! A pass of a pack rendered onto a quad with the engine's own constants, the pass's textures stood in by a checkerboard, and every vertex input written from its semantic.

use std::collections::BTreeMap;

use glam::Vec2;
use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{ConstantBuffer, Pass, Program, SignatureElement};
use thiserror::Error;

use super::bindings::{Requirement, Resource};
use super::engine::EngineConstants;
use super::frame::read_back;
pub use super::frame::Rendered;
use super::link::match_interpolation;
use super::pack::{GamePack, PackError};
use super::pipeline::{build_pass_pipeline, entry_of, vertex_layout, PipelineError};
use super::stand_in::{bind_groups_with, draw_vertices, Provided};

#[derive(Debug, Error)]
pub enum QuadError {
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
    #[error("the pass has no vertex stage")]
    NoVertexStage,
    #[error("the pass writes no colour")]
    NoColour,
    #[error("{0}")]
    Device(String),
}

/// What one vertex of the quad says for an input: its semantic and index, and four values, given the corner it sits at.
pub fn vertex_value(semantic: &str, corner: Vec2) -> [f32; 4] {
    match semantic {
        "POSITION" => [corner.x, corner.y, 0.0, 1.0],
        "NORMAL" => [0.0, 0.0, 1.0, 0.0],
        "TANGENT" => [1.0, 0.0, 0.0, 1.0],
        "COLOR" => [1.0, 1.0, 1.0, 1.0],
        // Every vertex follows the first bone entirely
        "BLENDWEIGHT" => [1.0, 0.0, 0.0, 0.0],
        "TEXCOORD" => [corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5, 0.0, 0.0],
        _ => [0.0; 4],
    }
}

/// A checkerboard of orange and white squares.
fn checker() -> Provided {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            let colour: [u8; 4] = if (x + y) % 2 == 0 {
                [240, 140, 40, 255]
            } else {
                [250, 250, 250, 255]
            };
            rgba.extend(colour);
        }
    }
    Provided::Texture2d {
        width: 8,
        height: 8,
        rgba,
        srgb: false,
    }
}

/// The two triangles of a quad, by the corner of each vertex.
const CORNERS: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
];

/// The six vertices of the quad, `stride` bytes each, with each attribute written from the semantic of the input at its location.
pub fn quad_vertices(
    attributes: &[wgpu::VertexAttribute],
    stride: u64,
    inputs: &[SignatureElement],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for corner in CORNERS {
        let mut written = vec![0u8; stride as usize];
        for attribute in attributes {
            let input = inputs
                .iter()
                .find(|i| i.register == attribute.shader_location);
            let values = input.map_or([0.0; 4], |i| vertex_value(&i.semantic, Vec2::from(corner)));
            let words = (attribute.format.size() / 4) as usize;
            for (i, value) in values.iter().take(words).enumerate() {
                let at = attribute.offset as usize + 4 * i;
                written[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend(written);
    }
    bytes
}

/// The programs of a pass that binds a vertex stage and at most a pixel stage.
pub fn pair_of(pass: &Pass) -> Option<(&String, Option<&String>)> {
    let vertex = pass.stages.get(&ShaderStageKind::Vertex)?;
    let others = pass
        .stages
        .keys()
        .any(|s| !matches!(s, ShaderStageKind::Vertex | ShaderStageKind::Pixel));
    (!others).then(|| (vertex, pass.stages.get(&ShaderStageKind::Pixel)))
}

/// A quad rendered with the vertex program `vertex` of `pack` and, when given, its pixel program `pixel`, into a `size`-square target.
///
/// A constant buffer is filled from `scene` by the name the game's reflection gives it and its variables, and a two-dimensional texture is the checkerboard; whatever else a pass binds is a stand-in.
pub fn render_pair(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    vertex: &str,
    pixel: Option<&str>,
    scene: &EngineConstants,
    size: u32,
) -> Result<Rendered, QuadError> {
    let manifest = pack.manifest();
    let mut vertex_module = pack.module(vertex)?;
    let pixel_module = pixel.map(|key| pack.module(key)).transpose()?;
    if let Some(pixel_module) = &pixel_module {
        match_interpolation(&mut vertex_module, pixel_module);
    }
    let programs: Vec<&Program> = [Some(vertex), pixel]
        .into_iter()
        .flatten()
        .filter_map(|key| manifest.programs.get(key))
        .collect();

    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let made = (|| {
        let built = build_pass_pipeline(device, &vertex_module, pixel_module.as_ref())?;

        // What each binding is called, and the layout of the buffer of that name
        let mut names: BTreeMap<(u32, u32), &str> = BTreeMap::new();
        let mut layouts: BTreeMap<&str, &ConstantBuffer> = BTreeMap::new();
        for program in &programs {
            for binding in &program.reflection.bindings {
                names.insert((binding.set, binding.binding), &binding.name);
            }
            for layout in manifest.constant_buffers_of(program) {
                layouts.insert(&layout.name, layout);
            }
        }
        let mut provide = |requirement: &Requirement| -> Option<Provided> {
            match &requirement.resource {
                Resource::Uniform { .. } => {
                    let name = names.get(&(requirement.group, requirement.binding))?;
                    layouts
                        .get(name)
                        .map(|layout| Provided::Bytes(scene.fill(layout)))
                }
                Resource::Texture { .. } => Some(checker()),
                _ => None,
            }
        };
        let groups = bind_groups_with(device, queue, &built, &mut provide);

        // One vertex for each corner, written for the inputs the vertex stage reads
        let entry = entry_of(&vertex_module, naga::ShaderStage::Vertex)?;
        let (attributes, stride) = vertex_layout(&vertex_module, entry)?;
        let inputs = &manifest
            .programs
            .get(vertex)
            .map(|p| p.reflection.inputs.as_slice())
            .unwrap_or_default();
        let bytes = quad_vertices(&attributes, stride, inputs);
        let targets = draw_vertices(device, queue, &built, &groups, &bytes, 6, size);
        let target = targets
            .into_iter()
            .next()
            .flatten()
            .ok_or(QuadError::NoColour)?;
        Ok::<_, QuadError>((
            target,
            built.colour_formats[0].unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
        ))
    })();
    let read = made.map(|(target, format)| read_back(device, queue, &target, format, size));
    let validation = pollster::block_on(validation.pop());
    let internal = pollster::block_on(internal.pop());
    let rendered = read?;
    match validation.or(internal) {
        None => Ok(rendered),
        Some(error) => Err(QuadError::Device(error.to_string().replace('\n', " | "))),
    }
}

#[cfg(test)]
mod tests;

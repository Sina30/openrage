//! One pass of an effect of the pack made ready to draw particles: the pipeline built and the resources bound once, so that a frame costs a write of the constants and of the vertices a batch has now, and a draw into a pass its caller owns.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Arc;

use or_io::gta5::shaders::spirv_pack::{ConstantBuffer, Program, SignatureElement};
use or_io::hash::string_hash_str;
use thiserror::Error;

use super::bindings::{Requirement, Resource};
use super::constants::Filler;
use super::engine::EngineConstants;
use super::link::match_interpolation;
use super::mesh::Image;
use super::pack::{GamePack, PackError};
use super::pipeline::{
    build_pass_pipeline_drawn, entry_of, vertex_streams, DrawState, PassPipeline, PipelineError,
    Streams, Target,
};
use super::quad::pair_of;
use super::stand_in::{bind_resources_with, Provided};

/// The textures a particle's pixel stage reads as its diffuse, by the name the game's reflection gives them: the particle effects' and the mesh effects'.
const DIFFUSE: [&str; 2] = ["DiffuseTexSampler", "DiffuseSampler"];

/// The depth map a soft particle fades out against.
const SOFT_DEPTH: &str = "DepthMapTexSampler";

#[derive(Debug, Clone, Error)]
pub enum EffectError {
    #[error("{0}")]
    Pack(String),
    #[error("{0}")]
    Pipeline(String),
    #[error("the pack has no effect named {0}")]
    NoEffect(String),
    #[error("{effect} has no technique named {technique}")]
    NoTechnique { effect: String, technique: String },
    #[error("the first pass of {0} has no vertex stage this draws")]
    NoPass(String),
    #[error("the vertex stage of {0} reads the vertex number, which this does not draw")]
    VertexNumber(String),
    #[error("the vertex stage of {0} reads bone weights, which a particle model has none of")]
    Skinned(String),
}

/// What particles of one kind draw with: the technique the rule names, how they blend and whether they cull and test and write depth, the texture and the constants of the batch.
#[derive(Debug, Clone, Default)]
pub struct EffectKind {
    /// The technique of the effect the particle rule names, such as `RGBA_lit` or `RGB_soft`.
    pub technique: String,
    pub blend: Option<wgpu::BlendState>,
    pub cull: Option<wgpu::Face>,
    pub depth_test: bool,
    pub depth_write: bool,
    /// The diffuse texture; white where there is none.
    pub texture: Option<Arc<Image>>,
    /// A constant by the name of the variable it sets.
    pub values: Vec<(String, Vec<f32>)>,
    /// The depth of the picture the particles are drawn into, which a soft one fades out against where a surface is behind it; none where the pass draws it as well, which it cannot read.
    pub depth: Option<wgpu::TextureView>,
}

/// What an effect's vertex stage is: which effect, how its vertices make primitives, and which inputs it reads once per instance, the rest being read once per vertex.
pub struct Shape {
    pub effect: &'static str,
    pub topology: wgpu::PrimitiveTopology,
    pub per_instance: fn(&SignatureElement) -> bool,
}

type PipelineKey = (
    String,
    Option<String>,
    Option<wgpu::BlendState>,
    Option<wgpu::Face>,
    wgpu::PrimitiveTopology,
    bool,
    bool,
);

/// The pipeline of a pass is built once for each pair of programs and state, and a pair that will not build is not tried again.
type Pipelines = HashMap<PipelineKey, Result<Arc<PassPipeline>, EffectError>>;

/// The pipelines built for one pack and one target, kept from a pass to the next.
///
/// ⚠ Only for the pack, the target and the device it was filled for: make a new one when any of them changes.
#[derive(Default)]
pub struct EffectCache {
    built: Pipelines,
}

impl EffectCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many pairs of programs and states have been tried, built or refused.
    pub fn len(&self) -> usize {
        self.built.len()
    }

    pub fn is_empty(&self) -> bool {
        self.built.is_empty()
    }
}

/// A uniform buffer a frame writes anew: the buffer, the layout of the game's buffer of that name, and what each of its variables holds until the engine sets it.
struct Uniform {
    buffer: wgpu::Buffer,
    layout: ConstantBuffer,
    /// What the effect declares for the buffer's variables, by name and as 32-bit words, cut to their sizes.
    defaults: Vec<(String, Vec<u32>)>,
    /// The kind's values by the name of the variable each sets, cut to its size.
    values: Vec<(String, Vec<f32>)>,
    /// The buffer's variables by the hash a particle rule's shader variable of that name carries: the variable's own name, and its name without a leading g where a capital follows it.
    rule_names: HashMap<u32, usize>,
    /// What the rule's variables hold this frame: the index of a variable of the layout and its value.
    rule_values: Vec<(usize, [f32; 4])>,
    /// What the buffer holds, so that a frame whose constants did not change writes nothing.
    written: Vec<u8>,
}

/// A vertex or index buffer that grows to hold what a frame writes into it.
struct Growing {
    buffer: wgpu::Buffer,
    capacity: u64,
    usage: wgpu::BufferUsages,
}

impl Growing {
    fn new(device: &wgpu::Device, capacity: u64, usage: wgpu::BufferUsages) -> Self {
        let capacity = capacity.max(4);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: capacity,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            capacity,
            usage,
        }
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        if bytes.len() as u64 > self.capacity {
            *self = Self::new(device, (bytes.len() as u64).next_power_of_two(), self.usage);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
    }
}

/// One input of a stream: where it is in a record, how many words it has, and what it reads.
#[derive(Debug, Clone, Copy)]
struct Slot<F> {
    offset: usize,
    words: usize,
    field: F,
}

/// The inputs of a stream resolved once to what each reads, so that writing a record is a few stores and not a search by name.
#[derive(Debug, Clone)]
pub struct Layout<F> {
    stride: usize,
    slots: Vec<Slot<F>>,
}

impl<F: Copy> Layout<F> {
    /// The layout of `attributes`, `stride` bytes a record, each read from the field `field_of` gives for the semantic and index of the input at its location; an input it gives none for is left at nought.
    pub fn new(
        attributes: &[wgpu::VertexAttribute],
        stride: u64,
        inputs: &[SignatureElement],
        field_of: impl Fn(&str, u32) -> Option<F>,
    ) -> Self {
        let slots = attributes
            .iter()
            .filter_map(|attribute| {
                let input = inputs
                    .iter()
                    .find(|i| i.register == attribute.shader_location)?;
                Some(Slot {
                    offset: attribute.offset as usize,
                    words: (attribute.format.size() / 4) as usize,
                    field: field_of(&input.semantic, input.index)?,
                })
            })
            .collect();
        Self {
            stride: stride as usize,
            slots,
        }
    }

    /// Append `count` records to `bytes`, each input as `value` gives it for the record and its field.
    pub fn write(
        &self,
        bytes: &mut Vec<u8>,
        count: usize,
        mut value: impl FnMut(usize, F) -> [f32; 4],
    ) {
        let start = bytes.len();
        bytes.resize(start + count * self.stride, 0);
        for record in 0..count {
            let base = start + record * self.stride;
            for slot in &self.slots {
                let values = value(record, slot.field);
                for (i, v) in values.iter().take(slot.words).enumerate() {
                    let at = base + slot.offset + 4 * i;
                    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

/// A pass of an effect made ready to draw: the layouts of its two vertex streams, and the buffers a frame writes.
pub struct EffectPass {
    pass: Arc<PassPipeline>,
    groups: BTreeMap<u32, wgpu::BindGroup>,
    uniforms: Vec<Uniform>,
    streams: Streams,
    inputs: Vec<SignatureElement>,
    vertices: Growing,
    instances: Growing,
    indices: Growing,
}

impl EffectPass {
    /// Prepare a pass of `shape`'s effect for `kind`, to draw into `target`, a pass of one colour attachment and the depth the target says.
    pub fn prepare(
        cache: &mut EffectCache,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        target: &Target,
        shape: &Shape,
        kind: &EffectKind,
    ) -> Result<Self, EffectError> {
        let pack_error = |e: PackError| EffectError::Pack(e.to_string());
        let pipeline_error = |e: PipelineError| EffectError::Pipeline(e.to_string());
        let manifest = pack.manifest();
        let effect = manifest
            .effects
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(shape.effect))
            .ok_or_else(|| EffectError::NoEffect(shape.effect.into()))?;
        let pass = effect
            .techniques
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&kind.technique))
            .ok_or_else(|| EffectError::NoTechnique {
                effect: shape.effect.into(),
                technique: kind.technique.clone(),
            })?
            .passes
            .first()
            .ok_or_else(|| EffectError::NoPass(kind.technique.clone()))?;
        let (vertex_key, pixel_key) =
            pair_of(pass).ok_or_else(|| EffectError::NoPass(kind.technique.clone()))?;
        let vertex_program = manifest
            .programs
            .get(vertex_key)
            .ok_or_else(|| EffectError::NoPass(kind.technique.clone()))?;
        let inputs = vertex_program.reflection.inputs.clone();
        if inputs.iter().any(|i| i.semantic == "SV_VertexID") {
            return Err(EffectError::VertexNumber(kind.technique.clone()));
        }
        let instance_locations: Vec<u32> = inputs
            .iter()
            .filter(|i| (shape.per_instance)(i))
            .map(|i| i.register)
            .collect();

        let key: PipelineKey = (
            vertex_key.clone(),
            pixel_key.cloned(),
            kind.blend,
            kind.cull,
            shape.topology,
            kind.depth_test,
            kind.depth_write,
        );
        let vertex_module = pack.module(vertex_key).map_err(pack_error)?;
        if !cache.built.contains_key(&key) {
            let mut vertex = vertex_module.clone();
            let pixel = pixel_key
                .map(|k| pack.module(k))
                .transpose()
                .map_err(pack_error)?;
            if let Some(pixel) = &pixel {
                match_interpolation(&mut vertex, pixel);
            }
            let target = Target {
                blend: kind.blend,
                colour_attachments: Some(1),
                ..*target
            };
            // The depth map a soft particle reads is a depth texture, which only an unfiltered binding takes, and with it the sampler that reads it
            let unfiltered_at: Vec<(u32, u32)> = pixel_key
                .and_then(|key| manifest.programs.get(key))
                .map(|program| {
                    (program.reflection.bindings.iter())
                        .filter(|binding| binding.name.eq_ignore_ascii_case(SOFT_DEPTH))
                        .map(|binding| (binding.set, binding.binding))
                        .collect()
                })
                .unwrap_or_default();
            let draw = DrawState {
                instance_locations: instance_locations.clone(),
                topology: shape.topology,
                cull_mode: kind.cull,
                depth_write: Some(kind.depth_write),
                depth_attachment: true,
                unfiltered_at,
                ..DrawState::default()
            };
            let built = build_pass_pipeline_drawn(
                device,
                &vertex,
                pixel.as_ref(),
                kind.depth_test,
                &target,
                &draw,
            )
            .map(Arc::new)
            .map_err(pipeline_error);
            cache.built.insert(key.clone(), built);
        }
        let built = cache.built[&key].clone()?;

        let programs: Vec<&Program> = [Some(vertex_key), pixel_key]
            .into_iter()
            .flatten()
            .filter_map(|k| manifest.programs.get(k))
            .collect();
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
            let name = names.get(&(requirement.group, requirement.binding))?;
            match &requirement.resource {
                Resource::Texture { .. }
                    if DIFFUSE.iter().any(|d| name.eq_ignore_ascii_case(d)) =>
                {
                    Some(kind.texture.as_ref().map_or_else(
                        || Provided::Texture2d {
                            width: 1,
                            height: 1,
                            rgba: vec![255; 4],
                            srgb: false,
                        },
                        |image| Provided::Texture2d {
                            width: image.width,
                            height: image.height,
                            rgba: image.rgba.clone(),
                            srgb: image.srgb,
                        },
                    ))
                }
                Resource::Texture { .. } if name.eq_ignore_ascii_case(SOFT_DEPTH) => {
                    kind.depth.clone().map(Provided::View)
                }
                // A depth is read as it is, by a sampler that blends nothing
                Resource::Sampler { .. } if name.eq_ignore_ascii_case(SOFT_DEPTH) => {
                    Some(Provided::Sampler(super::samplers::point(device)))
                }
                _ => None,
            }
        };
        let bound = bind_resources_with(device, queue, &built, &mut provide);

        let mut uniforms = Vec::new();
        for requirements in built.requirements.values() {
            for requirement in requirements {
                let Resource::Uniform { .. } = requirement.resource else {
                    continue;
                };
                let Some(buffer) = bound.buffers.get(&(requirement.group, requirement.binding))
                else {
                    continue;
                };
                let Some(layout) = names
                    .get(&(requirement.group, requirement.binding))
                    .and_then(|name| layouts.get(name))
                else {
                    continue;
                };
                let values = kind
                    .values
                    .iter()
                    .filter_map(|(name, numbers)| {
                        let variable = layout.variables.iter().find(|v| &v.name == name)?;
                        let room = (variable.size / 4) as usize;
                        Some((name.clone(), numbers[..numbers.len().min(room)].to_vec()))
                    })
                    .collect();
                let defaults = layout
                    .variables
                    .iter()
                    .filter_map(|variable| {
                        let words = effect.defaults.get(&variable.name)?;
                        let room = (variable.size / 4) as usize;
                        Some((
                            variable.name.clone(),
                            words[..words.len().min(room)].to_vec(),
                        ))
                    })
                    .collect();
                let mut rule_names = HashMap::new();
                for (index, variable) in layout.variables.iter().enumerate() {
                    rule_names.insert(string_hash_str(&variable.name), index);
                    let bare = variable
                        .name
                        .strip_prefix('g')
                        .filter(|rest| rest.starts_with(char::is_uppercase));
                    if let Some(bare) = bare {
                        rule_names.entry(string_hash_str(bare)).or_insert(index);
                    }
                }
                uniforms.push(Uniform {
                    buffer: buffer.clone(),
                    layout: (*layout).clone(),
                    defaults,
                    values,
                    rule_names,
                    rule_values: Vec::new(),
                    written: Vec::new(),
                });
            }
        }

        let entry = entry_of(&vertex_module, naga::ShaderStage::Vertex).map_err(pipeline_error)?;
        let streams =
            vertex_streams(&vertex_module, entry, &instance_locations).map_err(pipeline_error)?;
        let instances = Growing::new(device, streams.instance_stride, wgpu::BufferUsages::VERTEX);
        let indices = Growing::new(device, 4, wgpu::BufferUsages::INDEX);
        let vertices = Growing::new(device, streams.vertex_stride, wgpu::BufferUsages::VERTEX);
        Ok(Self {
            pass: built,
            groups: bound.groups,
            uniforms,
            streams,
            inputs,
            vertices,
            instances,
            indices,
        })
    }

    /// The layouts of the vertex buffer and the instance buffer.
    pub fn streams(&self) -> &Streams {
        &self.streams
    }

    /// The vertex stage's inputs, by semantic, register and mask.
    pub fn inputs(&self) -> &[SignatureElement] {
        &self.inputs
    }

    /// Set what a particle rule's shader variables hold, each by the hash of its name: a constant takes the value of the variable whose name is its own, or its own without a leading g. A variable no constant of this pass has is left out.
    pub fn set_rule_values(&mut self, rule: &[(u32, [f32; 4])]) {
        for uniform in &mut self.uniforms {
            uniform.rule_values.clear();
            uniform.rule_values.extend(
                rule.iter()
                    .filter_map(|(hash, value)| Some((*uniform.rule_names.get(hash)?, *value))),
            );
        }
    }

    /// Write the constants of this frame, what the effect declares and the kind's over it, the rule's over those and the scene's over all, to the buffers whose contents changed.
    pub fn write_constants(&mut self, queue: &wgpu::Queue, scene: &EngineConstants) {
        let values = scene.values();
        for uniform in &mut self.uniforms {
            let mut filler = Filler::new(&uniform.layout);
            for (variable, words) in &uniform.defaults {
                let _ = filler.set_words(variable, words);
            }
            for (variable, numbers) in &uniform.values {
                let _ = filler.set(variable, numbers);
            }
            for (index, value) in &uniform.rule_values {
                let variable = &uniform.layout.variables[*index];
                let room = ((variable.size / 4) as usize).min(value.len());
                let _ = filler.set(&variable.name, &value[..room]);
            }
            for (variable, numbers) in &values {
                let _ = filler.set(variable, numbers);
            }
            let mut bytes = filler.bytes();
            // A write is a whole number of words, and no longer than the buffer
            let len = bytes.len().min(uniform.buffer.size() as usize) & !3;
            bytes.truncate(len);
            if bytes != uniform.written {
                queue.write_buffer(&uniform.buffer, 0, &bytes);
                uniform.written = bytes;
            }
        }
    }

    /// Replace the per-vertex buffer's contents.
    pub fn set_vertices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        self.vertices.write(device, queue, bytes);
    }

    /// Replace the per-instance buffer's contents.
    pub fn set_instances(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        self.instances.write(device, queue, bytes);
    }

    /// The GPU bytes this pass holds: its vertex, instance and index buffers, and its constant buffers.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        crate::viewport::ResidentBytes {
            particle_instance_buffers: self.vertices.capacity
                + self.instances.capacity
                + self.indices.capacity,
            uniform_buffers: self.uniforms.iter().map(|u| u.buffer.size()).sum(),
            ..Default::default()
        }
    }

    /// Replace the index buffer's contents with 32-bit indices.
    pub fn set_indices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, indices: &[u32]) {
        self.indices
            .write(device, queue, bytemuck::cast_slice(indices));
    }

    fn bind(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pass.pipeline);
        pass.set_vertex_buffer(0, self.vertices.buffer.slice(..));
        if self.streams.instance_stride > 0 {
            pass.set_vertex_buffer(1, self.instances.buffer.slice(..));
        }
        for (group, bind_group) in &self.groups {
            pass.set_bind_group(*group, bind_group, &[]);
        }
    }

    /// Draw `vertices` of the vertex buffer, once for each of `instances` of the instance buffer, into `pass`, which must have the colour and depth formats this was prepared for.
    pub fn record(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        vertices: Range<u32>,
        instances: Range<u32>,
    ) {
        self.bind(pass);
        pass.draw(vertices, instances);
    }

    /// Draw `indices` of the index buffer, once, into `pass`, which must have the colour and depth formats this was prepared for.
    pub fn record_indexed(&self, pass: &mut wgpu::RenderPass<'_>, indices: Range<u32>) {
        self.bind(pass);
        pass.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(indices, 0, 0..1);
    }
}

#[cfg(test)]
mod tests;

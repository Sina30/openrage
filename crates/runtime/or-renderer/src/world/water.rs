//! GTA VI's water over the frame the game's passes lit: a mesh of its own, drawn over a copy of
//! that frame by its depth along the ray, the day's medium, the sky it reflects and the day's fog.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::game_shaders::deferred::{DEPTH_STENCIL_FORMAT, HDR_FORMAT};
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::mesh::Image;
use crate::game_shaders::stand_in::{constant_view, row_view, uploaded_view};
use crate::viewport::{MeshVertex, ViewportRenderer};

use super::water_shader::WATER_SHADER;

/// A far depth, for a frame that made none: the ray then runs to the ground under the water.
const FAR: f32 = 60_000.0;
/// The depth over which a shore fades in, in metres.
const SHORE: f32 = 0.5;
/// How many times the ripples repeat a metre.
const RIPPLE_SCALE: f32 = 0.04;

/// What the water is made of, as the day states it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterLook {
    /// How much of each channel a metre of water takes out.
    pub extinction: [f32; 3],
    /// The share of each channel taken out that the water gives back as its own colour.
    pub albedo: [f32; 3],
    /// What the sky it reflects is multiplied by.
    pub reflection: f32,
}

/// The program's `Water` uniform.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WaterUniform {
    extinction: [f32; 4],
    albedo: [f32; 4],
    light: [f32; 4],
    screen: [f32; 4],
    fog: [[f32; 4]; 5],
    fog_colour: [[f32; 4]; 4],
}

/// The water's pipeline, what it reads, and the mesh of this frame.
pub(crate) struct WaterPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    clamped: wgpu::Sampler,
    repeated: wgpu::Sampler,
    fresnel: wgpu::TextureView,
    far: wgpu::TextureView,
    flat: wgpu::TextureView,
    ripples: Option<wgpu::TextureView>,
    mesh: Option<(wgpu::Buffer, wgpu::Buffer, u32)>,
    look: WaterLook,
    inputs: Option<wgpu::BindGroup>,
}

impl WaterPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &wgpu::BindGroupLayout,
        light: &wgpu::BindGroupLayout,
    ) -> Self {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport-water-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1),
                texture(2),
                texture(3),
                texture(4),
                texture(5),
                sampler(6),
                sampler(7),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport-water-shader"),
            source: wgpu::ShaderSource::Wgsl(WATER_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport-water-pipeline-layout"),
            bind_group_layouts: &[Some(camera), Some(&layout), Some(light)],
            immediate_size: 0,
        });
        // The position, and the colour after the normal and the texture coordinate.
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: std::mem::offset_of!(MeshVertex, colour) as wgpu::BufferAddress,
                shader_location: 3,
            },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport-water-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_water"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            // The nearer a surface the greater its depth; the water writes none, so what blends after it still shows.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            // It replaces what it covers, which it read from a copy, and keeps the word the frame holds in its alpha.
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_water"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler_of = |address_mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("viewport-water-sampler"),
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                address_mode_w: address_mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            pipeline,
            layout,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport-water-uniform"),
                size: std::mem::size_of::<WaterUniform>() as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            clamped: sampler_of(wgpu::AddressMode::ClampToEdge),
            repeated: sampler_of(wgpu::AddressMode::Repeat),
            fresnel: row_view(device, queue, &crate::game_shaders::water::blend_row()),
            far: constant_view(device, queue, [FAR, 0.0, 0.0, 0.0]),
            flat: constant_view(device, queue, [0.5, 0.5, 1.0, 1.0]),
            ripples: None,
            mesh: None,
            look: WaterLook {
                extinction: [0.0; 3],
                albedo: [0.0; 3],
                reflection: 1.0,
            },
            inputs: None,
        }
    }

    /// The mesh to draw and what it is made of; `None` draws no water.
    fn set(&mut self, device: &wgpu::Device, water: Option<(&[MeshVertex], &[u32], WaterLook)>) {
        if let Some((_, _, look)) = water {
            self.look = look;
        }
        let mesh = water.map(|(vertices, indices, _)| (vertices, indices));
        self.mesh = mesh
            .filter(|(_, indices)| !indices.is_empty())
            .map(|(vertices, indices)| {
                let buffer = |label, contents: &[u8], usage| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents,
                        usage,
                    })
                };
                (
                    buffer(
                        "viewport-water-vertices",
                        bytemuck::cast_slice(vertices),
                        wgpu::BufferUsages::VERTEX,
                    ),
                    buffer(
                        "viewport-water-indices",
                        bytemuck::cast_slice(indices),
                        wgpu::BufferUsages::INDEX,
                    ),
                    indices.len() as u32,
                )
            });
    }

    /// Whether there is water to draw, which then reads a copy of the frame and its depth.
    pub(crate) fn drawn(&self) -> bool {
        self.mesh.is_some()
    }

    /// The ripples of the surface, where the install gives a map of them; once given they are kept.
    pub(crate) fn give_ripples(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map: Option<&Image>,
    ) {
        if let (None, Some(map)) = (&self.ripples, map) {
            self.ripples = Some(uploaded_view(
                device, queue, map.width, map.height, &map.rgba, map.srgb,
            ));
        }
    }

    /// Bind what the water reads this frame: the copy of the lit frame, its depth where the frame made one, and the map of the sky.
    pub(crate) fn bind(
        &mut self,
        device: &wgpu::Device,
        behind: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        sky: &wgpu::TextureView,
    ) {
        let views = [
            behind,
            depth.unwrap_or(&self.far),
            sky,
            &self.fresnel,
            self.ripples.as_ref().unwrap_or(&self.flat),
        ];
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: self.uniform.as_entire_binding(),
        }];
        entries.extend(
            (1u32..)
                .zip(views)
                .map(|(binding, view)| wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        );
        entries.push(wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&self.clamped),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Sampler(&self.repeated),
        });
        self.inputs = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport-water-bg"),
            layout: &self.layout,
            entries: &entries,
        }));
    }

    /// Write what the water is lit and fogged by this frame, for a frame `width` by `height`.
    pub(crate) fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        (width, height): (u32, u32),
    ) {
        let rgb = |c: [f32; 3], w: f32| [c[0], c[1], c[2], w];
        // Without a moment of the day there is no fog: nothing hides the water.
        let (fog, fog_colour) = match &engine.lighting {
            Some(lighting) => (
                lighting.fog_params.map(|p| p.to_array()),
                lighting.fog_colours.map(|c| c.to_array()),
            ),
            None => ([[0.0; 4]; 5], [[0.0; 4]; 4]),
        };
        let uniform = WaterUniform {
            extinction: rgb(self.look.extinction, 0.0),
            albedo: rgb(self.look.albedo, self.look.reflection),
            light: rgb(engine.water_light().to_array(), SHORE),
            screen: [
                1.0 / width.max(1) as f32,
                1.0 / height.max(1) as f32,
                f32::from(u8::from(self.ripples.is_some())),
                RIPPLE_SCALE,
            ],
            fog,
            fog_colour,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Draw the water into `pass`, seen through `camera` and lit by `light`.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera: &wgpu::BindGroup,
        light: &wgpu::BindGroup,
    ) {
        let (Some((vertices, indices, count)), Some(inputs)) = (&self.mesh, &self.inputs) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, inputs, &[]);
        pass.set_bind_group(2, light, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..*count, 0, 0..1);
    }
}

impl ViewportRenderer {
    /// The water the game's frame draws over what it lit: its mesh, each vertex's colour holding in red how deep the ground lies under it, and what it is made of; `None` draws none.
    pub fn set_world_water(
        &mut self,
        device: &wgpu::Device,
        water: Option<(&[MeshVertex], &[u32], WaterLook)>,
    ) {
        self.world.water.set(device, water);
    }
}

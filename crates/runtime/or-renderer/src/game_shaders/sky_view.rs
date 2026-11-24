//! The sky drawn from a table of its radiance in every direction above the horizon, worked out by an atmosphere model for a moment of the day, in place of the colours the game's dome blends.

use std::cell::Cell;
use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::deferred::{DEPTH_STENCIL_FORMAT, HDR_FORMAT};
use super::engine::EngineConstants;

/// How many texels the table has round the compass and from the horizon to the zenith.
pub const AROUND: u32 = 128;
pub const UP: u32 = 64;

/// The radiance of the sky towards each texel of the table, and the exposure the frame takes it at.
#[derive(Clone, PartialEq)]
pub struct SkyView {
    /// Radiance in cd/m², row by row from the horizon up, each row round the compass from the west through the south.
    pub radiance: Arc<Vec<[f32; 3]>>,
    pub exposure: f32,
}

impl std::fmt::Debug for SkyView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkyView")
            .field("texels", &self.radiance.len())
            .field("exposure", &self.exposure)
            .finish()
    }
}

/// The way texel column `x` and row `y` of the table looks, at its centre: the rows closer together near the horizon, where the sky changes most.
pub fn direction(x: u32, y: u32) -> Vec3 {
    let u = (x as f32 + 0.5) / AROUND as f32;
    let v = (y as f32 + 0.5) / UP as f32;
    let around = (u - 0.5) * std::f32::consts::TAU;
    let up = v * v * std::f32::consts::FRAC_PI_2;
    Vec3::new(up.cos() * around.cos(), up.cos() * around.sin(), up.sin())
}

impl SkyView {
    /// The table `radiance` gives at each of its directions, taken at `exposure`.
    pub fn of(radiance: impl Fn(Vec3) -> [f32; 3], exposure: f32) -> Self {
        let texels = (0..UP)
            .flat_map(|y| (0..AROUND).map(move |x| (x, y)))
            .map(|(x, y)| radiance(direction(x, y)))
            .collect();
        Self {
            radiance: Arc::new(texels),
            exposure,
        }
    }

    /// The exposed radiance towards `view`, between the four nearest texels; the horizon's under it.
    pub fn sample(&self, view: Vec3) -> Vec3 {
        let view = view.normalize_or(Vec3::Z);
        let u = view.y.atan2(view.x) / std::f32::consts::TAU + 0.5;
        let v = (view.z.clamp(0.0, 1.0).asin() / std::f32::consts::FRAC_PI_2).sqrt();
        let x = u * AROUND as f32 - 0.5;
        let y = (v * UP as f32 - 0.5).clamp(0.0, (UP - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f32, y: f32| {
            let column = (x as i64).rem_euclid(i64::from(AROUND)) as usize;
            let row = (y as usize).min(UP as usize - 1);
            Vec3::from(self.radiance[row * AROUND as usize + column])
        };
        let low = at(x0, y0).lerp(at(x0 + 1.0, y0), fx);
        let high = at(x0, y0 + 1.0).lerp(at(x0 + 1.0, y0 + 1.0), fx);
        low.lerp(high, fy) * self.exposure
    }
}

const SHADER: &str = r#"
struct Frame {
    unproject: mat4x4<f32>,
    exposure: vec4<f32>,
};
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var table: texture_2d<f32>;
@group(0) @binding(2) var linear: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) clip: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let clip = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    // At the depth the frame is cleared to, so that only what nothing was drawn at passes.
    return Out(vec4<f32>(clip, 0.0, 1.0), clip);
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let point = frame.unproject * vec4<f32>(in.clip, 1.0, 1.0);
    let view = normalize(point.xyz / point.w);
    let u = atan2(view.y, view.x) / 6.2831853 + 0.5;
    let v = sqrt(asin(clamp(view.z, 0.0, 1.0)) / 1.5707963);
    let radiance = textureSampleLevel(table, linear, vec2<f32>(u, v), 0.0).rgb;
    return vec4<f32>(radiance * frame.exposure.x, 0.0);
}
"#;

/// The pass that adds the table's sky over the pixels nothing was drawn at, after the game's dome has laid its clouds and stars there.
pub struct SkyViewPass {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    /// The table last written, and whether the frame has one to draw.
    written: Cell<Option<Arc<Vec<[f32; 3]>>>>,
    shown: Cell<bool>,
}

/// The rows of a table in its texture, each a half-float colour and an alpha of one.
fn texels(radiance: &[[f32; 3]]) -> Vec<u8> {
    radiance
        .iter()
        .flat_map(|c| [c[0], c[1], c[2], 1.0])
        .flat_map(|v| or_io::half::f32_to_half(v).to_le_bytes())
        .collect()
}

impl SkyViewPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky-view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky-view"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Equal),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky-view"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky-view"),
            size: wgpu::Extent3d {
                width: AROUND,
                height: UP,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky-view"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky-view"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            pipeline,
            uniforms,
            texture,
            bind,
            written: Cell::new(None),
            shown: Cell::new(false),
        }
    }

    /// Write this frame's camera and `view`'s exposure, and its table where it is another; none draws nothing.
    pub fn write(&self, queue: &wgpu::Queue, engine: &EngineConstants, view: Option<&SkyView>) {
        self.shown.set(view.is_some());
        let Some(view) = view else {
            return;
        };
        let last = self.written.take();
        if !last
            .as_ref()
            .is_some_and(|l| Arc::ptr_eq(l, &view.radiance))
        {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &texels(&view.radiance),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(AROUND * 8),
                    rows_per_image: Some(UP),
                },
                wgpu::Extent3d {
                    width: AROUND,
                    height: UP,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.written.set(Some(Arc::clone(&view.radiance)));
        // The way each pixel looks, with the camera at the centre of the sky.
        let turn = Mat4::from_mat3(glam::Mat3::from_mat4(engine.view));
        let unproject = (engine.projection * turn).inverse();
        let mut words: Vec<f32> = unproject.to_cols_array().to_vec();
        words.extend([view.exposure, 0.0, 0.0, 0.0]);
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&words));
    }

    /// Add the sky into `pass`, where a frame was written with a table.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.shown.get() {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_samples_back_the_radiance_it_was_made_of_towards_each_way() {
        let radiance = |v: Vec3| [v.x + 2.0, v.y + 2.0, v.z];
        let table = SkyView::of(radiance, 2.0);
        for way in [
            Vec3::new(1.0, 0.0, 0.2),
            Vec3::new(-0.3, 0.8, 0.5),
            Vec3::new(0.0, -1.0, 0.05),
            Vec3::Z,
        ] {
            let way = way.normalize();
            let want = Vec3::from(radiance(way)) * 2.0;
            let got = table.sample(way);
            assert!((got - want).length() < 0.08, "{way:?}: {got:?} {want:?}");
        }
        // Under the horizon is the horizon's.
        let under = table.sample(Vec3::new(1.0, 0.0, -0.5));
        assert!((under - table.sample(Vec3::X)).length() < 1e-4);
    }
}

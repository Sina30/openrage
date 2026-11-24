//! The sky drawn from a table of its radiance in every direction above the horizon, worked out by an atmosphere model for a moment of the day, in place of the colours the game's dome blends.

use std::cell::Cell;
use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::deferred::{DEPTH_STENCIL_FORMAT, HDR_FORMAT};
use super::engine::EngineConstants;

/// How many texels the table has round the compass and from the horizon to the zenith.
pub const AROUND: u32 = 128;
pub const UP: u32 = 64;

/// The radiance of the sky towards each texel of the table, the exposure the frame takes it at, and the clouds drawn over it.
#[derive(Clone, PartialEq)]
pub struct SkyView {
    /// Radiance in cd/m², row by row from the horizon up, each row round the compass from the west through the south.
    pub radiance: Arc<Vec<[f32; 3]>>,
    pub exposure: f32,
    pub clouds: Option<Clouds>,
}

/// A layer of clouds marched through over the table, its light in the frame's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clouds {
    /// Its base and top in metres above the sea, how much of the sky it covers, and its kind from heaps to towers.
    pub layer: [f32; 4],
    /// The radius in metres of the shell it is curved on, its extinction per metre where whole, the eccentricity of its scattering, and how far in metres the wind has carried it.
    pub shape: [f32; 4],
    /// The way towards the body that lights it, x east, y north, z up.
    pub towards: [f32; 3],
    /// The light of that body where it falls on the layer, and the sky's radiance over the layer, both exposed.
    pub light: [f32; 3],
    pub sky: [f32; 3],
    /// What the air takes out of light per metre between the camera and a cloud.
    pub haze: f32,
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
            clouds: None,
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
    // xyz the camera, w one where clouds are drawn
    eye: vec4<f32>,
    // base, top, cover, kind
    layer: vec4<f32>,
    // shell radius, extinction, eccentricity, drift
    shape: vec4<f32>,
    // xyz towards the light, w the haze per metre
    towards: vec4<f32>,
    light: vec4<f32>,
    sky: vec4<f32>,
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

fn hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i), hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(hash(i + vec3<f32>(0.0, 1.0, 0.0)), hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(hash(i + vec3<f32>(0.0, 0.0, 1.0)), hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(hash(i + vec3<f32>(0.0, 1.0, 1.0)), hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

// ⚠ Inferred: a stand-in for the build's weather map (`textures/weathermaps`), whose texels are not read.
fn coverage(q: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var p = vec3<f32>(q / 2500.0, 0.0);
    for (var i = 0; i < 4; i = i + 1) {
        sum = sum + noise(p) * amplitude;
        p = p * 2.03 + vec3<f32>(17.0, 31.0, 0.0);
        amplitude = amplitude * 0.5;
    }
    let even = smoothstep(0.25, 0.70, sum / 0.9375);
    let cover = frame.layer.z;
    return smoothstep(1.0 - cover - 0.1, 1.0 - cover + 0.1, even);
}

// The same profile as the title's cloud layer: rising over the first few hundredths, gone where the kind's tops end.
fn profile(along: f32, kind: f32) -> f32 {
    let reach = 0.35 + 0.65 * kind;
    return smoothstep(0.0, 0.07, along) * (1.0 - smoothstep(0.55 * reach, reach, along));
}

// The height above the sea of a point on the shell centred under the camera.
fn height_of(p: vec3<f32>) -> f32 {
    let r = frame.shape.x;
    let centred = vec3<f32>(p.xy - frame.eye.xy, p.z + r);
    return length(centred) - r;
}

// The extinction per metre at `p`, and what the cloud of its column takes out above it.
fn cloud_at(p: vec3<f32>) -> vec2<f32> {
    let thick = frame.layer.y - frame.layer.x;
    let h = height_of(p);
    let along = (h - frame.layer.x) / thick;
    if (along <= 0.0 || along >= 1.0) {
        return vec2<f32>(0.0);
    }
    let q = p.xy + vec2<f32>(frame.shape.w, 0.0);
    let covered = coverage(q);
    let shape = covered * profile(along, frame.layer.w);
    if (shape <= 0.0) {
        return vec2<f32>(0.0);
    }
    let detail = noise(vec3<f32>(q, h) / 350.0) * 0.65 + noise(vec3<f32>(q, h) / 110.0) * 0.35;
    let s = clamp((shape - (1.0 - detail) * 0.45) / 0.55, 0.0, 1.0) * frame.shape.y;
    // ⚠ Inferred: the column above as whole cloud up to where its kind's tops end.
    let reach = (0.35 + 0.65 * frame.layer.w) * 0.8;
    let above = max(reach - along, 0.0) * thick * covered * frame.shape.y;
    return vec2<f32>(s, above);
}

fn density(p: vec3<f32>) -> f32 {
    return cloud_at(p).x;
}

// What a slab of optical depth `tau` lets through of an even light, as the title's cloud layer has it.
fn through_slab(tau: f32) -> f32 {
    return 1.0 / (1.0 + 0.75 * tau * (1.0 - frame.shape.z));
}

// The near and far distances at which the way from the camera meets a sphere of radius `r` about the shell's centre, or none.
// ⚠ The shell is hundreds of kilometres across: every square is taken as a difference of the
// heights, or single floats lose the metres the layer lies in.
fn sphere(view: vec3<f32>, r: f32) -> vec2<f32> {
    let shell = frame.shape.x;
    let height = r - shell;
    let b = (frame.eye.z + shell) * view.z;
    // The camera's distance from the centre squared, less the sphere's radius squared.
    let c = (frame.eye.z - height) * (frame.eye.z + height + 2.0 * shell);
    let d = b * b - c;
    if (d < 0.0) {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(d);
    let q = select(-b + s, -b - s, b > 0.0);
    let other = select(c / q, 0.0, q == 0.0);
    return vec2<f32>(min(q, other), max(q, other));
}

fn henyey_greenstein(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (12.566371 * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

const STEPS: i32 = 64;
const LIGHT_STEPS: i32 = 6;
// The farthest a cloud is drawn, and the most of the layer one way is marched through, in metres.
const FARTHEST: f32 = 40000.0;
const LONGEST: f32 = 10000.0;

// The sky towards `view` with the clouds in front of it: their light, and what they let through as alpha.
fn clouded(view: vec3<f32>, behind: vec3<f32>, pixel: vec2<f32>) -> vec4<f32> {
    let r = frame.shape.x;
    let top = sphere(view, r + frame.layer.y);
    let base = sphere(view, r + frame.layer.x);
    if (top.y <= 0.0) {
        return vec4<f32>(behind, 1.0);
    }
    var start = max(top.x, 0.0);
    var end = top.y;
    if (frame.eye.z < frame.layer.x) {
        start = base.y;
    } else if (base.x > 0.0) {
        end = min(end, base.x);
    }
    if (start >= end || start > FARTHEST) {
        return vec4<f32>(behind, 1.0);
    }
    let span = min(end - start, LONGEST);
    let dt = span / f32(STEPS);
    // Interleaved gradient noise starts each pixel's march a little apart, so that the steps do not show as bands.
    let jitter = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
    let towards = frame.towards.xyz;
    let c = dot(view, towards);
    let g = frame.shape.z;
    var t = start + dt * jitter;
    var through = 1.0;
    var light = vec3<f32>(0.0);
    var weighted = 0.0;
    for (var i = 0; i < STEPS; i = i + 1) {
        let p = frame.eye.xyz + view * t;
        let here = cloud_at(p);
        let s = here.x;
        if (s > 0.0) {
            var tau = 0.0;
            var along = 30.0;
            for (var j = 0; j < LIGHT_STEPS; j = j + 1) {
                let reach = 30.0 * exp2(f32(j));
                tau = tau + density(p + towards * (along + 0.5 * reach)) * reach;
                along = along + reach;
            }
            // ⚠ Inferred: what is scattered more than once as octaves of ever less extinction and eccentricity.
            var sun = 0.0;
            var share = 1.0;
            for (var k = 0; k < 3; k = k + 1) {
                sun = sun + share * exp(-share * tau) * henyey_greenstein(c, share * g);
                share = share * 0.5;
            }
            // The sky and the light the cloud above has scattered reach in as far as the slab above lets them.
            let down = frame.sky.rgb + frame.light.rgb * max(towards.z, 0.0) * 0.31830989;
            let lit = frame.light.rgb * sun + down * through_slab(here.y);
            let kept = exp(-s * dt);
            light = light + through * lit * (1.0 - kept);
            weighted = weighted + through * (1.0 - kept) * t;
            through = through * kept;
            if (through < 0.01) {
                break;
            }
        }
        t = t + dt;
    }
    // The air between fades a far cloud into the sky behind it.
    let seen = 1.0 - through;
    let air = exp(-frame.towards.w * weighted / max(seen, 1e-4));
    let cloud = light * air + behind * seen * (1.0 - air);
    return vec4<f32>(behind * through + cloud, through);
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let point = frame.unproject * vec4<f32>(in.clip, 1.0, 1.0);
    let view = normalize(point.xyz / point.w);
    let u = atan2(view.y, view.x) / 6.2831853 + 0.5;
    let v = sqrt(asin(clamp(view.z, 0.0, 1.0)) / 1.5707963);
    let radiance = textureSampleLevel(table, linear, vec2<f32>(u, v), 0.0).rgb * frame.exposure.x;
    if (frame.eye.w < 0.5) {
        return vec4<f32>(radiance, 1.0);
    }
    return clouded(view, radiance, in.position.xy);
}
"#;

/// The bytes of the pass's uniforms: the unprojection and seven rows of four.
const UNIFORM_BYTES: u64 = 176;

/// The uniforms of a frame looking through `unproject` from `eye` at `view`, as the shader's `Frame` lays them.
fn words(unproject: Mat4, eye: Vec3, view: &SkyView) -> Vec<f32> {
    let mut words: Vec<f32> = unproject.to_cols_array().to_vec();
    words.extend([view.exposure, 0.0, 0.0, 0.0]);
    let drawn = f32::from(u8::from(view.clouds.is_some()));
    words.extend([eye.x, eye.y, eye.z, drawn]);
    let clouds = view.clouds.unwrap_or(Clouds {
        layer: [0.0, 1.0, 0.0, 0.0],
        shape: [1.0, 0.0, 0.0, 0.0],
        towards: [0.0, 0.0, 1.0],
        light: [0.0; 3],
        sky: [0.0; 3],
        haze: 0.0,
    });
    words.extend(clouds.layer);
    words.extend(clouds.shape);
    words.extend(clouds.towards);
    words.push(clouds.haze);
    words.extend(clouds.light);
    words.push(0.0);
    words.extend(clouds.sky);
    words.push(0.0);
    words
}

/// The pass that adds the table's sky over the pixels nothing was drawn at, after the game's dome has laid its stars and discs there, and the clouds in front of all of it.
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
                    // What the dome laid (its stars, the discs) shows through as much as the clouds let through.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::SrcAlpha,
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
            size: UNIFORM_BYTES,
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
        let eye = engine.view.inverse().w_axis.truncate();
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&words(unproject, eye, view)),
        );
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
    fn the_shader_validates_and_its_uniforms_fill_the_frame_it_declares() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("the WGSL parses");
        naga::valid::Validator::new(Default::default(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("the WGSL validates");
        let frame = (module.types.iter())
            .find(|(_, t)| t.name.as_deref() == Some("Frame"))
            .map(|(_, t)| t.inner.size(module.to_ctx()))
            .expect("a Frame");
        let mut view = SkyView::of(|_| [1.0; 3], 1.0);
        assert_eq!(
            words(Mat4::IDENTITY, Vec3::ZERO, &view).len() * 4,
            frame as usize
        );
        assert_eq!(u64::from(frame), UNIFORM_BYTES);
        view.clouds = Some(Clouds {
            layer: [200.0, 1500.0, 0.5, 0.5],
            shape: [318_000.0, 0.04, 0.9, 0.0],
            towards: [0.0, 0.0, 1.0],
            light: [1.0; 3],
            sky: [0.5; 3],
            haze: 1e-5,
        });
        let drawn = words(Mat4::IDENTITY, Vec3::new(1.0, 2.0, 3.0), &view);
        assert_eq!(&drawn[20..24], &[1.0, 2.0, 3.0, 1.0]);
        assert_eq!(drawn[24], 200.0);
    }

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

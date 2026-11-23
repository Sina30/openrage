//! A pack on disk of a few synthetic effects, and the surfaces and materials that draw with it, for tests that draw meshes.

use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{BindingClass, Reflection, SignatureElement};

use super::mesh::{Coverage, Image, Material, Surface};
use super::test_pack::{binding, element, variable, PackBuilder};

pub(crate) const VERTEX: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
}
@group(0) @binding(1) var<uniform> wvp: mat4x4<f32>;
@vertex fn main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> Out {
    var out: Out;
    out.position = wvp * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.shade = max(normal.z, 0.0);
    return out;
}";

pub(crate) const PIXEL: &str = "
@group(1) @binding(3) var<uniform> tint: vec4<f32>;
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) uv: vec2<f32>, @location(1) shade: f32) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, uv) * tint * shade;
}";

/// A pixel stage that writes a whole colour and a nought alpha, as the game's water does.
pub(crate) const WATER_PIXEL: &str = "
@fragment fn main(@location(0) uv: vec2<f32>, @location(1) shade: f32) -> @location(0) vec4<f32> {
    return vec4<f32>(0.5, 0.25, 0.125, 0.0);
}";

/// A pixel stage that shows the direction of the sun as a colour.
pub(crate) const SUN_PIXEL: &str = "
@group(1) @binding(4) var<uniform> light: vec4<f32>;
@fragment fn main(@location(0) uv: vec2<f32>, @location(1) shade: f32) -> @location(0) vec4<f32> {
    return vec4<f32>(light.xyz * 0.5 + 0.5, 1.0);
}";

/// A pixel stage that reads the coarsest level of its texture, whatever the size it is seen at.
pub(crate) const LOD_PIXEL: &str = "
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) uv: vec2<f32>, @location(1) shade: f32) -> @location(0) vec4<f32> {
    return textureSampleLevel(tex, samp, uv, 6.0);
}";

/// A pixel stage that writes two colour targets.
pub(crate) const TWO_PIXEL: &str = "
struct Out {
    @location(0) first: vec4<f32>,
    @location(1) second: vec4<f32>,
}
@fragment fn main(@location(0) uv: vec2<f32>, @location(1) shade: f32) -> Out {
    var out: Out;
    out.first = vec4<f32>(1.0);
    out.second = vec4<f32>(0.5);
    return out;
}";

/// A pack with the effect `synthetic`, whose forward technique draws a texture tinted by a material constant, and `sunlit`, which shows where the light is.
pub(crate) fn pack_builder() -> PackBuilder {
    PackBuilder::new()
        .buffer(
            "m",
            "rage_matrices",
            64,
            vec![variable("gWorldViewProj", 0, 64)],
        )
        .buffer(
            "t",
            "megashader_locals",
            16,
            vec![variable("matTint", 0, 16)],
        )
        .program(
            "vs",
            ShaderStageKind::Vertex,
            VERTEX,
            Reflection {
                bindings: vec![binding("rage_matrices", "cbuffer", BindingClass::Cbv, 0, 1)],
                constant_buffers: vec!["m".into()],
                inputs: vec![
                    element("POSITION", 0, 0, 7),
                    element("NORMAL", 0, 1, 7),
                    element("TEXCOORD", 0, 2, 3),
                ],
                outputs: Vec::new(),
            },
        )
        .program(
            "ps",
            ShaderStageKind::Pixel,
            PIXEL,
            Reflection {
                bindings: vec![
                    binding("megashader_locals", "cbuffer", BindingClass::Cbv, 1, 3),
                    binding("DiffuseSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: vec!["t".into()],
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .buffer(
            "l",
            "lighting_globals",
            16,
            vec![variable("gDirectionalLight", 0, 16)],
        )
        .program(
            "ps_sun",
            ShaderStageKind::Pixel,
            SUN_PIXEL,
            Reflection {
                bindings: vec![binding(
                    "lighting_globals",
                    "cbuffer",
                    BindingClass::Cbv,
                    1,
                    4,
                )],
                constant_buffers: vec!["l".into()],
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .program(
            "ps_lod",
            ShaderStageKind::Pixel,
            LOD_PIXEL,
            Reflection {
                bindings: vec![
                    binding("DiffuseSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: Vec::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            "coarse",
            "lightweight0_draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps_lod"),
            ],
        )
        .program(
            "ps_two",
            ShaderStageKind::Pixel,
            TWO_PIXEL,
            Reflection {
                bindings: Vec::new(),
                constant_buffers: Vec::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            "two",
            "lightweight0_draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps_two"),
            ],
        )
        .program(
            "ps_water",
            ShaderStageKind::Pixel,
            WATER_PIXEL,
            Reflection {
                bindings: Vec::new(),
                constant_buffers: Vec::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            "water_test",
            "alt4_draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps_water"),
            ],
        )
        .effect(
            "synthetic",
            "lightweight0_draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps"),
            ],
        )
        .effect(
            "sunlit",
            "lightweight0_draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps_sun"),
            ],
        )
}

/// [`pack_builder`] written out.
pub(crate) fn pack_on_disk() -> tempfile::TempDir {
    pack_builder().write()
}

/// A unit quad of two triangles at height `z`, facing up, with texture coordinates over its corners.
pub(crate) fn quad(z: f32) -> Surface {
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    Surface {
        positions: corners.iter().map(|c| [c[0], c[1], z]).collect(),
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: [(
            0,
            corners
                .iter()
                .map(|c| [c[0] * 0.5 + 0.5, c[1] * 0.5 + 0.5])
                .collect(),
        )]
        .into_iter()
        .collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Surface::default()
    }
}

pub(crate) fn material(tint: [f32; 4], texture: Option<[u8; 4]>) -> Material {
    Material {
        effect: "synthetic".into(),
        vectors: vec![("matTint".into(), vec![tint])],
        textures: texture
            .map(|rgba| {
                (
                    "DiffuseSampler".to_string(),
                    Image {
                        width: 2,
                        height: 2,
                        rgba: rgba.repeat(4),
                        srgb: false,
                    },
                )
            })
            .into_iter()
            .collect(),
        coverage: Coverage::Opaque,
        pass: 0,
        views: Default::default(),
    }
}

/// The vertex stage of the game's sprite, as far as the corner and the texture coordinate go.
pub(crate) const SPRITE_VERTEX: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) uv: vec2<f32>,
}
@group(0) @binding(1) var<uniform> wvp: mat4x4<f32>;
@vertex fn main(
    @location(0) centre: vec3<f32>,
    @location(1) colour: vec4<f32>,
    @location(6) v6: vec4<f32>,
    @location(7) v7: vec4<f32>,
    @location(8) v8: vec4<f32>,
    @location(9) v9: vec4<f32>,
    @location(10) v10: vec4<f32>,
    @location(13) corner: vec3<f32>,
) -> Out {
    var table_s = array<f32, 3>(v9.x, 0.5, v9.y);
    var table_t = array<f32, 3>(v9.z, 0.5, v9.w);
    let s = table_s[i32(corner.x)];
    let t = table_t[i32(corner.y)];
    let a = v7.xyz;
    let w = vec3<f32>(v6.w, v7.w, v8.w);
    let c = v8.xyz;
    let d = v6.xyz;
    let world = centre + a + w + (c - w) * s + (d - a) * t;
    var out: Out;
    out.position = wvp * vec4<f32>(world, 1.0);
    out.colour = colour;
    out.uv = vec2<f32>(v10.x + s * (v10.y - v10.x), v10.w + t * (v10.z - v10.w));
    return out;
}";

pub(crate) const SPRITE_PIXEL: &str = "
@group(1) @binding(5) var<uniform> locals: vec4<f32>;
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) colour: vec4<f32>, @location(1) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, uv) * colour * locals.x;
}";

fn sprite_inputs() -> Vec<SignatureElement> {
    vec![
        element("POSITION", 0, 0, 7),
        element("COLOR", 0, 1, 15),
        element("TEXCOORD", 6, 6, 15),
        element("TEXCOORD", 7, 7, 15),
        element("TEXCOORD", 8, 8, 15),
        element("TEXCOORD", 9, 9, 15),
        element("TEXCOORD", 10, 10, 15),
        element("TEXCOORD", 3, 13, 7),
    ]
}

/// A pack with the effect `ptfx_sprite` and one technique, whose constant defaults to one; `extra_input` is one more input the vertex stage lists in its reflection.
pub(crate) fn sprite_pack(technique: &str, extra_input: Option<(&str, u32)>) -> tempfile::TempDir {
    let mut vertex_inputs = sprite_inputs();
    if let Some((semantic, register)) = extra_input {
        vertex_inputs.push(element(semantic, 0, register, 1));
    }
    PackBuilder::new()
        .buffer(
            "m",
            "rage_matrices",
            64,
            vec![variable("gWorldViewProj", 0, 64)],
        )
        .buffer(
            "l",
            "ptfx_sprite_locals2",
            16,
            vec![variable("gSuperAlpha", 0, 16)],
        )
        .program(
            "vs",
            ShaderStageKind::Vertex,
            SPRITE_VERTEX,
            Reflection {
                bindings: vec![binding("rage_matrices", "cbuffer", BindingClass::Cbv, 0, 1)],
                constant_buffers: vec!["m".into()],
                inputs: vertex_inputs,
                outputs: Vec::new(),
            },
        )
        .program(
            "ps",
            ShaderStageKind::Pixel,
            SPRITE_PIXEL,
            Reflection {
                bindings: vec![
                    binding("ptfx_sprite_locals2", "cbuffer", BindingClass::Cbv, 1, 5),
                    binding("DiffuseTexSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseTexSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: vec!["l".into()],
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            super::sprite::EFFECT,
            technique,
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps"),
            ],
        )
        .default_of(super::sprite::EFFECT, "gSuperAlpha", &[1.0f32.to_bits(); 4])
        .write()
}

/// The vertex stage of the game's trail, as far as it goes: a world position, a colour and a texture coordinate, passed on.
pub(crate) const TRAIL_VERTEX: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) uv: vec2<f32>,
}
@group(0) @binding(1) var<uniform> wvp: mat4x4<f32>;
@vertex fn main(
    @location(0) position: vec3<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) uv: vec2<f32>,
) -> Out {
    var out: Out;
    out.position = wvp * vec4<f32>(position, 1.0);
    out.colour = colour;
    out.uv = uv;
    return out;
}";

pub(crate) const TRAIL_PIXEL: &str = "
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) colour: vec4<f32>, @location(1) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, uv) * colour;
}";

/// A pack with the effect `ptfx_trail` and one technique that draws a texture by the colour of each vertex.
pub(crate) fn trail_pack(technique: &str) -> tempfile::TempDir {
    PackBuilder::new()
        .buffer(
            "m",
            "rage_matrices",
            64,
            vec![variable("gWorldViewProj", 0, 64)],
        )
        .program(
            "vs",
            ShaderStageKind::Vertex,
            TRAIL_VERTEX,
            Reflection {
                bindings: vec![binding("rage_matrices", "cbuffer", BindingClass::Cbv, 0, 1)],
                constant_buffers: vec!["m".into()],
                inputs: vec![
                    element("POSITION", 0, 0, 7),
                    element("COLOR", 0, 1, 15),
                    element("TEXCOORD", 0, 2, 3),
                ],
                outputs: Vec::new(),
            },
        )
        .program(
            "ps",
            ShaderStageKind::Pixel,
            TRAIL_PIXEL,
            Reflection {
                bindings: vec![
                    binding("DiffuseTexSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseTexSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: Vec::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            super::trail::EFFECT,
            technique,
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps"),
            ],
        )
        .write()
}

/// The vertex stage of the game's mesh effect, as far as it goes: a world position, a colour, a texture coordinate and a normal.
pub(crate) const MODEL_VERTEX: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) uv: vec2<f32>,
}
@group(0) @binding(1) var<uniform> wvp: mat4x4<f32>;
@vertex fn main(
    @location(0) position: vec3<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) normal: vec3<f32>,
) -> Out {
    var out: Out;
    out.position = wvp * vec4<f32>(position + normal * 0.0, 1.0);
    out.colour = colour;
    out.uv = uv;
    return out;
}";

pub(crate) const MODEL_PIXEL: &str = "
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) colour: vec4<f32>, @location(1) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, uv) * colour;
}";

/// A pack with the effect `ptfx_model` and one technique that draws a texture, bound as the mesh effects name it, by the colour of each vertex; `extra_input` is one more input the vertex stage lists in its reflection.
pub(crate) fn model_pack(technique: &str, extra_input: Option<(&str, u32)>) -> tempfile::TempDir {
    let mut inputs = vec![
        element("POSITION", 0, 0, 7),
        element("COLOR", 0, 1, 15),
        element("TEXCOORD", 0, 2, 3),
        element("NORMAL", 0, 3, 7),
    ];
    if let Some((semantic, register)) = extra_input {
        inputs.push(element(semantic, 0, register, 15));
    }
    PackBuilder::new()
        .buffer(
            "m",
            "rage_matrices",
            64,
            vec![variable("gWorldViewProj", 0, 64)],
        )
        .program(
            "vs",
            ShaderStageKind::Vertex,
            MODEL_VERTEX,
            Reflection {
                bindings: vec![binding("rage_matrices", "cbuffer", BindingClass::Cbv, 0, 1)],
                constant_buffers: vec!["m".into()],
                inputs,
                outputs: Vec::new(),
            },
        )
        .program(
            "ps",
            ShaderStageKind::Pixel,
            MODEL_PIXEL,
            Reflection {
                bindings: vec![
                    binding("DiffuseSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: Vec::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            super::model::EFFECT,
            technique,
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps"),
            ],
        )
        .write()
}

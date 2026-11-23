use or_io::gta5::shaders::spirv_pack::{BindingClass, Reflection};

use super::super::test_pack::{binding, element, gpu, variable, PackBuilder};
use super::*;

const VERTEX: &str = "
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@group(0) @binding(1) var<uniform> wvp: mat4x4<f32>;
@vertex fn main(@location(0) position: vec3<f32>, @location(2) uv: vec2<f32>) -> Out {
    var out: Out;
    out.position = wvp * vec4<f32>(position, 1.0);
    out.uv = uv;
    return out;
}";

const PIXEL: &str = "
@group(1) @binding(2) var<uniform> scalars: vec4<f32>;
@group(1) @binding(32) var tex: texture_2d<f32>;
@group(1) @binding(16) var samp: sampler;
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, uv) * scalars;
}";

/// A pack of a textured pair as a bake would write it, with the vertex stage's matrix buffer named `matrices` and its position input a `POSITION` unless told otherwise.
fn pack_on_disk(matrices: &str, position: &str) -> tempfile::TempDir {
    PackBuilder::new()
        .buffer(
            "m",
            "rage_matrices",
            64,
            vec![variable("gWorldViewProj", 0, 64)],
        )
        .buffer(
            "s",
            "misc_globals",
            16,
            vec![variable("globalScalars", 0, 16)],
        )
        .program(
            "vs",
            ShaderStageKind::Vertex,
            VERTEX,
            Reflection {
                bindings: vec![binding(matrices, "cbuffer", BindingClass::Cbv, 0, 1)],
                constant_buffers: vec!["m".into()],
                inputs: vec![element(position, 0, 0, 7), element("TEXCOORD", 0, 2, 3)],
                outputs: Vec::new(),
            },
        )
        .program(
            "ps",
            ShaderStageKind::Pixel,
            PIXEL,
            Reflection {
                bindings: vec![
                    binding("misc_globals", "cbuffer", BindingClass::Cbv, 1, 2),
                    binding("DiffuseSampler", "texture", BindingClass::Srv, 1, 32),
                    binding("DiffuseSampler", "sampler", BindingClass::Sampler, 1, 16),
                ],
                constant_buffers: vec!["s".into()],
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
        )
        .effect(
            "e",
            "draw",
            &[
                (ShaderStageKind::Vertex, "vs"),
                (ShaderStageKind::Pixel, "ps"),
            ],
        )
        .write()
}

const SIZE: u32 = 64;

fn render(dir: &tempfile::TempDir, test: &str) -> Option<Rendered> {
    let (device, queue) = gpu(test)?;
    let pack = GamePack::open(dir.path()).unwrap();
    Some(
        render_pair(
            &device,
            &queue,
            &pack,
            "vs",
            Some("ps"),
            &EngineConstants::default(),
            SIZE,
        )
        .expect("the pair renders"),
    )
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_pair_is_rendered_with_its_constants_found_by_name_and_its_vertices_written_by_semantic() {
    let dir = pack_on_disk("rage_matrices", "POSITION");
    let Some(rendered) = render(&dir, "a_pair_is_rendered_with_its_constants") else {
        return;
    };
    assert_eq!(rendered.format, wgpu::TextureFormat::Rgba8Unorm);
    let area = (SIZE * SIZE) as usize;
    // A 45 degree camera three units from a quad two wide covers about two thirds of a side squared
    let touched = rendered.touched();
    assert!(
        touched > area * 4 / 10 && touched < area * 9 / 10,
        "{touched} of {area}"
    );
    // The checker's two colours, filtered, so more than two
    assert!(rendered.colours() >= 3, "{} colours", rendered.colours());
    // A corner is outside the quad and the middle inside it
    assert_eq!(&rendered.bytes[..4], &[0, 0, 0, 0]);
    let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    assert!(rendered.bytes[middle..middle + 3].iter().any(|b| *b > 100));
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_buffer_the_reflection_names_otherwise_is_not_filled_and_nothing_is_drawn() {
    let dir = pack_on_disk("some_other_name", "POSITION");
    let Some(rendered) = render(&dir, "a_buffer_the_reflection_names_otherwise") else {
        return;
    };
    assert_eq!(rendered.touched(), 0, "a matrix of zeros draws nothing");
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn an_input_whose_semantic_is_not_position_does_not_get_the_quads_corners() {
    let dir = pack_on_disk("rage_matrices", "BLENDWEIGHT");
    let Some(rendered) = render(&dir, "an_input_whose_semantic_is_not_position") else {
        return;
    };
    assert_eq!(
        rendered.touched(),
        0,
        "every vertex at the origin draws nothing"
    );
}

#[test]
fn a_semantic_is_written_as_the_value_a_quad_vertex_has_for_it() {
    let corner = Vec2::new(1.0, -1.0);
    assert_eq!(vertex_value("POSITION", corner), [1.0, -1.0, 0.0, 1.0]);
    assert_eq!(vertex_value("NORMAL", corner), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(vertex_value("TANGENT", corner), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(vertex_value("COLOR", corner), [1.0; 4]);
    assert_eq!(vertex_value("BLENDWEIGHT", corner), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(vertex_value("BLENDINDICES", corner), [0.0; 4]);
    // Texture coordinates run from the top left
    assert_eq!(vertex_value("TEXCOORD", corner), [1.0, 1.0, 0.0, 0.0]);
    assert_eq!(
        vertex_value("TEXCOORD", Vec2::new(-1.0, 1.0))[..2],
        [0.0, 0.0]
    );
}

#[test]
fn the_pixels_a_render_touched_and_the_colours_in_it_are_counted() {
    let rendered = Rendered {
        size: 2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        bytes: vec![
            0, 0, 0, 0, //
            10, 0, 0, 0, //
            10, 0, 0, 0, //
            0, 0, 0, 255,
        ],
    };
    assert_eq!(rendered.touched(), 3);
    assert_eq!(rendered.colours(), 3);
    let narrow = Rendered {
        size: 2,
        format: wgpu::TextureFormat::R8Unorm,
        bytes: vec![0, 0, 7, 7],
    };
    assert_eq!((narrow.touched(), narrow.colours()), (2, 2));
}

#[test]
fn a_pass_of_a_vertex_stage_and_at_most_a_pixel_stage_has_a_pair() {
    let pass = |stages: &[ShaderStageKind]| Pass {
        stages: stages.iter().map(|s| (*s, format!("{s:?}"))).collect(),
    };
    use ShaderStageKind::{Geometry, Pixel, Vertex};
    let both = pass(&[Vertex, Pixel]);
    assert_eq!(
        pair_of(&both).map(|(v, p)| (v.as_str(), p.map(|p| p.as_str()))),
        Some(("Vertex", Some("Pixel")))
    );
    assert_eq!(
        pair_of(&pass(&[Vertex])).map(|(_, p)| p.is_some()),
        Some(false)
    );
    assert!(pair_of(&pass(&[Pixel])).is_none());
    assert!(pair_of(&pass(&[Vertex, Pixel, Geometry])).is_none());
}

#[test]
fn each_attribute_of_a_vertex_is_written_from_the_input_at_its_location() {
    // A position of three at location 0 and a texture coordinate of two at location 2, packed
    let attributes = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 12,
            shader_location: 2,
        },
        // An input the reflection does not list is left at zero
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: 20,
            shader_location: 9,
        },
    ];
    let inputs = [element("POSITION", 0, 0, 7), element("TEXCOORD", 0, 2, 3)];
    let bytes = quad_vertices(&attributes, 24, &inputs);
    assert_eq!(bytes.len(), 6 * 24);
    let words: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    // The first corner is (-1, -1): the position, then the texture coordinate from the top left, then zero
    assert_eq!(&words[..6], &[-1.0, -1.0, 0.0, 0.0, 1.0, 0.0]);
    // The third corner is (1, 1)
    assert_eq!(&words[12..18], &[1.0, 1.0, 0.0, 1.0, 0.0, 0.0]);
}

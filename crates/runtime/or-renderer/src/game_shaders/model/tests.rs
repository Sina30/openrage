use super::*;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::mesh::Image;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::synthetic::model_pack;
use crate::game_shaders::test_pack::{draw_into, gpu};
use glam::Mat4;
use std::sync::Arc;

const SIZE: u32 = 16;

/// The camera of the scene is nothing: a point's place on the target is its own x and y.
fn flat() -> EngineConstants {
    EngineConstants {
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        ..Default::default()
    }
}

fn kind() -> EffectKind {
    EffectKind {
        technique: "draw".into(),
        depth_test: true,
        depth_write: true,
        ..Default::default()
    }
}

fn vertex(x: f32, y: f32, u: f32, v: f32) -> ModelVertex {
    ModelVertex {
        position: Vec3::new(x, y, 0.0),
        normal: Vec3::Z,
        uv: [u, v],
        colour: [1.0; 4],
    }
}

/// A square of side one as two indexed triangles, the top of its texture at the top.
fn square() -> (Vec<ModelVertex>, Vec<u32>) {
    (
        vec![
            vertex(-0.5, -0.5, 0.0, 1.0),
            vertex(0.5, -0.5, 1.0, 1.0),
            vertex(0.5, 0.5, 1.0, 0.0),
            vertex(-0.5, 0.5, 0.0, 0.0),
        ],
        vec![0, 1, 2, 0, 2, 3],
    )
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    kind: &EffectKind,
    mesh: &(Vec<ModelVertex>, Vec<u32>),
) -> Result<Rendered, EffectError> {
    let target = Target::default();
    let mut pass = ModelPass::prepare(&mut EffectCache::new(), device, queue, pack, &target, kind)?;
    pass.update(device, queue, &flat(), &mesh.0, &mesh.1);
    Ok(draw_into(device, queue, &target, SIZE, &mut |render| {
        pass.record(render)
    }))
}

fn at(rendered: &Rendered, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIZE + x) * 4) as usize;
    [
        rendered.bytes[i],
        rendered.bytes[i + 1],
        rendered.bytes[i + 2],
        rendered.bytes[i + 3],
    ]
}

#[test]
fn what_the_vertex_stage_reads_is_the_vertex_by_semantic() {
    let vertex = ModelVertex {
        position: Vec3::new(1.0, 2.0, 3.0),
        normal: Vec3::new(0.0, 0.0, 1.0),
        uv: [0.25, 0.75],
        colour: [0.1, 0.2, 0.3, 0.4],
    };
    assert_eq!(model_value("POSITION", 0, &vertex), [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(model_value("NORMAL", 0, &vertex), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(model_value("TEXCOORD", 0, &vertex), [0.25, 0.75, 0.0, 0.0]);
    assert_eq!(model_value("TEXCOORD", 1, &vertex), [0.25, 0.75, 0.0, 0.0]);
    assert_eq!(model_value("COLOR", 0, &vertex), [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(model_value("TANGENT", 0, &vertex), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(model_value("BLENDWEIGHT", 0, &vertex), [0.0; 4]);
}

#[test]
fn an_indexed_square_covers_what_its_vertices_say_with_its_texture_the_right_way_up() {
    let Some((device, queue)) = gpu("an_indexed_square_covers_what_its_vertices_say") else {
        return;
    };
    let dir = model_pack("draw", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let mut kind = kind();
    // A red top row over a green one
    kind.texture = Some(Arc::new(Image {
        width: 1,
        height: 2,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        srgb: false,
    }));
    let rendered = render(&device, &queue, &pack, &kind, &square()).unwrap();
    // Columns and rows 4 to 11 have their centres inside the square
    assert_eq!(rendered.touched(), 8 * 8);
    let top = at(&rendered, 8, 5);
    let bottom = at(&rendered, 8, 10);
    assert!(top[0] > 200 && top[1] < 60, "the top is red: {top:?}");
    assert!(
        bottom[1] > 200 && bottom[0] < 60,
        "the bottom is green: {bottom:?}"
    );
}

#[test]
fn a_mesh_with_no_indices_draws_nothing() {
    let Some((device, queue)) = gpu("a_mesh_with_no_indices_draws_nothing") else {
        return;
    };
    let dir = model_pack("draw", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let (vertices, _) = square();
    let rendered = render(&device, &queue, &pack, &kind(), &(vertices, Vec::new())).unwrap();
    assert_eq!(rendered.touched(), 0);
}

#[test]
fn a_technique_the_pack_lacks_and_a_skinned_one_are_refused_by_name() {
    let Some((device, queue)) = gpu("a_technique_the_pack_lacks_model") else {
        return;
    };
    let dir = model_pack("draw", None);
    let pack = GamePack::open(dir.path()).unwrap();
    let missing = EffectKind {
        technique: "lightweight0_draw".into(),
        ..kind()
    };
    let error = render(&device, &queue, &pack, &missing, &square())
        .err()
        .unwrap();
    assert!(
        matches!(error, EffectError::NoTechnique { effect, technique } if effect == "ptfx_model" && technique == "lightweight0_draw")
    );

    let dir = model_pack("drawskinned", Some(("BLENDWEIGHT", 4)));
    let skinned = GamePack::open(dir.path()).unwrap();
    let kind = EffectKind {
        technique: "drawskinned".into(),
        ..kind()
    };
    let error = render(&device, &queue, &skinned, &kind, &square())
        .err()
        .unwrap();
    assert!(matches!(error, EffectError::Skinned(name) if name == "drawskinned"));
}

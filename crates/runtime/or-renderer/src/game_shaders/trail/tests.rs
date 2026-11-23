use super::*;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::mesh::Image;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::synthetic::trail_pack;
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
        ..Default::default()
    }
}

fn point(x: f32, y: f32, u: f32, v: f32) -> TrailPoint {
    TrailPoint {
        position: Vec3::new(x, y, 0.0),
        colour: [1.0; 4],
        uv: [u, v],
        normal: Vec3::Z,
    }
}

/// The corners of a square, in the order a strip wants them, the top of the texture at the top.
fn square() -> Vec<TrailPoint> {
    vec![
        point(-0.5, -0.5, 0.0, 1.0),
        point(0.5, -0.5, 1.0, 1.0),
        point(-0.5, 0.5, 0.0, 0.0),
        point(0.5, 0.5, 1.0, 0.0),
    ]
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    kind: &EffectKind,
    points: &[TrailPoint],
) -> Result<Rendered, EffectError> {
    let target = Target::default();
    let mut pass = TrailPass::prepare(&mut EffectCache::new(), device, queue, pack, &target, kind)?;
    pass.update(device, queue, &flat(), points);
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
fn what_the_vertex_stage_reads_is_the_point_by_semantic() {
    let point = TrailPoint {
        position: Vec3::new(1.0, 2.0, 3.0),
        colour: [0.1, 0.2, 0.3, 0.4],
        uv: [0.5, 0.6],
        normal: Vec3::new(0.0, 1.0, 0.0),
    };
    assert_eq!(trail_value("POSITION", 0, &point), [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(trail_value("COLOR", 0, &point), [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(trail_value("TEXCOORD", 0, &point), [0.5, 0.6, 0.0, 0.0]);
    assert_eq!(trail_value("TEXCOORD", 1, &point), [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(trail_value("TEXCOORD", 7, &point), [0.0; 4]);
}

#[test]
fn a_strip_of_four_points_covers_the_square_they_make_with_its_texture_the_right_way_up() {
    let Some((device, queue)) = gpu("a_strip_of_four_points_covers_the_square") else {
        return;
    };
    let dir = trail_pack("draw");
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
fn a_strip_of_fewer_than_four_points_draws_nothing() {
    let Some((device, queue)) = gpu("a_strip_of_fewer_than_four_points") else {
        return;
    };
    let dir = trail_pack("draw");
    let pack = GamePack::open(dir.path()).unwrap();
    let three = square()[..3].to_vec();
    let rendered = render(&device, &queue, &pack, &kind(), &three).unwrap();
    assert_eq!(rendered.touched(), 0);
}

#[test]
fn a_technique_the_pack_lacks_is_refused_by_name() {
    let Some((device, queue)) = gpu("a_technique_the_pack_lacks_trail") else {
        return;
    };
    let dir = trail_pack("draw");
    let pack = GamePack::open(dir.path()).unwrap();
    let missing = EffectKind {
        technique: "RGBA_lit".into(),
        ..kind()
    };
    let error = render(&device, &queue, &pack, &missing, &square())
        .err()
        .unwrap();
    assert!(
        matches!(error, EffectError::NoTechnique { effect, technique } if effect == "ptfx_trail" && technique == "RGBA_lit")
    );
}

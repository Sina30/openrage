use super::*;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::synthetic::trail_pack;
use crate::game_shaders::test_pack::{draw_into, gpu};
use crate::particles::ParticleBlendMode;
use glam::Mat4;

const SIZE: u32 = 16;

/// The camera of the scene is nothing: a point's place on the target is its own x and y.
fn flat() -> EngineConstants {
    EngineConstants {
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        ..Default::default()
    }
}

fn vertex(x: f32, y: f32) -> TrailVertex {
    TrailVertex {
        position: [x, y, 0.0],
        color: [1.0, 0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
    }
}

fn batch(technique: &str, vertices: Vec<TrailVertex>) -> TrailBatch {
    TrailBatch {
        blend: ParticleBlendMode::Alpha,
        vertices,
        constants: Vec::new(),
        shader_technique: technique.into(),
        diffuse_image: None,
    }
}

/// A square of side one, in the order a strip wants its corners.
fn square() -> Vec<TrailVertex> {
    vec![
        vertex(-0.5, -0.5),
        vertex(0.5, -0.5),
        vertex(-0.5, 0.5),
        vertex(0.5, 0.5),
    ]
}

fn setup(test: &str) -> Option<(wgpu::Device, wgpu::Queue, tempfile::TempDir, Arc<GamePack>)> {
    let (device, queue) = gpu(test)?;
    let dir = trail_pack("draw");
    let pack = Arc::new(GamePack::open(dir.path()).unwrap());
    Some((device, queue, dir, pack))
}

fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    trails: &GameTrails,
    batches: &[TrailBatch],
    accept: &dyn Fn(&TrailBatch) -> bool,
) -> Rendered {
    draw_into(device, queue, &Target::default(), SIZE, &mut |pass| {
        trails.draw(pass, batches, accept)
    })
}

#[test]
fn a_vertexs_normal_is_the_direction_to_the_eye() {
    let point = point_of(&vertex(0.0, 3.0), Vec3::new(0.0, 3.0, 5.0));
    assert!(
        point.normal.abs_diff_eq(Vec3::Z, 1e-6),
        "{:?}",
        point.normal
    );
    assert_eq!(point.position, Vec3::new(0.0, 3.0, 0.0));
    assert_eq!(point.colour, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn the_strip_of_a_batch_is_drawn_where_its_vertices_put_it() {
    let Some((device, queue, _dir, pack)) = setup("the_strip_of_a_batch_is_drawn") else {
        return;
    };
    let mut trails = GameTrails::new(pack, Target::default());
    let batches = [batch("draw", square())];
    trails.prepare(
        &device,
        &queue,
        &batches,
        &flat(),
        Vec3::new(0.0, 0.0, 10.0),
    );
    assert_eq!(trails.drawn_batches(), 1);
    let rendered = draw(&device, &queue, &trails, &batches, &|_| true);
    // Columns and rows 4 to 11 have their centres inside the square
    assert_eq!(rendered.touched(), 8 * 8);
    let i = ((8 * SIZE + 8) * 4) as usize;
    assert_eq!(&rendered.bytes[i..i + 4], &[255, 0, 0, 255]);
}

#[test]
fn a_batch_without_a_technique_or_one_the_caller_does_not_take_draws_nothing() {
    let Some((device, queue, _dir, pack)) = setup("a_trail_batch_without_a_technique") else {
        return;
    };
    let mut trails = GameTrails::new(pack, Target::default());
    let eye = Vec3::new(0.0, 0.0, 10.0);
    let batches = [batch("", square())];
    trails.prepare(&device, &queue, &batches, &flat(), eye);
    assert_eq!(trails.drawn_batches(), 0, "no technique, no pass");

    let batches = [batch("draw", square())];
    trails.prepare(&device, &queue, &batches, &flat(), eye);
    assert_eq!(
        draw(&device, &queue, &trails, &batches, &|_| false).touched(),
        0
    );
    assert!(draw(&device, &queue, &trails, &batches, &|_| true).touched() > 0);
}

#[test]
fn a_batch_of_too_few_vertices_for_a_frame_keeps_its_pass_and_draws_none() {
    let Some((device, queue, _dir, pack)) = setup("a_trail_batch_of_too_few_vertices") else {
        return;
    };
    let mut trails = GameTrails::new(pack, Target::default());
    let eye = Vec3::new(0.0, 0.0, 10.0);
    let touched = |trails: &mut GameTrails, vertices: Vec<TrailVertex>| {
        let batches = [batch("draw", vertices)];
        trails.prepare(&device, &queue, &batches, &flat(), eye);
        draw(&device, &queue, trails, &batches, &|_| true).touched()
    };
    assert!(touched(&mut trails, square()) > 0);
    assert_eq!(touched(&mut trails, square()[..3].to_vec()), 0);
    assert_eq!(trails.drawn_batches(), 1, "the pass is still there");
    assert!(touched(&mut trails, square()) > 0);
}

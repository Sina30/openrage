use super::*;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::synthetic::model_pack;
use crate::game_shaders::test_pack::{draw_into, gpu};
use crate::particle_models::ModelVertex;
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

fn vertex(x: f32, y: f32) -> ModelVertex {
    ModelVertex {
        position: [x, y, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
    }
}

/// A square of side one as two triangles.
fn square() -> Arc<ModelGeometry> {
    Arc::new(ModelGeometry {
        vertices: vec![
            vertex(-0.5, -0.5),
            vertex(0.5, -0.5),
            vertex(0.5, 0.5),
            vertex(-0.5, 0.5),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        size: [1.0; 3],
    })
}

fn instance(x: f32, y: f32) -> ModelInstance {
    ModelInstance {
        position: [x, y, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
        color: [1.0, 0.0, 0.0, 1.0],
    }
}

fn batch(technique: &str, instances: Vec<ModelInstance>) -> ModelBatch {
    ModelBatch {
        instances,
        blend: ParticleBlendMode::Alpha,
        shader_technique: technique.into(),
        diffuse_image: None,
        geometry: Some(square()),
    }
}

fn setup(test: &str) -> Option<(wgpu::Device, wgpu::Queue, tempfile::TempDir, Arc<GamePack>)> {
    let (device, queue) = gpu(test)?;
    let dir = model_pack("draw", None);
    let pack = Arc::new(GamePack::open(dir.path()).unwrap());
    Some((device, queue, dir, pack))
}

fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    models: &GameModels,
    batches: &[ModelBatch],
) -> Rendered {
    draw_into(device, queue, &Target::default(), SIZE, &mut |pass| {
        models.draw(pass, batches)
    })
}

fn laid_out(instances: &[ModelInstance]) -> Vec<WorldVertex> {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    lay_out(&square(), instances, &mut vertices, &mut indices);
    vertices
}

#[test]
fn an_instance_puts_the_mesh_in_the_world_scaled_then_turned_then_moved() {
    let turn = std::f32::consts::FRAC_1_SQRT_2;
    let moved = ModelInstance {
        position: [10.0, 0.0, 0.0],
        rotation: [0.0, 0.0, turn, turn],
        scale: [2.0, 1.0, 1.0],
        color: [0.1, 0.2, 0.3, 0.4],
    };
    let vertices = laid_out(&[moved]);
    // The corner (0.5, -0.5) is stretched to (1, -0.5), then a quarter turn about z takes it to (0.5, 1)
    assert!(
        vertices[1]
            .position
            .abs_diff_eq(Vec3::new(10.5, 1.0, 0.0), 1e-5),
        "{:?}",
        vertices[1].position
    );
    assert!(vertices[1].normal.abs_diff_eq(Vec3::Z, 1e-5));
    assert_eq!(vertices[1].colour, [0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn a_mesh_is_drawn_at_the_size_the_instance_states_whatever_size_it_was_made_at() {
    // A mesh four wide and two high, drawn as a particle one by one: its box becomes the dimensions
    let wide = ModelGeometry {
        vertices: vec![vertex(-2.0, -1.0), vertex(2.0, 1.0)],
        indices: vec![0, 1],
        size: [4.0, 2.0, 0.0],
    };
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    lay_out(&wide, &[instance(0.0, 0.0)], &mut vertices, &mut indices);
    assert!(vertices[0]
        .position
        .abs_diff_eq(Vec3::new(-0.5, -0.5, 0.0), 1e-6));
    assert!(vertices[1]
        .position
        .abs_diff_eq(Vec3::new(0.5, 0.5, 0.0), 1e-6));

    // And twice the dimensions is twice the size; a box with no depth leaves that axis as stated
    let twice = ModelInstance {
        scale: [2.0, 2.0, 3.0],
        ..instance(0.0, 0.0)
    };
    let flat = ModelGeometry {
        vertices: vec![ModelVertex {
            position: [1.0, 1.0, 1.0],
            ..vertex(0.0, 0.0)
        }],
        indices: vec![0],
        size: [2.0, 2.0, 0.0],
    };
    lay_out(&flat, &[twice], &mut vertices, &mut indices);
    assert!(vertices[0]
        .position
        .abs_diff_eq(Vec3::new(1.0, 1.0, 3.0), 1e-6));
}
#[test]
fn each_instance_gets_its_own_copy_of_the_mesh_and_its_indices() {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    lay_out(
        &square(),
        &[instance(0.0, 0.0), instance(1.0, 0.0)],
        &mut vertices,
        &mut indices,
    );
    assert_eq!(vertices.len(), 8);
    assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
}

#[test]
fn a_normal_of_a_stretched_model_follows_the_surface() {
    // A slope of one stretched twice as wide in x leans half as far
    let geometry = ModelGeometry {
        vertices: vec![ModelVertex {
            position: [0.0; 3],
            normal: Vec3::new(1.0, 0.0, 1.0).normalize().into(),
            uv: [0.0; 2],
        }],
        indices: vec![0],
        size: [1.0; 3],
    };
    let stretched = ModelInstance {
        scale: [2.0, 1.0, 1.0],
        ..instance(0.0, 0.0)
    };
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    lay_out(&geometry, &[stretched], &mut vertices, &mut indices);
    let expected = Vec3::new(0.5, 0.0, 1.0).normalize();
    assert!(vertices[0].normal.abs_diff_eq(expected, 1e-5));
}

#[test]
fn the_instances_of_a_batch_are_drawn_where_they_stand() {
    let Some((device, queue, _dir, pack)) = setup("the_instances_of_a_batch_are_drawn") else {
        return;
    };
    let mut models = GameModels::new(pack, Target::default());
    let batches = [batch("draw", vec![instance(-0.5, 0.0), instance(0.5, 0.0)])];
    models.prepare(&device, &queue, &batches, &flat());
    assert_eq!(models.drawn_batches(), 1);
    let rendered = draw(&device, &queue, &models, &batches);
    // Two squares of eight columns by eight rows side by side
    assert_eq!(rendered.touched(), 2 * 8 * 8);
    let i = ((8 * SIZE + 4) * 4) as usize;
    assert_eq!(&rendered.bytes[i..i + 4], &[255, 0, 0, 255]);
}

#[test]
fn a_batch_without_a_technique_or_a_mesh_draws_nothing_and_one_without_instances_keeps_its_pass() {
    let Some((device, queue, _dir, pack)) = setup("a_model_batch_without_a_technique") else {
        return;
    };
    let mut models = GameModels::new(pack, Target::default());
    let stand = || vec![instance(0.0, 0.0)];
    let mut batches = [batch("", stand())];
    models.prepare(&device, &queue, &batches, &flat());
    assert_eq!(models.drawn_batches(), 0, "no technique, no pass");

    batches[0] = batch("draw", stand());
    batches[0].geometry = None;
    models.prepare(&device, &queue, &batches, &flat());
    assert_eq!(models.drawn_batches(), 0, "no mesh, no pass");

    batches[0] = batch("draw", stand());
    models.prepare(&device, &queue, &batches, &flat());
    assert!(draw(&device, &queue, &models, &batches).touched() > 0);
    batches[0].instances.clear();
    models.prepare(&device, &queue, &batches, &flat());
    assert_eq!(models.drawn_batches(), 1, "the pass is still there");
    assert_eq!(draw(&device, &queue, &models, &batches).touched(), 0);
}

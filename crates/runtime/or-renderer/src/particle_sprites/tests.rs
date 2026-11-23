use super::*;
use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::frame::Rendered;
use crate::game_shaders::synthetic::sprite_pack;
use crate::game_shaders::test_pack::{draw_into, gpu};
use crate::particles::ParticleBlendMode;
use glam::Mat4;

const SIZE: u32 = 16;

fn near(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-5)
}

fn instance(position: [f32; 3]) -> ParticleInstance {
    ParticleInstance {
        position,
        dimensions: [0.5; 4],
        color: [1.0, 0.0, 0.0, 1.0],
        ..Default::default()
    }
}

/// A batch of the technique `technique`, aligned to the y axis so that, seen from `EYE`, its width runs along x and its height along y.
fn batch(technique: &str, instances: Vec<ParticleInstance>) -> ParticleBatch {
    let mut batch = ParticleBatch::new(
        ParticleBlendMode::Alpha,
        ParticleAlignment::World,
        instances,
    );
    batch.shader_technique = technique.into();
    batch.align_axis = [0.0, 1.0, 0.0];
    batch
}

const EYE: Vec3 = Vec3::new(0.0, 0.0, 10.0);

/// The camera of the scene is nothing: a point's place on the target is its own x and y.
fn flat() -> EngineConstants {
    EngineConstants {
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        ..Default::default()
    }
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
fn a_sprite_facing_the_camera_runs_its_width_to_the_right_and_its_height_up() {
    // A camera on the negative y axis, looking along y with z up
    let eye = Vec3::new(0.0, -10.0, 0.0);
    let (right, up) = basis(
        ParticleAlignment::Camera,
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::Z,
        eye,
    );
    assert!(near(right, Vec3::X), "{right:?}");
    assert!(near(up, Vec3::Z), "{up:?}");
}

#[test]
fn a_sprite_aligned_to_its_velocity_runs_its_height_along_it() {
    let eye = Vec3::new(0.0, -10.0, 0.0);
    let (right, up) = basis(
        ParticleAlignment::Velocity,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, 2.0),
        Vec3::Z,
        eye,
    );
    assert!(near(up, Vec3::Z), "{up:?}");
    assert!(near(right, Vec3::X), "{right:?}");
}

#[test]
fn a_particles_four_dimensions_sum_to_its_width_and_height_and_its_spin_turns_it() {
    let mut batch = batch("RGBA", Vec::new());
    batch.alignment = ParticleAlignment::Camera;
    let eye = Vec3::new(0.0, -10.0, 0.0);
    let mut particle = ParticleInstance {
        dimensions: [0.5, 1.5, 0.25, 0.75],
        ..instance([1.0, 2.0, 3.0])
    };
    let (right, up) = basis(
        ParticleAlignment::Camera,
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::ZERO,
        Vec3::Z,
        eye,
    );
    let sprite = sprite_of(&particle, &batch, eye);
    // Two high and one wide, so half a width to the right and half a height up
    assert!(near(sprite.right, right * 0.5), "{:?}", sprite.right);
    assert!(near(sprite.up, up * 1.0), "{:?}", sprite.up);
    assert_eq!(sprite.centre, Vec3::new(1.0, 2.0, 3.0));

    // A quarter turn carries the right to the up and the up to the left, each keeping its own length
    particle.rotation = [std::f32::consts::FRAC_PI_2, 0.0, 0.0, 0.0];
    let turned = sprite_of(&particle, &batch, eye);
    assert!(near(turned.right, up * 1.0), "{:?}", turned.right);
    assert!(near(turned.up, right * -0.5), "{:?}", turned.up);
}

#[test]
fn a_frame_number_is_a_cell_of_the_atlas_counted_along_its_rows() {
    assert_eq!(frame_cell(0.0, 1, 1, [0.0; 2]), [0.0, 1.0, 0.0, 1.0]);
    // Frame 5 of four columns by two rows: the second column of the second row
    assert_eq!(frame_cell(5.0, 2, 4, [0.0; 2]), [0.25, 0.5, 0.5, 1.0]);
    assert_eq!(frame_cell(5.7, 2, 4, [0.0; 2]), [0.25, 0.5, 0.5, 1.0]);
    assert_eq!(
        frame_cell(0.0, 1, 1, [0.5, -0.25]),
        [0.5, 1.5, -0.25, 0.75],
        "the offset moves the whole cell"
    );
}

fn setup(test: &str) -> Option<(wgpu::Device, wgpu::Queue, tempfile::TempDir, Arc<GamePack>)> {
    let (device, queue) = gpu(test)?;
    let dir = sprite_pack("RGBA", None);
    let pack = Arc::new(GamePack::open(dir.path()).unwrap());
    Some((device, queue, dir, pack))
}

#[test]
fn the_sprites_of_a_batch_are_drawn_where_the_particles_put_them_and_in_their_colour() {
    let Some((device, queue, _dir, pack)) = setup("the_sprites_of_a_batch_are_drawn") else {
        return;
    };
    let mut sprites = GameSprites::new(pack, Target::default());
    // A unit square centred at x 0.2, seen from straight above it: x from -0.3 to 0.7, y from -0.5 to 0.5
    let batches = [batch("RGBA", vec![instance([0.2, 0.0, 0.0])])];
    sprites.prepare(
        &device,
        &queue,
        &batches,
        &flat(),
        Vec3::new(0.2, 0.0, 10.0),
    );
    assert_eq!(sprites.drawn_batches(), 1);
    let target = Target::default();
    let rendered = draw_into(&device, &queue, &target, SIZE, &mut |pass| {
        sprites.draw(pass, &batches, &|_| true)
    });
    // Columns 6 to 13 and rows 4 to 11 have their centres inside it
    assert_eq!(rendered.touched(), 8 * 8);
    assert_eq!(at(&rendered, 8, 8), [255, 0, 0, 255]);
    assert_eq!(at(&rendered, 4, 8), [0; 4]);
}

#[test]
fn a_batch_without_a_technique_or_one_the_caller_does_not_take_draws_nothing() {
    let Some((device, queue, _dir, pack)) = setup("a_batch_without_a_technique") else {
        return;
    };
    let mut sprites = GameSprites::new(pack, Target::default());
    let batches = [batch("", vec![instance([0.0; 3])])];
    sprites.prepare(&device, &queue, &batches, &flat(), EYE);
    assert_eq!(sprites.drawn_batches(), 0, "no technique, no pass");

    let batches = [batch("RGBA", vec![instance([0.0; 3])])];
    sprites.prepare(&device, &queue, &batches, &flat(), EYE);
    let target = Target::default();
    let skipped = draw_into(&device, &queue, &target, SIZE, &mut |pass| {
        sprites.draw(pass, &batches, &|_| false)
    });
    assert_eq!(skipped.touched(), 0);
    let taken = draw_into(&device, &queue, &target, SIZE, &mut |pass| {
        sprites.draw(pass, &batches, &|_| true)
    });
    assert!(taken.touched() > 0);
}

#[test]
fn a_batch_with_no_particles_for_a_frame_keeps_its_pass_and_draws_none() {
    let Some((device, queue, _dir, pack)) = setup("a_batch_with_no_particles_for_a_frame") else {
        return;
    };
    let mut sprites = GameSprites::new(pack, Target::default());
    let target = Target::default();
    let touched = |sprites: &mut GameSprites, particles: usize| {
        let batches = [batch("RGBA", vec![instance([0.0; 3]); particles])];
        sprites.prepare(&device, &queue, &batches, &flat(), EYE);
        draw_into(&device, &queue, &target, SIZE, &mut |pass| {
            sprites.draw(pass, &batches, &|_| true)
        })
        .touched()
    };
    assert!(touched(&mut sprites, 1) > 0);
    assert_eq!(touched(&mut sprites, 0), 0);
    assert_eq!(sprites.drawn_batches(), 1, "the pass is still there");
    assert!(touched(&mut sprites, 1) > 0);
}

#[test]
fn a_rule_variable_sets_the_constant_named_for_it_with_or_without_the_g() {
    let Some((device, queue, _dir, pack)) = setup("a_rule_variable_sets_the_constant") else {
        return;
    };
    let hash = or_io::hash::string_hash_str;
    // The pixel under the middle of a sprite, drawn with these values for the rule's variables
    let centre = |constants: Vec<(u32, [f32; 4])>| {
        let mut sprites = GameSprites::new(pack.clone(), Target::default());
        let mut batches = [batch("RGBA", vec![instance([0.2, 0.0, 0.0])])];
        batches[0].constants = constants;
        sprites.prepare(
            &device,
            &queue,
            &batches,
            &flat(),
            Vec3::new(0.2, 0.0, 10.0),
        );
        let target = Target::default();
        let rendered = draw_into(&device, &queue, &target, SIZE, &mut |pass| {
            sprites.draw(pass, &batches, &|_| true)
        });
        at(&rendered, 8, 8)
    };
    let half = [0.5, 0.0, 0.0, 0.0];
    let plain = centre(Vec::new());
    assert_eq!(plain, [255, 0, 0, 255]);

    // The synthetic pixel stage multiplies by `gSuperAlpha`, and a half of it is a quarter of the red once blended
    let without_g = centre(vec![(hash("SuperAlpha"), half)]);
    assert!(
        (60..=68).contains(&without_g[0]) && without_g[3] < 140,
        "{without_g:?}"
    );
    assert_eq!(centre(vec![(hash("gSuperAlpha"), half)]), without_g);
    // A variable no constant of the pass carries changes nothing
    assert_eq!(centre(vec![(hash("NoSuchVariable"), half)]), plain);
}

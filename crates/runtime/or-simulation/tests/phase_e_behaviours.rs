#![allow(clippy::field_reassign_with_default)]

use glam::{Mat4, Vec3, Vec4};
use or_io::decl::Gen8;
use or_io::gta5::particle::KeyframeMode;
use or_io::gta5::particle::{PtxEffectRule, PtxEmitterRule, PtxParticleRule};
use or_particle::behaviour::{AttractorBehaviour, ColourBehaviour, SizeBehaviour};
use or_simulation::behaviour::{AttractorGeometry, BehaviourImpl, BehaviourParams};
use or_simulation::behaviours::{Attractor, Colour, Size};
use or_simulation::point::{PointBuffer, PointFlags};
use or_simulation::random::RandomTable;
use or_simulation::random_table::PtxRandomTable;
fn one_point_buf() -> PointBuffer {
    let mut buf = PointBuffer::default();
    buf.spawn(1);
    buf.life_ratio[0] = 0.5;
    buf.emitter_life_ratio[0] = 0.5;
    buf.playback_rate[0] = 1.0;
    buf.rand_index[0] = 7;
    buf.color[0] = Vec4::new(0.5, 0.5, 0.5, 0.5);
    buf.flags[0].insert(PointFlags::IS_BEING_INITIALISED);
    buf
}

fn make_params<'a>(
    rule: &'a PtxParticleRule<Gen8>,
    effect: &'a PtxEffectRule<Gen8>,
    emitter: &'a PtxEmitterRule,
    table: &'a RandomTable,
    ptx_table: &'a PtxRandomTable,
    attractor_pos: Vec3,
) -> BehaviourParams<'a> {
    BehaviourParams {
        dt: 1.0 / 60.0,
        effect_time_ratio: 0.5,
        emitter_time_ratio: 0.5,
        effect_zoom: 1.0,
        emitter_zoom: 1.0,
        effect_matrix: Mat4::IDENTITY,
        emitter_matrix: Mat4::IDENTITY,
        effect_velocity: Vec3::ZERO,
        emitter_velocity: Vec3::ZERO,
        gravity: 0.0,
        wind_velocity: Vec3::ZERO,
        random: table,
        ptx_random_table: ptx_table,
        evo_list: None,
        evo_values: &[],
        particle_rule: rule,
        shader_vars: &[],
        effect_rule: effect,
        emitter_rule: emitter,
        camera_pos: Vec3::ZERO,
        camera_basis: glam::Mat3::IDENTITY,
        emitter_colour_tint: Vec4::ONE,
        emitter_attractor_pos: attractor_pos,
        attractor_geometry: AttractorGeometry::default(),
        effect_colour_tint_override: Vec4::ONE,
        effect_rgba_rand_index: 0,
        collision_bvh: None,
        river_field: None,
        wind_field: None,
        wall_time_seconds: 0.0,
        real_dt: 1.0 / 60.0,
        water_field: None,
        water_z_camera: 0.0,
        air_resistance: 1.0,
        lod_alpha_mult: 1.0,
        sinks: or_simulation::PtfxSinks::none(),
        effect_spawner_trigger_ratio: -1.0,
    }
}

#[test]
fn proportional_size_uses_single_bias_for_xyz() {
    let mut buf = one_point_buf();
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let table = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &table, &ptx_table, Vec3::ZERO);

    let mut data = SizeBehaviour::default();
    data.is_proportional = true;
    data.whd_min_keyframes = None;
    data.whd_max_keyframes = None;
    let size = Size::from_data(data.clone());
    size.update_point(0, &mut buf, &params);
    let dim = buf.dimensions[0];
    assert!(
        (dim.x - dim.y).abs() < 1e-6 && (dim.y - dim.z).abs() < 1e-6,
        "proportional dims should be equal; got {:?}",
        dim
    );
}

#[test]
fn attractor_pulls_toward_attractor_pos_not_emitter_matrix() {
    let mut buf = one_point_buf();
    buf.curr_pos[0] = Vec3::new(5.0, 0.0, 0.0);
    buf.curr_vel[0] = Vec3::ZERO;

    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let table = RandomTable::new();
    let attractor_pos = Vec3::new(0.0, 10.0, 0.0);
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &table, &ptx_table, attractor_pos);
    params.attractor_geometry = AttractorGeometry {
        pos: attractor_pos,
        dir: Vec3::Y,
        radius_near: 0.0,
        radius_far: 100.0,
        radius_orbit: 0.0,
        length: 0.5,
    };

    let mut att_data = AttractorBehaviour::default();
    att_data.strength_min = 5.0;
    att_data.strength_max = 5.0;
    let att = Attractor::from_data(att_data);
    att.update_point(0, &mut buf, &params);

    let v = buf.curr_vel[0];
    assert!(v.y > 0.0, "expected +Y pull, got {:?}", v);
    assert!(
        v.y.abs() > v.x.abs(),
        "Y component should dominate, got {:?}",
        v
    );
}

#[test]
fn spawned_colour_tint_folds_into_effect_tint_not_emitter_tint() {
    let mut buf = one_point_buf();
    let rule = PtxParticleRule::default();
    let mut effect = PtxEffectRule::default();
    effect.colour_tint_max_enable = or_io::decl::Bool8::new(false);
    let emitter = PtxEmitterRule::default();
    let table = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &table, &ptx_table, Vec3::ZERO);
    params.emitter_colour_tint = Vec4::ONE;
    params.effect_colour_tint_override = Vec4::new(0.5, 0.0, 0.0, 1.0);

    let mut data = ColourBehaviour::default();
    data.rgba_min.r = 1.0;
    data.rgba_min.g = 1.0;
    data.rgba_min.b = 1.0;
    data.rgba_min.a = 1.0;
    data.rgba_max_enable = false;
    data.rgb_can_tint = false;
    data.keyframe_mode = KeyframeMode::Point;
    let colour = Colour::from_data(data);
    colour.update_point(0, &mut buf, &params);
    let c = buf.color[0];
    assert!(
        (c.x - 0.5).abs() < 1e-3,
        "R should be 0.5 (single fold), got {}",
        c.x
    );
    assert!(
        c.y.abs() < 1e-3,
        "G should be 0 (override killed it), got {}",
        c.y
    );
    assert!(
        c.z.abs() < 1e-3,
        "B should be 0 (override killed it), got {}",
        c.z
    );
    assert!((c.w - 1.0).abs() < 1e-3, "alpha should be 1.0, got {}", c.w);
}

#[test]
fn rgb_can_tint_preserves_alpha() {
    let mut buf = one_point_buf();
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let table = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &table, &ptx_table, Vec3::ZERO);
    params.emitter_colour_tint = Vec4::new(0.5, 0.5, 0.5, 0.5);

    let mut data = ColourBehaviour::default();
    data.rgba_min.r = 1.0;
    data.rgba_min.g = 1.0;
    data.rgba_min.b = 1.0;
    data.rgba_min.a = 1.0;
    data.rgba_max_enable = false;
    data.rgb_can_tint = true;
    data.keyframe_mode = KeyframeMode::Point;
    let colour = Colour::from_data(data);
    colour.update_point(0, &mut buf, &params);
    let c = buf.color[0];
    assert!((c.x - 0.5).abs() < 1e-3, "R should be 0.5, got {}", c.x);
    assert!(
        (c.w - 1.0).abs() < 1e-3,
        "alpha should remain 1.0, got {}",
        c.w
    );
}

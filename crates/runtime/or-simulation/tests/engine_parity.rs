#![allow(clippy::field_reassign_with_default)]

use glam::{Mat4, Vec3, Vec4};
use or_io::decl::Gen8;
use or_io::gta5::particle::{PtxEffectRule, PtxEmitterRule, PtxParticleRule};
use or_simulation::behaviour::{AttractorGeometry, BehaviourImpl, BehaviourParams};
use or_simulation::behaviours::{Acceleration, Age, Pause, Velocity};
use or_simulation::point::{PointBuffer, PointFlags};
use or_simulation::random::RandomTable;
use or_simulation::random_table::PtxRandomTable;
fn one_point(life_ratio: f32) -> PointBuffer {
    let mut buf = PointBuffer::default();
    buf.spawn(0);
    buf.life_ratio[0] = life_ratio;
    buf.emitter_life_ratio[0] = life_ratio;
    buf.one_over_life[0] = 1.0;
    buf.playback_rate[0] = 1.0;
    buf.flags[0].remove(PointFlags::IS_BEING_INITIALISED);
    buf
}

fn make_params<'a>(
    rule: &'a PtxParticleRule<Gen8>,
    effect: &'a PtxEffectRule<Gen8>,
    emitter: &'a PtxEmitterRule,
    rand: &'a RandomTable,
    ptx_table: &'a PtxRandomTable,
    dt: f32,
) -> BehaviourParams<'a> {
    BehaviourParams {
        dt,
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
        random: rand,
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
        emitter_attractor_pos: Vec3::ZERO,
        attractor_geometry: AttractorGeometry::default(),
        effect_colour_tint_override: Vec4::ONE,
        effect_rgba_rand_index: 0,
        collision_bvh: None,
        river_field: None,
        wind_field: None,
        wall_time_seconds: 0.0,
        real_dt: dt,
        water_field: None,
        water_z_camera: 0.0,
        air_resistance: 1.0,
        lod_alpha_mult: 1.0,
        sinks: or_simulation::PtfxSinks::none(),
        effect_spawner_trigger_ratio: -1.0,
    }
}

#[test]
fn age_advances_life_by_one_over_life_times_dt() {
    let buf = one_point(0.0);
    let particle_life = 2.0_f32;
    let mut buf2 = buf.clone();
    buf2.one_over_life[0] = 1.0 / particle_life;
    buf2.playback_rate[0] = 2.0;
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.5);
    let age = Age::from_data(or_particle::behaviour::AgeBehaviour::default());
    age.update_point(0, &mut buf2, &params);
    assert!(
        (buf2.life_ratio[0] - 0.25).abs() < 1e-5,
        "got {}",
        buf2.life_ratio[0]
    );
}

#[test]
fn velocity_init_point_does_not_seed_xyz_minmax() {
    let mut buf = one_point(0.0);
    buf.flags[0].insert(PointFlags::IS_BEING_INITIALISED);
    let mut data = or_particle::behaviour::VelocityBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(7.0, 8.0, 9.0);
    data.xyz_max = or_particle::domain::Vec3::new(7.0, 8.0, 9.0);
    let v = Velocity::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.016);
    v.init_point(0, &mut buf, &params);
    assert!(
        buf.curr_vel[0].length() < 1e-5,
        "velocity got seeded: {:?}",
        buf.curr_vel[0]
    );
}

#[test]
fn pause_clamps_life_on_transition_frame() {
    let mut buf = one_point(0.6);
    let mut data = or_particle::behaviour::PauseBehaviour::default();
    data.pause_life_ratio = 0.5;
    data.unpause_time = 100.0;
    let p = Pause::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.016);
    p.update_point(0, &mut buf, &params);
    assert!(buf.flags[0].contains(PointFlags::LIFE_PAUSED));
    assert!(
        (buf.life_ratio[0] - 0.5).abs() < 1e-5,
        "life not clamped: {}",
        buf.life_ratio[0]
    );
}

#[test]
fn acceleration_applies_engine_zero_zero_one_normalisation() {
    let mut buf = one_point(0.5);
    let mut data = or_particle::behaviour::AccelerationBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(100.0, 0.0, 0.0);
    data.xyz_max = or_particle::domain::Vec3::new(100.0, 0.0, 0.0);
    let a = Acceleration::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    a.update_point(0, &mut buf, &params);
    assert!(
        buf.curr_vel[0].x.abs() < 1e-5,
        "expected ~0 m/s (engine: empty accn_kfp zeroes the contribution), got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn ptx_random_table_cursor_wraps_at_256() {
    let table = PtxRandomTable::default();
    table.set_cursor(254);
    assert_eq!(table.next_index(), 254);
    assert_eq!(table.next_index(), 255);
    assert_eq!(table.next_index(), 0);
    assert_eq!(table.next_index(), 1);
}

use or_io::gta5::particle::ReferenceSpace;
use or_particle::behaviour::{
    AccelerationBehaviour, AnimateTextureBehaviour, DampeningBehaviour, WindBehaviour,
    ZCullingBehaviour,
};
use or_simulation::behaviours::{AnimateTexture, ConstantWaterField, Dampening, Wind, ZCulling};
fn zcull_data(mode: u32, refspace: ReferenceSpace) -> ZCullingBehaviour {
    let mut d = ZCullingBehaviour::default();
    d.mode = mode;
    d.reference_space = refspace;
    d
}

#[test]
fn zcull_cull_under_kills_below_world_plane() {
    let zc = ZCulling::from_data(zcull_data(0, ReferenceSpace::WORLD));
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, -1.0);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    zc.update_point(0, &mut buf, &params);
    assert!(
        buf.is_dead[0],
        "particle below plane should die in CULL_UNDER"
    );
}

#[test]
fn zcull_cull_over_kills_above_world_plane() {
    let zc = ZCulling::from_data(zcull_data(1, ReferenceSpace::WORLD));
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, 1.0);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    zc.update_point(0, &mut buf, &params);
    assert!(
        buf.is_dead[0],
        "particle above plane should die in CULL_OVER"
    );
}

#[test]
fn zcull_clamp_under_snaps_z_to_plane() {
    let zc = ZCulling::from_data(zcull_data(2, ReferenceSpace::WORLD));
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, -3.0);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    zc.update_point(0, &mut buf, &params);
    assert!(!buf.is_dead[0], "CLAMP_UNDER must NOT kill the particle");
    assert!(
        (buf.curr_pos[0].z - 0.0).abs() < 1e-5,
        "z should snap to cull plane, got {}",
        buf.curr_pos[0].z
    );
}

#[test]
fn zcull_water_z_camera_uses_pre_sampled_height() {
    let zc = ZCulling::from_data(zcull_data(0, ReferenceSpace::WATER_Z_CAMERA));
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, 4.5);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    params.water_z_camera = 5.0;
    zc.update_point(0, &mut buf, &params);
    assert!(buf.is_dead[0], "particle below water_z_camera must die");
}

#[test]
fn animate_texture_frame_blending_preserves_ratio() {
    let mut buf = one_point(0.0);
    buf.tex_frame_ratio[0] = 0.4;
    let mut data = AnimateTextureBehaviour::default();
    data.anim_rate_min = 0.0;
    data.anim_rate_max = 0.0;
    data.do_frame_blending = true;
    let at = AnimateTexture::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.0);
    at.update_point(0, &mut buf, &params);
    assert!(
        (buf.tex_frame_ratio[0] - 0.4).abs() < 1e-5,
        "with frame blending on, ratio must persist for the GPU `tex_blend` lerp"
    );
}

#[test]
fn animate_texture_no_frame_blending_zeros_ratio() {
    let mut buf = one_point(0.0);
    buf.tex_frame_ratio[0] = 0.4;
    let mut data = AnimateTextureBehaviour::default();
    data.anim_rate_min = 0.0;
    data.anim_rate_max = 0.0;
    data.do_frame_blending = false;
    let at = AnimateTexture::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.0);
    at.update_point(0, &mut buf, &params);
    assert!(
        buf.tex_frame_ratio[0].abs() < 1e-6,
        "with frame blending off, ratio must clear so GPU `mix(a,b,0)` shows frame_a only"
    );
}

#[test]
fn wind_outside_low_lod_is_no_op() {
    let mut buf = one_point(0.5);
    buf.curr_pos[0] = Vec3::new(100.0, 0.0, 0.0);
    let initial_vel = Vec3::new(2.0, 0.0, 0.0);
    buf.curr_vel[0] = initial_vel;
    let mut data = WindBehaviour::default();
    data.high_lod_range = 5.0;
    data.low_lod_range = 10.0;
    let w = Wind::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    params.wind_velocity = Vec3::new(50.0, 0.0, 0.0);
    params.camera_pos = Vec3::ZERO;
    w.update_point(0, &mut buf, &params);
    assert_eq!(
        buf.curr_vel[0], initial_vel,
        "particle outside low_lod_range must not be touched by wind"
    );
}

#[test]
fn acceleration_zoom_flag_scales_force() {
    let mut buf = one_point(0.5);
    let mut data = AccelerationBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(1.0, 0.0, 0.0);
    data.xyz_max = or_particle::domain::Vec3::new(1.0, 0.0, 0.0);
    data.is_affected_by_zoom = true;
    let a = Acceleration::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.accn_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0);
    params.effect_zoom = 4.0;
    params.emitter_zoom = 2.0;
    a.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0].x - 8.0).abs() < 1e-3,
        "expected 8 m/s, got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn acceleration_gravity_flag_inverts_z() {
    let mut buf = one_point(0.5);
    let mut data = AccelerationBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(0.0, 0.0, 1.0);
    data.xyz_max = or_particle::domain::Vec3::new(0.0, 0.0, 1.0);
    data.enable_gravity = true;
    let a = Acceleration::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.accn_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0);
    params.gravity = 9.81;
    a.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0].z + 9.81).abs() < 1e-3,
        "expected -9.81 m/s, got {}",
        buf.curr_vel[0].z
    );
}

#[test]
fn dampening_air_resistance_flag_scales_factor() {
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::new(5.0, 0.0, 0.0);
    let mut data = DampeningBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(1.0, 0.0, 0.0);
    data.xyz_max = or_particle::domain::Vec3::new(1.0, 0.0, 0.0);
    data.enable_air_resistance = true;
    let d = Dampening::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.dampening_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    params.air_resistance = 2.0;
    d.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0].x - 4.0).abs() < 1e-3,
        "expected 4 m/s with air-resistance × 2, got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn zcull_water_z_point_uses_field_sampler() {
    let zc = ZCulling::from_data(zcull_data(0, ReferenceSpace::WATER_Z_POINT));
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, 9.5);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    let field = ConstantWaterField { surface_z: 10.0 };
    params.water_field = Some(&field);
    zc.update_point(0, &mut buf, &params);
    assert!(buf.is_dead[0], "particle below sampled water Z must die");
}

use or_particle::behaviour::{
    DampeningBehaviour as DampB, MatrixWeightBehaviour, NoiseBehaviour, RiverBehaviour,
};
use or_simulation::behaviours::{
    ConstantRiverField, MatrixWeight, Noise as NoiseSim, River as RiverSim,
};

#[test]
fn dampening_effect_space_scales_matrix_columns_not_vector() {
    use glam::EulerRot;
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::new(5.0, 0.0, 0.0);
    let mut data = DampB::default();
    data.xyz_min = or_particle::domain::Vec3::new(0.0, 1.0, 0.0);
    data.xyz_max = or_particle::domain::Vec3::new(0.0, 1.0, 0.0);
    data.reference_space = ReferenceSpace::EFFECT;
    data.enable_air_resistance = false;
    let d = Dampening::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.dampening_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    params.effect_matrix = Mat4::from_euler(EulerRot::ZYX, std::f32::consts::FRAC_PI_2, 0.0, 0.0);
    d.update_point(0, &mut buf, &params);
    assert!(
        buf.curr_vel[0].x.abs() < 5.0,
        "EFFECT-space damping must shrink world X, got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn wind_delta_mult_clamps_at_one_for_full_match() {
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::ZERO;
    let mut data = WindBehaviour::default();
    data.influence_keyframes = Some(or_particle::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0, 0.0, 0.0, 0.0],
            value: [30.0, 30.0, 0.0, 0.0],
        }],
        ..Default::default()
    });
    data.ignore_mtx_weight = true;
    data.high_lod_range = 1e6;
    data.low_lod_range = 1e6;
    let w = Wind::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 30.0);
    params.wind_velocity = Vec3::new(7.0, 0.0, 0.0);
    w.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0].x - 7.0).abs() < 1e-3,
        "engine: influence*dt = 30/30 = 1 ⇒ snap to air vel; got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn wind_delta_mult_partial_for_low_influence() {
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::ZERO;
    let mut data = WindBehaviour::default();
    data.influence_keyframes = Some(or_particle::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0, 0.0, 0.0, 0.0],
            value: [10.0, 10.0, 0.0, 0.0],
        }],
        ..Default::default()
    });
    data.ignore_mtx_weight = true;
    data.high_lod_range = 1e6;
    data.low_lod_range = 1e6;
    let w = Wind::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 30.0);
    params.wind_velocity = Vec3::new(9.0, 0.0, 0.0);
    w.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0].x - 3.0).abs() < 1e-3,
        "engine: influence=10 dt=1/30 ⇒ delta_mult = 1/3; expected 3 m/s, got {}",
        buf.curr_vel[0].x
    );
}

#[test]
fn river_delta_mult_clamps_at_one() {
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::ZERO;
    let data = RiverBehaviour {
        velocity_mult: 2.0,
        influence: 1000.0,
    };
    let r = RiverSim::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    let field = ConstantRiverField {
        velocity: Vec3::new(1.0, 0.0, 0.0),
    };
    params.river_field = Some(&field);
    r.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_vel[0] - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3,
        "engine: high influence saturates to 1 ⇒ snap to mult*river; got {:?}",
        buf.curr_vel[0]
    );
}

#[test]
fn matrix_weight_uses_effect_velocity_for_pinning() {
    let mut buf = one_point(0.5);
    buf.curr_pos[0] = Vec3::ZERO;
    buf.prev_pos[0] = Vec3::ZERO;
    buf.spawn_pos[0] = Vec3::ZERO;
    buf.matrix_weight[0] = 0.0;
    let mut data = MatrixWeightBehaviour::default();
    data.weight_keyframes = Some(or_particle::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0, 0.0, 0.0, 0.0],
            value: [1.0, 1.0, 0.0, 0.0],
        }],
        ..Default::default()
    });
    data.reference_space = ReferenceSpace::EFFECT;
    let mw = MatrixWeight::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.matrix_weight_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    params.effect_velocity = Vec3::new(10.0, 0.0, 0.0);
    mw.update_point(0, &mut buf, &params);
    assert!(
        (buf.curr_pos[0].x - 1.0).abs() < 0.05,
        "pinned particle must follow effect velocity; got curr_pos.x = {}",
        buf.curr_pos[0].x
    );
}

#[test]
fn update_culled_effect_freezes_clock() {
    use or_simulation::EffectInst;
    let mut rule = PtxEffectRule::<Gen8>::default();
    rule.duration_min = 1.0;
    rule.duration_max = 1.0;
    rule.distance_culling_cull_dist = 1.0;
    rule.update_when_distance_culled = or_io::decl::Bool8::new(false);
    rule.emit_when_distance_culled = or_io::decl::Bool8::new(false);
    let mut eff = EffectInst::new(rule, vec![]);
    let random = RandomTable::default();
    let ptx = PtxRandomTable::default();
    eff.set_camera_pos(Vec3::new(100.0, 0.0, 0.0));
    eff.update_with_world(0.5, &random, &ptx, 0.0, Vec3::ZERO, None, None);
    assert!(eff.is_update_culled, "should be update-culled");
    assert!(
        eff.time_seconds < 1e-5,
        "engine: culled effect freezes; expected time_seconds = 0, got {}",
        eff.time_seconds
    );
}

#[test]
fn acceleration_empty_emitter_kfp_zeroes_force() {
    let mut buf = one_point(0.5);
    let mut data = AccelerationBehaviour::default();
    data.xyz_min = or_particle::domain::Vec3::new(50.0, 0.0, 0.0);
    data.xyz_max = or_particle::domain::Vec3::new(50.0, 0.0, 0.0);
    let a = Acceleration::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    a.update_point(0, &mut buf, &params);
    assert!(buf.curr_vel[0].x.abs() < 1e-5);
}

#[test]
fn size_sprite_tblr_layout_matches_engine_swizzle() {
    use or_io::gta5::particle::KeyframeMode;
    use or_particle::behaviour::SizeBehaviour;
    use or_simulation::behaviours::Size as SizeSim;
    let mut buf = one_point(0.5);
    buf.curr_vel[0] = Vec3::ZERO;
    let mut data = SizeBehaviour::default();
    data.is_proportional = true;
    data.keyframe_mode = KeyframeMode::Point;
    data.whd_min_keyframes = Some(or_particle::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0, 0.0, 0.0, 0.0],
            value: [4.0, 8.0, 0.0, 0.0],
        }],
        ..Default::default()
    });
    data.whd_max_keyframes = data.whd_min_keyframes.clone();
    let s = SizeSim::from_data(data);
    let mut rule = PtxParticleRule::default();
    rule.draw_type = 0;
    let effect = PtxEffectRule::default();
    let mut emitter = PtxEmitterRule::default();
    emitter.size_scalar_kfp = or_particle::keyframe::constant([100.0; 4]);
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.016);
    s.update_point(0, &mut buf, &params);
    let dim = buf.dimensions[0];
    assert!(
        (dim.x - 4.0).abs() < 0.5
            && (dim.y - 4.0).abs() < 0.5
            && (dim.z - 2.0).abs() < 0.5
            && (dim.w - 2.0).abs() < 0.5,
        "expected sprite TBLR ≈ (4, 4, 2, 2), got {:?}",
        dim
    );
}

#[test]
fn effect_inst_stop_pins_clock() {
    use or_simulation::EffectInst;
    let mut rule = PtxEffectRule::<Gen8>::default();
    rule.duration_min = 1.0;
    rule.duration_max = 1.0;
    let mut eff = EffectInst::new(rule, vec![]);
    let random = RandomTable::default();
    let ptx = PtxRandomTable::default();
    eff.update_with_world(0.3, &random, &ptx, 0.0, Vec3::ZERO, None, None);
    assert!(eff.time_seconds < 0.5);
    eff.stop();
    assert!((eff.time_seconds - eff.duration).abs() < 1e-5);
}

#[test]
fn effect_inst_finish_kill_drops_particles() {
    use or_simulation::EffectInst;
    let mut rule = PtxEffectRule::<Gen8>::default();
    rule.duration_min = 1.0;
    rule.duration_max = 1.0;
    let eff = EffectInst::new(rule, vec![]);
    let mut eff = eff;
    assert!(!eff.is_finished);
    eff.finish(true);
    assert!(eff.is_finished);
}

#[test]
fn effect_inst_relocate_shifts_live_particles() {
    use or_simulation::EffectInst;
    let mut rule = PtxEffectRule::<Gen8>::default();
    rule.duration_min = 1.0;
    rule.duration_max = 1.0;
    let mut eff = EffectInst::new(rule, vec![]);
    eff.relocate(Mat4::from_translation(Vec3::new(7.0, 0.0, 0.0)));
    assert!((eff.matrix.w_axis.truncate() - Vec3::new(7.0, 0.0, 0.0)).length() < 1e-5);
    assert!((eff.current_pos - Vec3::new(7.0, 0.0, 0.0)).length() < 1e-5);
}

#[test]
fn effect_inst_with_random_table_pulls_three_indices() {
    use or_simulation::EffectInst;
    let mut rule = PtxEffectRule::<Gen8>::default();
    rule.duration_min = 1.0;
    rule.duration_max = 1.0;
    let table = PtxRandomTable::default();
    table.set_cursor(100);
    let eff = EffectInst::with_random_table(rule, vec![], &table);
    assert_eq!(eff.rgba_rand_index, 100);
    assert_eq!(eff.zoom_scalar_rand_index, 101);
    assert_eq!(eff.trail_tex_info_rand_index, 102);
    assert_eq!(table.next_index(), 103);
}

#[test]
fn query_kpd_point_uses_per_particle_snapshot_not_live_slider() {
    use or_io::gta5::particle::RawKeyframeEntry;
    use or_particle::keyframe::KeyframePropData;
    use or_particle::rules::{EvolutionList, EvolvedKeyframe, EvolvedKeyframeProp};
    let mut buf = PointBuffer::default();
    buf.spawn(0);
    buf.set_evo_snapshot(0, &[0.25_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

    let kpd = Some(KeyframePropData {
        property_id: 0xDEAD_BEEF,
        ..Default::default()
    });
    let evo_list = EvolutionList {
        evolutions: vec![or_particle::rules::Evolution {
            name: "TestSlider".into(),
            name_hash: 0,
            override_value: 0.0,
            is_overridden: false,
        }],
        evolved_keyframe_props: vec![EvolvedKeyframeProp {
            property_id: 0xDEAD_BEEF,
            blend_mode: 0,
            evolved_keyframes: vec![EvolvedKeyframe {
                evolution_index: 0,
                is_lod_evolution: false,
                entries: vec![RawKeyframeEntry {
                    time: [0.0; 4],
                    value: [1.0, 0.0, 0.0, 0.0],
                }],
            }],
        }],
    };

    let rand = RandomTable::default();
    let ptx_table = PtxRandomTable::default();
    let rule = PtxParticleRule::default();
    let eff = PtxEffectRule::default();
    let emit = PtxEmitterRule::default();
    let live_evo: Vec<f32> = vec![1.0; 8];
    let mut params = make_params(&rule, &eff, &emit, &rand, &ptx_table, 0.016);
    params.evo_list = Some(&evo_list);
    params.evo_values = &live_evo;

    let v = params.query_kpd_point(&buf, 0, &kpd, 0.0, Vec4::ZERO);
    assert!(
        (v.x - 0.25).abs() < 1e-5,
        "expected snapshot=0.25 to drive query, got {}",
        v.x
    );

    let v_live = params.query_kpd(&kpd, 0.0, Vec4::ZERO);
    assert!(
        (v_live.x - 1.0).abs() < 1e-5,
        "live-slider path must still see slider=1.0, got {}",
        v_live.x
    );
}

#[test]
fn per_particle_evo_snapshot_captured_at_spawn() {
    use or_simulation::point::PointBuffer;
    let mut buf = PointBuffer::default();
    buf.spawn(0);
    buf.spawn(1);
    let evo = vec![0.25_f32, 0.5, 0.75, 1.0];
    buf.set_evo_snapshot(0, &evo);
    let snap = buf.evo_snapshot(0);
    assert!(snap.len() >= 4);
    assert!((snap[0] - 0.25).abs() < 1e-5);
    assert!((snap[1] - 0.5).abs() < 1e-5);
    assert!((snap[2] - 0.75).abs() < 1e-5);
    assert!((snap[3] - 1.0).abs() < 1e-5);
    let snap1 = buf.evo_snapshot(1);
    assert!(snap1[0].abs() < 1e-5);
}

#[test]
fn colour_folds_lod_alpha_mult_and_resets_alpha_mult() {
    use or_io::gta5::particle::KeyframeMode;
    use or_particle::behaviour::ColourBehaviour;
    use or_simulation::behaviours::Colour as ColourSim;
    let mut buf = one_point(0.5);
    buf.alpha_mult[0] = 0.5;
    let mut data = ColourBehaviour::default();
    data.rgba_min.r = 1.0;
    data.rgba_min.g = 1.0;
    data.rgba_min.b = 1.0;
    data.rgba_min.a = 1.0;
    data.rgba_max_enable = false;
    data.keyframe_mode = KeyframeMode::Point;
    let c = ColourSim::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    params.lod_alpha_mult = 0.5;
    c.update_point(0, &mut buf, &params);
    assert!(
        (buf.color[0].w - 0.25).abs() < 1e-3,
        "expected alpha 0.25 (1*0.5*0.5), got {}",
        buf.color[0].w
    );
    assert!((buf.alpha_mult[0] - 1.0).abs() < 1e-5);
}

#[test]
fn animate_texture_loop_mode_zero_resets_init_to_zero() {
    use or_io::gta5::particle::{AnimTexLoopMode, KeyframeMode};
    use or_particle::behaviour::AnimateTextureBehaviour;
    use or_simulation::behaviours::AnimateTexture;
    let mut data = AnimateTextureBehaviour::default();
    data.anim_rate_min = 60.0;
    data.anim_rate_max = 60.0;
    data.loop_mode = AnimTexLoopMode::ZERO;
    data.keyframe_mode = KeyframeMode::Point;
    let at = AnimateTexture::from_data(data).with_frame_range(2, 5);
    let mut buf = one_point(0.0);
    buf.tex_frame_a[0] = 5;
    buf.tex_frame_b[0] = 5;
    buf.tex_frame_ratio[0] = 0.99;
    buf.init_tex_frame[0] = 2;
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.5);
    at.update_point(0, &mut buf, &params);
    assert_eq!(buf.init_tex_frame[0], 0);
}

/// The position fix must happen before the kill-chance roll, otherwise dead
/// particles stay embedded in the surface.
#[test]
fn collision_kill_chance_runs_after_position_fix() {
    use or_particle::behaviour::CollisionBehaviour;
    use or_simulation::behaviours::Collision as CollisionSim;
    let mut data = CollisionBehaviour::default();
    data.collision_chance = 100;
    data.kill_chance = 100;
    data.rest_speed = 0.0;
    let c = CollisionSim::from_data(data);
    let mut buf = one_point(0.0);
    buf.curr_pos[0] = Vec3::new(0.0, 0.0, -1.0);
    buf.prev_pos[0] = Vec3::new(0.0, 0.0, 1.0);
    buf.curr_vel[0] = Vec3::new(0.0, 0.0, -5.0);
    buf.flags[0].insert(or_simulation::point::PointFlags::COLLIDES);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 1.0 / 60.0);
    c.update_point(0, &mut buf, &params);
    assert!(
        buf.flags[0].contains(or_simulation::point::PointFlags::HAS_COLLIDED),
        "HAS_COLLIDED must be set even when killing"
    );
    assert!(buf.is_dead[0], "kill_chance=100 must kill");
    assert!(
        buf.curr_pos[0].z >= -1e-4,
        "kill must happen AFTER position fix; got z = {}",
        buf.curr_pos[0].z
    );
}

#[test]
fn noise_effect_space_uses_column_scale_not_vector_rotate() {
    use glam::EulerRot;
    let mut buf = one_point(0.5);
    buf.curr_pos[0] = Vec3::ZERO;
    let mut data = NoiseBehaviour::default();
    data.pos_noise_min_keyframes = Some(or_particle::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0, 0.0, 0.0, 0.0],
            value: [0.0, 100.0, 0.0, 0.0],
        }],
        ..Default::default()
    });
    data.pos_noise_max_keyframes = data.pos_noise_min_keyframes.clone();
    data.reference_space = ReferenceSpace::EFFECT;
    let n = NoiseSim::from_data(data);
    let rule = PtxParticleRule::default();
    let effect = PtxEffectRule::default();
    let emitter = PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx_table = PtxRandomTable::default();
    let mut params = make_params(&rule, &effect, &emitter, &rand, &ptx_table, 0.1);
    params.effect_matrix = Mat4::from_euler(EulerRot::ZYX, std::f32::consts::FRAC_PI_2, 0.0, 0.0);
    n.update_point(0, &mut buf, &params);
    assert!(
        buf.curr_pos[0].x.abs() > 1e-3,
        "EFFECT-space noise authored on Y must displace world-X via the column-scale form; got {:?}",
        buf.curr_pos[0]
    );
}

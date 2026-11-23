mod synthetic;

use glam::{Mat3, Mat4, Vec3, Vec4};
use or_particle::behaviour::{DecalBehaviour, FogVolumeBehaviour, LightBehaviour, LiquidBehaviour};
use or_simulation::{
    behaviour::{AttractorGeometry, BehaviourImpl, BehaviourParams},
    behaviours::{DrawDecal, DrawFogVolume, DrawLight, Liquid},
    point::{PointBuffer, PointFlags},
    random::RandomTable,
    random_table::PtxRandomTable,
    sinks::{
        AudioEvent, AudioSink, DecalInfo, DecalPoolInfo, DecalPoolSink, DecalSink, DecalTrailInfo,
        FireInfo, FireSink, FogVolumeInfo, FogVolumeSink, LightInfo, LightSink, LiquidInfo,
        LiquidSink, PtfxSinks,
    },
};
use std::sync::Mutex;

#[derive(Debug, Default)]
struct CountingSink {
    fire: Mutex<Vec<FireInfo>>,
    decal: Mutex<Vec<DecalInfo>>,
    decal_pool: Mutex<Vec<DecalPoolInfo>>,
    liquid: Mutex<Vec<LiquidInfo>>,
    fog: Mutex<Vec<FogVolumeInfo>>,
    light: Mutex<Vec<LightInfo>>,
    audio: Mutex<Vec<String>>,
}
impl FireSink for CountingSink {
    fn add_fire(&self, info: FireInfo) {
        self.fire.lock().unwrap().push(info);
    }
}
impl DecalSink for CountingSink {
    fn add_decal(&self, info: DecalInfo) {
        self.decal.lock().unwrap().push(info);
    }
    fn add_decal_trail(&self, _info: DecalTrailInfo) {}
}
impl DecalPoolSink for CountingSink {
    fn add_pool_decal(&self, info: DecalPoolInfo) {
        self.decal_pool.lock().unwrap().push(info);
    }
}
impl LiquidSink for CountingSink {
    fn add_liquid(&self, info: LiquidInfo) {
        self.liquid.lock().unwrap().push(info);
    }
}
impl FogVolumeSink for CountingSink {
    fn add_fog_volume(&self, info: FogVolumeInfo) {
        self.fog.lock().unwrap().push(info);
    }
}
impl LightSink for CountingSink {
    fn add_light(&self, info: LightInfo) {
        self.light.lock().unwrap().push(info);
    }
}
impl AudioSink for CountingSink {
    fn on_event(&self, event: AudioEvent<'_>) {
        let s = match event {
            AudioEvent::Start { name, .. } => format!("start:{name}"),
            AudioEvent::Stop { name, .. } => format!("stop:{name}"),
            AudioEvent::Recycle { name, .. } => format!("recycle:{name}"),
        };
        self.audio.lock().unwrap().push(s);
    }
}

fn one_alive_point() -> PointBuffer {
    let mut buf = PointBuffer::default();
    let _ = buf.spawn(0);
    buf.curr_pos[0] = Vec3::new(1.0, 2.0, 3.0);
    buf.curr_vel[0] = Vec3::new(0.5, 0.0, 0.0);
    buf.life_ratio[0] = 0.3;
    buf.one_over_life[0] = 0.5;
    buf.color[0] = Vec4::ONE;
    buf.dimensions[0] = Vec4::ONE;
    buf.alpha_mult[0] = 1.0;
    buf.flags[0].remove(PointFlags::IS_BEING_INITIALISED);
    buf
}

fn make_params<'a>(
    rule: &'a or_io::gta5::particle::PtxParticleRule<or_io::decl::Gen8>,
    eff: &'a or_io::gta5::particle::PtxEffectRule<or_io::decl::Gen8>,
    em: &'a or_io::gta5::particle::PtxEmitterRule,
    rand: &'a RandomTable,
    ptx: &'a PtxRandomTable,
    evo: &'a [f32],
    sinks: PtfxSinks<'a>,
) -> BehaviourParams<'a> {
    BehaviourParams {
        dt: 1.0 / 60.0,
        effect_time_ratio: 0.0,
        emitter_time_ratio: 0.0,
        effect_zoom: 1.0,
        emitter_zoom: 1.0,
        effect_matrix: Mat4::IDENTITY,
        emitter_matrix: Mat4::IDENTITY,
        effect_velocity: Vec3::ZERO,
        emitter_velocity: Vec3::ZERO,
        gravity: 0.0,
        wind_velocity: Vec3::ZERO,
        random: rand,
        ptx_random_table: ptx,
        evo_list: None,
        evo_values: evo,
        particle_rule: rule,
        shader_vars: &[],
        effect_rule: eff,
        emitter_rule: em,
        camera_pos: Vec3::ZERO,
        camera_basis: Mat3::IDENTITY,
        emitter_colour_tint: Vec4::ONE,
        emitter_attractor_pos: Vec3::ZERO,
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
        sinks,
        effect_spawner_trigger_ratio: -1.0,
    }
}

#[test]
fn draw_decal_pushes_decal_info_to_sink() {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: Some(&sink),
        decal_pool: None,
        liquid: None,
        fog_volume: None,
        light: None,
        audio: None,
    };
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    let beh = DrawDecal::from_data(DecalBehaviour::default());
    beh.update_point(0, &mut buf, &params);
    assert_eq!(sink.decal.lock().unwrap().len(), 1);
}

#[test]
fn draw_light_pushes_light_info_to_sink() {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: None,
        decal_pool: None,
        liquid: None,
        fog_volume: None,
        light: Some(&sink),
        audio: None,
    };
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    let beh = DrawLight::from_data(LightBehaviour::default());
    beh.update_point(0, &mut buf, &params);
    assert_eq!(sink.light.lock().unwrap().len(), 1);
}

#[test]
fn draw_fog_volume_pushes_fog_info_to_sink() {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: None,
        decal_pool: None,
        liquid: None,
        fog_volume: Some(&sink),
        light: None,
        audio: None,
    };
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    let beh = DrawFogVolume::from_data(FogVolumeBehaviour::default());
    beh.update_point(0, &mut buf, &params);
    assert_eq!(sink.fog.lock().unwrap().len(), 1);
}

#[test]
fn liquid_pushes_liquid_info_to_sink_after_collision() {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    buf.flags[0].insert(PointFlags::HAS_COLLIDED);
    buf.coln_pos[0] = Vec3::new(2.0, 3.0, 4.0);
    buf.coln_normal[0] = Vec3::Z;
    buf.curr_vel[0] = Vec3::new(5.0, 0.0, 0.0);
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: None,
        decal_pool: None,
        liquid: Some(&sink),
        fog_volume: None,
        light: None,
        audio: None,
    };
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    let beh = Liquid::from_data(LiquidBehaviour::default());
    beh.update_point(0, &mut buf, &params);
    assert!(
        !sink.liquid.lock().unwrap().is_empty(),
        "liquid sink should receive a record after collision"
    );
}

#[test]
fn no_sink_is_silent_no_op() {
    let mut buf = one_alive_point();
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks::none();
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    DrawDecal::from_data(DecalBehaviour::default()).update_point(0, &mut buf, &params);
    DrawLight::from_data(LightBehaviour::default()).update_point(0, &mut buf, &params);
    DrawFogVolume::from_data(FogVolumeBehaviour::default()).update_point(0, &mut buf, &params);
}

#[derive(Debug, Default)]
struct StepSink(std::sync::atomic::AtomicUsize);

impl LightSink for StepSink {
    fn begin(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn add_light(&self, _info: LightInfo) {}
}

#[test]
fn a_light_sink_hears_of_each_update_that_runs_and_of_none_that_does_not() {
    use or_simulation::{build, PreviewTarget, PtfxManager};
    let asset = synthetic::list_of(1, 1.0, 60.0, 0.5, &[]);
    let inst = build(&asset, PreviewTarget::Effect(0)).expect("the synthetic effect builds");
    let sink = std::sync::Arc::new(StepSink::default());
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.sink_light = Some(sink.clone());
    manager.clock.play();
    manager.tick(1.0 / 60.0);
    manager.tick(1.0 / 60.0);
    let ran = sink.0.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(ran, 2, "one hearing for each update");
    manager.clock.pause();
    manager.tick(1.0 / 60.0);
    assert_eq!(
        sink.0.load(std::sync::atomic::Ordering::Relaxed),
        ran,
        "a paused clock updates nothing, so the lights of the last update stay"
    );
}

/// A curve that holds `value` for the whole of a life.
fn held(value: [f32; 4]) -> Option<or_particle::keyframe::KeyframePropData> {
    Some(or_particle::keyframe::KeyframePropData {
        entries: vec![or_io::gta5::particle::RawKeyframeEntry {
            time: [0.0; 4],
            value,
        }],
        property_id: 0,
        rand_index: 0,
        invert_bias_link: 0,
    })
}

fn fog_of(data: FogVolumeBehaviour) -> Vec<FogVolumeInfo> {
    fog_of_particle(data, Vec4::ONE)
}

/// [`fog_of`] for a particle of `colour`.
fn fog_of_particle(data: FogVolumeBehaviour, colour: Vec4) -> Vec<FogVolumeInfo> {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    buf.color[0] = colour;
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: None,
        decal_pool: None,
        liquid: None,
        fog_volume: Some(&sink),
        light: None,
        audio: None,
    };
    let params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    DrawFogVolume::from_data(data).update_point(0, &mut buf, &params);
    let made = sink.fog.lock().unwrap().clone();
    made
}

#[test]
fn a_fog_volume_carries_the_range_the_rotation_and_the_lighting_its_rule_states() {
    let made = fog_of(FogVolumeBehaviour {
        density_range_keyframes: held([0.8, 0.8, 12.0, 12.0]),
        scale_min_keyframes: held([2.0, 3.0, 4.0, 0.0]),
        scale_max_keyframes: held([2.0, 3.0, 4.0, 0.0]),
        rotation_min_keyframes: held([90.0, 0.0, 0.0, 0.0]),
        rotation_max_keyframes: held([90.0, 0.0, 0.0, 0.0]),
        rgb_tint_min_keyframes: held([0.25, 0.5, 0.75, 1.0]),
        rgb_tint_max_keyframes: held([0.25, 0.5, 0.75, 1.0]),
        falloff: 2.5,
        hdr_mult: 1.5,
        lighting_type: 1,
        use_ground_fog_colour: true,
        ..FogVolumeBehaviour::default()
    });
    let [fog] = made[..] else {
        panic!("one volume expected: {made:?}");
    };
    assert_eq!(fog.range, 12.0);
    assert_eq!(fog.density, 0.8);
    assert_eq!(fog.scale, Vec3::new(2.0, 3.0, 4.0));
    assert!((fog.rotation.x - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert_eq!(fog.colour, Vec4::new(0.25, 0.5, 0.75, 1.0));
    assert_eq!((fog.falloff, fog.hdr_mult), (2.5, 1.5));
    assert_eq!(fog.lighting, or_simulation::sinks::FogLighting::Directional);
    assert!(fog.use_ground_fog_colour);
}

#[test]
fn a_fog_volume_with_no_range_or_no_density_is_not_one() {
    for density_range in [[0.8, 0.8, 0.0, 0.0], [0.0, 0.0, 12.0, 12.0]] {
        let made = fog_of(FogVolumeBehaviour {
            density_range_keyframes: held(density_range),
            ..FogVolumeBehaviour::default()
        });
        assert!(made.is_empty(), "{density_range:?}: {made:?}");
    }
}

#[test]
fn a_fog_volume_is_tinted_by_the_particle_where_the_rule_asks_and_by_the_rule_alone_otherwise() {
    let rule = |from_particle| FogVolumeBehaviour {
        rgb_tint_min_keyframes: held([1.0, 1.0, 0.5, 1.0]),
        rgb_tint_max_keyframes: held([1.0, 1.0, 0.5, 1.0]),
        colour_tint_from_particle: from_particle,
        ..FogVolumeBehaviour::default()
    };
    let particle = Vec4::new(0.5, 1.0, 1.0, 1.0);
    let [tinted] = fog_of_particle(rule(true), particle)[..] else {
        panic!("one volume expected");
    };
    assert_eq!(tinted.colour, Vec4::new(0.5, 1.0, 0.5, 1.0));
    let [plain] = fog_of_particle(rule(false), particle)[..] else {
        panic!("one volume expected");
    };
    assert_eq!(plain.colour, Vec4::new(1.0, 1.0, 0.5, 1.0));
}

fn light_of(data: LightBehaviour, effect: Mat4, emitter: Mat4) -> Vec<LightInfo> {
    let sink = CountingSink::default();
    let mut buf = one_alive_point();
    let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
    let ef = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
    let em = or_io::gta5::particle::PtxEmitterRule::default();
    let rand = RandomTable::new();
    let ptx = PtxRandomTable::default();
    let evo: Vec<f32> = vec![];
    let bundle = PtfxSinks {
        fire: None,
        decal: None,
        decal_pool: None,
        liquid: None,
        fog_volume: None,
        light: Some(&sink),
        audio: None,
    };
    let mut params = make_params(&pr, &ef, &em, &rand, &ptx, &evo, bundle);
    params.effect_matrix = effect;
    params.emitter_matrix = emitter;
    DrawLight::from_data(data).update_point(0, &mut buf, &params);
    let made = sink.light.lock().unwrap().clone();
    made
}

fn lamp(light_type: u32) -> LightBehaviour {
    LightBehaviour {
        intensity_keyframes: held([2.0, 2.0, 4.0, 4.0]),
        range_keyframes: held([6.0, 6.0, 35.0, 35.0]),
        light_type,
        ..LightBehaviour::default()
    }
}

#[test]
fn a_light_carries_the_falloff_its_intensity_curve_states_and_an_omni_light_no_cone() {
    let [light] = light_of(lamp(0), Mat4::IDENTITY, Mat4::IDENTITY)[..] else {
        panic!("one light expected");
    };
    assert_eq!(
        (light.intensity, light.range, light.falloff),
        (2.0, 6.0, 4.0)
    );
    assert_eq!((light.direction, light.cone_half_angle), (Vec3::ZERO, 0.0));
}

#[test]
fn a_spot_light_points_along_the_axis_its_type_names_and_opens_by_the_angle_its_range_curve_states()
{
    let turned = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let emitter = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
    // Types one to three follow the effect's axes, four to six the emitter's
    for (light_type, from, want) in [
        (1, turned, Vec3::Y),
        (2, turned, -Vec3::X),
        (3, turned, Vec3::Z),
        (4, emitter, Vec3::X),
        (5, emitter, Vec3::Z),
        (6, emitter, -Vec3::Y),
    ] {
        let (effect, emitter) = if light_type < 4 {
            (from, Mat4::IDENTITY)
        } else {
            (Mat4::IDENTITY, from)
        };
        let [light] = light_of(lamp(light_type), effect, emitter)[..] else {
            panic!("one light expected");
        };
        assert!(
            (light.direction - want).length() < 1e-5,
            "type {light_type}: {:?}",
            light.direction
        );
        assert_eq!(light.cone_half_angle, 35.0, "type {light_type}");
    }
}

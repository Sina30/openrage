use crate::behaviour::{BehaviourImpl, DrawKind};
use or_particle::behaviour::Behaviour;
use std::collections::HashSet;
use std::sync::Mutex;
mod acceleration;
mod age;
mod animate_texture;
mod attractor;
mod collision;
mod colour;
mod dampening;
mod decal_pool;
mod draw_decal;
mod draw_fog_volume;
mod draw_light;
mod draw_model;
mod draw_sprite;
mod draw_trail;
mod fade;
mod gravity;
mod liquid;
mod matrix_weight;
mod noise;
mod pause;
mod river;
mod rotation;
mod size;
mod velocity;
mod water;
mod wind;
mod wind_field;
mod zculling;
pub use acceleration::Acceleration;
pub use age::Age;
pub use animate_texture::AnimateTexture;
pub use attractor::Attractor;
pub use collision::Collision;
pub use colour::Colour;
pub use dampening::Dampening;
pub use decal_pool::{DecalPool, DecalPoolBatch, DecalPoolDecal, DecalPoolEmit};
pub use draw_decal::DrawDecal;
pub use draw_fog_volume::DrawFogVolume;
pub use draw_light::DrawLight;
pub use draw_model::DrawModel;
pub use draw_sprite::DrawSprite;
pub use draw_trail::DrawTrail;
pub use fade::{FadeIn, FadeOut};
pub use gravity::Gravity;
pub use liquid::{Liquid, LiquidBatch, LiquidEmit, LiquidPool};
pub use matrix_weight::MatrixWeight;
pub use noise::Noise;
pub use pause::Pause;
pub use river::{ConstantRiverField, River, RiverField, WaterFlowAsRiverField, WaterFlowSampler};
pub use rotation::{Rotation, RotationSpeed};
pub use size::Size;
pub use velocity::Velocity;
pub use water::{ConstantWaterField, WaterField};
pub use wind::Wind;
pub use wind_field::{ConstantWindField, WindField};
pub use zculling::{ZCullMode, ZCulling};
const BEHAVIOUR_HASH_FIRE: u32 = or_particle::rage::hash::string_hash(b"ptxu_Fire");
const BEHAVIOUR_HASH_DECAL_TRAIL: u32 = or_particle::rage::hash::string_hash(b"ptxu_DecalTrail");
#[derive(Debug, Clone)]
struct FireStub;
impl BehaviourImpl for FireStub {
    fn order(&self) -> f32 {
        crate::behaviour::order::FIRE
    }
    fn name(&self) -> &'static str {
        "ptxu_Fire"
    }
    fn init_point(
        &self,
        idx: usize,
        buf: &mut crate::point::PointBuffer,
        _params: &crate::behaviour::BehaviourParams<'_>,
    ) {
        buf.flags[idx].insert(crate::point::PointFlags::COLLIDES);
    }
    fn update_point(
        &self,
        idx: usize,
        buf: &mut crate::point::PointBuffer,
        params: &crate::behaviour::BehaviourParams<'_>,
    ) {
        if !buf.flags[idx].contains(crate::point::PointFlags::HAS_COLLIDED) {
            return;
        }
        let speed = buf.curr_vel[idx].length();
        if speed < 0.1 {
            return;
        }
        let Some(sink) = params.sinks.fire else {
            return;
        };
        let pos = buf.coln_pos[idx];
        let normal = buf.coln_normal[idx];
        let life = buf.life_ratio[idx].clamp(0.0, 1.0);
        sink.add_fire(crate::sinks::FireInfo {
            pos,
            dir: normal,
            scale: speed.max(0.1),
            colour: glam::Vec4::new(buf.color[idx].x, buf.color[idx].y, buf.color[idx].z, 1.0),
            life_seconds: (1.0 - life) * 5.0,
        });
    }
}

#[derive(Debug, Clone)]
struct DecalTrailStub;
impl BehaviourImpl for DecalTrailStub {
    fn order(&self) -> f32 {
        crate::behaviour::order::DECAL_TRAIL
    }
    fn name(&self) -> &'static str {
        "ptxu_DecalTrail"
    }
    fn update_point(
        &self,
        _idx: usize,
        _buf: &mut crate::point::PointBuffer,
        _params: &crate::behaviour::BehaviourParams<'_>,
    ) {
    }
}

pub fn build_dispatch(
    behaviours: &[Behaviour],
    rule: &or_io::gta5::particle::PtxParticleRule<or_io::decl::Gen8>,
) -> Vec<Box<dyn BehaviourImpl>> {
    let mut out: Vec<Box<dyn BehaviourImpl>> = Vec::with_capacity(behaviours.len());
    for b in behaviours {
        if let Some(boxed) = build_one(b, rule) {
            out.push(boxed);
        }
    }
    out.sort_by(|a, b| {
        a.order()
            .partial_cmp(&b.order())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn build_one(
    b: &Behaviour,
    rule: &or_io::gta5::particle::PtxParticleRule<or_io::decl::Gen8>,
) -> Option<Box<dyn BehaviourImpl>> {
    Some(match b {
        Behaviour::Acceleration(x) => Box::new(Acceleration::from_data(x.clone())),
        Behaviour::Velocity(x) => Box::new(Velocity::from_data(x.clone())),
        Behaviour::Rotation(x) => Box::new(Rotation::from_data(x.clone())),
        Behaviour::RotationSpeed(x) => Box::new(RotationSpeed::from_data(x.clone())),
        Behaviour::Colour(x) => Box::new(Colour::from_data(x.clone())),
        Behaviour::Size(x) => Box::new(Size::from_data(x.clone())),
        Behaviour::AnimateTexture(x) => Box::new(
            AnimateTexture::from_data(x.clone())
                .with_frame_range(rule.tex_frame_id_min as u32, rule.tex_frame_id_max as u32),
        ),
        Behaviour::FadeIn(x) => Box::new(FadeIn::from_data(x.clone())),
        Behaviour::FadeOut(x) => Box::new(FadeOut::from_data(x.clone())),
        Behaviour::Noise(x) => Box::new(Noise::from_data(x.clone())),
        Behaviour::Wind(x) => Box::new(Wind::from_data(x.clone())),
        Behaviour::Dampening(x) => Box::new(Dampening::from_data(x.clone())),
        Behaviour::Gravity(x) => Box::new(Gravity::from_data(x.clone())),
        Behaviour::Collision(x) => Box::new(Collision::from_data(x.clone())),
        Behaviour::Attractor(x) => Box::new(Attractor::from_data(x.clone())),
        Behaviour::MatrixWeight(x) => Box::new(MatrixWeight::from_data(x.clone())),
        Behaviour::Pause(x) => Box::new(Pause::from_data(x.clone())),
        Behaviour::Age(x) => Box::new(Age::from_data(x.clone())),
        Behaviour::ZCulling(x) => Box::new(ZCulling::from_data(x.clone())),
        Behaviour::DrawSprite(x) => Box::new(DrawSprite::from_data(x.clone())),
        Behaviour::DrawTrail(x) => Box::new(DrawTrail::from_data(x.clone())),
        Behaviour::DrawModel(x) => Box::new(DrawModel::from_data(x.clone())),
        Behaviour::Decal(x) => Box::new(DrawDecal::from_data(x.clone())),
        Behaviour::FogVolume(x) => Box::new(DrawFogVolume::from_data(x.clone())),
        Behaviour::Light(x) => Box::new(DrawLight::from_data(x.clone())),
        Behaviour::Liquid(x) => Box::new(Liquid::from_data(x.clone())),
        Behaviour::River(x) => Box::new(River::from_data(x.clone())),
        Behaviour::DecalPool(x) => Box::new(DecalPool::from_data(x.clone())),
        Behaviour::Unknown { type_id, .. } => match *type_id {
            BEHAVIOUR_HASH_FIRE => Box::new(FireStub),
            BEHAVIOUR_HASH_DECAL_TRAIL => Box::new(DecalTrailStub),
            _ => {
                warn_unknown_dropped(*type_id);
                return None;
            }
        },
    })
}

fn warn_unknown_dropped(type_id: u32) {
    static SEEN: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
    let Ok(mut guard) = SEEN.lock() else {
        return;
    };
    let set = guard.get_or_insert_with(HashSet::new);
    if set.insert(type_id) {
        tracing::warn!(
            type_id = format!("0x{type_id:08x}"),
            "or-simulation: dropping unmapped Behaviour::Unknown from dispatch \
             (no editor sim impl); particle preview will skip its update step"
        );
    }
}

pub fn detect_draw_kind(behaviours: &[Behaviour], fallback: DrawKind) -> DrawKind {
    for b in behaviours {
        match b {
            Behaviour::DrawSprite(_) => return DrawKind::Sprite,
            Behaviour::DrawTrail(_) => return DrawKind::Trail,
            Behaviour::DrawModel(_) => return DrawKind::Model,
            Behaviour::Decal(_) => return DrawKind::Decal,
            _ => {}
        }
    }
    fallback
}

#[cfg(test)]
mod fire_stub_tests {
    use super::*;
    use crate::behaviour::BehaviourImpl;
    use crate::point::{PointBuffer, PointFlags};
    use crate::sinks::{FireInfo, FireSink, PtfxSinks};
    #[derive(Debug, Default)]
    struct CaptureFireSink {
        events: std::sync::Mutex<Vec<FireInfo>>,
    }
    impl FireSink for CaptureFireSink {
        fn add_fire(&self, info: FireInfo) {
            self.events.lock().unwrap().push(info);
        }
    }

    fn build_params<'a>(
        random: &'a crate::random::RandomTable,
        ptx_table: &'a crate::random_table::PtxRandomTable,
        evo: &'a [f32],
        pr: &'a or_io::gta5::particle::PtxParticleRule<or_io::decl::Gen8>,
        er: &'a or_io::gta5::particle::PtxEffectRule<or_io::decl::Gen8>,
        em: &'a or_io::gta5::particle::PtxEmitterRule,
        sinks: PtfxSinks<'a>,
    ) -> crate::behaviour::BehaviourParams<'a> {
        crate::behaviour::BehaviourParams {
            dt: 0.0,
            effect_time_ratio: 0.0,
            emitter_time_ratio: 0.0,
            effect_zoom: 1.0,
            emitter_zoom: 1.0,
            effect_matrix: glam::Mat4::IDENTITY,
            emitter_matrix: glam::Mat4::IDENTITY,
            effect_velocity: glam::Vec3::ZERO,
            emitter_velocity: glam::Vec3::ZERO,
            gravity: 0.0,
            wind_velocity: glam::Vec3::ZERO,
            random,
            ptx_random_table: ptx_table,
            evo_list: None,
            evo_values: evo,
            particle_rule: pr,
            shader_vars: &[],
            effect_rule: er,
            emitter_rule: em,
            camera_pos: glam::Vec3::ZERO,
            camera_basis: glam::Mat3::IDENTITY,
            emitter_colour_tint: glam::Vec4::ONE,
            emitter_attractor_pos: glam::Vec3::ZERO,
            attractor_geometry: crate::behaviour::AttractorGeometry::default(),
            effect_colour_tint_override: glam::Vec4::ONE,
            effect_rgba_rand_index: 0,
            collision_bvh: None,
            river_field: None,
            wind_field: None,
            wall_time_seconds: 0.0,
            real_dt: 0.0,
            water_field: None,
            water_z_camera: 0.0,
            air_resistance: 1.0,
            lod_alpha_mult: 1.0,
            sinks,
            effect_spawner_trigger_ratio: -1.0,
        }
    }

    #[test]
    fn init_point_sets_collides_flag() {
        let mut buf = PointBuffer::default();
        let _ = buf.spawn(0);
        let random = crate::random::RandomTable::new();
        let ptx_table = crate::random_table::PtxRandomTable::default();
        let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
        let er = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
        let em = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![];
        let params = build_params(&random, &ptx_table, &evo, &pr, &er, &em, PtfxSinks::none());
        FireStub.init_point(0, &mut buf, &params);
        assert!(buf.flags[0].contains(PointFlags::COLLIDES));
    }

    #[test]
    fn update_point_publishes_fire_info_on_collision() {
        let mut buf = PointBuffer::default();
        let _ = buf.spawn(0);
        buf.curr_pos[0] = glam::Vec3::new(1.0, 2.0, 3.0);
        buf.curr_vel[0] = glam::Vec3::new(0.0, 0.0, 1.0);
        buf.coln_pos[0] = glam::Vec3::new(1.0, 2.0, 0.0);
        buf.coln_normal[0] = glam::Vec3::Z;
        buf.color[0] = glam::Vec4::new(1.0, 0.5, 0.25, 1.0);
        buf.life_ratio[0] = 0.5;
        buf.flags[0].insert(PointFlags::HAS_COLLIDED);

        let random = crate::random::RandomTable::new();
        let ptx_table = crate::random_table::PtxRandomTable::default();
        let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
        let er = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
        let em = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![];
        let sink = CaptureFireSink::default();
        let mut sinks = PtfxSinks::none();
        sinks.fire = Some(&sink);
        let params = build_params(&random, &ptx_table, &evo, &pr, &er, &em, sinks);
        FireStub.update_point(0, &mut buf, &params);
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1, "expected one fire event");
        let info = events[0];
        assert_eq!(info.pos, glam::Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(info.dir, glam::Vec3::Z);
        assert!(info.scale > 0.0);
    }

    #[test]
    fn update_point_silent_without_collision() {
        let mut buf = PointBuffer::default();
        let _ = buf.spawn(0);
        buf.curr_vel[0] = glam::Vec3::new(0.0, 0.0, 1.0);
        let random = crate::random::RandomTable::new();
        let ptx_table = crate::random_table::PtxRandomTable::default();
        let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
        let er = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
        let em = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![];
        let sink = CaptureFireSink::default();
        let mut sinks = PtfxSinks::none();
        sinks.fire = Some(&sink);
        let params = build_params(&random, &ptx_table, &evo, &pr, &er, &em, sinks);
        FireStub.update_point(0, &mut buf, &params);
        assert!(sink.events.lock().unwrap().is_empty());
    }
}

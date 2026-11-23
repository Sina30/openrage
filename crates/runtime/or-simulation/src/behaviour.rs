use crate::point::PointBuffer;
use crate::random::RandomTable;
use crate::random_table::PtxRandomTable;
use glam::{Mat4, Vec3};
use or_particle::rules::EvolutionList;
#[derive(Debug, Clone, Copy, Default)]
pub struct AttractorGeometry {
    pub pos: Vec3,
    pub dir: Vec3,
    pub radius_near: f32,
    pub radius_far: f32,
    pub radius_orbit: f32,
    pub length: f32,
}

pub mod order {
    pub const AGE: f32 = 10.0;
    pub const ACCELERATION: f32 = 20.0;
    pub const ATTRACTOR: f32 = 30.0;
    pub const VELOCITY: f32 = 40.0;
    pub const PAUSE: f32 = 45.0;
    pub const ROTATION: f32 = 50.0;
    pub const NOISE: f32 = 60.0;
    pub const SIZE: f32 = 70.0;
    pub const DAMPENING: f32 = 80.0;
    pub const MATRIX_WEIGHT: f32 = 90.0;
    pub const RIVER: f32 = 100.0;
    pub const WIND: f32 = 110.0;
    pub const ZCULLING: f32 = 120.0;
    pub const COLLISION: f32 = 130.0;
    pub const ANIMATE_TEXTURE: f32 = 140.0;
    pub const COLOUR: f32 = 150.0;
    pub const FIRE: f32 = 160.0;
    pub const DECAL: f32 = 170.0;
    pub const DECAL_POOL: f32 = 180.0;
    pub const DECAL_TRAIL: f32 = 190.0;
    pub const LIQUID: f32 = 200.0;
    pub const LIGHT: f32 = 210.0;
    pub const FOG_VOLUME: f32 = 220.0;
    pub const DRAW_SPRITE: f32 = 230.0;
    pub const DRAW_TRAIL: f32 = 240.0;
    pub const DRAW_MODEL: f32 = 250.0;
    pub const ROTATION_SPEED: f32 = 50.0;
    pub const GRAVITY: f32 = 20.0;
    pub const FADE_IN: f32 = 149.0;
    pub const FADE_OUT: f32 = 149.0;
    pub const DRAW_DECAL: f32 = 170.0;
    pub const DRAW_LIGHT: f32 = 210.0;
    pub const DRAW_FOG_VOLUME: f32 = 220.0;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    #[default]
    Sprite,
    Trail,
    Model,
    Decal,
}

impl DrawKind {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Sprite,
            1 => Self::Model,
            2 => Self::Trail,
            _ => Self::Sprite,
        }
    }
}

pub struct BehaviourParams<'a> {
    pub dt: f32,
    pub effect_time_ratio: f32,
    pub emitter_time_ratio: f32,
    pub effect_zoom: f32,
    pub emitter_zoom: f32,
    pub effect_matrix: Mat4,
    pub emitter_matrix: Mat4,
    pub effect_velocity: Vec3,
    pub emitter_velocity: Vec3,
    pub gravity: f32,
    pub wind_velocity: Vec3,
    pub random: &'a RandomTable,
    pub ptx_random_table: &'a PtxRandomTable,
    pub evo_list: Option<&'a EvolutionList>,
    pub evo_values: &'a [f32],
    pub particle_rule: &'a or_io::gta5::particle::PtxParticleRule<or_io::decl::Gen8>,
    pub shader_vars: &'a [or_io::gta5::particle::ShaderVar],
    pub effect_rule: &'a or_io::gta5::particle::PtxEffectRule<or_io::decl::Gen8>,
    pub emitter_rule: &'a or_io::gta5::particle::PtxEmitterRule,
    pub camera_pos: Vec3,
    pub camera_basis: glam::Mat3,
    pub emitter_colour_tint: glam::Vec4,
    pub emitter_attractor_pos: Vec3,
    pub attractor_geometry: AttractorGeometry,
    pub effect_spawner_trigger_ratio: f32,
    pub effect_rgba_rand_index: u8,
    pub effect_colour_tint_override: glam::Vec4,
    pub collision_bvh: Option<&'a crate::collision::BvhCollider>,
    pub river_field: Option<&'a (dyn crate::behaviours::RiverField + Send + Sync)>,
    pub wind_field: Option<&'a (dyn crate::behaviours::WindField + Send + Sync)>,
    pub wall_time_seconds: f32,
    pub real_dt: f32,
    pub water_field: Option<&'a (dyn crate::behaviours::WaterField + Send + Sync)>,
    pub water_z_camera: f32,
    pub air_resistance: f32,
    pub lod_alpha_mult: f32,
    pub sinks: crate::sinks::PtfxSinks<'a>,
}

impl<'a> BehaviourParams<'a> {
    pub fn query_kpd(
        &self,
        kpd: &Option<or_particle::KeyframePropData>,
        time: f32,
        default_value: glam::Vec4,
    ) -> glam::Vec4 {
        crate::kfp::query_kpd_with_evo(kpd, time, self.evo_list, self.evo_values, default_value)
    }

    pub fn query_kfp(&self, kfp: &or_io::gta5::particle::PtxKeyframeProp, time: f32) -> glam::Vec4 {
        crate::kfp::query_kfp_with_evo(kfp, time, self.evo_list, self.evo_values)
    }

    #[inline]
    pub fn clock_time(&self, clock: crate::kfp::KfpClock, point_life: f32) -> f32 {
        crate::kfp::key_time_for_mode(
            clock,
            point_life,
            self.emitter_time_ratio,
            self.effect_time_ratio,
        )
    }

    pub fn query_kfp_at(
        &self,
        kfp: &or_io::gta5::particle::PtxKeyframeProp,
        clock: crate::kfp::KfpClock,
        point_life: f32,
    ) -> glam::Vec4 {
        let t = self.clock_time(clock, point_life);
        self.query_kfp(kfp, t)
    }

    #[inline]
    pub fn water_z_at(&self, pos: glam::Vec3) -> f32 {
        match self.water_field {
            Some(f) => f.water_z_at(pos),
            None => self.water_z_camera,
        }
    }

    pub fn query_kpd_at(
        &self,
        kpd: &Option<or_particle::KeyframePropData>,
        clock: crate::kfp::KfpClock,
        point_life: f32,
        default_value: glam::Vec4,
    ) -> glam::Vec4 {
        let t = self.clock_time(clock, point_life);
        self.query_kpd(kpd, t, default_value)
    }

    pub fn query_kpd_point(
        &self,
        buf: &crate::point::PointBuffer,
        idx: usize,
        kpd: &Option<or_particle::KeyframePropData>,
        time: f32,
        default_value: glam::Vec4,
    ) -> glam::Vec4 {
        let snap = buf.evo_snapshot(idx);
        let evo: &[f32] = if snap.is_empty() {
            self.evo_values
        } else {
            snap
        };
        crate::kfp::query_kpd_with_evo(kpd, time, self.evo_list, evo, default_value)
    }

    pub fn query_kfp_point(
        &self,
        buf: &crate::point::PointBuffer,
        idx: usize,
        kfp: &or_io::gta5::particle::PtxKeyframeProp,
        time: f32,
    ) -> glam::Vec4 {
        let snap = buf.evo_snapshot(idx);
        let evo: &[f32] = if snap.is_empty() {
            self.evo_values
        } else {
            snap
        };
        crate::kfp::query_kfp_with_evo(kfp, time, self.evo_list, evo)
    }

    pub fn query_kpd_at_point(
        &self,
        buf: &crate::point::PointBuffer,
        idx: usize,
        kpd: &Option<or_particle::KeyframePropData>,
        clock: crate::kfp::KfpClock,
        point_life: f32,
        default_value: glam::Vec4,
    ) -> glam::Vec4 {
        let t = self.clock_time(clock, point_life);
        self.query_kpd_point(buf, idx, kpd, t, default_value)
    }

    pub fn query_kfp_at_point(
        &self,
        buf: &crate::point::PointBuffer,
        idx: usize,
        kfp: &or_io::gta5::particle::PtxKeyframeProp,
        clock: crate::kfp::KfpClock,
        point_life: f32,
    ) -> glam::Vec4 {
        let t = self.clock_time(clock, point_life);
        self.query_kfp_point(buf, idx, kfp, t)
    }

    pub fn frame_ratio_for_point(&self, buf: &crate::point::PointBuffer, idx: usize) -> f32 {
        let life_ratio = buf.life_ratio[idx];
        let ool = buf.one_over_life[idx].max(1e-6);
        let dt = self.real_dt.max(1e-6);
        (life_ratio / ool / dt).clamp(0.001, 1.0)
    }
}

pub trait BehaviourImpl: Send + Sync + std::fmt::Debug {
    fn order(&self) -> f32;
    fn name(&self) -> &'static str;
    fn init_point(&self, _idx: usize, _buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {}
    fn update_point(&self, _idx: usize, _buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {}
    fn update_finalize(&self, _buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {}
    fn drain_emits(&self, _sink: &mut EmitSink) {}
}

#[derive(Debug, Default)]
pub struct EmitSink {
    pub liquid: Vec<crate::behaviours::LiquidEmit>,
    pub decal_pool: Vec<crate::behaviours::DecalPoolEmit>,
}

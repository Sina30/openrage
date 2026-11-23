use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::{bias_value_kpd, KfpClock};
use crate::point::{PointBuffer, PointFlags};
use glam::{Mat3, Vec3, Vec4};
use or_io::gta5::particle::ReferenceSpace;
use or_particle::behaviour::DampeningBehaviour;
#[derive(Debug, Clone)]
pub struct Dampening {
    pub data: DampeningBehaviour,
}

impl Dampening {
    pub fn from_data(data: DampeningBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Dampening {
    fn order(&self) -> f32 {
        order::DAMPENING
    }
    fn name(&self) -> &'static str {
        "ptxu_Dampening"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }
        let life = buf.life_ratio[idx];
        let bias = bias_value_kpd(
            params.random,
            &self.data.xyz_min_keyframes,
            buf.rand_index[idx],
        );
        let default_min = Vec4::new(
            self.data.xyz_min.x,
            self.data.xyz_min.y,
            self.data.xyz_min.z,
            0.0,
        );
        let default_max = Vec4::new(
            self.data.xyz_max.x,
            self.data.xyz_max.y,
            self.data.xyz_max.z,
            0.0,
        );
        let dmin = params.query_kpd(&self.data.xyz_min_keyframes, life, default_min);
        let dmax = params.query_kpd(&self.data.xyz_max_keyframes, life, default_max);
        let damp = dmin.lerp(dmax, bias);
        let mut damp_v = Vec3::new(damp.x, damp.y, damp.z);
        // Engine: multiply by
        // the emitter's `m_dampScalarKFP * 0.01`. KFP samples at the
        // **emitter** life ratio (`vKeyTimePointEmitter`), not the
        // particle's. Engine multiplies *unconditionally* — an empty
        // scalar KFP collapses dampening to zero (i.e. no drag), which
        // is the artist-facing identity for "no damping authored".
        let damp_kfp = params
            .query_kfp_at(
                &params.emitter_rule.dampening_scalar_kfp,
                KfpClock::Emitter,
                life,
            )
            .x;
        damp_v *= damp_kfp * 0.01;
        let world_damp = damping_in_reference_space(self.data.reference_space, damp_v, params);
        let mut world_damp = world_damp;
        if self.data.enable_air_resistance {
            world_damp *= params.air_resistance;
        }
        let dt = params.dt * buf.playback_rate[idx];
        let scale = Vec3::new(
            (1.0 - world_damp.x * dt).max(0.0),
            (1.0 - world_damp.y * dt).max(0.0),
            (1.0 - world_damp.z * dt).max(0.0),
        );
        buf.curr_vel[idx] *= scale;
    }
}

fn damping_in_reference_space(
    space: ReferenceSpace,
    damp_v: Vec3,
    params: &BehaviourParams<'_>,
) -> Vec3 {
    if space == ReferenceSpace::WORLD {
        return damp_v;
    }
    let basis: Mat3 = match space {
        ReferenceSpace::EFFECT => Mat3::from_mat4(params.effect_matrix),
        ReferenceSpace::EMITTER => Mat3::from_mat4(params.emitter_matrix),
        ReferenceSpace::CAMERA => params.camera_basis,
        _ => return damp_v,
    };
    let cx = (basis.col(0) * damp_v.x).abs();
    let cy = (basis.col(1) * damp_v.y).abs();
    let cz = (basis.col(2) * damp_v.z).abs();
    cx + cy + cz
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dampening_abs_keeps_negatives_positive() {
        let random = crate::random::RandomTable::new();
        let ptx_table = crate::random_table::PtxRandomTable::default();
        let pr = or_io::gta5::particle::PtxParticleRule::<or_io::decl::Gen8>::default();
        let er = or_io::gta5::particle::PtxEffectRule::<or_io::decl::Gen8>::default();
        let em = or_io::gta5::particle::PtxEmitterRule::default();
        let evo: Vec<f32> = vec![];
        let params = BehaviourParams {
            dt: 0.0,
            effect_time_ratio: 0.0,
            emitter_time_ratio: 0.0,
            effect_zoom: 1.0,
            emitter_zoom: 1.0,
            effect_matrix: glam::Mat4::IDENTITY,
            emitter_matrix: glam::Mat4::IDENTITY,
            effect_velocity: Vec3::ZERO,
            emitter_velocity: Vec3::ZERO,
            gravity: 0.0,
            wind_velocity: Vec3::ZERO,
            random: &random,
            ptx_random_table: &ptx_table,
            evo_list: None,
            evo_values: &evo,
            particle_rule: &pr,
            shader_vars: &[],
            effect_rule: &er,
            emitter_rule: &em,
            camera_pos: Vec3::ZERO,
            camera_basis: glam::Mat3::IDENTITY,
            emitter_colour_tint: glam::Vec4::ONE,
            emitter_attractor_pos: Vec3::ZERO,
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
            sinks: crate::sinks::PtfxSinks::none(),
            effect_spawner_trigger_ratio: -1.0,
        };
        let damp = Vec3::new(-0.5, 1.0, -2.0);
        let out = damping_in_reference_space(ReferenceSpace::EFFECT, damp, &params);
        assert!((out.x - 0.5).abs() < 1e-5);
        assert!((out.y - 1.0).abs() < 1e-5);
        assert!((out.z - 2.0).abs() < 1e-5);
    }
}

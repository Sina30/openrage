use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use glam::{Mat3, Vec3, Vec4};
use or_io::gta5::particle::ReferenceSpace;
use or_particle::behaviour::NoiseBehaviour;
#[derive(Debug, Clone)]
pub struct Noise {
    pub data: NoiseBehaviour,
}

impl Noise {
    pub fn from_data(data: NoiseBehaviour) -> Self {
        Self { data }
    }
}

fn noise_in_reference_space(
    space: ReferenceSpace,
    noise: Vec3,
    amp: Vec3,
    params: &BehaviourParams<'_>,
) -> Vec3 {
    let scaled = Vec3::new(noise.x * amp.x, noise.y * amp.y, noise.z * amp.z);
    if space == ReferenceSpace::WORLD {
        return scaled;
    }
    let basis: Mat3 = match space {
        ReferenceSpace::EFFECT => Mat3::from_mat4(params.effect_matrix),
        ReferenceSpace::EMITTER => Mat3::from_mat4(params.emitter_matrix),
        ReferenceSpace::CAMERA => params.camera_basis,
        _ => return scaled,
    };
    basis.col(0) * scaled.x + basis.col(1) * scaled.y + basis.col(2) * scaled.z
}

fn fresh_signed_vec3(table: &crate::random::RandomTable, rand_index: u8, frame_seed: u32) -> Vec3 {
    let base = (rand_index as u32)
        .wrapping_mul(2654435761)
        .wrapping_add(frame_seed);
    Vec3::new(
        table.signed(base),
        table.signed(base.wrapping_add(0x9E3779B9)),
        table.signed(base.wrapping_add(0xC9E5_2299)),
    )
}

impl BehaviourImpl for Noise {
    fn order(&self) -> f32 {
        order::NOISE
    }
    fn name(&self) -> &'static str {
        "ptxu_Noise"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }
        let life = buf.life_ratio[idx];
        let pos_min = params.query_kpd(&self.data.pos_noise_min_keyframes, life, Vec4::ZERO);
        let pos_max = params.query_kpd(&self.data.pos_noise_max_keyframes, life, Vec4::ZERO);
        let vel_min = params.query_kpd(&self.data.vel_noise_min_keyframes, life, Vec4::ZERO);
        let vel_max = params.query_kpd(&self.data.vel_noise_max_keyframes, life, Vec4::ZERO);

        let rand_seed = buf.rand_index[idx] as u32;
        let pos_bias = params.random.value(rand_seed.wrapping_add(0xA10C_BEEF));
        let vel_bias = params.random.value(rand_seed.wrapping_add(0x9E37_79B9));

        let pos_amp = pos_min.lerp(pos_max, pos_bias);
        let vel_amp = vel_min.lerp(vel_max, vel_bias);

        let frame_seed = (params.effect_time_ratio * 65536.0) as u32;

        let dt = params.dt * buf.playback_rate[idx];
        let scale = dt;

        if pos_amp.length_squared() > 1e-12 {
            let noise = fresh_signed_vec3(params.random, buf.rand_index[idx], frame_seed);
            let pos_world = noise_in_reference_space(
                self.data.reference_space,
                noise,
                Vec3::new(pos_amp.x, pos_amp.y, pos_amp.z) * scale,
                params,
            );
            buf.curr_pos[idx] += pos_world;
        }

        if vel_amp.length_squared() > 1e-12 {
            let noise = fresh_signed_vec3(
                params.random,
                buf.rand_index[idx],
                frame_seed.wrapping_add(0xCAFE_F00D),
            );
            let vel_world = noise_in_reference_space(
                self.data.reference_space,
                noise,
                Vec3::new(vel_amp.x, vel_amp.y, vel_amp.z) * scale,
                params,
            );

            if self.data.keep_constant_speed {
                let cur = buf.curr_vel[idx];
                let speed = cur.length();
                if speed > 1e-4 {
                    let perturbed = cur + vel_world;
                    let p_len = perturbed.length();
                    if p_len > 1e-6 {
                        buf.curr_vel[idx] = perturbed * (speed / p_len);
                    }
                }
            } else {
                buf.curr_vel[idx] += vel_world;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_world_pass_through_preserves_sign() {
        let space = ReferenceSpace::WORLD;
        let amp = Vec3::splat(1.0);
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
        let signed_noise = Vec3::new(-0.5, 0.25, -1.0);
        let out = noise_in_reference_space(space, signed_noise, amp, &params);
        assert_eq!(out, Vec3::new(-0.5, 0.25, -1.0));
    }

    #[test]
    fn noise_effect_space_with_identity_basis_equals_world() {
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
        let noise = Vec3::new(0.7, -0.2, 0.5);
        let amp = Vec3::new(2.0, 3.0, 4.0);
        let world = noise_in_reference_space(ReferenceSpace::WORLD, noise, amp, &params);
        let effect = noise_in_reference_space(ReferenceSpace::EFFECT, noise, amp, &params);
        assert!((world - effect).length() < 1e-5, "{world:?} vs {effect:?}");
    }
}

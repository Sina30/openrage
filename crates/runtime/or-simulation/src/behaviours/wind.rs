use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::bias_value_kpd;
use crate::point::{PointBuffer, PointFlags};
use glam::Vec4;
use or_particle::behaviour::WindBehaviour;
#[derive(Debug, Clone)]
pub struct Wind {
    pub data: WindBehaviour,
}

impl Wind {
    pub fn from_data(data: WindBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Wind {
    fn order(&self) -> f32 {
        order::WIND
    }
    fn name(&self) -> &'static str {
        "ptxu_Wind"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }
        let cam_dist_sq = (buf.curr_pos[idx] - params.camera_pos).length_squared();
        let low_lod_sq = self.data.low_lod_range * self.data.low_lod_range;
        let high_lod_sq = self.data.high_lod_range * self.data.high_lod_range;
        if low_lod_sq > 0.0 && cam_dist_sq > low_lod_sq {
            return;
        }
        let in_high_lod = high_lod_sq > 0.0 && cam_dist_sq <= high_lod_sq;
        let disturbance_mode = if in_high_lod {
            self.data.high_lod_disturbance_mode
        } else {
            self.data.low_lod_disturbance_mode
        };
        let global_wind = match params.wind_field {
            Some(field) => field.sample_local(buf.curr_pos[idx], params.wall_time_seconds),
            None => params.wind_velocity,
        };
        let mut air_vel = global_wind;
        if disturbance_mode > 0 {
            let p = buf.curr_pos[idx];
            let t = params.wall_time_seconds;
            let s1 = (p.x * 0.20 + p.z * 0.13 + t * 1.7).sin();
            let c1 = (p.y * 0.17 + p.z * 0.11 + t * 1.3).cos();
            air_vel += glam::Vec3::new(s1, c1 * 0.5, s1 * c1 * 0.5) * global_wind.length() * 0.25;
            if disturbance_mode >= 2 {
                let s2 = (p.x * 0.43 - p.z * 0.27 + t * 3.1).sin();
                let c2 = (p.y * 0.31 + p.z * 0.19 - t * 2.5).cos();
                air_vel +=
                    glam::Vec3::new(c2, s2 * 0.4, c2 * s2 * 0.6) * global_wind.length() * 0.18;
            }
        }
        let influence_kfp = params.query_kpd(
            &self.data.influence_keyframes,
            buf.life_ratio[idx],
            Vec4::ZERO,
        );
        let bias = bias_value_kpd(
            params.random,
            &self.data.influence_keyframes,
            buf.rand_index[idx],
        );
        let mut influence = influence_kfp.x + (influence_kfp.y - influence_kfp.x) * bias;
        if !self.data.ignore_mtx_weight {
            influence *= (1.0 - buf.matrix_weight[idx]).clamp(0.0, 1.0);
        }
        let dt = params.real_dt;
        let delta_vel = air_vel - buf.curr_vel[idx];
        let delta_mult = (influence * dt).clamp(0.0, 1.0);
        buf.curr_vel[idx] += delta_vel * delta_mult;
    }
}

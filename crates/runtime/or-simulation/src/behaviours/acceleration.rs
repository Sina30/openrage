use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::{bias_value_kpd, KfpClock};
use crate::point::PointBuffer;
use crate::reference_space;
use glam::{Vec3, Vec4};
use or_particle::behaviour::AccelerationBehaviour;
#[derive(Debug, Clone)]
pub struct Acceleration {
    pub data: AccelerationBehaviour,
}

impl Acceleration {
    pub fn from_data(data: AccelerationBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Acceleration {
    fn order(&self) -> f32 {
        order::ACCELERATION
    }

    fn name(&self) -> &'static str {
        "ptxu_Acceleration"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let life = buf.life_ratio[idx];
        let rand_idx = buf.rand_index[idx];
        let default = Vec4::new(
            self.data.xyz_min.x,
            self.data.xyz_min.y,
            self.data.xyz_min.z,
            0.0,
        );
        let amin = params.query_kpd(&self.data.xyz_min_keyframes, life, default);
        let amax = params.query_kpd(
            &self.data.xyz_max_keyframes,
            life,
            Vec4::new(
                self.data.xyz_max.x,
                self.data.xyz_max.y,
                self.data.xyz_max.z,
                0.0,
            ),
        );
        let bias = bias_value_kpd(params.random, &self.data.xyz_min_keyframes, rand_idx);
        let mut accel = Vec3::new(
            amin.x + (amax.x - amin.x) * bias,
            amin.y + (amax.y - amin.y) * bias,
            amin.z + (amax.z - amin.z) * bias,
        );

        if self.data.is_affected_by_zoom {
            accel *= params.effect_zoom * params.emitter_zoom;
        }

        accel = reference_space::transform_vector(self.data.reference_space, accel, params);

        if self.data.enable_gravity {
            accel.z *= -params.gravity;
        }

        let accn_kfp = params
            .query_kfp_at(
                &params.emitter_rule.accn_scalar_kfp,
                KfpClock::Emitter,
                life,
            )
            .x;
        accel *= accn_kfp * 0.01;

        let dt = params.dt * buf.playback_rate[idx];
        if !buf.flags[idx].contains(crate::point::PointFlags::AT_REST) {
            buf.curr_vel[idx] += accel * dt;
        }
    }
}

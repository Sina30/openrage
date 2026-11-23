use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::kfp::{bias_value_kpd, KfpClock};
use crate::point::PointBuffer;
use glam::{Vec3, Vec4};
use or_io::gta5::particle::ReferenceSpace;
use or_particle::behaviour::MatrixWeightBehaviour;
#[derive(Debug, Clone)]
pub struct MatrixWeight {
    pub data: MatrixWeightBehaviour,
}

impl MatrixWeight {
    pub fn from_data(data: MatrixWeightBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for MatrixWeight {
    fn order(&self) -> f32 {
        order::MATRIX_WEIGHT
    }
    fn name(&self) -> &'static str {
        "ptxu_MatrixWeight"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if (params.particle_rule.flags & 0x02) != 0 {
            return;
        }
        let kfp = params.query_kpd(&self.data.weight_keyframes, buf.life_ratio[idx], Vec4::ZERO);
        let bias = bias_value_kpd(
            params.random,
            &self.data.weight_keyframes,
            buf.rand_index[idx],
        );
        let mut weight = kfp.x + (kfp.y - kfp.x) * bias;

        let emit_w_kfp = params
            .query_kfp_at(
                &params.emitter_rule.matrix_weight_scalar_kfp,
                KfpClock::Emitter,
                buf.life_ratio[idx],
            )
            .x;
        weight *= emit_w_kfp * 0.01;
        let frame_ratio = params.frame_ratio_for_point(buf, idx);
        weight *= frame_ratio;
        weight = weight.clamp(0.0, 1.0);
        let dt = params.dt * buf.playback_rate[idx] * frame_ratio;

        let frame_velocity = match self.data.reference_space {
            ReferenceSpace::EFFECT => params.effect_velocity,
            ReferenceSpace::EMITTER => params.emitter_velocity,
            _ => Vec3::ZERO,
        };
        let delta = frame_velocity * dt * weight;
        buf.prev_pos[idx] += delta;
        buf.curr_pos[idx] += delta;
        buf.spawn_pos[idx] += delta;

        buf.matrix_weight[idx] = weight;
    }
}

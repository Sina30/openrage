use crate::behaviour::{order, BehaviourImpl, BehaviourParams, DrawKind};
use crate::kfp::{bias_value_kpd, key_time_for_mode, KeyframeMode, KfpClock};
use crate::point::PointBuffer;
use glam::Vec4;
use or_particle::behaviour::SizeBehaviour;
#[derive(Debug, Clone)]
pub struct Size {
    pub data: SizeBehaviour,
}

impl Size {
    pub fn from_data(data: SizeBehaviour) -> Self {
        Self { data }
    }

    fn mode(&self) -> KeyframeMode {
        match self.data.keyframe_mode {
            or_io::gta5::particle::KeyframeMode::Point => KeyframeMode::Point,
            or_io::gta5::particle::KeyframeMode::Effect => KeyframeMode::Effect,
            or_io::gta5::particle::KeyframeMode::Emitter => KeyframeMode::Emitter,
        }
    }
}

impl BehaviourImpl for Size {
    fn order(&self) -> f32 {
        order::SIZE
    }
    fn name(&self) -> &'static str {
        "ptxu_Size"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let t = key_time_for_mode(
            self.mode(),
            buf.life_ratio[idx],
            params.emitter_time_ratio,
            params.effect_time_ratio,
        );
        let rand_idx = buf.rand_index[idx];

        let whd_min = params.query_kpd(&self.data.whd_min_keyframes, t, Vec4::splat(1.0));
        let whd_max = params.query_kpd(&self.data.whd_max_keyframes, t, Vec4::splat(1.0));

        let whd = if self.data.is_proportional {
            let bias = bias_value_kpd(params.random, &self.data.whd_min_keyframes, rand_idx);
            whd_min.lerp(whd_max, bias)
        } else {
            let bx = bias_value_kpd(params.random, &self.data.whd_min_keyframes, rand_idx);
            let by = bias_value_kpd(
                params.random,
                &self.data.whd_min_keyframes,
                rand_idx.wrapping_add(1),
            );
            let bz = bias_value_kpd(
                params.random,
                &self.data.whd_min_keyframes,
                rand_idx.wrapping_add(2),
            );
            Vec4::new(
                whd_min.x + (whd_max.x - whd_min.x) * bx,
                whd_min.y + (whd_max.y - whd_min.y) * by,
                whd_min.z + (whd_max.z - whd_min.z) * bz,
                whd_min.w,
            )
        };

        let tblr_scalar = params.query_kpd(&self.data.tblr_scalar_keyframes, t, Vec4::splat(1.0));
        let tblr_vel_scalar = params.query_kpd(&self.data.tblr_vel_scalar_keyframes, t, Vec4::ZERO);
        let speed = buf.curr_vel[idx].length();
        let draw_kind = DrawKind::from_raw(params.particle_rule.draw_type);
        let mut dim = match draw_kind {
            DrawKind::Sprite | DrawKind::Trail => {
                let half = whd * 0.5;
                let tblr = Vec4::new(half.y, half.y, half.x, half.x);
                let scaled = tblr * tblr_scalar;
                scaled + tblr_vel_scalar * speed
            }
            DrawKind::Model | DrawKind::Decal => {
                let mut d = whd * tblr_scalar;
                if tblr_vel_scalar.w.abs() > 1e-6 {
                    d.w += tblr_vel_scalar.w * speed;
                }
                d
            }
        };

        // Engine chain: multiply by
        // `m_sizeScalarKFP * 0.01` from the emitter rule. Engine
        // samples this KFP at the **emitter** life ratio
        // (`vKeyTimePointEmitter`), not the per-particle / per-mode
        // `t`. The multiply is **unconditional** —
        // empty KFP returns 0 and zeroes the size, which is the
        // artist-facing identity for "no scale authored".
        let size_kfp = params
            .query_kfp_at(
                &params.emitter_rule.size_scalar_kfp,
                KfpClock::Emitter,
                buf.life_ratio[idx],
            )
            .x;
        dim *= size_kfp * 0.01;
        dim *= params.effect_zoom;

        buf.dimensions[idx] = dim;
    }
}

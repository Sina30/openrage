use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use glam::Vec4;
use or_io::gta5::particle::ReferenceSpace;
use or_particle::behaviour::ZCullingBehaviour;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZCullMode {
    CullUnder,
    CullOver,
    ClampUnder,
    ClampOver,
}

impl ZCullMode {
    fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::CullUnder,
            1 => Self::CullOver,
            2 => Self::ClampUnder,
            3 => Self::ClampOver,
            _ => Self::CullUnder,
        }
    }

    fn under_is_danger(self) -> bool {
        matches!(self, Self::CullUnder | Self::ClampUnder)
    }

    fn is_clamp(self) -> bool {
        matches!(self, Self::ClampUnder | Self::ClampOver)
    }
}

#[derive(Debug, Clone)]
pub struct ZCulling {
    pub data: ZCullingBehaviour,
}

impl ZCulling {
    pub fn from_data(data: ZCullingBehaviour) -> Self {
        Self { data }
    }

    fn cull_z_at(&self, idx: usize, buf: &PointBuffer, params: &BehaviourParams<'_>) -> f32 {
        match self.data.reference_space {
            ReferenceSpace::EFFECT => params.effect_matrix.w_axis.z,
            ReferenceSpace::EMITTER => params.emitter_matrix.w_axis.z,
            ReferenceSpace::WATER_Z_CAMERA => params.water_z_camera,
            ReferenceSpace::WATER_Z_POINT => params.water_z_at(buf.curr_pos[idx]),
            _ => 0.0,
        }
    }

    fn update_one(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let mode = ZCullMode::from_raw(self.data.mode);
        let life = buf.life_ratio[idx];
        let height_kfp = params.query_kpd(&self.data.height_keyframes, life, Vec4::splat(0.0));
        let fade_kfp = params.query_kpd(&self.data.fade_dist_keyframes, life, Vec4::splat(0.0));
        let bias_h = params
            .random
            .value(buf.rand_index[idx] as u32 + 0xC0FF_EE07);
        let bias_f = params
            .random
            .value(buf.rand_index[idx] as u32 + 0xC0FF_EE08);
        let height = height_kfp.x + (height_kfp.y - height_kfp.x) * bias_h;
        let fade_dist = (fade_kfp.x + (fade_kfp.y - fade_kfp.x) * bias_f).max(0.0);

        let cull_z = self.cull_z_at(idx, buf, params) + height;
        let point_z = buf.curr_pos[idx].z;

        let in_danger = if mode.under_is_danger() {
            point_z < cull_z
        } else {
            point_z > cull_z
        };

        let safe_dist = if mode.under_is_danger() {
            (point_z - cull_z).max(0.0)
        } else {
            (cull_z - point_z).max(0.0)
        };
        let fade_alpha = if fade_dist <= 1e-6 || safe_dist >= fade_dist {
            1.0
        } else {
            (safe_dist / fade_dist).clamp(0.0, 1.0)
        };

        if in_danger {
            if mode.is_clamp() {
                buf.curr_pos[idx].z = cull_z;
            } else {
                buf.is_dead[idx] = true;
                buf.flags[idx].insert(PointFlags::ZCULLED);
                return;
            }
        }
        buf.alpha_mult[idx] *= fade_alpha;
    }
}

impl BehaviourImpl for ZCulling {
    fn order(&self) -> f32 {
        order::ZCULLING
    }
    fn name(&self) -> &'static str {
        "ptxu_ZCull"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_one(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_one(idx, buf, params);
    }

    fn update_finalize(&self, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if self.data.reference_space != ReferenceSpace::WATER_Z_POINT {
            return;
        }
        for i in 0..buf.len() {
            if buf.is_dead[i] {
                continue;
            }
            self.update_one(i, buf, params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_decode_matches_engine_enum() {
        assert_eq!(ZCullMode::from_raw(0), ZCullMode::CullUnder);
        assert_eq!(ZCullMode::from_raw(1), ZCullMode::CullOver);
        assert_eq!(ZCullMode::from_raw(2), ZCullMode::ClampUnder);
        assert_eq!(ZCullMode::from_raw(3), ZCullMode::ClampOver);
        assert_eq!(ZCullMode::from_raw(99), ZCullMode::CullUnder);
    }
}

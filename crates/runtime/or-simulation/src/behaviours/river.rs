use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use glam::Vec3;
use or_particle::behaviour::RiverBehaviour;
pub trait RiverField: std::fmt::Debug {
    fn velocity_at(&self, pos: Vec3) -> Vec3;
}

#[derive(Debug, Clone, Copy)]
pub struct ConstantRiverField {
    pub velocity: Vec3,
}

impl RiverField for ConstantRiverField {
    fn velocity_at(&self, _pos: Vec3) -> Vec3 {
        self.velocity
    }
}

pub trait WaterFlowSampler: std::fmt::Debug + Send + Sync {
    fn sample_flow(&self, pos: Vec3) -> Vec3;
}

#[derive(Debug)]
pub struct WaterFlowAsRiverField<T: WaterFlowSampler>(pub T);

impl<T: WaterFlowSampler> RiverField for WaterFlowAsRiverField<T> {
    fn velocity_at(&self, pos: Vec3) -> Vec3 {
        self.0.sample_flow(pos)
    }
}

#[derive(Debug, Clone)]
pub struct River {
    pub data: RiverBehaviour,
}

impl River {
    pub fn from_data(data: RiverBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for River {
    fn order(&self) -> f32 {
        order::RIVER
    }
    fn name(&self) -> &'static str {
        "ptxu_River"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        let Some(field) = params.river_field else {
            return;
        };
        let curr_pos = buf.curr_pos[idx];
        let mut river_vel = field.velocity_at(curr_pos);
        river_vel *= self.data.velocity_mult;
        let delta = river_vel - buf.curr_vel[idx];
        let delta_mult = (self.data.influence * params.real_dt).clamp(0.0, 1.0);
        buf.curr_vel[idx] += delta * delta_mult;
    }

    fn update_finalize(&self, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        for i in 0..buf.len() {
            self.update_point(i, buf, params);
        }
    }
}

use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use glam::Vec3;
use or_particle::behaviour::GravityBehaviour;
#[derive(Debug, Clone)]
pub struct Gravity {
    pub data: GravityBehaviour,
}

impl Gravity {
    pub fn from_data(data: GravityBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Gravity {
    fn order(&self) -> f32 {
        order::GRAVITY
    }
    fn name(&self) -> &'static str {
        "ptxu_Gravity"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::AT_REST) {
            return;
        }
        let dir = Vec3::new(
            self.data.direction.x,
            self.data.direction.y,
            self.data.direction.z,
        );
        let dt = params.dt * buf.playback_rate[idx];
        buf.curr_vel[idx] += dir * (self.data.strength * params.gravity * dt);
    }
}

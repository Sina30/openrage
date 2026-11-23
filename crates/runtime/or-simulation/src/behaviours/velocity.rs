use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use or_particle::behaviour::VelocityBehaviour;
#[derive(Debug, Clone)]
pub struct Velocity {
    pub data: VelocityBehaviour,
}

impl Velocity {
    pub fn from_data(data: VelocityBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Velocity {
    fn order(&self) -> f32 {
        order::VELOCITY
    }
    fn name(&self) -> &'static str {
        "ptxu_Velocity"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        self.update_point(idx, buf, params);
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(crate::point::PointFlags::AT_REST) {
            return;
        }
        let dt = params.dt * buf.playback_rate[idx];
        buf.curr_pos[idx] += buf.curr_vel[idx] * dt;
    }
}

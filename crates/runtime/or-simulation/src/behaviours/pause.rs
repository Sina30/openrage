use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use or_particle::behaviour::PauseBehaviour;
#[derive(Debug, Clone)]
pub struct Pause {
    pub data: PauseBehaviour,
}

impl Pause {
    pub fn from_data(data: PauseBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Pause {
    fn order(&self) -> f32 {
        order::PAUSE
    }
    fn name(&self) -> &'static str {
        "ptxu_Pause"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if self.data.pause_life_ratio <= -1.0 + 1e-6 {
            return;
        }
        if buf.flags[idx].contains(PointFlags::LIFE_UNPAUSED) {
            return;
        }
        if !buf.flags[idx].contains(PointFlags::LIFE_PAUSED) {
            if buf.life_ratio[idx] > self.data.pause_life_ratio {
                buf.flags[idx].insert(PointFlags::LIFE_PAUSED);
                buf.pause_timer[idx] = self.data.unpause_time.max(0.0);
            }
            if !buf.flags[idx].contains(PointFlags::LIFE_PAUSED) {
                return;
            }
        }
        buf.life_ratio[idx] = self.data.pause_life_ratio;
        let particle_pos = buf.curr_pos[idx];
        let timer_done = self.data.unpause_time > 0.0 && {
            buf.pause_timer[idx] -= params.real_dt;
            buf.pause_timer[idx] <= 0.0
        };
        let effect_pos = params.effect_matrix.w_axis.truncate();
        let effect_dist_done = self.data.unpause_effect_dist > 0.0
            && (effect_pos - particle_pos).length() > self.data.unpause_effect_dist;
        let cam_dist_done = self.data.unpause_cam_dist > 0.0
            && (params.camera_pos - particle_pos).length() > self.data.unpause_cam_dist;
        if timer_done || effect_dist_done || cam_dist_done {
            buf.flags[idx].remove(PointFlags::LIFE_PAUSED);
            buf.flags[idx].insert(PointFlags::LIFE_UNPAUSED);
        }
    }
}

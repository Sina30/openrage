use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use or_particle::behaviour::DrawTrailBehaviour;
#[derive(Debug, Clone)]
pub struct DrawTrail {
    pub data: DrawTrailBehaviour,
}

impl DrawTrail {
    pub fn from_data(data: DrawTrailBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for DrawTrail {
    fn order(&self) -> f32 {
        order::DRAW_TRAIL
    }
    fn name(&self) -> &'static str {
        "ptxd_Trail"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {
        buf.flags[idx].insert(PointFlags::IS_TRAIL);
    }

    fn update_finalize(&self, buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {
        if self.data.smoothness_x <= 0.0 && self.data.smoothness_y <= 0.0 {
            return;
        }
        let n = buf.len();
        let snapshot = buf.curr_pos.clone();
        for i in 0..n {
            let prev_i = match buf.prev_in_chain[i] {
                Some(p) if !buf.is_broken_trail[i] => p,
                _ => continue,
            };
            let next_i = match buf.next_in_chain[i] {
                Some(p) if !buf.is_broken_trail[p] => p,
                _ => continue,
            };
            let pos_prev = snapshot[prev_i];
            let pos_curr = snapshot[i];
            let pos_next = snapshot[next_i];
            let prev_to_curr = pos_curr - pos_prev;
            let prev_to_next = pos_next - pos_prev;
            let denom = prev_to_next.dot(prev_to_next);
            if denom < 1e-6 {
                continue;
            }
            let proj_t = prev_to_curr.dot(prev_to_next) / denom;
            let pos_proj = pos_prev + prev_to_next * proj_t;
            let pos_mid = (pos_next + pos_prev) * 0.5;
            let mut new_pos = pos_curr;
            new_pos += (pos_proj - pos_curr) * self.data.smoothness_y;
            new_pos += (pos_mid - pos_curr) * self.data.smoothness_x;
            buf.curr_pos[i] = new_pos;
        }
    }
}

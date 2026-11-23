use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use or_particle::behaviour::AgeBehaviour;
#[derive(Debug, Clone)]
pub struct Age {
    pub data: AgeBehaviour,
}

impl Age {
    pub fn from_data(data: AgeBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for Age {
    fn order(&self) -> f32 {
        order::AGE
    }
    fn name(&self) -> &'static str {
        "ptxu_Age"
    }

    fn update_point(&self, idx: usize, buf: &mut PointBuffer, params: &BehaviourParams<'_>) {
        if buf.flags[idx].contains(PointFlags::LIFE_PAUSED) {
            return;
        }
        let dt = params.dt;
        let prev_life = buf.life_ratio[idx];
        let inc = buf.one_over_life[idx] * dt;
        let mut next_life = prev_life + inc;

        let trigger = params.effect_spawner_trigger_ratio;
        let prev_life_spawn = if buf.flags[idx].contains(PointFlags::IS_BEING_INITIALISED) {
            0.0
        } else {
            prev_life
        };
        if dt > 0.0 && trigger >= 0.0 && trigger >= prev_life_spawn && trigger < next_life {
            buf.flags[idx].insert(PointFlags::SPAWN_EFFECT);
        }

        if next_life > 1.0 {
            if buf.flags[idx].contains(PointFlags::LOOPED) {
                next_life -= 1.0;
            } else {
                next_life = 1.0;
                buf.is_dead[idx] = true;
            }
        }
        buf.life_ratio[idx] = next_life;
    }
}

use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::PointBuffer;
use or_particle::behaviour::{FadeInBehaviour, FadeOutBehaviour};
#[derive(Debug, Clone)]
pub struct FadeIn {
    pub data: FadeInBehaviour,
}

impl FadeIn {
    pub fn from_data(data: FadeInBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for FadeIn {
    fn order(&self) -> f32 {
        order::FADE_IN
    }
    fn name(&self) -> &'static str {
        "ptxu_FadeIn"
    }
    fn update_point(&self, idx: usize, buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {
        let d = self.data.duration.max(1e-4);
        let t = (buf.life_ratio[idx] / d).clamp(0.0, 1.0);
        buf.alpha_mult[idx] *= t;
    }
}

#[derive(Debug, Clone)]
pub struct FadeOut {
    pub data: FadeOutBehaviour,
}

impl FadeOut {
    pub fn from_data(data: FadeOutBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for FadeOut {
    fn order(&self) -> f32 {
        order::FADE_OUT
    }
    fn name(&self) -> &'static str {
        "ptxu_FadeOut"
    }
    fn update_point(&self, idx: usize, buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {
        let d = self.data.duration.max(1e-4);
        let life = buf.life_ratio[idx];
        let start = (1.0 - d).max(0.0);
        let t = if life >= start {
            ((1.0 - life) / d).clamp(0.0, 1.0)
        } else {
            1.0
        };
        buf.alpha_mult[idx] *= t;
    }
}

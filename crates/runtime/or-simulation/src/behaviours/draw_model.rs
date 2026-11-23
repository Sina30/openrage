use crate::behaviour::{order, BehaviourImpl, BehaviourParams};
use crate::point::{PointBuffer, PointFlags};
use or_particle::behaviour::DrawModelBehaviour;
#[derive(Debug, Clone)]
pub struct DrawModel {
    pub data: DrawModelBehaviour,
}

impl DrawModel {
    pub fn from_data(data: DrawModelBehaviour) -> Self {
        Self { data }
    }
}

impl BehaviourImpl for DrawModel {
    fn order(&self) -> f32 {
        order::DRAW_MODEL
    }
    fn name(&self) -> &'static str {
        "ptxd_Model"
    }

    fn init_point(&self, idx: usize, buf: &mut PointBuffer, _params: &BehaviourParams<'_>) {
        buf.flags[idx].insert(PointFlags::IS_MODEL);
    }
}

use glam::Vec3;
pub trait WindField: std::fmt::Debug {
    fn sample_local(&self, pos: Vec3, time_seconds: f32) -> Vec3;
}

#[derive(Debug, Clone, Copy)]
pub struct ConstantWindField {
    pub velocity: Vec3,
}

impl WindField for ConstantWindField {
    fn sample_local(&self, _pos: Vec3, _time_seconds: f32) -> Vec3 {
        self.velocity
    }
}

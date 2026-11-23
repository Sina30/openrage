use glam::Vec3;
pub trait WaterField: std::fmt::Debug {
    fn water_z_at(&self, pos: Vec3) -> f32;
}

#[derive(Debug, Clone, Copy)]
pub struct ConstantWaterField {
    pub surface_z: f32,
}

impl WaterField for ConstantWaterField {
    fn water_z_at(&self, _pos: Vec3) -> f32 {
        self.surface_z
    }
}

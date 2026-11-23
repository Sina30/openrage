//! What the bridge publishes to the state each frame, for the Statistics panel.

use super::RenderBridge;
use crate::render::gpu_budget::{DriverObjects, GpuUsage};

impl RenderBridge {
    /// Count the device's objects through `instance`, which made it.
    pub fn with_instance(mut self, instance: wgpu::Instance) -> Self {
        self.instance = Some(instance);
        self
    }

    /// What the bridge holds on the GPU, by the class the Statistics panel shows it under.
    pub(super) fn gpu_usage(&self) -> GpuUsage {
        let resident = self.renderer.resident_bytes();
        GpuUsage {
            line_buffer_bytes: resident.line_buffers as usize,
            particle_instance_bytes: resident.particle_instance_buffers as usize,
            uniform_buffer_bytes: resident.uniform_buffers as usize,
            default_texture_bytes: resident.default_textures as usize,
            texture_bytes: self.gpu_textures.bytes(),
            buffer_bytes: self
                .cached_mesh_draws
                .iter()
                .map(|d| (d.vertex_buffer.size() + d.index_buffer.size()) as usize)
                .sum(),
            target_bytes: self.renderer.target_bytes() as usize,
            thumbnail_bytes: self.thumbnails.bytes() as usize,
            driver: DriverObjects::read(self.instance.as_ref(), &self.device),
        }
    }
}

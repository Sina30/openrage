//! The models of particle batches as the preview lays them out for `particle_meshes`, which draws them with the game's own passes.

use crate::particles::ParticleBlendMode;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub struct ModelInstance {
    pub position: [f32; 3],
    /// A quaternion, x y z w.
    pub rotation: [f32; 4],
    /// The size the model's bounding box is drawn at: the particle's dimensions, with the camera factor.
    pub scale: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// A model's mesh on the CPU, which the game's own passes lay out in the world for each instance.
#[derive(Debug, Default)]
pub struct ModelGeometry {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    /// The size of the drawable's bounding box: an instance's scale is the size its box is drawn at, so the mesh is scaled by that over this.
    pub size: [f32; 3],
}

pub struct ModelBatch {
    pub instances: Vec<ModelInstance>,
    pub blend: ParticleBlendMode,
    /// The technique of `ptfx_model` the game's own passes draw the batch with; none draws nothing there.
    pub shader_technique: String,
    /// The decoded diffuse texture the game's own passes bind, white where there is none.
    pub diffuse_image: Option<Arc<crate::game_shaders::mesh::Image>>,
    pub geometry: Option<Arc<ModelGeometry>>,
}

//! The particle hosts that draw with the game's own passes, bundled.

use std::sync::Arc;

use glam::Vec3;

use crate::game_shaders::engine::EngineConstants;
use crate::game_shaders::pack::GamePack;
use crate::game_shaders::pipeline::Target;
use crate::particle_meshes::GameModels;
use crate::particle_models::ModelBatch;
use crate::particle_sprites::GameSprites;
use crate::particle_trails::GameTrails;
use crate::particles::ParticleBatch;
use crate::trails::TrailBatch;

/// What a frame draws of particles.
pub struct FrameBatches<'a> {
    pub sprites: &'a [ParticleBatch],
    pub trails: &'a [TrailBatch],
    pub models: &'a [ModelBatch],
}

/// The sprites, trails and models of a frame, each with the passes the game draws it with.
pub struct GameParticles {
    pub sprites: GameSprites,
    pub trails: GameTrails,
    pub models: GameModels,
}

impl GameParticles {
    /// Hosts drawing into `target`.
    pub fn new(pack: &Arc<GamePack>, target: Target) -> Self {
        Self {
            sprites: GameSprites::new(pack.clone(), target),
            trails: GameTrails::new(pack.clone(), target),
            models: GameModels::new(pack.clone(), target),
        }
    }

    /// The GPU bytes the three hosts hold.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        self.sprites.resident_bytes() + self.trails.resident_bytes() + self.models.resident_bytes()
    }

    /// Lay out `batches` for a frame seen from `eye` with the constants `scene` gives.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &FrameBatches<'_>,
        scene: &EngineConstants,
        eye: Vec3,
    ) {
        self.sprites
            .prepare(device, queue, batches.sprites, scene, eye);
        self.trails
            .prepare(device, queue, batches.trails, scene, eye);
        self.models.prepare(device, queue, batches.models, scene);
    }
}

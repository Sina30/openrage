//! The trails of particle batches as the preview lays them out for `particle_trails`, which draws them with the game's own passes.

use crate::particles::ParticleBlendMode;
use std::sync::Arc;

pub const MAX_TRAIL_CONTROL_POINTS: usize = 64;

/// Ceiling on a `ptxd_Trail` behaviour's `tessellation_u` / `tessellation_v`.
///
/// ⚠ Both are plain `u32`s read straight out of a `.ypt` behaviour tail, and the
/// builder turns them into loop bounds and `Vec::with_capacity` arguments
/// **every frame**, so without a ceiling `0xFFFFFFFF` in `tessellation_u` asks
/// for roughly 1.7e10 tessellated nodes per chain. The engine clamps them too.
///
/// Measured over the staged `.ypt` corpus (`core.ypt`, `cut_paletoscore`,
/// `scr_bigscore`, `scr_paletoscore` and both gen9 files), 85 `DrawTrail`
/// behaviours in total: `tessellation_u` takes the values {1, 2, 3, 4} and
/// `tessellation_v` the values {1, 2, 3, 4, 6}. So 64 is more than ten times
/// the largest value the game itself ships.
pub const MAX_TRAIL_TESSELLATION: u32 = 64;

/// A vertex of a trail's strip.
#[derive(Clone, Copy, Debug, Default)]
pub struct TrailVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

pub struct TrailBatch {
    pub blend: ParticleBlendMode,
    pub vertices: Vec<TrailVertex>,
    /// The values of the rule's vector and keyframe shader variables this frame, by the hash of each one's name, which the pass's constants of that name take.
    pub constants: Vec<(u32, [f32; 4])>,
    /// The technique of `ptfx_trail` the rule names, which the game's own passes draw the batch with; none draws nothing there.
    pub shader_technique: String,
    /// The decoded diffuse texture the game's own passes bind, white where there is none.
    pub diffuse_image: Option<Arc<crate::game_shaders::mesh::Image>>,
}

pub fn trail_tessellation_enabled(tessellation_v: u32) -> bool {
    tessellation_v >= 2
}

//! The light the particles of an effect give: each one a sphere drawn by the game's own point light pass over the frame the sun lit.

use glam::Vec3;

use super::deferred::Inputs;
use super::light_volumes::LightVolumes;
use super::pack::GamePack;
use super::screen::ScreenError;

const EFFECT: &str = "point";
const TECHNIQUE: &str = "pointCM";

/// A light that shines the same all round, from a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    pub colour: Vec3,
    pub intensity: f32,
    /// How far from the position it reaches.
    pub radius: f32,
    /// How fast it fades out to the radius.
    pub falloff: f32,
}

/// What the input `semantic` of the vertex stage reads of a light.
fn light_value(semantic: &str, index: u32, light: &PointLight) -> [f32; 4] {
    let (p, c) = (light.position, light.colour);
    match (semantic, index) {
        ("TEXCOORD", 0) => [p.x, p.y, p.z, light.radius],
        ("TEXCOORD", 1) => [light.intensity, light.falloff, 0.0, 0.0],
        ("COLOR", 0) => [c.x, c.y, c.z, 1.0],
        _ => [0.0; 4],
    }
}

/// The light of the particles that shines all round, drawn into the frame the sun lit.
pub type PointLights = LightVolumes<PointLight>;

impl PointLights {
    /// The pass, drawn additively into colour targets of `colour`, over what `frame` holds.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        colour: wgpu::TextureFormat,
        frame: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        Self::build(
            device,
            queue,
            pack,
            colour,
            frame,
            EFFECT,
            TECHNIQUE,
            light_value,
        )
    }
}

#[cfg(test)]
mod tests;

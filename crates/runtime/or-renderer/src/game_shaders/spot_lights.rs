//! The light of a particle rule that shines in a cone: each one a volume drawn by the game's own spot light pass over the frame the sun lit.

use glam::Vec3;

use super::deferred::Inputs;
use super::light_volumes::LightVolumes;
use super::pack::GamePack;
use super::screen::ScreenError;

const EFFECT: &str = "spot";
const TECHNIQUE: &str = "spotCM";

/// The least and the most an outer angle of the cone is, in degrees, and the inner angle's least.
const LEAST_ANGLE: f32 = 1.0;
const MOST_ANGLE: f32 = 89.0;

/// A light that shines in a cone: from a point, along an axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotLight {
    pub position: Vec3,
    /// The way it shines, a unit vector.
    pub direction: Vec3,
    pub colour: Vec3,
    pub intensity: f32,
    /// How far from the position it reaches.
    pub radius: f32,
    /// How fast it fades out to the radius.
    pub falloff: f32,
    /// The angle from the axis to the edge of the cone, in degrees.
    pub outer_degrees: f32,
}

impl SpotLight {
    /// The cosine and the sine of the outer angle, and how steeply the light fades between the inner angle and the outer: what the pass reads of a cone.
    fn cone(&self) -> [f32; 3] {
        let outer = self.outer_degrees.clamp(LEAST_ANGLE, MOST_ANGLE);
        // The inner angle of a particle's light is nought, which the game holds to a degree
        let inner = LEAST_ANGLE.min(outer - 1.0);
        let (cos_outer, cos_inner) = (outer.to_radians().cos(), inner.to_radians().cos());
        [
            cos_outer,
            outer.to_radians().sin(),
            1.0 / (cos_inner - cos_outer).max(1e-6),
        ]
    }
}

/// What the input `semantic` of the vertex stage reads of a light.
fn light_value(semantic: &str, index: u32, light: &SpotLight) -> [f32; 4] {
    let (p, d, c) = (light.position, light.direction, light.colour);
    let [cos, sin, scale] = light.cone();
    match (semantic, index) {
        ("TEXCOORD", 0) => [p.x, p.y, p.z, light.radius],
        ("TEXCOORD", 1) => [d.x, d.y, d.z, light.falloff],
        ("TEXCOORD", 2) => [cos, sin, scale, light.intensity],
        ("COLOR", 0) => [c.x, c.y, c.z, 1.0],
        _ => [0.0; 4],
    }
}

/// The light of the particles that shines in a cone, drawn into the frame the sun lit.
pub type SpotLights = LightVolumes<SpotLight>;

impl SpotLights {
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

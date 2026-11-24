//! Lights that shine all round from a segment: each one a volume drawn by the game's own capsule light pass over the frame the sun lit.

use glam::Vec3;

use super::deferred::Inputs;
use super::light_volumes::LightVolumes;
use super::pack::GamePack;
use super::screen::ScreenError;

const EFFECT: &str = "capsule";
const TECHNIQUE: &str = "capsule";

/// A light that shines the same all round from a segment: lit as from the nearest point of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapsuleLight {
    /// The middle of the segment.
    pub position: Vec3,
    /// Along the segment, a unit vector.
    pub direction: Vec3,
    /// The segment's length.
    pub length: f32,
    pub colour: Vec3,
    pub intensity: f32,
    /// How far from the segment it reaches.
    pub radius: f32,
    /// How fast it fades out to the radius.
    pub falloff: f32,
}

/// What the input `semantic` of the vertex stage reads of a light.
fn light_value(semantic: &str, index: u32, light: &CapsuleLight) -> [f32; 4] {
    // The pass's segment runs from its position along the direction for its extent.
    let (d, c) = (light.direction, light.colour);
    let start = light.position - d * (light.length / 2.0);
    match (semantic, index) {
        ("TEXCOORD", 0) => [start.x, start.y, start.z, light.radius],
        ("TEXCOORD", 1) => [d.x, d.y, d.z, light.falloff],
        ("TEXCOORD", 2) => [light.length, light.intensity, 0.0, 0.0],
        ("COLOR", 0) => [c.x, c.y, c.z, 1.0],
        _ => [0.0; 4],
    }
}

/// The lights that shine from a segment, drawn into the frame the sun lit.
pub type CapsuleLights = LightVolumes<CapsuleLight>;

impl CapsuleLights {
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
mod tests {
    use super::*;

    #[test]
    fn the_pass_is_given_the_segment_from_its_start() {
        let light = CapsuleLight {
            position: Vec3::new(10.0, 0.0, 5.0),
            direction: Vec3::X,
            length: 4.0,
            colour: Vec3::new(0.5, 0.25, 0.125),
            intensity: 7.0,
            radius: 3.0,
            falloff: 2.0,
        };
        assert_eq!(light_value("TEXCOORD", 0, &light), [8.0, 0.0, 5.0, 3.0]);
        assert_eq!(light_value("TEXCOORD", 1, &light), [1.0, 0.0, 0.0, 2.0]);
        assert_eq!(light_value("TEXCOORD", 2, &light), [4.0, 7.0, 0.0, 0.0]);
        assert_eq!(light_value("COLOR", 0, &light), [0.5, 0.25, 0.125, 1.0]);
    }
}

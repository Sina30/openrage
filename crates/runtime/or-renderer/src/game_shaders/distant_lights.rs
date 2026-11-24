//! Lights seen from afar drawn as the glow of their source: each one a camera-facing sprite of the pack's `DistantLights` pass, added into the lit frame where nothing nearer stands.

use std::sync::atomic::{AtomicU32, Ordering};

use glam::Vec3;
use or_io::gta5::shaders::spirv_pack::SignatureElement;
use wgpu::util::DeviceExt;

use super::deferred::{lighting_values, Screen, DEPTH_STENCIL_FORMAT};
use super::engine::EngineConstants;
use super::light_volumes::record_bytes;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::screen::{vertex_inputs, ScreenError, ScreenPass, Space};
use super::stand_in::Provided;

const EFFECT: &str = "im";
const TECHNIQUE: &str = "DistantLights";
/// The pass that measures the distance in three dimensions.
const PASS: usize = 0;

/// How many sprites are drawn at once: more than the distant lights the lists around a camera over the city hold.
pub const MOST: usize = 1 << 18;

/// The side of the texture a sprite's shape is read from, in texels.
const DOT: u32 = 32;

/// One distant light: where it stands, its colour, and what the pass multiplies the colour by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistantLight {
    pub position: Vec3,
    pub colour: Vec3,
    /// The sprite's radiance at its middle, in the frame's units, before the pass's own distance terms.
    pub strength: f32,
}

/// How the sprites are sized: their side in metres, and how much it grows a metre along the view, past where that makes it larger.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpriteSize {
    pub side: f32,
    pub per_metre: f32,
}

/// What the pass multiplies a sprite `distance` metres from the eye by, on top of its strength: one to 500 m, rising to sixteen at 2,500 m.
pub fn distance_gain(distance: f32) -> f32 {
    1.0 + 15.0 * ((distance - 500.0) * 0.0005).clamp(0.0, 1.0)
}

/// The texel at `x, y` of the shape a sprite is drawn with: the pass takes its red to the fourth power, which falls from one in the middle to nought at the edge of the circle the square holds.
fn dot_texel(x: u32, y: u32) -> u8 {
    let at = |i: u32| (i as f32 + 0.5) / DOT as f32 * 2.0 - 1.0;
    let r = (at(x).hypot(at(y))).min(1.0);
    ((1.0 - r) * 255.0).round() as u8
}

/// How much of a sprite's square its shape fills, as the pass reads it: the mean of the red to the fourth power.
pub fn dot_coverage() -> f32 {
    let mut sum = 0.0;
    for y in 0..DOT {
        for x in 0..DOT {
            sum += (f32::from(dot_texel(x, y)) / 255.0).powi(4);
        }
    }
    sum / (DOT * DOT) as f32
}

fn dot() -> Provided {
    let rgba = (0..DOT * DOT)
        .flat_map(|i| {
            let v = dot_texel(i % DOT, i / DOT);
            [v, v, v, 255]
        })
        .collect();
    Provided::Texture2d {
        width: DOT,
        height: DOT,
        rgba,
        srgb: false,
    }
}

/// What the input `semantic` of the vertex stage reads of a light.
fn light_value(semantic: &str, light: &DistantLight) -> [f32; 4] {
    let (p, c) = (light.position, light.colour);
    match semantic {
        "POSITION" => [p.x, p.y, p.z, 1.0],
        "COLOR" => [c.x, c.y, c.z, light.strength],
        _ => [0.0; 4],
    }
}

/// The six corners of a sprite, the pass laying each at its texture coordinate less a half.
const CORNERS: [[f32; 2]; 6] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
];

/// The sprites of the distant lights, drawn into the lit frame.
pub struct DistantLights {
    pass: ScreenPass,
    inputs: Vec<SignatureElement>,
    corners: wgpu::Buffer,
    instances: wgpu::Buffer,
    drawn: AtomicU32,
}

impl DistantLights {
    /// The pass, drawn additively into colour targets of `colour`, tested against the depth of the frame and writing none.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        colour: wgpu::TextureFormat,
    ) -> Result<Self, ScreenError> {
        let inputs =
            vertex_inputs(pack, EFFECT, TECHNIQUE, PASS).ok_or_else(|| ScreenError::NoPass {
                effect: EFFECT.into(),
                technique: TECHNIQUE.into(),
                pass: PASS,
            })?;
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let target = Target {
            colour,
            depth: DEPTH_STENCIL_FORMAT,
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            blend: Some(wgpu::BlendState {
                color: add,
                alpha: add,
            }),
            colour_attachments: Some(1),
        };
        let draw = DrawState {
            instance_locations: (inputs.iter())
                .filter(|i| i.semantic != "TEXCOORD")
                .map(|i| i.register)
                .collect(),
            depth_attachment: true,
            depth_tested: true,
            depth_write: Some(false),
            write_mask: Some(wgpu::ColorWrites::COLOR),
            ..DrawState::default()
        };
        let pass = ScreenPass::new(
            device,
            queue,
            pack,
            EFFECT,
            TECHNIQUE,
            PASS,
            &target,
            &draw,
            Space::Unit,
            &mut |name, _| match name {
                "DiffuseSampler" => Some(dot()),
                _ => None,
            },
        )?;
        let streams = pass.streams();
        let corners: Vec<u8> =
            (CORNERS.iter())
                .flat_map(|&[u, v]| {
                    record_bytes(&streams.vertex, streams.vertex_stride, &inputs, &|input| {
                        match input.semantic.as_str() {
                            "TEXCOORD" => [u, v, 0.0, 0.0],
                            _ => [0.0; 4],
                        }
                    })
                })
                .collect();
        let corners = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("distant-light-corners"),
            contents: &corners,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("distant-lights"),
            size: streams.instance_stride.max(4) * MOST as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            pass,
            inputs,
            corners,
            instances,
            drawn: AtomicU32::new(0),
        })
    }

    /// Write the constants of this frame and the first [`MOST`] of `lights`, sized as `size` says.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        size: SpriteSize,
        lights: &[DistantLight],
    ) {
        let lights = &lights[..lights.len().min(MOST)];
        self.drawn.store(lights.len() as u32, Ordering::Relaxed);
        if lights.is_empty() {
            return;
        }
        let mut values = engine.values();
        values.extend(lighting_values(engine, screen));
        // The pass's own fade by distance is off, and every light is drawn from where it stands: the caller fades each one.
        values.insert("refMipBlurParams", vec![0.0, 0.0, 1.0, 0.0]);
        values.insert("GeneralParams0", vec![size.side, 0.0, 0.0, 1.0]);
        // The noise is a white stand-in, so a twinkle of one takes nothing away.
        values.insert("GeneralParams1", vec![size.per_metre, 1.0, 0.0, 1.0]);
        self.pass.write(queue, &values);
        let streams = self.pass.streams();
        let stride = streams.instance_stride as usize;
        // Each attribute's place in a record and the semantic it reads, found once.
        let slots: Vec<(usize, usize, &str)> = (streams.instance.iter())
            .filter_map(|a| {
                let input = (self.inputs.iter()).find(|i| i.register == a.shader_location)?;
                Some((
                    a.offset as usize,
                    (a.format.size() / 4) as usize,
                    input.semantic.as_str(),
                ))
            })
            .collect();
        let mut bytes = vec![0u8; stride * lights.len()];
        for (record, light) in bytes.chunks_exact_mut(stride.max(1)).zip(lights) {
            for &(offset, words, semantic) in &slots {
                let values = light_value(semantic, light);
                for (i, v) in values.iter().take(words).enumerate() {
                    let at = offset + 4 * i;
                    record[at..at + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        queue.write_buffer(&self.instances, 0, &bytes);
    }

    /// Draw the lights the frame was written with into `pass`: its colour target of the format this was built for, and the depth of the frame, which it only reads.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        let drawn = self.drawn.load(Ordering::Relaxed);
        if drawn == 0 {
            return;
        }
        self.pass.record_instanced(
            pass,
            &self.corners,
            CORNERS.len() as u32,
            &self.instances,
            drawn,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shape_is_a_disc_bright_in_the_middle_and_dark_at_its_edge() {
        let middle = dot_texel(DOT / 2, DOT / 2);
        assert!(middle > 240, "{middle}");
        assert_eq!(dot_texel(0, 0), 0);
        // The fourth power of a cone over a disc fills a sixtieth of pi of its square.
        let coverage = dot_coverage();
        let exact = std::f32::consts::PI / 60.0;
        assert!((coverage - exact).abs() < 0.1 * exact, "{coverage} {exact}");
    }

    #[test]
    fn a_light_gives_the_pass_its_place_and_its_colour_times_its_strength() {
        let light = DistantLight {
            position: Vec3::new(1.0, 2.0, 3.0),
            colour: Vec3::new(0.5, 0.25, 1.0),
            strength: 8.0,
        };
        assert_eq!(light_value("POSITION", &light), [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(light_value("COLOR", &light), [0.5, 0.25, 1.0, 8.0]);
        assert_eq!(light_value("TEXCOORD", &light), [0.0; 4]);
    }
}

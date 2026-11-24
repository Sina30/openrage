//! The glow of a light's source: each corona a camera-facing quad of the pack's `corona` pass, which hides it as far as the depth of the frame stands nearer than it about its middle, added into the lit frame.

use std::sync::atomic::{AtomicU32, Ordering};

use glam::Vec3;
use or_io::gta5::shaders::spirv_pack::SignatureElement;

use super::deferred::{lighting_values, Screen, DEPTH_STENCIL_FORMAT};
use super::distant_lights::dot;
use super::engine::EngineConstants;
use super::light_volumes::record_bytes;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::screen::{vertex_inputs, ScreenError, ScreenPass, Space};
use super::stand_in::Provided;

const EFFECT: &str = "im";
const TECHNIQUE: &str = "corona";
/// The pass that tests the depth of the frame and fades a corona the camera stands in.
const PASS: usize = 0;

/// How many coronas are drawn at once: more than the placed lights about a camera state.
pub const MOST: usize = 8192;

/// The corners of a quad, as two triangles: across the camera and up it, each from minus one to one.
const CORNERS: [[f32; 2]; 6] = [
    [-1.0, 1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [1.0, -1.0],
    [-1.0, -1.0],
];

/// One corona as the frame draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corona {
    pub position: Vec3,
    /// Half its side, in metres.
    pub size: f32,
    pub colour: Vec3,
    /// Its radiance at its middle, in the frame's units.
    pub intensity: f32,
}

/// What the pass's constants say of every corona: how wide about its middle the depth is read, in the share of its size on the screen and at most in pixels, and how near the edge of the screen it starts to fade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoronaLook {
    pub occlusion_share: f32,
    pub occlusion_pixels: f32,
    pub screen_edge: f32,
}

/// What the input `semantic` of the vertex stage reads at `corner` of `corona`, whose quad spans `right` and `up`.
fn corner_value(
    semantic: &str,
    corona: &Corona,
    [x, y]: [f32; 2],
    (right, up): (Vec3, Vec3),
) -> [f32; 4] {
    let (p, c) = (corona.position, corona.colour);
    let at = p + (right * x + up * y) * corona.size;
    let (u, v) = ((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    match semantic {
        "POSITION" => [at.x, at.y, at.z, 1.0],
        // The pass reads the texture's second coordinate from the colour's alpha, and the intensity where it would be
        "COLOR" => [c.x, c.y, c.z, v],
        "TEXCOORD" => [u, corona.intensity, 0.0, 0.0],
        "NORMAL" => [p.x, p.y, p.z, 1.0],
        _ => [0.0; 4],
    }
}

/// The coronas of a frame, drawn into the lit frame.
pub struct Coronas {
    pass: ScreenPass,
    inputs: Vec<SignatureElement>,
    vertices: wgpu::Buffer,
    drawn: AtomicU32,
}

impl Coronas {
    /// The pass, drawn additively into colour targets of `colour`, reading the depth of the frame from `depth`, in distances along the view.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        colour: wgpu::TextureFormat,
        depth: &wgpu::TextureView,
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
            depth_compare: wgpu::CompareFunction::Always,
            blend: Some(wgpu::BlendState {
                color: add,
                alpha: add,
            }),
            colour_attachments: Some(1),
        };
        // The pass tests the depth itself, so the frame's depth target is only carried
        let draw = DrawState {
            depth_attachment: true,
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
            &mut Self::provide(depth),
        )?;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("coronas"),
            size: pass.streams().vertex_stride.max(4) * (MOST * CORNERS.len()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            pass,
            inputs,
            vertices,
            drawn: AtomicU32::new(0),
        })
    }

    fn provide(
        depth: &wgpu::TextureView,
    ) -> impl FnMut(&str, &super::bindings::Requirement) -> Option<Provided> + '_ {
        use super::bindings::Resource;
        move |name, requirement| match (name, &requirement.resource) {
            (_, Resource::Sampler { .. }) => None,
            ("CoronasDepthMapSampler", _) => Some(Provided::View(depth.clone())),
            ("DiffuseSampler", _) => Some(dot()),
            _ => None,
        }
    }

    /// Read the depth of the frame from `depth` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, depth: &wgpu::TextureView) {
        self.pass.bind(device, queue, &mut Self::provide(depth));
    }

    /// Write the constants of this frame and the first [`MOST`] of `coronas`.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        (look, coronas): (CoronaLook, &[Corona]),
    ) {
        let coronas = &coronas[..coronas.len().min(MOST)];
        self.drawn.store(coronas.len() as u32, Ordering::Relaxed);
        if coronas.is_empty() {
            return;
        }
        let mut values = engine.values();
        values.extend(lighting_values(engine, screen));
        values.insert("refMipBlurParams", vec![look.screen_edge, 0.0, 0.0, 0.0]);
        values.insert(
            "GeneralParams0",
            vec![look.occlusion_share, look.occlusion_pixels, 0.0, 0.0],
        );
        values.insert("GeneralParams1", vec![0.0; 4]);
        self.pass.write(queue, &values);
        let camera = engine.view.inverse();
        let axes = (camera.x_axis.truncate(), camera.y_axis.truncate());
        let streams = self.pass.streams();
        let mut bytes =
            Vec::with_capacity(coronas.len() * CORNERS.len() * streams.vertex_stride as usize);
        for corona in coronas {
            for corner in CORNERS {
                bytes.extend(record_bytes(
                    &streams.vertex,
                    streams.vertex_stride,
                    &self.inputs,
                    &|input| corner_value(&input.semantic, corona, corner, axes),
                ));
            }
        }
        queue.write_buffer(&self.vertices, 0, &bytes);
    }

    /// Draw the coronas the frame was written with into `pass`: the lit frame, with the depth target of the frame, which it does not use.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        let drawn = self.drawn.load(Ordering::Relaxed);
        if drawn > 0 {
            (self.pass).record_mesh(pass, &self.vertices, drawn * CORNERS.len() as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_lies_its_size_across_and_up_from_the_middle_and_carries_the_middle_and_intensity() {
        let corona = Corona {
            position: Vec3::new(10.0, 20.0, 30.0),
            size: 2.0,
            colour: Vec3::new(1.0, 0.5, 0.25),
            intensity: 7.0,
        };
        let axes = (Vec3::X, Vec3::Z);
        assert_eq!(
            corner_value("POSITION", &corona, [1.0, -1.0], axes),
            [12.0, 20.0, 28.0, 1.0]
        );
        assert_eq!(
            corner_value("COLOR", &corona, [1.0, 1.0], axes),
            [1.0, 0.5, 0.25, 1.0]
        );
        assert_eq!(
            corner_value("TEXCOORD", &corona, [-1.0, 1.0], axes),
            [0.0, 7.0, 0.0, 0.0]
        );
        assert_eq!(
            corner_value("NORMAL", &corona, [-1.0, -1.0], axes),
            [10.0, 20.0, 30.0, 1.0]
        );
    }
}

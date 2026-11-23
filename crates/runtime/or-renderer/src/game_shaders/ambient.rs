//! The game's screen-space ambient occlusion: the depth of the frame at half size, how much of what is around each pixel stands in front of it, blurred where depth is alike and laid over the ambient scales of the G-buffer.

use std::collections::BTreeMap;

use super::bindings::{Requirement, Resource};
use super::deferred::{lighting_values, Inputs, Screen, DEPTH_STENCIL_FORMAT, GBUFFER_FORMAT};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::samplers::{linear, point};
use super::screen::{ScreenError, ScreenPass, Space, Written};
use super::stand_in::Provided;
use super::targets::render_target;

const EFFECT: &str = "ssao";
const TECHNIQUE: &str = "SSAO";
/// The passes of the technique, by what they do.
const DOWNSCALE: usize = 0;
const UPSCALE: usize = 4;
const PROCESS: usize = 7;
const BLUR_X: usize = 8;
const BLUR_Y: usize = 9;

/// The format of the half-size depth, in distances along the view.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// The format of the occlusion, which is one at a pixel nothing stands before.
const OCCLUSION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// How far the pixels around one are looked at, in pixels of the whole frame.
const REACH: f32 = 32.0;

/// How strong the time of day's setting must be for the pass to run at all.
const LEAST_STRENGTH: f32 = 0.01;

/// What the keyframe's setting is multiplied by to make the exponent of the occlusion.
pub const STRENGTH_PER_SETTING: f32 = 0.25;

/// The size of a target that holds half of what `size` pixels do.
fn half(size: u32) -> u32 {
    size.div_ceil(2).max(1)
}

/// A target of half the size of the frame.
fn half_target(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    render_target(device, label, (half(width), half(height)), format, usage)
}

/// The half-size targets the passes hand each other: the depth, and two of occlusion that the blurs pass between them.
struct Targets {
    depth: wgpu::TextureView,
    first: wgpu::TextureView,
    second: wgpu::TextureView,
}

impl Targets {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            // The depth can be copied out, to read what the pass wrote
            depth: half_target(
                device,
                "ssao-depth",
                size,
                DEPTH_FORMAT,
                wgpu::TextureUsages::COPY_SRC,
            ),
            first: half_target(
                device,
                "ssao-occlusion",
                size,
                OCCLUSION_FORMAT,
                wgpu::TextureUsages::empty(),
            ),
            second: half_target(
                device,
                "ssao-blur",
                size,
                OCCLUSION_FORMAT,
                wgpu::TextureUsages::empty(),
            ),
        }
    }
}

/// Which of the passes a resource is bound for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Downscale,
    Process,
    BlurX,
    BlurY,
    Upscale,
}

/// What the pass of `stage` binds under `name`: what it reads, with the sampler that reads it, or nothing.
fn provide(
    device: &wgpu::Device,
    stage: Stage,
    inputs: &Inputs<'_>,
    targets: &Targets,
    name: &str,
    requirement: &Requirement,
) -> Option<Provided> {
    use wgpu::FilterMode::{Linear, Nearest};
    let (view, filter) = match (stage, name) {
        (Stage::Downscale, "GBufferTextureSamplerDepth") => (&inputs.depth.depth, Nearest),
        (Stage::Process | Stage::BlurX | Stage::BlurY, "PointSampler2") => {
            (&targets.depth, Nearest)
        }
        (Stage::BlurX, "gDeferredLightSampler0P") => (&targets.first, Nearest),
        (Stage::BlurY, "gDeferredLightSampler0P") => (&targets.second, Nearest),
        (Stage::Upscale, "gDeferredLightSampler2") => (&targets.first, Linear),
        _ => return None,
    };
    Some(match requirement.resource {
        Resource::Sampler { .. } => Provided::Sampler(match filter {
            Nearest => point(device),
            Linear => linear(device),
        }),
        _ => Provided::View(view.clone()),
    })
}

/// The values of the variables the passes read, by the names the game gives them.
fn values(
    engine: &EngineConstants,
    screen: &Screen,
    strength: f32,
) -> BTreeMap<&'static str, Vec<f32>> {
    let mut values = lighting_values(engine, screen);
    let projection = values["deferredProjectionParams"].clone();
    let (x, y) = (
        2.0 / screen.width.max(1) as f32,
        2.0 / screen.height.max(1) as f32,
    );
    let (reach_x, reach_y) = (x * REACH, y * REACH);
    values.extend([
        ("g_projParams", projection.clone()),
        (
            "gNormalOffset",
            vec![x * projection[0], 0.0, 0.0, y * projection[1]],
        ),
        (
            "gOffsetScale0",
            vec![
                0.75 * reach_x,
                0.75 * reach_y,
                -0.25 * reach_x,
                -0.25 * reach_y,
            ],
        ),
        (
            "gOffsetScale1",
            vec![reach_x, -reach_y, -0.5 * reach_x, 0.5 * reach_y],
        ),
        ("g_SSAOStrength", vec![strength]),
    ]);
    values
}

/// Whether a time of day's `strength` is one the pass runs at.
pub fn runs_at(strength: f32) -> bool {
    strength >= LEAST_STRENGTH
}

/// The game's ambient occlusion over a G-buffer: it darkens the ambient light a surface takes, where other surfaces stand close around it.
pub struct AmbientPass {
    downscale: ScreenPass,
    process: ScreenPass,
    blur_x: ScreenPass,
    blur_y: ScreenPass,
    upscale: ScreenPass,
    targets: Targets,
    /// The target of the G-buffer that holds the scales of the ambient light, which the occlusion multiplies.
    scales: wgpu::TextureView,
    /// What the constants were written for: they do not move with the camera.
    written: Written<([u32; 2], [f32; 4], f32)>,
}

impl AmbientPass {
    /// The passes, over the G-buffer and depth `inputs` hold.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        inputs: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let targets = Targets::new(device, inputs.gbuffer.size());
        let target = |colour, blend| Target {
            colour,
            depth: DEPTH_STENCIL_FORMAT,
            depth_compare: wgpu::CompareFunction::Always,
            blend,
            colour_attachments: Some(1),
        };
        let build = |stage: Stage, pass: usize, target: &Target, draw: &DrawState| {
            ScreenPass::new(
                device,
                queue,
                pack,
                EFFECT,
                TECHNIQUE,
                pass,
                target,
                draw,
                Space::Clip,
                &mut |name, requirement| {
                    provide(device, stage, inputs, &targets, name, requirement)
                },
            )
        };
        let plain = DrawState::default();
        // The depth of the frame is a depth texture, which only an unfiltered binding takes
        let over_depth = DrawState {
            unfiltered: true,
            ..DrawState::default()
        };
        // The occlusion scales the two ambient scales it is drawn over, and leaves the rest of what they sit in
        let product = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::SrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let over_scales = DrawState {
            write_mask: Some(wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN),
            ..DrawState::default()
        };
        Ok(Self {
            downscale: build(
                Stage::Downscale,
                DOWNSCALE,
                &target(DEPTH_FORMAT, None),
                &over_depth,
            )?,
            process: build(
                Stage::Process,
                PROCESS,
                &target(OCCLUSION_FORMAT, None),
                &plain,
            )?,
            blur_x: build(
                Stage::BlurX,
                BLUR_X,
                &target(OCCLUSION_FORMAT, None),
                &plain,
            )?,
            blur_y: build(
                Stage::BlurY,
                BLUR_Y,
                &target(OCCLUSION_FORMAT, None),
                &plain,
            )?,
            upscale: build(
                Stage::Upscale,
                UPSCALE,
                &target(GBUFFER_FORMAT, Some(product)),
                &over_scales,
            )?,
            targets,
            scales: inputs.gbuffer.ambient_scales().clone(),
            written: Written::default(),
        })
    }

    /// Read `inputs` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, inputs: &Inputs<'_>) {
        self.written.reset();
        self.targets = Targets::new(device, inputs.gbuffer.size());
        self.scales = inputs.gbuffer.ambient_scales().clone();
        let targets = &self.targets;
        for (stage, pass) in [
            (Stage::Downscale, &mut self.downscale),
            (Stage::Process, &mut self.process),
            (Stage::BlurX, &mut self.blur_x),
            (Stage::BlurY, &mut self.blur_y),
            (Stage::Upscale, &mut self.upscale),
        ] {
            pass.bind(device, queue, &mut |name, requirement| {
                provide(device, stage, inputs, targets, name, requirement)
            });
        }
    }

    /// The half-size depth of the frame, in distances along the view, which the downscale pass writes.
    pub fn depth(&self) -> &wgpu::TextureView {
        &self.targets.depth
    }

    /// The textures of the half-size targets, for the bytes they hold.
    pub fn textures(&self) -> impl Iterator<Item = &wgpu::Texture> {
        [
            &self.targets.depth,
            &self.targets.first,
            &self.targets.second,
        ]
        .into_iter()
        .map(wgpu::TextureView::texture)
    }

    /// Write the constants of this frame to every pass, for an occlusion of `strength`.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        strength: f32,
    ) {
        let values = values(engine, screen, strength);
        let projection = &values["g_projParams"];
        let key = (
            [screen.width, screen.height],
            [projection[0], projection[1], projection[2], projection[3]],
            strength,
        );
        if !self.written.changed(key) {
            return;
        }
        for pass in [
            &self.downscale,
            &self.process,
            &self.blur_x,
            &self.blur_y,
            &self.upscale,
        ] {
            pass.write(queue, &values);
        }
    }

    /// Work out the occlusion of the frame and multiply the ambient scales of the G-buffer by it, where `strength` is one the passes run at; with `depth_wanted` the depth is made where they do not run.
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, strength: f32, depth_wanted: bool) {
        let occludes = runs_at(strength);
        if !occludes && !depth_wanted {
            return;
        }
        let steps = [
            ("ssao-downscale", &self.targets.depth, &self.downscale),
            ("ssao-process", &self.targets.first, &self.process),
            ("ssao-blur-x", &self.targets.second, &self.blur_x),
            ("ssao-blur-y", &self.targets.first, &self.blur_y),
        ];
        let made = if occludes { steps.len() } else { 1 };
        for (label, view, pass) in steps.into_iter().take(made) {
            let mut pass_in = begin(
                encoder,
                label,
                view,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            pass.record(&mut pass_in);
        }
        if occludes {
            let mut pass = begin(encoder, "ssao-upscale", &self.scales, wgpu::LoadOp::Load);
            self.upscale.record(&mut pass);
        }
    }
}

/// A pass that draws into the one colour target `view`.
fn begin<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &'static str,
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests;

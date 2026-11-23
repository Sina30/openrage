//! The game's anti-aliasing pass over the frame mapped to the screen: it reads each pixel and those around it, by the luminance the mapping left in alpha, and blends the pixels along an edge.

use super::bindings::{Requirement, Resource};
use super::deferred::{lighting_values, Inputs, Screen, DEPTH_STENCIL_FORMAT};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::samplers::linear;
use super::screen::{unit_quad, ScreenError, ScreenPass, Space};
use super::stand_in::Provided;

const EFFECT: &str = "postfx";
const TECHNIQUE: &str = "PostFx";
/// The pass that reads the whole frame by a quad: the game's anti-aliasing, which the pass beside it does with a gather.
const FXAA: usize = 98;

/// What the game tunes the pass by: how much of a lone bright pixel is blended away, the contrast an edge must have to be blended, and the darkest it may be to count at all.
const SUBPIX: f32 = 0.317;
const EDGE_THRESHOLD: f32 = 0.333;
const EDGE_THRESHOLD_MIN: f32 = 0.0833;

/// The game's anti-aliasing pass over the whole of a frame already mapped to the screen.
pub struct AntiAliasPass {
    screen: ScreenPass,
}

impl AntiAliasPass {
    /// The pass, drawn into colour targets of `display`, over the mapped frame `inputs` hold.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        display: wgpu::TextureFormat,
        inputs: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let target = Target {
            colour: display,
            depth: DEPTH_STENCIL_FORMAT,
            depth_compare: wgpu::CompareFunction::Always,
            blend: None,
            colour_attachments: Some(1),
        };
        let screen = ScreenPass::new(
            device,
            queue,
            pack,
            EFFECT,
            TECHNIQUE,
            FXAA,
            &target,
            // The alpha of the mapped frame is the luminance the pass reads, and the screen keeps the one it was cleared to
            &DrawState {
                write_mask: Some(wgpu::ColorWrites::COLOR),
                ..DrawState::default()
            },
            Space::Unit,
            &mut |name, requirement| provide(device, inputs, name, requirement),
        )?;
        Ok(Self { screen })
    }

    /// Read `inputs` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, inputs: &Inputs<'_>) {
        self.screen.bind(device, queue, &mut |name, requirement| {
            provide(device, inputs, name, requirement)
        });
    }

    /// Write the constants of this frame.
    pub fn write(&self, queue: &wgpu::Queue, engine: &EngineConstants, screen: &Screen) {
        let mut values = engine.values();
        values.extend(lighting_values(engine, screen));
        values.extend(unit_quad());
        values.extend([
            (
                "rcpFrame",
                vec![
                    1.0 / screen.width.max(1) as f32,
                    1.0 / screen.height.max(1) as f32,
                ],
            ),
            ("QualitySubpix", vec![SUBPIX]),
            ("EdgeThreshold", vec![EDGE_THRESHOLD]),
            ("EdgeThresholdMin", vec![EDGE_THRESHOLD_MIN]),
        ]);
        self.screen.write(queue, &values);
    }

    /// Draw the pass into `pass`: the target it was built for, with no depth.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.screen.record(pass);
    }
}

/// What the pass binds under `name`: the mapped frame, with the sampler that reads it, or nothing.
fn provide(
    device: &wgpu::Device,
    inputs: &Inputs<'_>,
    name: &str,
    requirement: &Requirement,
) -> Option<Provided> {
    match (&requirement.resource, name) {
        (Resource::Texture { .. }, "BackBufferTexture") => Some(Provided::View(inputs.ldr.clone())),
        (Resource::Sampler { .. }, "BackBufferSampler") => Some(Provided::Sampler(linear(device))),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

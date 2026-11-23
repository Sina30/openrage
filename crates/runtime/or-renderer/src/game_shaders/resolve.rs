//! The game's simple post-processing pass over a lit frame: the exposure, the filmic curve and the colour correction that bring the light to the screen.

use super::bindings::{Requirement, Resource};
use super::deferred::{
    lighting_values, over_what_was_drawn, over_what_was_not_drawn, Inputs, Screen,
    DEPTH_STENCIL_FORMAT,
};
use super::engine::EngineConstants;
use super::mesh::scene::MATERIAL_CLEAR;
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::samplers::{linear, point};
use super::screen::{unit_quad, ScreenError, ScreenPass, Space, Written};
use super::stand_in::Provided;
use super::tone::Tone;

/// The effect, the technique and the pass that map a frame to the screen with the one exposure and one curve: the simplified post-processing pass.
const EFFECT: &str = "postfx";
const TECHNIQUE: &str = "PostFx";
const SIMPLE: usize = 65;

/// The format of the texel the passes read the exposure from.
const ADAPTATION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The lit frame mapped to the screen by the game's own pass, where something was drawn.
pub struct ResolvePass {
    screen: ScreenPass,
    /// The same pass over the pixels nothing was drawn at, added to what the target holds there, which is how the bloom shows round the edge of what was drawn.
    halo: ScreenPass,
    /// The same pass over the pixels nothing was drawn at, replacing what the target holds there, which is how the sky is mapped to the screen.
    background: ScreenPass,
    adaptation: wgpu::Texture,
    adaptation_view: wgpu::TextureView,
    /// The exposure the texel holds.
    exposure: Written<[f32; 2]>,
}

impl ResolvePass {
    /// The pass, drawn into colour targets of `display`, over the lit frame `inputs` hold.
    ///
    /// The pass leaves the luminance of each pixel in its alpha, for the anti-aliasing to read; where `to_screen` says the target is the screen, it writes colour only, so that the screen stays as opaque as it was cleared.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        display: wgpu::TextureFormat,
        to_screen: bool,
        inputs: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let adaptation = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("exposure"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ADAPTATION_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let adaptation_view = adaptation.create_view(&Default::default());
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
            SIMPLE,
            &target,
            &DrawState {
                write_mask: to_screen.then_some(wgpu::ColorWrites::COLOR),
                ..over_what_was_drawn()
            },
            Space::Unit,
            &mut |name, requirement| provide(device, inputs, &adaptation_view, name, requirement),
        )?;
        let halo = ScreenPass::new(
            device,
            queue,
            pack,
            EFFECT,
            TECHNIQUE,
            SIMPLE,
            &Target {
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                ..target
            },
            &DrawState {
                write_mask: Some(wgpu::ColorWrites::COLOR),
                ..over_what_was_not_drawn()
            },
            Space::Unit,
            &mut |name, requirement| provide(device, inputs, &adaptation_view, name, requirement),
        )?;
        let background = ScreenPass::new(
            device,
            queue,
            pack,
            EFFECT,
            TECHNIQUE,
            SIMPLE,
            &target,
            &DrawState {
                write_mask: to_screen.then_some(wgpu::ColorWrites::COLOR),
                ..over_what_was_not_drawn()
            },
            Space::Unit,
            &mut |name, requirement| provide(device, inputs, &adaptation_view, name, requirement),
        )?;
        Ok(Self {
            screen,
            halo,
            background,
            adaptation,
            adaptation_view,
            exposure: Written::default(),
        })
    }

    /// Read `inputs` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, inputs: &Inputs<'_>) {
        let view = &self.adaptation_view;
        for pass in [&mut self.screen, &mut self.halo, &mut self.background] {
            pass.bind(device, queue, &mut |name, requirement| {
                provide(device, inputs, view, name, requirement)
            });
        }
    }

    /// Write the constants of this frame, and the exposure `tone` gives.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        engine: &EngineConstants,
        screen: &Screen,
        tone: &Tone,
    ) {
        let mut values = engine.values();
        values.extend(lighting_values(engine, screen));
        values.extend(tone.values());
        values.extend(unit_quad());
        self.screen.write(queue, &values);
        self.halo.write(queue, &values);
        self.background.write(queue, &values);
        if !self.exposure.changed(tone.adaptation()) {
            return;
        }
        let texel: Vec<u8> = tone
            .adaptation()
            .into_iter()
            .flat_map(|v| or_io::half::f32_to_half(v).to_le_bytes())
            .collect();
        queue.write_texture(
            self.adaptation.as_image_copy(),
            &texel,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draw the pass into `pass`: the display target it was built for, and the depth and stencil of the frame, which it only reads.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_stencil_reference(MATERIAL_CLEAR);
        self.screen.record(pass);
    }

    /// Draw the frame where nothing was drawn, replacing what the target holds there, in the same pass as [`Self::record`].
    pub fn record_background(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_stencil_reference(MATERIAL_CLEAR);
        self.background.record(pass);
    }

    /// Draw the light that lies where nothing was drawn, over what the target holds there, in the same pass as [`Self::record`].
    pub fn record_halo(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_stencil_reference(MATERIAL_CLEAR);
        self.halo.record(pass);
    }
}

/// What the pass binds under `name`: the lit frame and the exposure with the samplers that read them, or nothing.
fn provide(
    device: &wgpu::Device,
    inputs: &Inputs<'_>,
    adaptation: &wgpu::TextureView,
    name: &str,
    requirement: &Requirement,
) -> Option<Provided> {
    match (&requirement.resource, name) {
        (Resource::Texture { .. }, "HDRSampler") => Some(Provided::View(inputs.hdr.clone())),
        (Resource::Sampler { .. }, "HDRSampler") => Some(Provided::Sampler(linear(device))),
        (Resource::Texture { .. }, "AdapLumSampler") => Some(Provided::View(adaptation.clone())),
        (Resource::Sampler { .. }, "AdapLumSampler") => Some(Provided::Sampler(point(device))),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

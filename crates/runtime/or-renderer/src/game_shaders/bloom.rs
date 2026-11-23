//! The game's bloom: the bright part of the lit frame, blurred at four sizes and put back over the frame it came from.

use super::bindings::{Requirement, Resource};
use super::deferred::{Inputs, Screen, DEPTH_STENCIL_FORMAT, HDR_FORMAT};
use super::pack::GamePack;
use super::pipeline::{DrawState, Target};
use super::samplers::{linear, point};
use super::screen::{unit_quad, ScreenError, ScreenPass, Space, Written};
use super::stand_in::Provided;
use super::targets::render_target;
use super::tone::Tone;

const EFFECT: &str = "postfx";
const TECHNIQUE: &str = "PostFx";
/// The passes of the technique, by what they do: the bright part of the frame, the least of four texels, a copy to another size, the blurs across and down, and the light of one size added to another.
const BRIGHT: usize = 76;
const LEAST: usize = 120;
const COPY: usize = 82;
const ACROSS: usize = 92;
const DOWN: usize = 93;
const ADD: usize = 94;

/// What the bloom adds back to the frame is nothing below this, and then the passes are not run.
const LEAST_INTENSITY: f32 = 0.001;

/// The format of the exposure the bright pass reads, as the resolve holds it.
const ADAPTATION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// A target a step reads or writes: the frame itself, or one of the buffers of the bloom, at half, a quarter, an eighth or a sixteenth of the frame, each with a scratch one beside it for the blurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Frame,
    Bloom,
    Half,
    HalfScratch,
    Quarter,
    QuarterScratch,
    Eighth,
    EighthScratch,
    Sixteenth,
    SixteenthScratch,
}

impl Slot {
    const BUFFERS: [Slot; 9] = [
        Slot::Bloom,
        Slot::Half,
        Slot::HalfScratch,
        Slot::Quarter,
        Slot::QuarterScratch,
        Slot::Eighth,
        Slot::EighthScratch,
        Slot::Sixteenth,
        Slot::SixteenthScratch,
    ];

    /// How many times smaller than the frame it is on a side.
    fn divisor(self) -> u32 {
        match self {
            Slot::Frame => 1,
            Slot::Bloom | Slot::Half | Slot::HalfScratch => 2,
            Slot::Quarter | Slot::QuarterScratch => 4,
            Slot::Eighth | Slot::EighthScratch => 8,
            Slot::Sixteenth | Slot::SixteenthScratch => 16,
        }
    }

    fn size(self, (width, height): (u32, u32)) -> (u32, u32) {
        let d = self.divisor();
        ((width / d).max(1), (height / d).max(1))
    }

    fn index(self) -> usize {
        Self::BUFFERS
            .iter()
            .position(|s| *s == self)
            .expect("a buffer of the bloom")
    }
}

/// How much of one size of blur is added to the next, and what the last of them adds to the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Amount {
    Large,
    Medium,
    Small,
    Whole,
}

/// What a step does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Bright,
    Least,
    Copy,
    Across,
    Down,
    Add(Amount),
}

/// The steps from the lit frame to the light put back over it, in order, each reading one slot and writing another.
fn plan() -> Vec<(Kind, Slot, Slot)> {
    use Kind::*;
    use Slot::*;
    let mut steps = vec![
        (Bright, Frame, Half),
        (Least, Half, Bloom),
        (Copy, Bloom, Half),
    ];
    let blur = |steps: &mut Vec<_>, slot, scratch, times| {
        for _ in 0..times {
            steps.push((Across, slot, scratch));
            steps.push((Down, scratch, slot));
        }
    };
    blur(&mut steps, Half, HalfScratch, 1);
    steps.push((Copy, Half, Quarter));
    blur(&mut steps, Quarter, QuarterScratch, 1);
    steps.push((Copy, Quarter, Eighth));
    blur(&mut steps, Eighth, EighthScratch, 2);
    steps.push((Copy, Eighth, Sixteenth));
    blur(&mut steps, Sixteenth, SixteenthScratch, 2);
    // The blurs are added back up, the largest first, and the sum is blurred once more at the half size
    steps.push((Add(Amount::Large), Sixteenth, Eighth));
    steps.push((Add(Amount::Medium), Eighth, Quarter));
    steps.push((Add(Amount::Small), Quarter, Half));
    blur(&mut steps, Half, HalfScratch, 1);
    steps.push((Add(Amount::Small), Half, Bloom));
    steps.push((Add(Amount::Whole), Bloom, Frame));
    steps
}

/// The buffers of the bloom, for one size of frame.
struct Buffers(Vec<wgpu::TextureView>);

impl Buffers {
    fn new(device: &wgpu::Device, frame: (u32, u32)) -> Self {
        Self(
            Slot::BUFFERS
                .iter()
                .map(|slot| {
                    render_target(
                        device,
                        "bloom",
                        slot.size(frame),
                        HDR_FORMAT,
                        wgpu::TextureUsages::empty(),
                    )
                })
                .collect(),
        )
    }

    fn view<'a>(&'a self, slot: Slot, frame: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
        match slot {
            Slot::Frame => frame,
            other => &self.0[other.index()],
        }
    }
}

/// What the pass of a step binds under `name`: the target it reads and the exposure, with the samplers that read them.
fn provide(
    device: &wgpu::Device,
    from: &wgpu::TextureView,
    adaptation: &wgpu::TextureView,
    name: &str,
    requirement: &Requirement,
) -> Option<Provided> {
    match (&requirement.resource, name) {
        (Resource::Texture { .. }, "HDRSampler" | "BloomSampler") => {
            Some(Provided::View(from.clone()))
        }
        (Resource::Sampler { .. }, "HDRSampler" | "BloomSampler") => {
            Some(Provided::Sampler(linear(device)))
        }
        (Resource::Texture { .. }, "AdapLumSampler") => Some(Provided::View(adaptation.clone())),
        (Resource::Sampler { .. }, "AdapLumSampler") => Some(Provided::Sampler(point(device))),
        _ => None,
    }
}

struct Step {
    kind: Kind,
    from: Slot,
    to: Slot,
    screen: ScreenPass,
}

/// The game's bloom over the frame the light is summed in.
pub struct BloomPass {
    steps: Vec<Step>,
    buffers: Buffers,
    adaptation: wgpu::Texture,
    adaptation_view: wgpu::TextureView,
    /// What the constants were written for: the size of the frame, the bloom the moment states and the exposure.
    written: Written<[f32; 14]>,
}

impl BloomPass {
    /// The passes, drawn into targets of the format the light is summed in, over the frame `inputs` hold.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        inputs: &Inputs<'_>,
    ) -> Result<Self, ScreenError> {
        let buffers = Buffers::new(device, inputs.gbuffer.size());
        let adaptation = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom-exposure"),
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
        let target = |blend| Target {
            colour: HDR_FORMAT,
            depth: DEPTH_STENCIL_FORMAT,
            depth_compare: wgpu::CompareFunction::Always,
            blend,
            colour_attachments: Some(1),
        };
        // What is added is added to what the target holds, the colour and not the alpha
        let added = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let plain = DrawState::default();
        let adding = DrawState {
            write_mask: Some(wgpu::ColorWrites::COLOR),
            ..DrawState::default()
        };
        // Each distinct pass is built once, and bound anew for each step that uses it
        let build = |pass: usize, blend, draw: &DrawState| {
            ScreenPass::new(
                device,
                queue,
                pack,
                EFFECT,
                TECHNIQUE,
                pass,
                &target(blend),
                draw,
                Space::Unit,
                &mut |_, _| None,
            )
        };
        let bases = [
            (BRIGHT, build(BRIGHT, None, &plain)?),
            (LEAST, build(LEAST, None, &plain)?),
            (COPY, build(COPY, None, &plain)?),
            (ACROSS, build(ACROSS, None, &plain)?),
            (DOWN, build(DOWN, None, &plain)?),
            (ADD, build(ADD, Some(added), &adding)?),
        ];
        let base_of = |kind: Kind| {
            let pass = match kind {
                Kind::Bright => BRIGHT,
                Kind::Least => LEAST,
                Kind::Copy => COPY,
                Kind::Across => ACROSS,
                Kind::Down => DOWN,
                Kind::Add(_) => ADD,
            };
            &bases
                .iter()
                .find(|(p, _)| *p == pass)
                .expect("a pass built")
                .1
        };
        let steps = plan()
            .into_iter()
            .map(|(kind, from, to)| {
                let source = buffers.view(from, inputs.hdr);
                let screen = base_of(kind).forked(device, queue, &mut |name, requirement| {
                    provide(device, source, &adaptation_view, name, requirement)
                });
                Step {
                    kind,
                    from,
                    to,
                    screen,
                }
            })
            .collect();
        Ok(Self {
            steps,
            buffers,
            adaptation,
            adaptation_view,
            written: Written::default(),
        })
    }

    /// Read `inputs` anew, for targets of another size.
    pub fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, inputs: &Inputs<'_>) {
        self.written.reset();
        self.buffers = Buffers::new(device, inputs.gbuffer.size());
        let (buffers, adaptation) = (&self.buffers, &self.adaptation_view);
        for step in &mut self.steps {
            let source = buffers.view(step.from, inputs.hdr);
            step.screen.bind(device, queue, &mut |name, requirement| {
                provide(device, source, adaptation, name, requirement)
            });
        }
    }

    /// The textures of the buffers, for the bytes they hold.
    pub fn textures(&self) -> impl Iterator<Item = &wgpu::Texture> {
        self.buffers.0.iter().map(wgpu::TextureView::texture)
    }

    /// Write the constants of this frame to every step, and the exposure `tone` gives.
    pub fn write(&self, queue: &wgpu::Queue, screen: &Screen, tone: &Tone) {
        // Nothing is drawn without bloom, and what is drawn with it moves with the day and not with the camera
        let [threshold, intensity, span, _] = tone.bloom.params();
        let key = [
            screen.width as f32,
            screen.height as f32,
            threshold,
            intensity,
            span,
            tone.bloom.amount(),
            tone.one_over_white_point(),
            tone.bloom_blend[0],
            tone.bloom_blend[1],
            tone.bloom_blend[2],
            tone.adaptation()[0],
            tone.adaptation()[1],
            0.0,
            0.0,
        ];
        if !Self::runs_for(tone) || !self.written.changed(key) {
            return;
        }
        let frame = (screen.width, screen.height);
        let texel = |slot: Slot| {
            let (w, h) = slot.size(frame);
            vec![1.0 / w as f32, 1.0 / h as f32, 0.0, 0.0]
        };
        let amount = |amount: Amount| match amount {
            Amount::Large => tone.bloom_blend[0],
            Amount::Medium => tone.bloom_blend[1],
            Amount::Small => tone.bloom_blend[2],
            Amount::Whole => tone.bloom.amount(),
        };
        for step in &self.steps {
            let mut values: std::collections::BTreeMap<&'static str, Vec<f32>> =
                unit_quad().into_iter().collect();
            match step.kind {
                Kind::Bright => {
                    values.insert("BloomParams", tone.bloom.params().to_vec());
                    values.insert("Filmic0", vec![0.0, 0.0, 0.0, tone.one_over_white_point()]);
                    values.insert("TexelSize", texel(step.from));
                }
                Kind::Least => {
                    values.insert("TexelSize", texel(step.from));
                }
                Kind::Copy => {}
                Kind::Across | Kind::Down => {
                    values.insert("BloomTexelSize", texel(step.from));
                    values.insert("TexelSize", texel(step.from));
                }
                Kind::Add(which) => {
                    values.insert("BloomParams", vec![0.0, 0.0, 0.0, amount(which)]);
                }
            }
            step.screen.write(queue, &values);
        }
        let texels: Vec<u8> = tone
            .adaptation()
            .into_iter()
            .flat_map(|v| or_io::half::f32_to_half(v).to_le_bytes())
            .collect();
        queue.write_texture(
            self.adaptation.as_image_copy(),
            &texels,
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

    /// Whether `tone` puts any bloom back over the frame.
    pub fn runs_for(tone: &Tone) -> bool {
        tone.bloom.intensity >= LEAST_INTENSITY
    }

    /// Blur the bright part of the frame `hdr` and add it back to the frame, where `tone` has any bloom.
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, hdr: &wgpu::TextureView, tone: &Tone) {
        if !Self::runs_for(tone) {
            return;
        }
        for step in &self.steps {
            let adding = matches!(step.kind, Kind::Add(_));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.buffers.view(step.to, hdr),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if adding {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            step.screen.record(&mut pass);
        }
    }
}

#[cfg(test)]
mod tests;

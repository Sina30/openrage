//! The lit frame's own luminance, sampled over a grid before it is mapped and read back a frame or two late, which the exposure is metered on.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The cells of the grid the frame is sampled in, across and down.
const ACROSS: u32 = 64;
const DOWN: u32 = 36;
/// The threads of one workgroup, across and down.
const GROUP: (u32, u32) = (8, 4);
/// The bins of the meter's histogram, logarithmic between the least and the most luminance seen.
const BINS: usize = 256;

const SHADER: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> seen: array<f32>;

const ACROSS: u32 = 64u;
const DOWN: u32 = 36u;

@compute @workgroup_size(8, 4)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= ACROSS || id.y >= DOWN) {
        return;
    }
    let size = textureDimensions(frame);
    let at = vec2<u32>(
        (id.x * 2u + 1u) * size.x / (2u * ACROSS),
        (id.y * 2u + 1u) * size.y / (2u * DOWN),
    );
    let colour = textureLoad(frame, at, 0).rgb;
    var luminance = dot(colour, vec3<f32>(0.2125, 0.7154, 0.0721));
    // Nothing that is not a number, nor past what half floats hold, counts.
    if (!(luminance >= 0.0) || luminance > 65504.0) {
        luminance = 0.0;
    }
    seen[id.y * ACROSS + id.x] = luminance;
}
"#;

/// The luminance a frame was sampled at, one value a cell of the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Metered {
    samples: Vec<f32>,
}

impl Metered {
    /// The samples `samples` of a frame.
    pub fn of(samples: Vec<f32>) -> Self {
        Self { samples }
    }

    /// The average the meter reads: the mean luminance of the samples left when the darkest `low` and the brightest `high` parts of them are put aside, each counted at its bin of 256 logarithmic between the least and the most; `None` where nothing is lit.
    pub fn average(&self, low: f32, high: f32) -> Option<f32> {
        let lit: Vec<f32> = (self.samples.iter().copied())
            .filter(|l| l.is_finite() && *l > 0.0)
            .collect();
        let least = lit.iter().copied().reduce(f32::min)?;
        let most = lit.iter().copied().fold(least, f32::max);
        let span = (most / least).ln();
        let top = (BINS - 1) as f32;
        let mut counts = [0u32; BINS];
        for l in &lit {
            let bin = match span > 0.0 {
                true => ((l / least).ln() / span * top).round() as usize,
                false => 0,
            };
            counts[bin.min(BINS - 1)] += 1;
        }
        let total = lit.len() as f32;
        let (low, high) = (low.clamp(0.0, 1.0), high.clamp(0.0, 1.0));
        let (from, to) = match low + high < 1.0 {
            true => (low * total, (1.0 - high) * total),
            false => (0.0, total),
        };
        let (mut below, mut sum, mut weight) = (0.0f32, 0.0f32, 0.0f32);
        for (bin, &n) in counts.iter().enumerate() {
            let n = n as f32;
            let kept = (below + n).clamp(from, to) - below.clamp(from, to);
            sum += kept * least * (span * bin as f32 / top).exp();
            weight += kept;
            below += n;
        }
        (weight > 0.0).then(|| sum / weight)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    /// A frame's samples are copied for reading, once its commands have been submitted.
    Recorded,
    Mapping,
}

/// The pass that samples the lit frame, and the buffers its samples are read back through.
pub(super) struct Meter {
    pipeline: wgpu::ComputePipeline,
    samples: wgpu::Buffer,
    readback: wgpu::Buffer,
    bound: RefCell<Option<wgpu::BindGroup>>,
    phase: Cell<Phase>,
    ready: Arc<AtomicBool>,
    /// The revision of the day the frame being read was drawn under.
    recorded_of: Cell<u64>,
    last: RefCell<Option<(u64, Metered)>>,
}

impl Meter {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport-meter"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("viewport-meter"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bytes = u64::from(ACROSS * DOWN) * 4;
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            pipeline,
            samples: buffer(
                "viewport-meter-samples",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(
                "viewport-meter-readback",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            bound: RefCell::new(None),
            phase: Cell::new(Phase::Idle),
            ready: Arc::new(AtomicBool::new(false)),
            recorded_of: Cell::new(0),
            last: RefCell::new(None),
        }
    }

    /// Bind `frame`, the lit target before it is mapped, for the next [`Meter::record`].
    pub(super) fn bind(&self, device: &wgpu::Device, frame: &wgpu::TextureView) {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport-meter"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(frame),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.samples.as_entire_binding(),
                },
            ],
        });
        *self.bound.borrow_mut() = Some(bind);
    }

    /// Sample the bound frame, drawn under the day of `revision`, unless the last samples are still being read.
    pub(super) fn record(&self, encoder: &mut wgpu::CommandEncoder, revision: u64) {
        let bound = self.bound.borrow();
        let Some(bind) = bound.as_ref().filter(|_| self.phase.get() == Phase::Idle) else {
            return;
        };
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("viewport-meter"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(ACROSS.div_ceil(GROUP.0), DOWN.div_ceil(GROUP.1), 1);
        }
        encoder.copy_buffer_to_buffer(&self.samples, 0, &self.readback, 0, self.samples.size());
        self.recorded_of.set(revision);
        self.phase.set(Phase::Recorded);
    }

    /// Read the samples an earlier [`Meter::record`] copied, once the device has finished them; call it after that frame's commands were submitted.
    pub(super) fn collect(&self, device: &wgpu::Device) {
        match self.phase.get() {
            Phase::Idle => return,
            Phase::Recorded => {
                self.ready.store(false, Ordering::Release);
                let ready = Arc::clone(&self.ready);
                self.readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |read| {
                        ready.store(read.is_ok(), Ordering::Release)
                    });
                self.phase.set(Phase::Mapping);
            }
            Phase::Mapping => {}
        }
        // A poll that has nothing to finish is not an error
        let _ = device.poll(wgpu::PollType::Poll);
        if !self.ready.load(Ordering::Acquire) {
            return;
        }
        let samples = {
            let view = self.readback.slice(..).get_mapped_range();
            let words = view.as_chunks::<4>().0;
            words.iter().map(|b| f32::from_le_bytes(*b)).collect()
        };
        self.readback.unmap();
        *self.last.borrow_mut() = Some((self.recorded_of.get(), Metered::of(samples)));
        self.phase.set(Phase::Idle);
    }

    /// The last samples read, where they were drawn under the day of `revision`.
    pub(super) fn last(&self, revision: u64) -> Option<Metered> {
        let last = self.last.borrow();
        let (of, metered) = last.as_ref()?;
        (*of == revision).then(|| metered.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_reads_the_mean_of_what_is_left_once_the_darkest_and_brightest_parts_are_put_aside()
    {
        let even = Metered::of(vec![3.0; 50]);
        assert!((even.average(0.3, 0.3).expect("lit") - 3.0).abs() < 1e-5);
        // Seventy dark and thirty bright: the middle two fifths are all dark.
        let mut frame = vec![1.0; 70];
        frame.extend([100.0; 30]);
        let frame = Metered::of(frame);
        let middle = frame.average(0.3, 0.3).expect("lit");
        assert!((middle - 1.0).abs() < 1e-4, "{middle}");
        let whole = frame.average(0.0, 0.0).expect("lit");
        assert!((whole - 30.7).abs() < 1e-3, "{whole}");
        // A bright quarter put aside leaves the rest's mean.
        let top = frame.average(0.0, 0.3).expect("lit");
        assert!((top - 1.0).abs() < 1e-4, "{top}");
        // Nothing lit, or nothing that is a number, reads nothing.
        assert_eq!(Metered::of(vec![0.0, f32::NAN]).average(0.0, 0.0), None);
    }
}

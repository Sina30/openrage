//! How long the stages of a frame take on the GPU, read back a frame late so that nothing waits on the device.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The most timestamps a frame writes, its start included.
const SLOTS: u32 = 32;

/// The features a device needs for [`GpuTimer`] to exist.
pub const FEATURES: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// Nothing is waiting to be read.
    Idle,
    /// A frame's timestamps are copied for reading, once its commands have been submitted.
    Recorded,
    /// The copy is being mapped.
    Mapping,
}

/// Timestamps written between the stages of a frame, and the time each stage took.
pub struct GpuTimer {
    set: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Nanoseconds a tick of the device's clock lasts.
    period: f32,
    /// The stage each timestamp after the first ends.
    marks: RefCell<Vec<&'static str>>,
    phase: Cell<Phase>,
    recording: Cell<bool>,
    ready: Arc<AtomicBool>,
    last: RefCell<Vec<(&'static str, f32)>>,
}

impl GpuTimer {
    /// A timer for `device`, or none where it lacks [`FEATURES`].
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(FEATURES) {
            return None;
        }
        let bytes = u64::from(SLOTS) * 8;
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("gpu-timer"),
                ty: wgpu::QueryType::Timestamp,
                count: SLOTS,
            }),
            resolved: buffer(
                "gpu-timer-resolved",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(
                "gpu-timer-readback",
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            ),
            period: queue.get_timestamp_period(),
            marks: RefCell::new(Vec::new()),
            phase: Cell::new(Phase::Idle),
            recording: Cell::new(false),
            ready: Arc::new(AtomicBool::new(false)),
            last: RefCell::new(Vec::new()),
        })
    }

    /// Start timing a frame, unless the last one is still being read.
    pub fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        let on = self.phase.get() == Phase::Idle;
        self.recording.set(on);
        if on {
            self.marks.borrow_mut().clear();
            encoder.write_timestamp(&self.set, 0);
        }
    }

    /// End the stage `label` here.
    pub fn mark(&self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        if !self.recording.get() {
            return;
        }
        let mut marks = self.marks.borrow_mut();
        let at = marks.len() as u32 + 1;
        if at < SLOTS {
            marks.push(label);
            encoder.write_timestamp(&self.set, at);
        }
    }

    /// Finish the frame: its timestamps are copied for [`GpuTimer::collect`].
    pub fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.recording.replace(false) {
            return;
        }
        let count = self.marks.borrow().len() as u32 + 1;
        encoder.resolve_query_set(&self.set, 0..count, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.readback, 0, u64::from(count) * 8);
        self.phase.set(Phase::Recorded);
    }

    /// Read the frame an earlier call to [`GpuTimer::end`] timed, once the device has finished it; call it after that frame's commands were submitted. Whether a frame was read.
    pub fn collect(&self, device: &wgpu::Device) -> bool {
        match self.phase.get() {
            Phase::Idle => return false,
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
            return false;
        }
        let marks = self.marks.borrow();
        {
            let view = self.readback.slice(..).get_mapped_range();
            let ticks: Vec<u64> = view
                .as_chunks::<8>()
                .0
                .iter()
                .take(marks.len() + 1)
                .map(|bytes| u64::from_le_bytes(*bytes))
                .collect();
            *self.last.borrow_mut() = marks
                .iter()
                .zip(ticks.windows(2))
                .map(|(label, pair)| {
                    let ticks = pair[1].saturating_sub(pair[0]);
                    (*label, ticks as f32 * self.period / 1.0e6)
                })
                .collect();
        }
        self.readback.unmap();
        self.phase.set(Phase::Idle);
        true
    }

    /// Each stage of the last frame read, with the milliseconds it took on the device.
    pub fn last(&self) -> Vec<(&'static str, f32)> {
        self.last.borrow().clone()
    }
}

#[cfg(test)]
mod tests;

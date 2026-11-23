use std::time::{Duration, Instant};

/// A span of the editor's frame loop that `FrameStats` times on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePhase {
    Updates,
    Acquire,
    Simulation,
    UiLayout,
    RenderEncode,
    Submit,
}

impl FramePhase {
    pub const ALL: [FramePhase; 6] = [
        FramePhase::Updates,
        FramePhase::Acquire,
        FramePhase::Simulation,
        FramePhase::UiLayout,
        FramePhase::RenderEncode,
        FramePhase::Submit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FramePhase::Updates => "Background results",
            FramePhase::Acquire => "Surface acquire",
            FramePhase::Simulation => "Simulation",
            FramePhase::UiLayout => "UI layout",
            FramePhase::RenderEncode => "Render encode",
            FramePhase::Submit => "Submit and present",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrameStats {
    last_tick: Option<Instant>,
    smoothed_fps: f32,
    smoothed_frame_ms: f32,
    smoothed_phase_ms: [f32; FramePhase::ALL.len()],
    pending: [Duration; FramePhase::ALL.len()],
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            last_tick: None,
            smoothed_fps: 0.0,
            smoothed_frame_ms: 0.0,
            smoothed_phase_ms: [0.0; FramePhase::ALL.len()],
            pending: [Duration::ZERO; FramePhase::ALL.len()],
        }
    }
}

fn smooth(smoothed: &mut f32, sample: f32, first: bool) {
    *smoothed = if first {
        sample
    } else {
        *smoothed * 0.9 + sample * 0.1
    };
}

impl FrameStats {
    pub fn fps(&self) -> f32 {
        self.smoothed_fps
    }

    /// The smoothed time between ticks, in milliseconds.
    pub fn frame_ms(&self) -> f32 {
        self.smoothed_frame_ms
    }

    /// The smoothed milliseconds per phase, in `FramePhase::ALL` order.
    pub fn phase_ms(&self) -> [f32; FramePhase::ALL.len()] {
        self.smoothed_phase_ms
    }

    /// Adds `elapsed` to `phase` for the frame the next `tick` closes.
    pub fn record(&mut self, phase: FramePhase, elapsed: Duration) {
        self.pending[phase as usize] += elapsed;
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        // Phases recorded since the previous tick are disjoint spans inside
        // this tick's interval, and every series shares one weight and one
        // first sample, so the smoothed phases never sum past the frame time.
        let pending = std::mem::take(&mut self.pending);
        if let Some(prev) = self.last_tick.replace(now) {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt > 0.0 {
                let inst = (1.0 / dt).clamp(0.0, 1000.0);
                if self.smoothed_fps == 0.0 {
                    self.smoothed_fps = inst;
                } else {
                    self.smoothed_fps = self.smoothed_fps * 0.9 + inst * 0.1;
                }
                let first = self.smoothed_frame_ms == 0.0;
                smooth(&mut self.smoothed_frame_ms, dt * 1000.0, first);
                for (smoothed, spent) in self.smoothed_phase_ms.iter_mut().zip(pending) {
                    smooth(smoothed, spent.as_secs_f32() * 1000.0, first);
                }
            }
        }
    }
}

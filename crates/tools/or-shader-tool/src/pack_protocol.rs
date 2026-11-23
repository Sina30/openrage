//! What `or-shader-tool` says on stdout when it bakes the SPIR-V pack for another program: one JSON object a line, the same type on both ends.

use serde::{Deserialize, Serialize};

/// One thing the helper reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The install's effects are being read, which comes before anything can be counted.
    Reading,
    /// `done` of `total` programs that need translating are translated.
    Progress { done: usize, total: usize },
    /// The pack is written.
    Finished {
        effects: usize,
        programs: usize,
        translated: usize,
        refused: usize,
        skipped: usize,
        reused: usize,
        baked: usize,
    },
    /// Who would write a pack now: what a pack must have been written by to be kept.
    Identity {
        translator: String,
        legalizer: String,
        pack_version: u32,
    },
}

impl Event {
    /// The line the helper writes, with no newline.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The event a line says, or `None` for a line that is anything else, such as a log line.
    pub fn parse(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

/// A progress callback that reports to `emit` at the start, the end and about every hundredth of the way between, so a long bake is not a line a program.
pub fn throttled(emit: impl Fn(Event) + Sync) -> impl Fn(usize, usize) + Sync {
    move |done, total| {
        let step = (total / 100).max(1);
        if done == 0 || done >= total || done % step == 0 {
            emit(Event::Progress { done, total });
        }
    }
}

#[cfg(test)]
mod tests;

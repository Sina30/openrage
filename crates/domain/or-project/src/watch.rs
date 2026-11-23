//! What a build reports while it runs and what can stop it: the file in hand, how many bytes of the plan are done, and a flag.
//!
//! A stopped build writes nothing that stays: it works in a hidden fresh folder, which it removes, and
//! the build it would have replaced is as it was. Once the folders are swapped there is nothing to stop.

pub use or_job::{Cancel, Progress};

use crate::build::BuildError;

/// The counters a screen reads and the flag a screen sets, both shared between clones.
#[derive(Clone, Default)]
pub struct Watch {
    pub progress: Progress,
    pub cancel: Cancel,
}

impl Watch {
    /// `Err` when somebody asked the build to stop, which the step in hand returns at once.
    pub(crate) fn go_on(&self) -> Result<(), BuildError> {
        match self.cancel.is_set() {
            true => Err(BuildError::Stopped),
            false => Ok(()),
        }
    }

    /// The file the build is working on, and how many of the plan's `total` bytes are done.
    pub(crate) fn at(&self, path: &str, done: u64, total: u64) {
        self.progress.now_on(path);
        self.progress.reached(done, total);
    }

    /// Every one of the plan's `total` bytes is done in the step that ends.
    pub(crate) fn all(&self, total: u64) {
        self.progress.reached(total, total);
    }
}

//! One window writes a pack: the lock that lets a second window read it and not write it.
//!
//! The lock is the operating system's advisory lock on `.openrage/lock` in the pack's folder, held for as long as the
//! [`PackLock`] lives. The system lets go of it when the process ends however it ends, so a window that crashed leaves
//! no lock behind to clean up by hand.
//!
//! ⚠ **A lock that cannot be had is not a reason to refuse an edit.** A folder the lock file cannot be made in, or a
//! drive that has no locks, gives a lock that guards nothing and says so in the log; only a lock another window holds refuses.

use std::fs::{File, TryLockError};
use std::path::Path;

use or_project::snapshot::PRIVATE_DIR;

use crate::backup::make_private;

const FILE: &str = "lock";

/// A pack another window is editing.
#[derive(Debug, thiserror::Error)]
#[error("another OpenRage window is editing {0}: it reads here and is written there")]
pub struct Held(pub String);

/// The right to write one pack, held until it is dropped.
#[derive(Debug)]
pub struct PackLock {
    file: Option<File>,
}

impl PackLock {
    /// Whether the system holds the lock for this window, and not only a lock that guards nothing.
    pub fn is_held(&self) -> bool {
        self.file.is_some()
    }
}

/// Whether another window holds the pack in `folder`: the lock is taken and let go at once, and only where a lock file shows that one was ever taken, so asking leaves no lock file in a pack nobody edited.
pub fn held_elsewhere(folder: &Path) -> bool {
    folder.join(PRIVATE_DIR).join(FILE).is_file() && acquire(folder).is_err()
}

/// Take the right to write the pack in `folder`, or say that another window has it.
pub fn acquire(folder: &Path) -> Result<PackLock, Held> {
    let opened = make_private(folder).and_then(|dir| {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(FILE))
    });
    let file = match opened {
        Ok(file) => file,
        Err(why) => {
            tracing::warn!(%why, folder = %folder.display(), "the pack could not be locked");
            return Ok(PackLock { file: None });
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(PackLock { file: Some(file) }),
        Err(TryLockError::WouldBlock) => Err(Held(folder.display().to_string())),
        Err(TryLockError::Error(why)) => {
            tracing::warn!(%why, folder = %folder.display(), "the pack could not be locked");
            Ok(PackLock { file: None })
        }
    }
}

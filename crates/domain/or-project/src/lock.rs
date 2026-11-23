//! One window writes a project: the lock that lets a second window read it and not write it.
//!
//! The lock is the operating system's lock on a file in a folder the caller names (the application's, beside its preferences), named for the project file, held for as long as
//! the [`ProjectLock`] lives. The system lets go of it when the process ends however it ends, so a window that crashed
//! leaves no lock behind to clean up by hand.
//!
//! ⚠ **Not in the project's folder.** Windows refuses to read a file another process has locked, so a lock file there would
//! stop a zip of the folder, and a copy of it, while the project is open.
//!
//! ⚠ **A lock that cannot be had is not a reason to refuse an edit.** A folder the lock file cannot be made in, or a
//! drive that has no locks, gives a lock that guards nothing ([`ProjectLock::is_held`] says false); only a lock another window holds refuses.

use std::fs::{File, TryLockError};
use std::path::Path;

use sha2::{Digest, Sha256};

/// A project another window has open.
#[derive(Debug, thiserror::Error)]
#[error("another OpenRage window has {0} open: it is read here and written there")]
pub struct Held(pub String);

/// The right to write one project, held until it is dropped.
#[derive(Debug)]
pub struct ProjectLock {
    file: Option<File>,
}

impl ProjectLock {
    /// Whether the system holds the lock for this window, and not only a lock that guards nothing.
    pub fn is_held(&self) -> bool {
        self.file.is_some()
    }
}

/// Take the right to write the project file `project`, keeping the lock file in `locks`, or say that another window has it.
pub fn acquire(project: &Path, locks: &Path) -> Result<ProjectLock, Held> {
    let place = lock_file(project, locks);
    let opened = place
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&place)
        });
    let file = match opened {
        Ok(file) => file,
        Err(_) => return Ok(ProjectLock { file: None }),
    };
    match file.try_lock() {
        Ok(()) => Ok(ProjectLock { file: Some(file) }),
        Err(TryLockError::WouldBlock) => Err(Held(project.file_name().map_or_else(
            || project.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ))),
        Err(TryLockError::Error(_)) => Ok(ProjectLock { file: None }),
    }
}

/// The file that locks the project file `project`: named by a hash of where the project is, so two project files in one folder are two locks.
fn lock_file(project: &Path, locks: &Path) -> std::path::PathBuf {
    let there = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let name = format!(
        "{:x}.lock",
        Sha256::digest(there.to_string_lossy().to_lowercase().as_bytes())
    );
    locks.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_window_is_told_and_the_lock_is_let_go_when_the_first_drops_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("p.openrage-proj");
        let locks = dir.path().join("locks");

        let first = acquire(&project, &locks).expect("the first window writes");
        assert!(first.is_held());
        let refused = acquire(&project, &locks).unwrap_err();
        assert!(refused.to_string().contains("p.openrage-proj"), "{refused}");

        drop(first);
        assert!(acquire(&project, &locks).is_ok(), "the lock is free again");
    }

    #[test]
    fn two_project_files_in_one_folder_are_two_locks() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (
            dir.path().join("a.openrage-proj"),
            dir.path().join("b.openrage-proj"),
        );
        let locks = dir.path().join("locks");
        let _first = acquire(&a, &locks).unwrap();
        assert!(acquire(&b, &locks).is_ok());
    }
}

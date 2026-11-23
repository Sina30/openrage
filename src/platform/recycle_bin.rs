//! Deleting a file or folder as the system's file manager does: into its bin, where it can be restored.

use std::path::Path;

/// Whether a [`recycle`]d path can be restored: Windows, a Mac and a Linux desktop each have a bin.
pub const RECOVERABLE: bool = true;

/// What the system calls the bin, in the words a user reads.
pub const NAME: &str = if cfg!(windows) {
    tr_key!("Recycle Bin")
} else {
    tr_key!("Trash")
};

/// Send `path` to the system's bin: the Recycle Bin, Finder's Trash or the desktop's.
///
/// ⚠ A test build and a self-check send it to `fake`'s bin instead, so neither reaches the user's.
pub fn recycle(path: &Path) -> std::io::Result<()> {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real(path);
    }
    fake::recycle(path)
}

#[cfg(all(windows, not(test)))]
fn real(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    // ⚠ `pFrom` is a list: each path ends in a NUL and the list in a second one,
    // and a relative path would resolve against the process's directory.
    let from: Vec<u16> = std::path::absolute(path)?
        .as_os_str()
        .encode_wide()
        .chain([0, 0])
        .collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        // ⚠ The editor has already asked, hence NOCONFIRMATION; WANTNUKEWARNING
        // is what makes the shell ask before deleting for good a path the bin
        // cannot take (too large, or on a drive with no bin) instead of doing so silently.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is double-NUL-terminated and outlives the call; every other
    // pointer is null, which the call accepts.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(std::io::Error::other(format!(
            "the shell could not recycle it (code {code:#x})"
        )));
    }
    if op.fAnyOperationsAborted != 0 {
        return Err(std::io::Error::other("the delete was cancelled"));
    }
    // A file skipped in the shell's own error dialog leaves its folder behind.
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(std::io::Error::other("part of it is still there"));
    }
    Ok(())
}

#[cfg(all(not(windows), not(test)))]
fn real(path: &Path) -> std::io::Result<()> {
    into_the_system_trash(path)
}

/// ⚠ Not deleted when the system cannot take it: a Trash that is unavailable, or a mount with none,
/// is an error the user reads while the file stays, where the old fallback deleted it for good.
#[cfg(not(windows))]
fn into_the_system_trash(path: &Path) -> std::io::Result<()> {
    trash::delete(path)
        .map_err(|error| std::io::Error::other(format!("the {NAME} could not take it: {error}")))
}

/// A stand-in for the Recycle Bin: a folder per test thread, where a test finds what a delete sent.
pub mod fake {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    thread_local! {
        static BIN: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
        static LEAVES_IN_PLACE: Cell<bool> = const { Cell::new(false) };
    }

    /// Make `dir` this thread's Recycle Bin.
    #[cfg(test)]
    pub fn use_bin(dir: &Path) {
        BIN.with(|bin| *bin.borrow_mut() = Some(dir.to_path_buf()));
    }

    /// Leave each path this thread deletes where it is, as a self-check's stand-in does.
    #[cfg(test)]
    pub fn leave_in_place() {
        LEAVES_IN_PLACE.with(|leaves| leaves.set(true));
    }

    pub(super) fn recycle(path: &Path) -> std::io::Result<()> {
        tracing::info!(
            "the Recycle Bin's stand-in was asked to take {}",
            path.display()
        );
        let Some(bin) = BIN.with(|bin| bin.borrow().clone()) else {
            // ⚠ A self-check has no bin and leaves the file where it is: a bin
            // in its scratch would be deleted for good when the run ends.
            return if cfg!(test) && !LEAVES_IN_PLACE.with(Cell::get) {
                Err(std::io::Error::other(
                    "this test thread has no Recycle Bin: call `fake::use_bin`",
                ))
            } else {
                Ok(())
            };
        };
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("a path with no name"))?;
        // A real bin keeps two of one name side by side, so this takes the first name that is free
        let mut into = bin.join(name);
        for number in 2.. {
            if std::fs::symlink_metadata(&into).is_err() {
                break;
            }
            let mut numbered = name.to_os_string();
            numbered.push(format!(" ({number})"));
            into = bin.join(numbered);
        }
        std::fs::rename(path, into)
    }
}

#[cfg(all(test, not(windows)))]
mod system_trash {
    use super::*;

    /// Moves a file into the real Trash of whoever runs it, so it is not part of a normal run.
    ///
    /// `cargo test --bin openrage -- --ignored a_real_delete`: the file is found where the system
    /// put it, and removed from there, so the Trash is left as it was.
    #[test]
    #[ignore = "puts a file in the real Trash"]
    fn a_real_delete_goes_to_the_system_trash_and_not_for_good() {
        let dir = tempfile::tempdir().expect("a folder");
        let name = format!("openrage-trash-test-{}.txt", std::process::id());
        let file = dir.path().join(&name);
        std::fs::write(&file, b"trashed").expect("a file");

        into_the_system_trash(&file).expect("the system's trash took it");
        assert!(!file.exists(), "the file is still where it was");

        // Finder's Trash on a Mac, the freedesktop one elsewhere: where the system put it.
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .expect("a home folder");
        let trashed = if cfg!(target_os = "macos") {
            home.join(".Trash").join(&name)
        } else {
            home.join(".local/share/Trash/files").join(&name)
        };
        assert_eq!(
            std::fs::read(&trashed).expect("it is in the trash, whole"),
            b"trashed"
        );
        std::fs::remove_file(&trashed).expect("left the trash as it was");
    }
}

#[cfg(test)]
mod stand_in {
    use super::*;

    #[test]
    fn two_folders_of_one_name_are_both_taken_as_a_real_bin_takes_them() {
        let scratch = tempfile::tempdir().expect("a folder");
        let bin = scratch.path().join("bin");
        std::fs::create_dir(&bin).expect("a bin");
        fake::use_bin(&bin);
        let mut sent = Vec::new();
        for parent in ["a", "b"] {
            let folder = scratch.path().join(parent).join("same");
            std::fs::create_dir_all(&folder).expect("a folder");
            std::fs::write(folder.join("note.txt"), parent).expect("a file");
            recycle(&folder).expect("the stand-in took it");
            assert!(!folder.exists(), "{parent}: it is still where it was");
            sent.push(parent);
        }
        let mut held: Vec<String> = std::fs::read_dir(&bin)
            .expect("the bin")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        held.sort();
        assert_eq!(held.len(), sent.len(), "the bin holds {held:?}");
        let notes: Vec<String> = held
            .iter()
            .map(|name| std::fs::read_to_string(bin.join(name).join("note.txt")).expect("whole"))
            .collect();
        assert!(
            notes.contains(&"a".to_string()) && notes.contains(&"b".to_string()),
            "{notes:?}"
        );
    }
}

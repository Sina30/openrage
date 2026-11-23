//! Files on the system clipboard, as Explorer's Copy and Paste hand them over.

use std::path::PathBuf;

/// Put `paths` on the clipboard as a file list Explorer pastes, in place of what it held.
///
/// ⚠ A test build and a self-check hand them to [`fake`] instead, so neither touches the clipboard the desktop shares.
pub fn offer_files(paths: &[PathBuf]) -> Result<(), String> {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real::offer_files(paths);
    }
    fake::offer_files(paths)
}

/// The file list another program, Explorer, put on the clipboard after this app's last offer; empty when there is none.
pub fn files_from_elsewhere() -> Vec<PathBuf> {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real::files_from_elsewhere();
    }
    fake::files_from_elsewhere()
}

/// Whether [`files_from_elsewhere`] has files, asked without opening the clipboard, as a menu does every frame.
pub fn holds_files_from_elsewhere() -> bool {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real::holds_files_from_elsewhere();
    }
    fake::holds_files_from_elsewhere()
}

#[cfg(all(windows, not(test)))]
mod real {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::System::DataExchange::{
        GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    };
    use windows_sys::Win32::System::Ole::CF_HDROP;

    /// The clipboard's sequence number once this app last offered files: anything there then is older than the offer.
    static OFFERED_AT: AtomicU32 = AtomicU32::new(0);

    pub(super) fn offer_files(paths: &[PathBuf]) -> Result<(), String> {
        let written = (|| {
            let mut clipboard = arboard::Clipboard::new()?;
            // ⚠ Emptied first: a file list set alone sits beside the text the clipboard held.
            clipboard.clear()?;
            clipboard.set().file_list(paths)
        })();
        // SAFETY: takes no argument and only reads the window station's counter.
        OFFERED_AT.store(unsafe { GetClipboardSequenceNumber() }, Ordering::Relaxed);
        written.map_err(|e| format!("the clipboard did not take the files: {e}"))
    }

    pub(super) fn files_from_elsewhere() -> Vec<PathBuf> {
        if !changed_since_offer() {
            return Vec::new();
        }
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get().file_list())
            .unwrap_or_default()
    }

    pub(super) fn holds_files_from_elsewhere() -> bool {
        // SAFETY: takes a format number and reads no memory of ours.
        changed_since_offer() && unsafe { IsClipboardFormatAvailable(u32::from(CF_HDROP)) } != 0
    }

    fn changed_since_offer() -> bool {
        // SAFETY: as in `offer_files`.
        let now = unsafe { GetClipboardSequenceNumber() };
        now != OFFERED_AT.load(Ordering::Relaxed)
    }
}

#[cfg(all(not(windows), not(test)))]
mod real {
    use std::path::PathBuf;

    pub(super) fn offer_files(_paths: &[PathBuf]) -> Result<(), String> {
        Err("files are handed to the file manager on Windows only".into())
    }

    pub(super) fn files_from_elsewhere() -> Vec<PathBuf> {
        Vec::new()
    }

    pub(super) fn holds_files_from_elsewhere() -> bool {
        false
    }
}

/// A stand-in for the clipboard, one per thread: what the app offered, and what a test says Explorer copied.
pub mod fake {
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct Held {
        files: Vec<PathBuf>,
        from_elsewhere: bool,
    }

    thread_local! {
        static HELD: RefCell<Held> = RefCell::new(Held::default());
    }

    pub(super) fn offer_files(paths: &[PathBuf]) -> Result<(), String> {
        let named: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        tracing::info!("the clipboard's stand-in was offered {}", named.join(", "));
        HELD.with(|held| {
            *held.borrow_mut() = Held {
                files: paths.to_vec(),
                from_elsewhere: false,
            }
        });
        Ok(())
    }

    pub(super) fn files_from_elsewhere() -> Vec<PathBuf> {
        HELD.with(|held| {
            let held = held.borrow();
            if held.from_elsewhere {
                held.files.clone()
            } else {
                Vec::new()
            }
        })
    }

    pub(super) fn holds_files_from_elsewhere() -> bool {
        !files_from_elsewhere().is_empty()
    }

    /// The files the app last offered, as Explorer would paste them.
    #[cfg(test)]
    pub fn offered() -> Vec<PathBuf> {
        HELD.with(|held| {
            let held = held.borrow();
            if held.from_elsewhere {
                Vec::new()
            } else {
                held.files.clone()
            }
        })
    }

    /// Explorer's Copy of `paths`.
    #[cfg(test)]
    pub fn copied_in_explorer(paths: Vec<PathBuf>) {
        HELD.with(|held| {
            *held.borrow_mut() = Held {
                files: paths,
                from_elsewhere: true,
            }
        });
    }
}

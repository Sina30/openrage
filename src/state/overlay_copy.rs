//! Copying a shipped archive into the install's mods overlay, where edits belong.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use or_job::{Cancel, Progress};
use or_title::{record_file, BaseRecord, RefreshPlan, StaleCopy};

use super::jobs::{file_name, JobKind, Outcome, RunningJob, Then};
use super::overlay_refresh::keep_record;
use super::{retail, AppState, GateOutcome};

/// A copy the user has been shown and has not yet answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayCopyOffer {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub bytes: u64,
    /// Set when the copy replaces one the overlay holds: what that replacement would throw away.
    pub replaces: Option<RefreshPlan>,
}

#[derive(Default)]
pub struct OverlayCopy {
    pub offer: Option<OverlayCopyOffer>,
    running: Option<Running>,
    /// Overlay copies whose original the title has since changed.
    pub stale: Vec<StaleCopy>,
    pub(super) checking: Option<Receiver<(PathBuf, Vec<StaleCopy>)>>,
    pub(super) weighing: Option<Weighing>,
}

struct Running {
    offer: OverlayCopyOffer,
    cancel: Cancel,
    progress: Progress,
    receiver: Receiver<Result<(), String>>,
}

/// A refresh's discards being worked out on a worker.
pub(super) struct Weighing {
    pub(super) stale: StaleCopy,
    pub(super) cancel: Cancel,
    pub(super) receiver: Receiver<Result<RefreshPlan, String>>,
}

impl OverlayCopy {
    pub fn running(&self) -> Option<&OverlayCopyOffer> {
        self.running.as_ref().map(|job| &job.offer)
    }

    /// The stale copy at `path`, if the install's change since it was made is known.
    pub fn stale_copy(&self, path: &Path) -> Option<&StaleCopy> {
        self.stale.iter().find(|stale| stale.copy == path)
    }

    /// The copy in flight, or the weighing before one, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let Some(job) = self.running.as_ref() else {
            let weighing = self.weighing.as_ref()?;
            return Some(RunningJob {
                kind: JobKind::OverlayCopy,
                doing: tr!(
                    "Weighing what refreshing {copy} would discard",
                    copy = file_name(&weighing.stale.copy)
                ),
                fraction: None,
                detail: String::new(),
                stopping: weighing.cancel.is_set(),
            });
        };
        let snap = job.progress.snapshot();
        Some(RunningJob {
            kind: JobKind::OverlayCopy,
            doing: tr!(
                "Copying {source} into the mods overlay",
                source = file_name(&job.offer.source)
            ),
            fraction: Some(snap.fraction()),
            detail: format!(
                "{} of {}",
                or_job::human_bytes(snap.attempted),
                or_job::human_bytes(job.offer.bytes)
            ),
            stopping: job.cancel.is_set(),
        })
    }

    pub fn cancel(&self) {
        if let Some(job) = &self.running {
            job.cancel.set();
        }
        if let Some(weighing) = &self.weighing {
            weighing.cancel.set();
        }
    }

    /// Run `work` as the one copy into the overlay.
    pub(super) fn start(
        &mut self,
        offer: OverlayCopyOffer,
        work: impl FnOnce(&Cancel, &Progress) -> Result<(), String> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let (cancel, progress) = (Cancel::new(), Progress::new());
        let (worker_cancel, worker_progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work(&worker_cancel, &worker_progress));
        });
        self.running = Some(Running {
            offer,
            cancel,
            progress,
            receiver: rx,
        });
    }
}

impl AppState {
    /// Offer to copy a shipped archive into the overlay; false when there is no copy to make.
    pub fn offer_overlay_copy(&mut self, source: &Path) -> bool {
        let Some(dest) = retail::overlay_copy_target(source, &self.prefs) else {
            return false;
        };
        if dest.exists() {
            self.show_info_toast(tr!(
                "The overlay already holds `{dest}`",
                dest = dest.display()
            ));
            return false;
        }
        let Ok(meta) = std::fs::metadata(source) else {
            return false;
        };
        self.overlay_copy.offer = Some(OverlayCopyOffer {
            source: source.to_path_buf(),
            dest,
            bytes: meta.len(),
            replaces: None,
        });
        true
    }

    /// Start the offered copy on a worker, or drop the offer.
    pub fn answer_overlay_copy(&mut self, accept: bool) {
        let Some(offer) = self.overlay_copy.offer.take() else {
            return;
        };
        if !accept {
            return;
        }
        if let Some(running) = self.overlay_copy.running() {
            let busy = tr!(
                "Not copied: the overlay copy of `{source}` is still running",
                source = running.source.display()
            );
            self.show_error_toast(busy);
            return;
        }
        // An overlay path never needs confirmation, so anything but `Proceed`
        // is a refusal the gate has already reported.
        if !matches!(self.gate_permits(Some(&offer.dest)), GateOutcome::Proceed) {
            return;
        }
        if offer.replaces.is_some() {
            if let Some(refusal) = self.overlay_refresh_blocker(&offer.dest) {
                self.show_error_toast(refusal);
                return;
            }
        }
        let (source, dest) = (offer.source.clone(), offer.dest.clone());
        let replace = offer.replaces.is_some();
        let tracking = self.base_tracking();
        self.overlay_copy.start(offer, move |cancel, progress| {
            // ⚠ Taken before the copy reads the original, so the record is of what the copy holds.
            let record = tracking.as_ref().map(|(records, crypto)| {
                (BaseRecord::of(&source, crypto), record_file(records, &dest))
            });
            copy_into_place(&source, &dest, replace, cancel, progress)?;
            keep_record(record);
            Ok(())
        });
    }

    /// Where a finished copy lands, each frame.
    pub fn poll_overlay_copy(&mut self) {
        let Some(job) = &self.overlay_copy.running else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err(tr!("The overlay copy stopped without reporting").to_string())
            }
        };
        let (dest, cancelled) = (job.offer.dest.clone(), job.cancel.is_set());
        let refreshed = job.offer.replaces.is_some();
        self.overlay_copy.running = None;
        match result {
            Ok(()) if refreshed => self.refreshed_overlay_copy(&dest),
            Ok(()) => {
                let said = tr!("Copied to the overlay: {dest}", dest = dest.display());
                self.show_toast(said.clone());
                let then = self.reveal_in_install(&dest);
                self.jobs
                    .record(JobKind::OverlayCopy, Outcome::Done(then), said);
            }
            Err(_) if cancelled => {
                let said = tr!("Nothing was copied into the overlay");
                self.jobs
                    .record(JobKind::OverlayCopy, Outcome::Stopped(Then::Nothing), said);
            }
            Err(e) => {
                self.show_error_toast(e.clone());
                self.jobs.record(JobKind::OverlayCopy, Outcome::Failed, e);
            }
        }
    }

    /// `path` revealed in the tree of the install open, else its folder opened.
    pub(super) fn reveal_in_install(&self, path: &Path) -> Then {
        let root = self.prefs.active_game_root();
        match root.and_then(|root| Some((root, path.strip_prefix(root).ok()?))) {
            Some((root, inner)) => Then::Reveal {
                source: root.clone(),
                inner: inner.to_string_lossy().replace('\\', "/"),
            },
            None => path.parent().map_or(Then::Nothing, |folder| {
                Then::OpenFolder(folder.to_path_buf())
            }),
        }
    }
}

/// ⚠ In chunks rather than `std::fs::copy`, which cannot be stopped part-way through an archive of gigabytes.
fn copy_into_place(
    source: &Path,
    dest: &Path,
    replace: bool,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<(), String> {
    let failed = |e: std::io::Error| tr!("Copy `{source}`: {e}", e = e, source = source.display());
    into_place(dest, replace, |part| {
        let mut from = std::fs::File::open(source).map_err(failed)?;
        let total = from.metadata().map_err(failed)?.len();
        let mut to = std::fs::File::create(part).map_err(failed)?;
        let mut chunk = vec![0u8; 1 << 20];
        let mut done = 0u64;
        progress.reached(0, total);
        loop {
            if cancel.is_set() {
                return Err(tr!("Stopped before the copy was whole").to_string());
            }
            let read = from.read(&mut chunk).map_err(failed)?;
            if read == 0 {
                return Ok(());
            }
            to.write_all(&chunk[..read]).map_err(failed)?;
            done += read as u64;
            progress.reached(done, total);
        }
    })
}

/// Write `bytes` as the overlay's copy at `dest`, landing as the copy does.
pub(super) fn write_into_place(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    into_place(dest, false, |part| {
        std::fs::write(part, bytes)
            .map_err(|e| tr!("Write `{part}`: {e}", e = e, part = part.display()))
    })
}

/// ⚠ Written beside the destination and renamed into place: the mount reads the
/// overlay's copy first, so a half-written archive under the real name would
/// stand in for the install's.
fn into_place(
    dest: &Path,
    replace: bool,
    fill: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let parent = dest
        .parent()
        .ok_or_else(|| tr!("`{dest}` has no parent directory", dest = dest.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| tr!("Create `{parent}`: {e}", e = e, parent = parent.display()))?;
    let part = dest.with_extension("rpf.part");
    let copied = fill(&part).and_then(|_| {
        // ⚠ `rename` replaces an existing file on Windows, and the overlay
        // copy may have appeared since the offer.
        // ponytail: check then rename leaves a tiny window; a no-replace move closes it.
        if !replace && dest.exists() {
            return Err(tr!(
                "The overlay already holds `{dest}`",
                dest = dest.display()
            ));
        }
        std::fs::rename(&part, dest)
            .map_err(|e| tr!("Move into `{dest}`: {e}", e = e, dest = dest.display()))
    });
    if copied.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    copied
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::state::prefs::EditMode;
    use crate::state::retail::{classify, PathClass};

    #[test]
    fn a_shipped_archive_becomes_an_overlay_file_through_the_copy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let install = tmp.path().join("GTAV");
        std::fs::create_dir_all(install.join("update")).expect("install dirs");
        std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
        let shipped = install.join("update").join("update.rpf");
        let bytes = b"shipped archive bytes".to_vec();
        std::fs::write(&shipped, &bytes).expect("archive");

        let mut state = AppState::new();
        state.retail_edit_session_allow = false;
        state.prefs.edit_mode = EditMode::Overlay;
        assert!(state.offer_overlay_copy(&shipped));
        let dest = install.join("mods").join("update").join("update.rpf");
        assert_eq!(
            state.overlay_copy.offer,
            Some(OverlayCopyOffer {
                source: shipped.clone(),
                dest: dest.clone(),
                bytes: bytes.len() as u64,
                replaces: None,
            }),
            "the offer states what it copies, where, and how much"
        );
        assert!(!dest.exists(), "nothing is written before the user answers");

        state.answer_overlay_copy(true);
        let deadline = Instant::now() + Duration::from_secs(30);
        while state.overlay_copy.running().is_some() {
            state.poll_overlay_copy();
            assert!(Instant::now() < deadline, "the copy never reported");
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(std::fs::read(&dest).expect("the overlay copy"), bytes);
        assert_eq!(std::fs::read(&shipped).expect("the shipped archive"), bytes);
        assert_eq!(classify(&dest, &state.prefs), PathClass::Overlay);
        assert!(
            !state.offer_overlay_copy(&shipped),
            "an overlay that already holds the archive is not offered for overwriting"
        );
    }

    #[test]
    fn an_overlay_copy_that_appears_after_the_offer_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("update.rpf");
        let dest = tmp.path().join("mods").join("update.rpf");
        std::fs::write(&shipped, b"shipped").expect("archive");
        std::fs::create_dir_all(dest.parent().expect("parent")).expect("overlay dir");
        std::fs::write(&dest, b"the user's copy").expect("overlay copy");

        assert!(copy_into_place(&shipped, &dest, false, &Cancel::new(), &Progress::new()).is_err());
        assert_eq!(std::fs::read(&dest).expect("dest"), b"the user's copy");
        assert!(!dest.with_extension("rpf.part").exists());
    }

    #[test]
    fn a_stopped_copy_leaves_nothing_in_the_overlay() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("update.rpf");
        let dest = tmp.path().join("mods").join("update.rpf");
        std::fs::write(&shipped, vec![7u8; 3 << 20]).expect("archive");
        let cancel = Cancel::new();
        cancel.set();

        assert!(copy_into_place(&shipped, &dest, false, &cancel, &Progress::new()).is_err());
        assert!(
            !dest.exists(),
            "a stopped copy stood in for the install's archive"
        );
        assert!(!dest.with_extension("rpf.part").exists());

        let progress = Progress::new();
        copy_into_place(&shipped, &dest, false, &Cancel::new(), &progress).expect("a whole copy");
        assert_eq!(std::fs::read(&dest).expect("dest").len(), 3 << 20);
        assert_eq!(progress.snapshot().fraction(), 1.0);
    }
}

//! Defragmenting an archive on disk: the size on a worker, the user's answer, then the rebuild and its write.
//!
//! The rebuild is [`Defrag`], the code `openrage-cli rpf defrag` runs.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::SystemTime;

use or_io::container::rpf::Defrag;
use or_io::container::RpfCryptoProvider;
use or_job::{Cancel, Progress};

use super::document::{DocumentId, RpfNodeId};
use super::jobs::{file_name, JobKind, Outcome as Ended, RunningJob, Then};
use super::rpf_archive_open::open_archive_from_disk;
use super::rpf_node::{ArchiveSource, RpfNodeBacking};
use super::{AppState, DocPayload, PendingRetailEdit};

/// A file's size and modification time, which must not move between the preview and the write.
type Stamp = (u64, Option<SystemTime>);

/// A rebuild's size worked out and waiting for the user's answer.
pub struct DefragOffer {
    pub doc_id: DocumentId,
    pub node_id: RpfNodeId,
    pub path: PathBuf,
    pub before: u64,
    pub after: u64,
    modified: Option<SystemTime>,
    crypto: Arc<dyn RpfCryptoProvider>,
}

pub(super) struct Written {
    doc_id: DocumentId,
    node_id: RpfNodeId,
    path: PathBuf,
    before: u64,
    after: u64,
    took_backup: bool,
}

pub(super) enum Outcome {
    Planned(Result<DefragOffer, String>),
    Written(Result<Written, String>),
    /// Asked to stop, so it wrote nothing.
    Stopped,
}

struct Running {
    path: PathBuf,
    doing: String,
    cancel: Cancel,
    progress: Progress,
    receiver: Receiver<Outcome>,
}

/// The one defragment the editor has in flight or on offer, if any.
#[derive(Default)]
pub struct DefragJobs {
    pub offer: Option<DefragOffer>,
    running: Option<Running>,
}

impl DefragJobs {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Whether a write to `path` reaches the archive in flight or on offer, which nothing else may write meanwhile.
    pub fn holds(&self, path: &Path) -> bool {
        self.running
            .as_ref()
            .is_some_and(|job| job.path.starts_with(path))
            || self
                .offer
                .as_ref()
                .is_some_and(|offer| offer.path.starts_with(path))
    }

    /// The defragment in flight, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let job = self.running.as_ref()?;
        let snap = job.progress.snapshot();
        Some(RunningJob {
            kind: JobKind::Defragment,
            doing: job.doing.clone(),
            fraction: (snap.total > 0).then(|| snap.fraction()),
            detail: tr!(
                "reading file {attempted} of {total}",
                attempted = snap.attempted,
                total = snap.total
            ),
            stopping: job.cancel.is_set(),
        })
    }

    /// ⚠ A rebuild reads every file before it writes, so it stops once the reading ends, and a write under way finishes.
    pub fn cancel(&self) {
        if let Some(job) = &self.running {
            job.cancel.set();
        }
    }

    /// Run `work` on `path` as the one defragment.
    pub(super) fn start(
        &mut self,
        path: PathBuf,
        doing: String,
        work: impl FnOnce(&Progress, &Cancel) -> Outcome + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let (cancel, progress) = (Cancel::new(), Progress::new());
        let (worker_cancel, worker_progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work(&worker_progress, &worker_cancel));
        });
        self.running = Some(Running {
            path,
            doing,
            cancel,
            progress,
            receiver: rx,
        });
    }
}

fn stamp(path: &Path) -> Result<Stamp, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((meta.len(), meta.modified().ok()))
}

/// The file's stamp, taken before it is read, and its rebuild.
fn plan_from_disk(
    path: &Path,
    crypto: &Arc<dyn RpfCryptoProvider>,
    progress: &Progress,
) -> Result<(Stamp, Defrag), String> {
    let before = stamp(path)?;
    let archive = open_archive_from_disk(path, Arc::clone(crypto))?;
    let total = archive.file_count() as u64;
    progress.reached(0, total);
    let plan = Defrag::plan(&archive, |n, _| progress.reached(n as u64 + 1, total))
        .map_err(|e| e.to_string())?;
    Ok((before, plan))
}

impl AppState {
    /// The file behind an archive node the explorer found on disk.
    pub fn defrag_target(&self, doc_id: DocumentId, node_id: RpfNodeId) -> Option<PathBuf> {
        let DocPayload::Rpf(rpf) = &self.document(doc_id)?.payload else {
            return None;
        };
        match &rpf.session.node(node_id)?.backing {
            RpfNodeBacking::Archive {
                source: ArchiveSource::Disk(Some(path)),
                ..
            } => Some(path.clone()),
            _ => None,
        }
    }

    /// Work out what defragmenting an archive would reclaim, if the edit gate permits it.
    pub fn defragment_archive(&mut self, doc_id: DocumentId, node_id: RpfNodeId) {
        let Some(path) = self.defrag_target(doc_id, node_id) else {
            return;
        };
        if self.defrag_busy() {
            return;
        }
        let action = PendingRetailEdit::Defragment { doc_id, node_id };
        if self
            .gate_tree_edit(Some(&path), action, "defragment")
            .is_ok()
        {
            self.defragment_archive_inner(doc_id, node_id);
        }
    }

    /// The archive's crypto, or `None` once a refusal is toasted.
    ///
    /// ⚠ The rebuild reads the file, not the session: unsaved edits would be
    /// lost, and an open tab would point into a tree the write replaces. A job
    /// reading the file reads it at the offsets it was planned with.
    fn defrag_crypto(
        &mut self,
        doc_id: DocumentId,
        node_id: RpfNodeId,
    ) -> Option<Arc<dyn RpfCryptoProvider>> {
        let Some(DocPayload::Rpf(rpf)) = self.document(doc_id).map(|d| &d.payload) else {
            return None;
        };
        let session = &rpf.session;
        let inside = |id: RpfNodeId| session.is_descendant(id, node_id);
        let read = self
            .defrag_target(doc_id, node_id)
            .and_then(|path| self.no_job_holds(&path, tr!("defragment it")).err());
        let refusal = if session
            .dirty_archive_ids()
            .into_iter()
            .filter_map(|id| session.archive_root_node(id))
            .any(inside)
        {
            tr!("Save or discard the archive's edits before defragmenting it").to_string()
        } else if rpf.entry_documents.keys().copied().any(inside) {
            tr!("Close the tabs opened from inside the archive before defragmenting it").to_string()
        } else if let Some(why) = read {
            why
        } else {
            return Some(Arc::clone(&session.crypto));
        };
        self.show_error_toast(refusal);
        None
    }

    /// Whether a defragment is in flight or on offer, toasted if so.
    ///
    /// ⚠ A second start would replace the one running, dropping its hold
    /// while its worker still writes; a base-game prompt can be confirmed
    /// long after it was raised, so the tail asks again.
    fn defrag_busy(&mut self) -> bool {
        let busy = self.defrag.is_running() || self.defrag.offer.is_some();
        if busy {
            self.show_error_toast(tr!("A defragment is already under way"));
        }
        busy
    }

    /// The tail of [`defragment_archive`](Self::defragment_archive), once the gate is satisfied.
    pub(super) fn defragment_archive_inner(&mut self, doc_id: DocumentId, node_id: RpfNodeId) {
        if self.defrag_busy() {
            return;
        }
        let Some(path) = self.defrag_target(doc_id, node_id) else {
            return;
        };
        let Some(crypto) = self.defrag_crypto(doc_id, node_id) else {
            return;
        };
        let worker_path = path.clone();
        let doing = tr!(
            "Working out what defragmenting {path} reclaims",
            path = file_name(&path)
        );
        self.defrag.start(path, doing, move |progress, cancel| {
            let planned = plan_from_disk(&worker_path, &crypto, progress);
            if cancel.is_set() {
                return Outcome::Stopped;
            }
            Outcome::Planned(planned.map(|((before, modified), plan)| DefragOffer {
                doc_id,
                node_id,
                path: worker_path,
                before,
                after: plan.planned,
                modified,
                crypto,
            }))
        });
    }

    /// Rebuild and write the offered archive, or drop the offer; the gate and the refusals are asked again.
    pub fn answer_defrag(&mut self, accept: bool) {
        let Some(offer) = self.defrag.offer.take() else {
            return;
        };
        if !accept {
            return;
        }
        let action = PendingRetailEdit::Defragment {
            doc_id: offer.doc_id,
            node_id: offer.node_id,
        };
        if self
            .gate_tree_edit(Some(&offer.path), action, "defragment")
            .is_err()
            || self.defrag_crypto(offer.doc_id, offer.node_id).is_none()
        {
            return;
        }
        let doing = tr!("Defragmenting {path}", path = file_name(&offer.path));
        self.defrag
            .start(offer.path.clone(), doing, move |progress, cancel| {
                let path = &offer.path;
                let previewed = (offer.before, offer.modified);
                let planned = plan_from_disk(path, &offer.crypto, progress);
                if cancel.is_set() {
                    return Outcome::Stopped;
                }
                Outcome::Written(planned.and_then(|(before, plan)| {
                    // ⚠ A rebuild of a file that changed since the preview would
                    // write the old contents over the change.
                    if before != previewed || stamp(path)? != previewed {
                        return Err(tr!(
                            "{path} changed since the preview read it; nothing was written",
                            path = path.display()
                        ));
                    }
                    let took_backup = plan.write_over(path).map_err(|e| e.to_string())?;
                    Ok(Written {
                        doc_id: offer.doc_id,
                        node_id: offer.node_id,
                        path: offer.path.clone(),
                        before: offer.before,
                        after: plan.planned,
                        took_backup,
                    })
                }))
            });
    }

    /// Where a finished preview or write lands, each frame.
    pub fn poll_defrag(&mut self) {
        let Some(job) = &self.defrag.running else {
            return;
        };
        let outcome = match job.receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Outcome::Planned(Err(tr!(
                "The defragment stopped without reporting"
            )
            .to_string())),
        };
        self.defrag.running = None;
        match outcome {
            Outcome::Planned(Ok(offer)) => self.defrag.offer = Some(offer),
            Outcome::Stopped => {
                self.jobs.record(
                    JobKind::Defragment,
                    Ended::Stopped(Then::Nothing),
                    tr!("Nothing was written"),
                );
            }
            Outcome::Written(Ok(done)) => {
                // ⚠ Every offset the session read from this file is stale now.
                self.expander.files_replaced();
                self.file_replaced(&done.path);
                if let Some(DocPayload::Rpf(rpf)) =
                    self.document_mut(done.doc_id).map(|d| &mut d.payload)
                {
                    rpf.session.unload_archive(done.node_id);
                }
                self.request_expand(done.doc_id, done.node_id);
                let backup = if done.took_backup {
                    tr!("; the file as it was is kept as .rpf.bak")
                } else {
                    ""
                };
                let said = tr!(
                    "Defragmented {path}: {size} to {size_2}{backup}",
                    backup = backup,
                    path = done.path.display(),
                    size = or_job::human_bytes(done.before),
                    size_2 = or_job::human_bytes(done.after)
                );
                self.show_toast(said.clone());
                let folder = done.path.parent().map(Path::to_path_buf);
                let then = folder.map_or(Then::Nothing, Then::OpenFolder);
                self.jobs
                    .record(JobKind::Defragment, Ended::Done(then), said);
            }
            Outcome::Planned(Err(e)) | Outcome::Written(Err(e)) => {
                self.show_error_toast(tr!("Defragment: {e}", e = e));
                self.jobs.record(JobKind::Defragment, Ended::Failed, e);
            }
        }
    }
}

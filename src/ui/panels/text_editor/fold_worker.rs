use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;

use ropey::Rope;

use super::folds::{compute_xml_folds, FoldRange};

pub struct FoldJob {
    pub gen: u64,
    pub rope: Arc<Rope>,
    pub cancel: Arc<AtomicBool>,
}

pub struct FoldUpdate {
    pub gen: u64,
    pub ranges: Vec<FoldRange>,
    pub tag_indices: Vec<u32>,
    pub tag_pool: Vec<String>,
}

pub struct FoldWorker {
    job_tx: SyncSender<FoldJob>,
    update_rx: Receiver<FoldUpdate>,
    _thread: thread::JoinHandle<()>,
}

impl FoldWorker {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = sync_channel::<FoldJob>(2);
        let (update_tx, update_rx) = sync_channel::<FoldUpdate>(4);
        let thread = thread::Builder::new()
            .name("text-editor-folds".into())
            .spawn(move || worker_loop(job_rx, update_tx))
            .expect("spawn fold worker thread");
        Self {
            job_tx,
            update_rx,
            _thread: thread,
        }
    }

    pub fn dispatch(&self, job: FoldJob) -> bool {
        self.job_tx.send(job).is_ok()
    }

    pub fn try_recv(&self) -> Option<FoldUpdate> {
        self.update_rx.try_recv().ok()
    }
}

fn worker_loop(job_rx: Receiver<FoldJob>, update_tx: SyncSender<FoldUpdate>) {
    while let Ok(job) = job_rx.recv() {
        let (ranges, tag_indices, tag_pool) =
            compute_xml_folds(&job.rope, &job.cancel, &mut |_| {});
        if job.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let _ = update_tx.send(FoldUpdate {
            gen: job.gen,
            ranges,
            tag_indices,
            tag_pool,
        });
    }
}

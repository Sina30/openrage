use std::ops::ControlFlow;
use std::ops::Range;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ropey::Rope;

use super::search::{find_iter, Query, MATCH_BATCH};

pub struct SearchJob {
    pub gen: u64,
    pub rope: Arc<Rope>,
    pub scope: Range<usize>,
    pub query: Query,
    pub cancel: Arc<AtomicBool>,
}

pub struct SearchUpdate {
    pub gen: u64,
    pub batch: Vec<Range<usize>>,
    pub done: bool,
    pub truncated: bool,
}

pub struct SearchWorker {
    job_tx: SyncSender<SearchJob>,
    update_rx: Receiver<SearchUpdate>,
    _thread: thread::JoinHandle<()>,
}

impl SearchWorker {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = sync_channel::<SearchJob>(2);
        let (update_tx, update_rx) = sync_channel::<SearchUpdate>(8);
        let thread = thread::Builder::new()
            .name("text-editor-search".into())
            .spawn(move || worker_loop(job_rx, update_tx))
            .expect("spawn search worker thread");
        Self {
            job_tx,
            update_rx,
            _thread: thread,
        }
    }

    pub fn dispatch(&self, job: SearchJob) -> bool {
        match self.job_tx.try_send(job) {
            Ok(()) => true,
            Err(TrySendError::Full(job)) => self.job_tx.send(job).is_ok(),
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn try_recv(&self) -> Option<SearchUpdate> {
        self.update_rx.try_recv().ok()
    }
}

fn worker_loop(job_rx: Receiver<SearchJob>, update_tx: SyncSender<SearchUpdate>) {
    while let Some(mut job) = recv_latest(&job_rx) {
        run_one_job(&mut job, &update_tx);
    }
}

fn recv_latest(rx: &Receiver<SearchJob>) -> Option<SearchJob> {
    let mut latest = match rx.recv() {
        Ok(j) => j,
        Err(_) => return None,
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok(newer) => {
                latest
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                latest = newer;
            }
            Err(RecvTimeoutError::Timeout) => return Some(latest),
            Err(RecvTimeoutError::Disconnected) => return Some(latest),
        }
    }
}

fn run_one_job(job: &mut SearchJob, update_tx: &SyncSender<SearchUpdate>) {
    let gen = job.gen;
    let mut total_emitted = 0usize;
    let send_batch = |batch: &[Range<usize>]| -> ControlFlow<()> {
        let upd = SearchUpdate {
            gen,
            batch: batch.to_vec(),
            done: false,
            truncated: false,
        };
        match update_tx.send(upd) {
            Ok(()) => ControlFlow::Continue(()),
            Err(_) => ControlFlow::Break(()),
        }
    };

    let outcome = find_iter(
        &job.rope,
        job.scope.clone(),
        &job.query,
        &job.cancel,
        &mut |b| {
            total_emitted += b.len();
            send_batch(b)
        },
    );

    let _ = update_tx.send(SearchUpdate {
        gen,
        batch: Vec::new(),
        done: true,
        truncated: outcome.truncated,
    });

    let _ = (total_emitted, MATCH_BATCH);
}

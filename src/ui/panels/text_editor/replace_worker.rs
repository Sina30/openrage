use std::ops::ControlFlow;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;

use ropey::{Rope, RopeBuilder};

use super::search::{find_iter, Query};

pub struct ReplaceJob {
    pub gen: u64,
    pub rope: Arc<Rope>,
    pub query: Query,
    pub replacement: String,
    pub regex_replacement: bool,
    pub cancel: Arc<AtomicBool>,
}

/// A finished or cancelled replace; `after` is `None` when it was cancelled.
pub struct ReplaceUpdate {
    pub gen: u64,
    pub match_count: usize,
    pub before: Arc<Rope>,
    pub after: Option<Arc<Rope>>,
}

pub struct ReplaceWorker {
    job_tx: SyncSender<ReplaceJob>,
    update_rx: Receiver<ReplaceUpdate>,
    _thread: thread::JoinHandle<()>,
}

impl ReplaceWorker {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = sync_channel::<ReplaceJob>(2);
        let (update_tx, update_rx) = sync_channel::<ReplaceUpdate>(8);
        let thread = thread::Builder::new()
            .name("text-editor-replace".into())
            .spawn(move || worker_loop(job_rx, update_tx))
            .expect("spawn replace worker thread");
        Self {
            job_tx,
            update_rx,
            _thread: thread,
        }
    }

    pub fn dispatch(&self, job: ReplaceJob) -> bool {
        self.job_tx.send(job).is_ok()
    }

    pub fn try_recv(&self) -> Option<ReplaceUpdate> {
        self.update_rx.try_recv().ok()
    }
}

fn worker_loop(job_rx: Receiver<ReplaceJob>, update_tx: SyncSender<ReplaceUpdate>) {
    while let Ok(job) = job_rx.recv() {
        run_one_job(job, &update_tx);
    }
}

fn run_one_job(job: ReplaceJob, update_tx: &SyncSender<ReplaceUpdate>) {
    let rope = job.rope.clone();

    let mut matches: Vec<std::ops::Range<usize>> = Vec::new();
    let _ = find_iter(
        &rope,
        0..rope.len_chars(),
        &job.query,
        &job.cancel,
        &mut |b| {
            matches.extend_from_slice(b);
            ControlFlow::Continue(())
        },
    );

    if job.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = update_tx.send(ReplaceUpdate {
            gen: job.gen,
            match_count: 0,
            before: rope,
            after: None,
        });
        return;
    }

    let mut builder = RopeBuilder::new();
    let mut prev_end_char = 0usize;
    let len_chars = rope.len_chars();

    let regex_for_expand: Option<&regex::Regex> = match &job.query {
        Query::Regex(r) if job.regex_replacement => Some(r),
        _ => None,
    };

    for m in &matches {
        if job.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = update_tx.send(ReplaceUpdate {
                gen: job.gen,
                match_count: 0,
                before: rope,
                after: None,
            });
            return;
        }
        if prev_end_char < m.start {
            append_slice(&mut builder, &rope, prev_end_char, m.start);
        }
        if let Some(re) = regex_for_expand {
            let matched: String = rope.slice(m.start..m.end).to_string();
            if let Some(caps) = re.captures(&matched) {
                let mut buf = String::new();
                caps.expand(&job.replacement, &mut buf);
                builder.append(&buf);
            } else {
                builder.append(&job.replacement);
            }
        } else {
            builder.append(&job.replacement);
        }
        prev_end_char = m.end;
    }
    if prev_end_char < len_chars {
        append_slice(&mut builder, &rope, prev_end_char, len_chars);
    }
    let after = Arc::new(builder.finish());
    let _ = update_tx.send(ReplaceUpdate {
        gen: job.gen,
        match_count: matches.len(),
        before: rope,
        after: Some(after),
    });
}

fn append_slice(builder: &mut RopeBuilder, rope: &Rope, start_char: usize, end_char: usize) {
    if end_char <= start_char {
        return;
    }
    let slice = rope.slice(start_char..end_char);
    for chunk in slice.chunks() {
        builder.append(chunk);
    }
}

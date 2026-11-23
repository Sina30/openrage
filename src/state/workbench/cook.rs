//! Live recook: a saved file inside a source folder cooks that one asset into the project's shadow folder.
//!
//! ⚠ **The cook runs on a worker and the watcher only says what changed.** A failure is data the
//! Problems panel lists, and the shadow folder keeps the previous output (`or_source::cook`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use or_source::cook::{Cooker, Recook};
use or_source::Problem;

/// How long a path stays quiet before it is cooked: an editor saves in several writes.
const QUIET: Duration = Duration::from_millis(300);

/// What the last cook of a source said.
#[derive(Debug, Clone, PartialEq)]
pub enum Cook {
    Cooked,
    Failed(Vec<Problem>),
}

/// The cooker's worker, the watcher that feeds it and what each source last cooked to.
pub struct Cooks {
    to_worker: Sender<PathBuf>,
    from_worker: Receiver<Recook>,
    events: Receiver<notify::Result<notify::Event>>,
    watcher: Option<RecommendedWatcher>,
    watched: HashSet<PathBuf>,
    /// Paths that changed, and when they last did.
    pending: HashMap<PathBuf, Instant>,
    cooked: HashMap<PathBuf, Cook>,
    /// Paths handed to the worker that it has not answered yet.
    #[cfg(test)]
    asked: usize,
}

impl Cooks {
    /// A cooker that writes into `shadow`, with no folder watched yet.
    pub fn new(shadow: PathBuf) -> Self {
        let (to_worker, jobs) = channel::<PathBuf>();
        let (results, from_worker) = channel();
        std::thread::spawn(move || {
            let mut cooker = Cooker::new(shadow);
            // Ends when the sender is dropped with the project.
            while let Ok(path) = jobs.recv() {
                if results.send(cooker.recook(&path)).is_err() {
                    break;
                }
            }
        });
        let (sender, events) = channel();
        let watcher = RecommendedWatcher::new(
            move |event| {
                let _ = sender.send(event);
            },
            notify::Config::default(),
        )
        .map_err(
            |e| tracing::warn!(error = %e, "no file watcher, so a saved image is not recooked"),
        )
        .ok();
        Self {
            to_worker,
            from_worker,
            events,
            watcher,
            watched: HashSet::new(),
            pending: HashMap::new(),
            cooked: HashMap::new(),
            #[cfg(test)]
            asked: 0,
        }
    }

    /// Stop hearing the disk, so that only [`Cooks::saved`] says what changed.
    ///
    /// ⚠ A test that tells the cook of a write itself needs it: the watcher reports the same write again, whenever it gets to it.
    #[cfg(test)]
    pub fn stop_listening(&mut self) {
        self.watcher = None;
    }

    /// Whether no path waits for its quiet time and the worker has answered every path it was given.
    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.asked == 0
    }

    /// Watch a source folder, and everything in it, for saves.
    pub fn watch(&mut self, dir: &Path) {
        if !self.watched.insert(dir.to_path_buf()) {
            return;
        }
        if let Some(watcher) = self.watcher.as_mut() {
            if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                tracing::warn!(dir = %dir.display(), error = %e, "a source folder is not watched");
            }
        }
    }

    /// Note that `path` changed at `now`: what the watcher does, and what a Recook asks.
    pub fn saved(&mut self, path: PathBuf, now: Instant) {
        self.pending.insert(path, now);
    }

    /// The cook of `dir`, the source folder, as it last ended.
    pub fn state(&self, dir: &Path) -> Option<&Cook> {
        self.cooked.get(dir)
    }

    /// The cook of `source` wrote a file that an open tab could not read again: its row reads Failed and Problems says why.
    pub fn reload_failed(&mut self, source: &Path, why: String) {
        self.cooked
            .insert(source.to_path_buf(), Cook::Failed(vec![Problem::new(why)]));
    }

    /// Hand the paths that have been quiet to the worker, and take what it has cooked since.
    pub fn poll(&mut self, now: Instant) -> Vec<Recook> {
        for event in self.events.try_iter().flatten() {
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                for path in event.paths {
                    self.pending.insert(path, now);
                }
            }
        }
        let due: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= QUIET)
            .map(|(path, _)| path.clone())
            .collect();
        for path in due {
            self.pending.remove(&path);
            let _ = self.to_worker.send(path);
            #[cfg(test)]
            {
                self.asked += 1;
            }
        }
        let mut done = Vec::new();
        for recook in self.from_worker.try_iter() {
            #[cfg(test)]
            {
                self.asked -= 1;
            }
            match &recook {
                Recook::Ignored(_) => continue,
                Recook::Unchanged { source } | Recook::Cooked { source, .. } => {
                    self.cooked.insert(source.clone(), Cook::Cooked);
                }
                Recook::Failed { source, problems } => {
                    self.cooked
                        .insert(source.clone(), Cook::Failed(problems.clone()));
                }
            }
            done.push(recook);
        }
        done
    }
}

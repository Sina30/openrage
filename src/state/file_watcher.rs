use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
    rx: Option<Receiver<notify::Result<Event>>>,
    entries: Arc<Mutex<HashMap<PathBuf, Pending>>>,
    debounce: Duration,
}

#[derive(Debug, Default)]
struct Pending {
    last_event: Option<Instant>,
    observed: bool,
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FileWatcher {
    pub fn new() -> Self {
        let entries: Arc<Mutex<HashMap<PathBuf, Pending>>> = Arc::default();
        let (tx, rx): (Sender<notify::Result<Event>>, _) = channel();
        let watcher: Option<RecommendedWatcher> = match RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        ) {
            Ok(w) => Some(w),
            Err(err) => {
                tracing::warn!(error = %err, "notify watcher init failed; live reload disabled");
                None
            }
        };
        Self {
            watcher,
            rx: Some(rx),
            entries,
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    pub fn watch(&mut self, path: PathBuf) {
        {
            let mut entries = self.entries.lock().unwrap();
            if entries.contains_key(&path) {
                return;
            }
            entries.insert(path.clone(), Pending::default());
        }
        if let Some(w) = self.watcher.as_mut() {
            let target = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            if let Err(err) = w.watch(&target, RecursiveMode::NonRecursive) {
                tracing::warn!(?target, error = %err, "notify watch failed");
            }
        }
    }

    pub fn poll_at(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(rx) = self.rx.as_ref() {
            while let Ok(res) = rx.try_recv() {
                let Ok(event) = res else { continue };
                if !is_write_event(&event.kind) {
                    continue;
                }
                for evt_path in &event.paths {
                    let canon = canonical_or_self(evt_path);
                    for (watched, pending) in entries.iter_mut() {
                        if same_path(watched, &canon) {
                            pending.last_event = Some(now);
                            pending.observed = true;
                        }
                    }
                }
            }
        }
        let mut changed = Vec::new();
        for (path, pending) in entries.iter_mut() {
            if !pending.observed {
                continue;
            }
            let Some(last) = pending.last_event else {
                continue;
            };
            if now.saturating_duration_since(last) >= self.debounce {
                changed.push(path.clone());
                pending.last_event = None;
            }
        }
        changed
    }

    pub fn poll(&mut self) -> Vec<PathBuf> {
        self.poll_at(Instant::now())
    }
}

fn is_write_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn canonical_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let ca = std::fs::canonicalize(a).ok();
    let cb = std::fs::canonicalize(b).ok();
    match (ca, cb) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn baseline_seed_emits_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.ypt");
        std::fs::write(&path, b"v1").unwrap();
        let mut w = FileWatcher::new();
        w.debounce = Duration::from_millis(0);
        w.watch(path.clone());
        let changes = w.poll_at(Instant::now());
        assert!(changes.is_empty(), "got unexpected changes: {:?}", changes);
    }

    #[test]
    fn write_then_debounce_emits_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.ypt");
        std::fs::write(&path, b"v1").unwrap();
        let mut w = FileWatcher::new();
        w.debounce = Duration::from_millis(0);
        w.watch(path.clone());
        let _ = w.poll_at(Instant::now());
        std::thread::sleep(Duration::from_millis(50));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        f.write_all(b"v2").unwrap();
        f.sync_all().unwrap();
        drop(f);
        std::thread::sleep(Duration::from_millis(150));
        let surfaced = w.poll_at(Instant::now());
        if surfaced.is_empty() {
            return;
        }
        assert_eq!(surfaced.len(), 1, "expected one reload, got {:?}", surfaced);
        let stable = w.poll_at(Instant::now() + Duration::from_millis(50));
        assert!(stable.is_empty(), "got duplicate reload: {:?}", stable);
    }
}

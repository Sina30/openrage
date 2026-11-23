//! A content search over the active install, on a worker thread.
//!
//! ⚠ The UI thread never touches the walk: a search over a retail install reads
//! hundreds of megabytes, and doing it inline is a frozen window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::needle::Needle;
use or_title::install::content::{search_contents, ContentHit, Progress, Query, Scope};
use or_title::install::LOOSE;

use super::jobs::{JobKind, RunningJob};
use super::memory::vec_bytes;
use super::navigation::Arrival;
use super::Selection;

/// What the user asked for, before it becomes a `Needle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Text,
    /// UTF-16 little-endian, which is how RAGE stores wide strings.
    Utf16,
    Hex,
}

impl SearchMode {
    pub const ALL: [SearchMode; 3] = [SearchMode::Text, SearchMode::Utf16, SearchMode::Hex];

    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Text => "Text",
            SearchMode::Utf16 => "UTF-16",
            SearchMode::Hex => "Hex",
        }
    }
}

/// What the search box currently holds, kept out of the session so editing
/// the query does not disturb a run in progress.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub pattern: String,
    pub mode: SearchMode,
    pub case_sensitive: bool,
    pub extensions: String,
    pub under: String,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            mode: SearchMode::Text,
            case_sensitive: false,
            extensions: String::new(),
            under: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub pattern: String,
    pub mode: SearchMode,
    pub case_sensitive: bool,
    /// Extensions to open, already split and lowercased. Empty means all.
    pub extensions: Vec<String>,
    /// Container or path prefix, or empty for the whole install.
    pub under: String,
    pub max_hits: usize,
}

impl SearchRequest {
    pub fn needle(&self) -> Result<Needle, String> {
        if self.pattern.trim().is_empty() {
            return Err("nothing to search for".into());
        }
        Ok(match self.mode {
            SearchMode::Text => Needle::text(&self.pattern, self.case_sensitive),
            SearchMode::Utf16 => Needle::utf16(&self.pattern, self.case_sensitive),
            SearchMode::Hex => Needle::hex(&self.pattern)?,
        })
    }

    fn scope(&self) -> Scope {
        let under = self.under.trim().replace('\\', "/");
        if under.is_empty() {
            Scope::Everywhere
        } else {
            Scope::Under(under)
        }
    }
}

/// Split a comma-separated extension filter the way the CLI does.
pub fn split_extensions(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

enum Message {
    Hit(Box<ContentHit>),
    Progress(Progress),
    Done(Progress),
}

#[derive(Default)]
pub struct SearchSession {
    pub hits: Vec<ContentHit>,
    pub progress: Progress,
    pub error: Option<String>,
    /// The hits selected in the panel, by index.
    pub selection: Selection,
    /// The install the hits were found in.
    pub root: Option<PathBuf>,
    /// What was searched for, as the user typed it.
    asked: String,
    rx: Option<Receiver<Message>>,
    handle: Option<JoinHandle<()>>,
    cancel: Option<Arc<AtomicBool>>,
}

/// How many hits the panel keeps. A whole-install text search for `a` finds
/// hundreds of thousands, and a list that long is neither useful nor cheap.
const MAX_KEPT_HITS: usize = 5_000;

impl SearchSession {
    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn start(
        &mut self,
        request: SearchRequest,
        install_root: PathBuf,
        provider: Arc<dyn RpfCryptoProvider>,
    ) {
        self.stop();
        let needle = match request.needle() {
            Ok(needle) => needle,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };

        self.hits.clear();
        self.progress = Progress::default();
        self.error = None;
        self.selection = Selection::default();
        self.root = Some(install_root.clone());
        self.asked = request.pattern.clone();

        let (tx, rx): (Sender<Message>, Receiver<Message>) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.rx = Some(rx);
        self.cancel = Some(Arc::clone(&cancel));
        self.handle = Some(std::thread::spawn(move || {
            run(request, needle, install_root, provider, tx, cancel);
        }));
    }

    /// Ask the worker to stop. It stops the walk rather than finishing it
    /// quietly, so a cancelled whole-install search costs what it has spent
    /// so far and nothing more.
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let _ = self.handle.take();
        self.rx = None;
    }

    /// The hits kept, each with its path, its offsets and its preview.
    pub fn held_bytes(&self) -> usize {
        let hits: usize = (self.hits.iter())
            .map(|hit| {
                let preview = hit.preview.as_ref().map_or(0, String::capacity);
                hit.path.capacity() + hit.container.capacity() + vec_bytes(&hit.offsets) + preview
            })
            .sum();
        vec_bytes(&self.hits) + hits + self.asked.capacity()
    }

    pub fn asked(&self) -> &str {
        &self.asked
    }

    /// The search in flight, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        self.is_running().then(|| RunningJob {
            kind: JobKind::Search,
            doing: format!("Searching the install for {}", self.asked),
            fraction: None,
            detail: format!(
                "{} file(s) looked at, {} hit(s)",
                self.progress.files_seen, self.progress.hits
            ),
            stopping: false,
        })
    }

    /// Drain whatever the worker has produced since the last frame; `true` once, when the search has finished.
    pub fn poll(&mut self) -> bool {
        let mut finished = false;
        if let Some(rx) = self.rx.as_ref() {
            loop {
                match rx.try_recv() {
                    Ok(Message::Hit(hit)) => {
                        if self.hits.len() < MAX_KEPT_HITS {
                            self.hits.push(*hit);
                        }
                    }
                    Ok(Message::Progress(progress)) => self.progress = progress,
                    Ok(Message::Done(progress)) => {
                        self.progress = progress;
                        finished = true;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished {
            self.rx = None;
            self.cancel = None;
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
        finished
    }

    pub fn truncated(&self) -> bool {
        self.hits.len() >= MAX_KEPT_HITS
    }

    /// The results as a tab-separated table, for the clipboard or a file.
    pub fn export(&self) -> String {
        let mut out = String::from("path\tcontainer\toffsets\tsize\n");
        for hit in &self.hits {
            let offsets: Vec<String> = hit.offsets.iter().map(|o| format!("0x{o:X}")).collect();
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                hit.path,
                hit.container,
                offsets.join(" "),
                hit.file_size
            ));
        }
        out
    }
}

impl Drop for SearchSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where a hit sits under the install root, as the tree names it.
pub fn hit_path(hit: &ContentHit) -> String {
    if hit.container == LOOSE {
        hit.path.clone()
    } else {
        format!("{}/{}", hit.container, hit.path)
    }
}

impl super::AppState {
    /// Go to the hits at `rows` in the install they were found in, then do what `then` says.
    pub fn go_to_search_hits(&mut self, rows: &[usize], then: Arrival) {
        let Some(root) = self.search.root.clone() else {
            return;
        };
        let inners = rows
            .iter()
            .filter_map(|&row| self.search.hits.get(row))
            .map(hit_path)
            .collect();
        self.walk_to(&root, inners, then);
    }
}

fn run(
    request: SearchRequest,
    needle: Needle,
    install_root: PathBuf,
    provider: Arc<dyn RpfCryptoProvider>,
    tx: Sender<Message>,
    cancel: Arc<AtomicBool>,
) {
    let scope = request.scope();
    let query = Query {
        needle: &needle,
        scope: &scope,
        extensions: &request.extensions,
        max_file_bytes: 64 << 20,
        hits_per_file: 64,
        preview_bytes: 64,
        max_hits: request.max_hits,
    };
    let mut warnings = Vec::new();
    let mut last_report = 0usize;
    let progress = search_contents(
        &install_root,
        provider,
        &query,
        &cancel,
        &mut warnings,
        &mut |hit, progress| {
            let _ = tx.send(Message::Hit(Box::new(hit.clone())));
            if progress.files_seen - last_report > 2_000 {
                last_report = progress.files_seen;
                let _ = tx.send(Message::Progress(progress.clone()));
            }
        },
    );
    for warning in warnings {
        tracing::debug!(%warning, "content search");
    }
    let _ = tx.send(Message::Done(progress));
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;

    fn request(pattern: &str, mode: SearchMode) -> SearchRequest {
        SearchRequest {
            pattern: pattern.into(),
            mode,
            case_sensitive: false,
            extensions: Vec::new(),
            under: String::new(),
            max_hits: 0,
        }
    }

    #[test]
    fn a_mode_decides_which_needle_the_pattern_becomes() {
        assert!(request("railgun", SearchMode::Text).needle().is_ok());
        assert!(request("railgun", SearchMode::Utf16).needle().is_ok());
        assert!(request("52504637", SearchMode::Hex).needle().is_ok());

        // ⚠ A user in hex mode typing a word gets told, rather than a search
        // for nothing that quietly finds nothing.
        let err = request("railgun", SearchMode::Hex).needle().unwrap_err();
        assert!(err.contains("hex"), "{err}");
        assert!(request("  ", SearchMode::Text).needle().is_err());
    }

    #[test]
    fn extensions_are_split_the_way_the_cli_splits_them() {
        assert_eq!(split_extensions(".Meta, YPT ,,ytd"), ["meta", "ypt", "ytd"]);
        assert!(split_extensions("").is_empty());
    }

    #[test]
    fn a_scope_is_empty_or_a_prefix_with_forward_slashes() {
        assert_eq!(request("x", SearchMode::Text).scope(), Scope::Everywhere);
        let mut req = request("x", SearchMode::Text);
        req.under = "  x64\\audio ".into();
        assert_eq!(req.scope(), Scope::Under("x64/audio".into()));
    }

    #[test]
    fn a_search_runs_to_completion_off_the_calling_thread() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.meta"), b"WEAPON_RAILGUN").unwrap();
        std::fs::write(dir.path().join("b.meta"), b"nothing").unwrap();

        let mut session = SearchSession::default();
        session.start(
            request("railgun", SearchMode::Text),
            dir.path().to_path_buf(),
            Arc::new(NoOpCryptoProvider::new()),
        );
        assert!(session.is_running());

        for _ in 0..300 {
            session.poll();
            if !session.is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(!session.is_running(), "the search never finished");
        assert_eq!(session.hits.len(), 1);
        assert_eq!(session.hits[0].path, "a.meta");
        assert_eq!(session.progress.hits, 1);
        assert!(!session.progress.stopped);
        assert!(session.export().contains("a.meta"));
        assert!(session.export().starts_with("path\tcontainer"));
    }

    #[test]
    fn a_bad_pattern_is_reported_rather_than_started() {
        let mut session = SearchSession::default();
        session.start(
            request("not hex", SearchMode::Hex),
            PathBuf::from("."),
            Arc::new(NoOpCryptoProvider::new()),
        );
        assert!(!session.is_running());
        assert!(session.error.is_some());
        assert!(session.hits.is_empty());
    }

    #[test]
    fn stopping_ends_the_session_without_waiting_for_the_walk() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..200 {
            std::fs::write(dir.path().join(format!("f{i:03}.meta")), b"railgun").unwrap();
        }
        let mut session = SearchSession::default();
        session.start(
            request("railgun", SearchMode::Text),
            dir.path().to_path_buf(),
            Arc::new(NoOpCryptoProvider::new()),
        );
        session.stop();
        assert!(!session.is_running());
    }
}

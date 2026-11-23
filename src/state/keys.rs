use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;

use or_io::container::rpf::crypto::extractor::ProgressCb;
use or_io::container::rpf::crypto::{
    extract_keys_from_magic, extract_keys_with_snapshot, ExtractionMode,
};
use or_io::container::{
    fingerprint_executable, EncryptDeriveProgress, ExtractionProgress, Gta5Keys,
    Gta5RetailCryptoProvider, KeysCache, NoOpCryptoProvider, RpfCryptoProvider,
};
use or_job::Cancel;

use super::jobs::{JobKind, RunningJob};
use super::prefs::{
    encrypt_cache_path, keys_cache_path_for, keys_cache_path_for_exe, user_snapshot, GtaKeysPrefs,
    Prefs,
};

fn prefs_keys_cache_path(prefs: &Prefs) -> Option<PathBuf> {
    if let Some(exe) = prefs.gta_keys.gta5_path.as_deref() {
        return keys_cache_path_for_exe(exe);
    }
    keys_cache_path_for(prefs.active_game.unwrap_or(or_title::Game::GtaVLegacy))
}

#[derive(Debug, Clone, Default)]
pub struct ExtractionState {
    pub is_running: bool,
    pub stage: String,
    pub found: usize,
    pub total: usize,
    pub last_error: Option<String>,
}

impl ExtractionState {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.found as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

pub enum ExtractionResult {
    Success {
        cache: KeysCache,
        keys: Arc<Gta5Keys>,
    },
    Failure {
        message: String,
    },
    /// The NG encrypt state was derived into the loaded keys; nothing was extracted.
    Derived,
    /// Asked to stop, so it wrote no cache.
    Stopped,
}

/// The one key job on a worker: what it is doing, the flag that stops it, and where it reports.
struct Worker {
    doing: &'static str,
    cancel: Cancel,
    receiver: Receiver<ExtractionProgressOrResult>,
}

pub struct KeysController {
    provider: Arc<dyn RpfCryptoProvider>,
    // Another title's cipher, which takes the place of GTA V's while that title is the active game.
    title_provider: Option<Arc<dyn RpfCryptoProvider>>,
    // What the Games page says of it: where its tables came from, or why there is none.
    title_status: Option<Result<String, String>>,
    keys: Option<Arc<Gta5Keys>>,
    pub extraction: ExtractionState,
    worker: Option<Worker>,
    pub pending_open: Vec<PathBuf>,
    pub show_prompt: bool,
    // The loaded cache was written without NG keys by a build that could not
    // reach the bundled snapshot; cleared once any extraction succeeds.
    cache_predates_snapshot: bool,
}

enum ExtractionProgressOrResult {
    Progress(ExtractionProgress),
    Done(ExtractionResult),
}

impl Default for KeysController {
    fn default() -> Self {
        Self {
            provider: Arc::new(NoOpCryptoProvider::new()),
            title_provider: None,
            title_status: None,
            keys: None,
            extraction: ExtractionState::default(),
            worker: None,
            pending_open: Vec::new(),
            show_prompt: false,
            cache_predates_snapshot: false,
        }
    }
}

impl KeysController {
    pub fn provider(&self) -> Arc<dyn RpfCryptoProvider> {
        Arc::clone(self.title_provider.as_ref().unwrap_or(&self.provider))
    }

    /// Open archives with another title's cipher until it is set to none.
    pub fn set_title_provider(
        &mut self,
        provider: Option<Arc<dyn RpfCryptoProvider>>,
        status: Option<Result<String, String>>,
    ) {
        self.title_provider = provider;
        self.title_status = status;
    }

    /// Another title's cipher: where it came from, or why it could not be built; none for GTA V.
    pub fn title_status(&self) -> Option<&Result<String, String>> {
        self.title_status.as_ref()
    }

    pub fn keys(&self) -> Option<Arc<Gta5Keys>> {
        self.keys.clone()
    }

    pub fn has_keys(&self) -> bool {
        self.keys.is_some()
    }

    /// Whether the loaded keys came from a cache one extraction could add NG keys to.
    pub fn predate_a_snapshot(&self) -> bool {
        self.keys.is_some() && self.cache_predates_snapshot
    }

    pub fn has_encrypt_state(&self) -> bool {
        self.keys
            .as_ref()
            .map(|k| k.has_encrypt_state())
            .unwrap_or(false)
    }

    /// One line per key class, plus the snapshot verdict, for the settings
    /// page. Empty when no keys are loaded.
    pub fn provenance_report(&self) -> Vec<String> {
        self.keys
            .as_ref()
            .map(|k| k.provenance.report())
            .unwrap_or_default()
    }

    pub fn snapshot_disagrees(&self) -> bool {
        self.keys.as_ref().is_some_and(|k| {
            k.provenance.snapshot_agreement
                == or_io::container::rpf::crypto::SnapshotAgreement::Disagrees
        })
    }

    pub fn start_encrypt_state_derivation(&mut self) {
        let keys = match self.keys.as_ref() {
            Some(k) if k.has_ng() && !k.has_encrypt_state() => Arc::clone(k),
            _ => return,
        };
        self.spawn(
            "Deriving the NG encrypt state",
            "Deriving NG encrypt state",
            17,
            move |progress, cancel| run_encrypt_state_derivation(keys, progress, cancel),
        );
    }

    /// Run `work` as the one key job; it reports through the callback and returns its result.
    pub(super) fn spawn(
        &mut self,
        doing: &'static str,
        stage: &str,
        total: usize,
        work: impl FnOnce(&ProgressCb<'_>, &Cancel) -> ExtractionResult + Send + 'static,
    ) {
        if self.worker.is_some() {
            tracing::warn!("A key job is already running; ignoring another");
            return;
        }
        self.extraction = ExtractionState {
            is_running: true,
            stage: stage.into(),
            found: 0,
            total,
            last_error: None,
        };
        let (tx, receiver) = channel();
        let cancel = Cancel::new();
        let worker_cancel = cancel.clone();
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress = move |p: ExtractionProgress| {
                let _ = progress_tx.send(ExtractionProgressOrResult::Progress(p));
            };
            let result = work(&progress, &worker_cancel);
            let _ = tx.send(ExtractionProgressOrResult::Done(result));
        });
        self.worker = Some(Worker {
            doing,
            cancel,
            receiver,
        });
    }

    /// The key job in flight, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let worker = self.worker.as_ref()?;
        let e = &self.extraction;
        Some(RunningJob {
            kind: JobKind::Keys,
            doing: worker.doing.to_string(),
            fraction: (e.total > 0).then(|| e.fraction()),
            detail: if e.total > 0 {
                format!("{}: {} of {}", e.stage, e.found, e.total)
            } else {
                e.stage.clone()
            },
            stopping: worker.cancel.is_set(),
        })
    }

    /// ⚠ The executable is read to its end before the flag is looked at, so the job stops then, writing nothing.
    pub fn cancel(&self) {
        if let Some(worker) = &self.worker {
            worker.cancel.set();
        }
    }

    pub fn try_load_cached(&mut self, prefs: &Prefs) {
        let path = match prefs_keys_cache_path(prefs) {
            Some(p) => p,
            None => return,
        };
        let cache = match KeysCache::read_from(&path) {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read keys cache; will require re-extract");
                return;
            }
        };
        if let Some(stored_sha) = &prefs.gta_keys.gta5_sha1_hex {
            if &cache.metadata.gta5_sha1_hex != stored_sha {
                tracing::warn!(
                    "Cached keys fingerprint mismatch; user must re-extract from gta5.exe"
                );
                return;
            }
        }
        let predates_snapshot = user_snapshot().would_complete(&cache);
        match cache.into_keys() {
            Ok(keys) => {
                let arc = Arc::new(keys);
                self.cache_predates_snapshot = predates_snapshot;
                self.try_install_cached_encrypt_state(&arc);
                self.provider = Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&arc)));
                self.keys = Some(arc);
                tracing::info!("Loaded GTA V keys from cache");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Cached keys decode failed");
            }
        }
    }

    fn try_install_cached_encrypt_state(&self, keys: &Arc<Gta5Keys>) {
        if !keys.has_ng() {
            return;
        }
        let path = match encrypt_cache_path() {
            Some(p) => p,
            None => return,
        };
        let cache = match or_io::container::EncryptCache::read_from(&path) {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read NG encrypt cache; will re-derive");
                return;
            }
        };
        if !cache.matches(&keys.ng_decrypt_tables) {
            tracing::info!(
                "NG encrypt cache fingerprint mismatch (decrypt tables changed); will re-derive"
            );
            return;
        }
        match cache.into_state() {
            Ok(state) => {
                keys.install_encrypt_state(Arc::new(state));
                tracing::info!("Loaded NG encrypt state from cache");
            }
            Err(e) => {
                tracing::warn!(error = %e, "NG encrypt cache decode failed; will re-derive");
            }
        }
    }

    pub fn forget(&mut self, prefs: &mut Prefs) {
        self.provider = Arc::new(NoOpCryptoProvider::new());
        self.keys = None;
        let cache = prefs_keys_cache_path(prefs);
        prefs.gta_keys.forget();
        if let Some(path) = cache {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = encrypt_cache_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn start_extraction(&mut self, gta5_path: PathBuf) {
        self.start_extraction_with(gta5_path, ExtractionMode::Full);
    }

    pub fn start_extraction_aes_only(&mut self, gta5_path: PathBuf) {
        self.start_extraction_with(gta5_path, ExtractionMode::AesOnly);
    }

    pub fn start_extraction_from_magic(&mut self, gta5_path: PathBuf, magic_path: PathBuf) {
        self.spawn(
            "Decoding magic.dat",
            "Decoding magic.dat",
            0,
            move |progress, cancel| {
                run_extraction(&gta5_path, cancel, |exe| {
                    extract_keys_from_magic(exe, &magic_path, progress)
                        .map_err(|e| format!("magic decode from {}: {e}", magic_path.display()))
                })
            },
        );
    }

    fn start_extraction_with(&mut self, gta5_path: PathBuf, mode: ExtractionMode) {
        self.spawn(
            "Reading the keys from the game's executable",
            "Starting",
            0,
            move |progress, cancel| {
                run_extraction(&gta5_path, cancel, |exe| {
                    extract_keys_with_snapshot(exe, mode, &user_snapshot(), progress)
                        .map_err(|e| format!("extract: {e}"))
                })
            },
        );
    }

    pub fn poll(&mut self) -> Option<ExtractionResult> {
        let receiver = &self.worker.as_ref()?.receiver;
        let mut completed: Option<ExtractionResult> = None;
        while let Ok(msg) = receiver.try_recv() {
            match msg {
                ExtractionProgressOrResult::Progress(p) => {
                    self.extraction.stage = p.stage.into();
                    self.extraction.found = p.found;
                    self.extraction.total = p.total;
                }
                ExtractionProgressOrResult::Done(result) => {
                    completed = Some(result);
                    break;
                }
            }
        }

        if let Some(result) = completed.as_ref() {
            self.worker = None;
            self.extraction.is_running = false;
            match result {
                ExtractionResult::Success { keys, .. } => {
                    let arc = Arc::clone(keys);
                    self.cache_predates_snapshot = false;
                    self.try_install_cached_encrypt_state(&arc);
                    self.provider = Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&arc)));
                    self.keys = Some(arc);
                    self.show_prompt = false;
                }
                ExtractionResult::Failure { message, .. } => {
                    self.extraction.last_error = Some(message.clone());
                }
                ExtractionResult::Derived | ExtractionResult::Stopped => {}
            }
        }

        completed
    }

    pub fn queue_pending(&mut self, path: PathBuf) {
        if !self.pending_open.iter().any(|p| p == &path) {
            self.pending_open.push(path);
        }
    }

    pub fn drain_pending(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.pending_open)
    }
}

/// The keys `read` takes from the executable at `gta5_path`, kept in the keys cache unless the job was stopped.
fn run_extraction(
    gta5_path: &Path,
    cancel: &Cancel,
    read: impl FnOnce(&Path) -> Result<Gta5Keys, String>,
) -> ExtractionResult {
    let fingerprint = match fingerprint_executable(gta5_path) {
        Ok(f) => f,
        Err(e) => {
            return ExtractionResult::Failure {
                message: format!("fingerprint: {e}"),
            }
        }
    };
    let keys = match read(gta5_path) {
        Ok(k) => k,
        Err(message) => return ExtractionResult::Failure { message },
    };
    if cancel.is_set() {
        return ExtractionResult::Stopped;
    }

    let cache = KeysCache::build(gta5_path.to_path_buf(), fingerprint, &keys);
    if let Some(path) = keys_cache_path_for_exe(gta5_path) {
        if let Err(e) = cache.write_to(&path) {
            tracing::warn!(error = %e, "Failed to persist keys cache; keys still loaded for this session");
        }
    }
    ExtractionResult::Success {
        cache,
        keys: Arc::new(keys),
    }
}

fn run_encrypt_state_derivation(
    keys: Arc<Gta5Keys>,
    progress: &ProgressCb<'_>,
    cancel: &Cancel,
) -> ExtractionResult {
    let progress_cb = |p: EncryptDeriveProgress| {
        progress(ExtractionProgress {
            stage: p.stage,
            found: p.found,
            total: p.total,
        })
    };

    // ⚠ Derived apart from the keys and installed only once it may be kept: `ensure_encrypt_state`
    // installs as it goes, so a stopped derivation would stay in use, cached nowhere.
    let derived = or_io::container::rpf::crypto::ng_encrypt_state::derive_encrypt_state(
        &keys.ng_decrypt_tables,
        &progress_cb,
    );
    if cancel.is_set() {
        return ExtractionResult::Stopped;
    }
    let state = match derived {
        Ok(state) => Arc::new(state),
        Err(e) => {
            return ExtractionResult::Failure {
                message: format!("derive NG encrypt state: {e}"),
            }
        }
    };
    keys.install_encrypt_state(Arc::clone(&state));
    if let Some(path) = encrypt_cache_path() {
        let cache = or_io::container::EncryptCache::build(&keys.ng_decrypt_tables, &state);
        if let Err(e) = cache.write_to(&path) {
            tracing::warn!(
                error = %e,
                "Failed to persist NG encrypt cache; will re-derive next session"
            );
        }
    }
    ExtractionResult::Derived
}

pub fn apply_successful_extraction(prefs: &mut GtaKeysPrefs, cache: &KeysCache) {
    prefs.gta5_path = Some(cache.metadata.gta5_path.clone());
    prefs.gta5_sha1_hex = Some(cache.metadata.gta5_sha1_hex.clone());
    prefs.gta5_size = Some(cache.metadata.gta5_size);
    prefs.extracted_at = Some(cache.metadata.extracted_at);
}

pub fn gta_install_root_from_prefs(prefs: &Prefs) -> Option<PathBuf> {
    let exe = prefs.gta_keys.gta5_path.as_ref()?;
    if exe.is_dir() {
        Some(exe.clone())
    } else {
        exe.parent().map(|p| p.to_path_buf())
    }
}

pub fn rpf_needs_keys(path: &Path) -> Option<bool> {
    use std::io::Read;
    let mut buf = [0u8; 16];
    let mut f = std::fs::File::open(path).ok()?;
    if f.read_exact(&mut buf).is_err() {
        return None;
    }
    let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if magic != 0x5250_4637 {
        return None;
    }
    let encryption = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
    let needs = !matches!(encryption, 0 | 0x4E45_504F);
    Some(needs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_noop() {
        let c = KeysController::default();
        let provider = c.provider();
        let mut buf = vec![0u8; 16];
        let res = provider.decrypt(
            or_io::container::RpfEncryption::Aes,
            or_io::container::CryptoTarget::Toc {
                archive_name: None,
                archive_size: 0,
            },
            &mut buf,
        );
        assert!(res.is_err());
    }

    #[test]
    fn extraction_state_fraction() {
        let mut s = ExtractionState::default();
        assert_eq!(s.fraction(), 0.0);
        s.total = 4;
        s.found = 2;
        assert_eq!(s.fraction(), 0.5);
    }

    #[test]
    fn pending_open_is_idempotent() {
        let mut c = KeysController::default();
        c.queue_pending(PathBuf::from("a.rpf"));
        c.queue_pending(PathBuf::from("a.rpf"));
        c.queue_pending(PathBuf::from("b.rpf"));
        assert_eq!(c.pending_open.len(), 2);
        let drained = c.drain_pending();
        assert_eq!(drained.len(), 2);
        assert!(c.pending_open.is_empty());
    }
}

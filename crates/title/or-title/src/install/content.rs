//! Looking inside an install's files rather than at their names.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::crawl::{self, ControlFlow, Found};
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::needle::{hex_preview, Needle};

/// Where in the install to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Everywhere,
    /// Only files whose container-plus-path starts with this, so a subtree
    /// or a single archive can be searched on its own.
    Under(String),
}

impl Scope {
    fn admits(&self, found: &Found<'_>) -> bool {
        match self {
            Scope::Everywhere => true,
            Scope::Under(prefix) => {
                let prefix = prefix.to_ascii_lowercase();
                let container = found.container.to_ascii_lowercase();
                let path = found.path.to_ascii_lowercase();
                container.starts_with(&prefix)
                    || path.starts_with(&prefix)
                    || format!("{container}/{path}").starts_with(&prefix)
            }
        }
    }
}

/// One place a needle was found.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    pub path: String,
    pub container: String,
    pub offsets: Vec<usize>,
    pub file_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// What a sweep did, whether or not it finished.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files_seen: usize,
    pub files_read: usize,
    pub bytes_read: u64,
    pub hits: usize,
    pub unreadable: usize,
    /// `true` when the walk was stopped rather than running out of files.
    pub stopped: bool,
}

pub struct Query<'a> {
    pub needle: &'a Needle,
    pub scope: &'a Scope,
    /// Extensions to look inside. Empty means every file.
    pub extensions: &'a [String],
    /// Files larger than this are skipped and counted as unreadable, so no
    /// archive body is pulled whole into memory.
    pub max_file_bytes: usize,
    pub hits_per_file: usize,
    pub preview_bytes: usize,
    /// Stop after this many files have matched.
    pub max_hits: usize,
}

/// Search file contents, calling `on_hit` as each match is found.
///
/// ⚠ Results are delivered as they are found rather than returned at the
/// end, and the search stops the moment `cancel` is set or `max_hits` is
/// reached. A whole-install content search takes minutes; one that can only
/// report when it is over cannot be stopped and cannot show progress.
pub fn search_contents(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    query: &Query<'_>,
    cancel: &AtomicBool,
    warnings: &mut Vec<String>,
    on_hit: &mut dyn FnMut(&ContentHit, &Progress),
) -> Progress {
    let mut progress = Progress::default();
    let outcome = crawl::crawl(game_root, provider, warnings, &mut |found| {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        progress.files_seen += 1;
        if !query.scope.admits(found) || !wanted_extension(found.leaf, query.extensions) {
            return ControlFlow::Continue(());
        }

        let bytes = match found.read() {
            Ok(bytes) => bytes,
            Err(_) => {
                progress.unreadable += 1;
                return ControlFlow::Continue(());
            }
        };
        if bytes.len() > query.max_file_bytes {
            progress.unreadable += 1;
            return ControlFlow::Continue(());
        }
        progress.files_read += 1;
        progress.bytes_read += bytes.len() as u64;

        let offsets = query.needle.find_in(&bytes, query.hits_per_file);
        if offsets.is_empty() {
            return ControlFlow::Continue(());
        }
        progress.hits += 1;
        let hit = ContentHit {
            path: found.path.to_string(),
            container: found.container.to_string(),
            preview: (query.preview_bytes > 0)
                .then(|| hex_preview(&bytes, offsets[0], query.preview_bytes)),
            offsets,
            file_size: bytes.len(),
        };
        on_hit(&hit, &progress);

        if query.max_hits > 0 && progress.hits >= query.max_hits {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    progress.stopped = outcome == ControlFlow::Break(());
    progress
}

fn wanted_extension(leaf: &str, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let ext = leaf.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    extensions.iter().any(|w| w.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("common").join("data")).unwrap();
        std::fs::write(
            root.join("common").join("data").join("weapons.meta"),
            b"<Item><Name>WEAPON_RAILGUN</Name></Item>",
        )
        .unwrap();
        std::fs::write(root.join("common").join("notes.txt"), b"railgun notes").unwrap();
        std::fs::write(root.join("unrelated.bin"), vec![0u8; 64]).unwrap();
        dir
    }

    fn run(dir: &Path, query: &Query<'_>, cancel: &AtomicBool) -> (Vec<ContentHit>, Progress) {
        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut hits = Vec::new();
        let progress = search_contents(
            dir,
            provider,
            query,
            cancel,
            &mut warnings,
            &mut |hit, _| {
                hits.push(hit.clone());
            },
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        (hits, progress)
    }

    fn query<'a>(needle: &'a Needle, scope: &'a Scope, extensions: &'a [String]) -> Query<'a> {
        Query {
            needle,
            scope,
            extensions,
            max_file_bytes: 1 << 20,
            hits_per_file: 16,
            preview_bytes: 32,
            max_hits: 0,
        }
    }

    #[test]
    fn a_text_search_reports_the_offset_of_every_hit() {
        let dir = fixture();
        let needle = Needle::text("RAILGUN", false);
        let (hits, progress) = run(
            dir.path(),
            &query(&needle, &Scope::Everywhere, &[]),
            &AtomicBool::new(false),
        );

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "common/data/weapons.meta");
        assert_eq!(hits[0].offsets, vec![19]);
        // The needle sits on the second row of the window; the row above it
        // is what makes the preview worth printing.
        let preview = hits[0].preview.as_deref().unwrap();
        assert!(preview.contains("RAILGUN"), "{preview}");
        assert!(preview.contains("<Item><Name>"), "{preview}");
        assert_eq!(hits[1].path, "common/notes.txt");
        assert_eq!(progress.hits, 2);
        assert_eq!(progress.files_seen, 3);
        assert!(!progress.stopped);
    }

    #[test]
    fn an_extension_filter_decides_what_is_even_opened() {
        let dir = fixture();
        let needle = Needle::text("railgun", false);
        let exts = vec!["meta".to_string()];
        let (hits, progress) = run(
            dir.path(),
            &query(&needle, &Scope::Everywhere, &exts),
            &AtomicBool::new(false),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "common/data/weapons.meta");
        assert_eq!(progress.files_seen, 3, "every file is still counted");
        assert_eq!(progress.files_read, 1, "only the wanted one was read");
    }

    #[test]
    fn a_scope_narrows_the_walk_to_a_subtree() {
        let dir = fixture();
        let needle = Needle::text("railgun", false);
        let scope = Scope::Under("common/data".into());
        let (hits, _) = run(
            dir.path(),
            &query(&needle, &scope, &[]),
            &AtomicBool::new(false),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "common/data/weapons.meta");

        let nothing = Scope::Under("x64".into());
        let (hits, _) = run(
            dir.path(),
            &query(&needle, &nothing, &[]),
            &AtomicBool::new(false),
        );
        assert!(hits.is_empty());
    }

    /// ⚠ Cancelling has to stop the walk. Asserted by the count, because a
    /// cancel that only suppresses output looks identical from outside.
    #[test]
    fn cancelling_stops_the_walk_and_says_it_was_stopped() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..100 {
            std::fs::write(dir.path().join(format!("f{i:03}.txt")), b"railgun").unwrap();
        }
        let needle = Needle::text("railgun", false);
        let cancel = AtomicBool::new(false);
        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut seen = 0usize;
        let progress = search_contents(
            dir.path(),
            provider,
            &query(&needle, &Scope::Everywhere, &[]),
            &cancel,
            &mut warnings,
            &mut |_, _| {
                seen += 1;
                if seen == 5 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(progress.stopped);
        assert_eq!(seen, 5);
        assert!(
            progress.files_seen < 100,
            "the walk ran to the end anyway: {} files",
            progress.files_seen
        );
    }

    #[test]
    fn a_hit_cap_stops_the_walk_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..50 {
            std::fs::write(dir.path().join(format!("f{i:03}.txt")), b"railgun").unwrap();
        }
        let needle = Needle::text("railgun", false);
        let mut q = query(&needle, &Scope::Everywhere, &[]);
        q.max_hits = 3;
        let (hits, progress) = run(dir.path(), &q, &AtomicBool::new(false));
        assert_eq!(hits.len(), 3);
        assert!(progress.stopped);
        assert!(progress.files_seen < 50);
    }

    /// ⚠ A content search must not pull an arbitrarily large file into
    /// memory. Over-size files are counted rather than silently ignored.
    #[test]
    fn an_oversize_file_is_skipped_and_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("big.bin"), vec![b'x'; 4096]).unwrap();
        std::fs::write(dir.path().join("small.bin"), b"railgun").unwrap();

        let needle = Needle::text("railgun", false);
        let mut q = query(&needle, &Scope::Everywhere, &[]);
        q.max_file_bytes = 1024;
        let (hits, progress) = run(dir.path(), &q, &AtomicBool::new(false));
        assert_eq!(hits.len(), 1);
        assert_eq!(progress.unreadable, 1);
        assert_eq!(progress.files_read, 1);
    }

    #[test]
    fn a_hex_search_finds_bytes_no_text_search_would() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("a.rpf.bak"),
            [0x52u8, 0x50, 0x46, 0x37, 0x00],
        )
        .unwrap();
        let needle = Needle::hex("52 50 46 37").expect("hex");
        let (hits, _) = run(
            dir.path(),
            &query(&needle, &Scope::Everywhere, &[]),
            &AtomicBool::new(false),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].offsets, vec![0]);
        assert_eq!(hits[0].file_size, 5);
    }
}

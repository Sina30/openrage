//! One walk over an install: loose files, archives, and archives inside them.
//!
//! ⚠ Every caller that wants "all of it" goes through here. A second
//! traversal is a second answer to how deep nesting goes, what a container
//! path looks like and which errors are warnings, and the two would agree
//! right up until one of them was changed.

use std::path::Path;
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::container::{RpfArchive, RpfNode};

/// How deep a `.rpf` inside a `.rpf` is followed.
pub const MAX_NEST_DEPTH: u32 = 8;

/// One file the walk found.
pub struct Found<'a> {
    /// Path within its container, or relative to the install root when loose.
    pub path: &'a str,
    /// The last path segment.
    pub leaf: &'a str,
    /// The archive chain that holds it, or `<loose>`.
    pub container: &'a str,
    source: Source<'a>,
}

enum Source<'a> {
    Loose(&'a Path),
    Entry {
        archive: &'a Arc<RpfArchive>,
        entry_index: u32,
    },
}

impl Found<'_> {
    /// The file's bytes, decompressed and decrypted.
    ///
    /// ⚠ Lazy on purpose: a crawl of a retail install visits about 400,000
    /// files, and a visitor that wants the bytes of a dozen of them must not
    /// pay for the rest.
    pub fn read(&self) -> Result<Vec<u8>, String> {
        match self.source {
            Source::Loose(path) => std::fs::read(path).map_err(|e| e.to_string()),
            Source::Entry {
                archive,
                entry_index,
            } => archive.read_entry(entry_index).map_err(|e| e.to_string()),
        }
    }

    /// Whether this entry is a paged RSC7 resource rather than plain bytes.
    pub fn is_resource(&self) -> bool {
        match self.source {
            Source::Loose(_) => false,
            Source::Entry {
                archive,
                entry_index,
                ..
            } => archive.resource_entry(entry_index).is_some(),
        }
    }
}

pub const LOOSE: &str = "<loose>";

/// Whether the walk should keep going.
///
/// ⚠ A visitor that cannot stop the walk turns "cancel" into "ignore the
/// rest of 400,000 files", which is the same wall-clock cost as not
/// cancelling. Every recursion level checks it.
pub use std::ops::ControlFlow;

/// Walk `game_root`, calling `visit` once per file until it breaks.
///
/// Returns whether the walk ran to the end.
pub fn crawl(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    warnings: &mut Vec<String>,
    visit: &mut dyn FnMut(&Found<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    crawl_where(game_root, provider, warnings, &mut |_| true, visit)
}

/// [`crawl`], opening only the archives on disk that `open` accepts; one it refuses is still visited as a file.
pub fn crawl_where(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    warnings: &mut Vec<String>,
    open: &mut dyn FnMut(&Path) -> bool,
    visit: &mut dyn FnMut(&Found<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let mut stack = vec![game_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("read_dir {}: {e}", dir.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let leaf = entry.file_name().to_string_lossy().into_owned();
            let rel = display_rel(game_root, &path);
            visit(&Found {
                path: &rel,
                leaf: &leaf,
                container: LOOSE,
                source: Source::Loose(&path),
            })?;
            if leaf.to_ascii_lowercase().ends_with(".rpf") && open(&path) {
                match open_rpf_file(&path, &provider) {
                    Ok(archive) => walk_rpf(&archive, &rel, 0, warnings, visit)?,
                    Err(e) => warnings.push(format!("open {rel}: {e}")),
                }
            }
        }
    }
    ControlFlow::Continue(())
}

fn walk_rpf(
    archive: &Arc<RpfArchive>,
    container: &str,
    depth: u32,
    warnings: &mut Vec<String>,
    visit: &mut dyn FnMut(&Found<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    for node in &archive.nodes {
        if node.is_dir() {
            continue;
        }
        visit(&Found {
            path: node.path(),
            leaf: node.name(),
            container,
            source: Source::Entry {
                archive,
                entry_index: node.entry_index(),
            },
        })?;
        if depth < MAX_NEST_DEPTH && is_rpf(node) {
            match archive.open_nested(node.entry_index()) {
                Ok(nested) => {
                    let nested = Arc::new(nested);
                    let nested_container = format!("{container}/{}", node.path());
                    walk_rpf(&nested, &nested_container, depth + 1, warnings, visit)?;
                }
                Err(e) => warnings.push(format!("{container}/{}: {e}", node.path())),
            }
        }
    }
    ControlFlow::Continue(())
}

fn is_rpf(node: &RpfNode) -> bool {
    node.name().to_ascii_lowercase().ends_with(".rpf")
}

fn open_rpf_file(
    path: &Path,
    provider: &Arc<dyn RpfCryptoProvider>,
) -> Result<Arc<RpfArchive>, String> {
    RpfArchive::open_with(path, Arc::clone(provider))
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

pub fn display_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;

    #[test]
    fn a_crawl_visits_every_loose_file_and_can_read_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub").join("inner")).unwrap();
        std::fs::write(root.join("a.meta"), b"alpha").unwrap();
        std::fs::write(root.join("sub").join("inner").join("b.meta"), b"beta").unwrap();

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut seen: Vec<(String, String, String)> = Vec::new();
        let finished = crawl(root, provider, &mut warnings, &mut |found| {
            seen.push((
                found.path.to_string(),
                found.container.to_string(),
                String::from_utf8(found.read().expect("read")).unwrap(),
            ));
            ControlFlow::Continue(())
        });
        assert_eq!(finished, ControlFlow::Continue(()));

        assert!(warnings.is_empty(), "{warnings:?}");
        seen.sort();
        assert_eq!(
            seen,
            vec![
                ("a.meta".into(), LOOSE.into(), "alpha".into()),
                ("sub/inner/b.meta".into(), LOOSE.into(), "beta".into()),
            ]
        );
    }

    #[test]
    fn a_crawl_descends_into_an_archive_and_reads_its_entries() {
        use or_io::container::RpfBuilder;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        RpfBuilder::default()
            .insert_with("data/names.nametable", b"BIFTA\0KALAHARI\0", false)
            .unwrap()
            .save(root.join("x64a.rpf"))
            .unwrap();

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut inner: Vec<(String, String, Vec<u8>)> = Vec::new();
        let _ = crawl(root, provider, &mut warnings, &mut |found| {
            if found.container != LOOSE {
                inner.push((
                    found.path.to_string(),
                    found.container.to_string(),
                    found.read().expect("read"),
                ));
            }
            ControlFlow::Continue(())
        });

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].0, "data/names.nametable");
        assert_eq!(inner[0].1, "x64a.rpf");
        assert_eq!(inner[0].2, b"BIFTA\0KALAHARI\0");
    }

    #[test]
    fn an_archive_the_caller_refuses_is_visited_and_not_opened() {
        use or_io::container::RpfBuilder;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for name in ["kept.rpf", "skipped.rpf"] {
            RpfBuilder::default()
                .insert_with("inside.meta", b"<x/>", false)
                .unwrap()
                .save(root.join(name))
                .unwrap();
        }

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut asked = Vec::new();
        let mut seen = Vec::new();
        let _ = crawl_where(
            root,
            provider,
            &mut warnings,
            &mut |path| {
                asked.push(display_rel(root, path));
                !path.ends_with("skipped.rpf")
            },
            &mut |found| {
                seen.push(format!("{}|{}", found.container, found.path));
                ControlFlow::Continue(())
            },
        );

        asked.sort();
        seen.sort();
        assert_eq!(asked, ["kept.rpf", "skipped.rpf"]);
        assert_eq!(
            seen,
            [
                "<loose>|kept.rpf",
                "<loose>|skipped.rpf",
                "kept.rpf|inside.meta"
            ],
            "the refused archive is a file of the listing and nothing inside it is"
        );
    }

    /// ⚠ Reading only binary entries works on `.meta` and loses every paged
    /// asset.
    #[test]
    fn a_resource_entry_reads_as_well_as_a_binary_one() {
        use or_io::container::RpfBuilder;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // An RSC7 header is what makes the builder file this as a resource.
        let mut resource = b"RSC7".to_vec();
        resource.extend_from_slice(&[0u8; 12]);
        resource.extend_from_slice(&[0xAB; 64]);

        RpfBuilder::default()
            .insert_with("data/plain.meta", b"<x/>", false)
            .unwrap()
            .insert_with("models/thing.ydr", resource, false)
            .unwrap()
            .save(root.join("x64a.rpf"))
            .unwrap();

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut read: Vec<(String, bool, usize)> = Vec::new();
        let _ = crawl(root, provider, &mut warnings, &mut |found| {
            if found.container != LOOSE {
                let bytes = found
                    .read()
                    .unwrap_or_else(|e| panic!("{}: {e}", found.path));
                read.push((found.path.to_string(), found.is_resource(), bytes.len()));
            }
            ControlFlow::Continue(())
        });

        read.sort();
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].0, "data/plain.meta");
        assert!(!read[0].1);
        assert_eq!(read[1].0, "models/thing.ydr");
        assert!(read[1].1, "an RSC7 payload should file as a resource");
        assert!(read[1].2 > 0);
    }

    /// ⚠ Breaking has to stop the walk, not merely be ignored for the rest
    /// of it.
    #[test]
    fn breaking_stops_the_walk_rather_than_running_it_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for i in 0..50 {
            std::fs::write(root.join(format!("f{i:02}.meta")), b"x").unwrap();
        }

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let mut visited = 0usize;
        let outcome = crawl(root, provider, &mut warnings, &mut |_| {
            visited += 1;
            if visited == 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        assert_eq!(outcome, ControlFlow::Break(()));
        assert_eq!(visited, 3, "the walk kept going after the break");
    }
}

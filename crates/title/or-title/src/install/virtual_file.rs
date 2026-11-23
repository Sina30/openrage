//! Reading the file an install holds at a path written the way the file explorer writes one: folders of the install, then archives as folders.
//!
//! ⚠ **A path is relative to wherever the explorer was opened,** which may be the install or a
//! folder above it, so a path that leads nowhere from the install's root is tried again without its
//! first folder, and so on. A path opened from a folder below the install leads nowhere, which is
//! said as `None` and not as a change.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::container::rpf::RpfArchive;

use crate::vfs::is_archive_segment;

/// The bytes of the file at `path` below the install at `root`, decoded as the explorer reads them; `None` when the install holds no file there.
///
/// An archive that cannot be opened is an error and not a missing file, since a person with no keys
/// has not learnt that the game removed anything.
pub fn read_virtual_file(
    root: &Path,
    path: &str,
    keys: &Arc<dyn RpfCryptoProvider>,
) -> Result<Option<Vec<u8>>, String> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let mut unreadable = None;
    for skip in 0..parts.len() {
        match descend(root, &parts[skip..], keys) {
            Ok(Some(bytes)) => return Ok(Some(bytes)),
            Ok(None) => {}
            Err(why) => {
                unreadable.get_or_insert(why);
            }
        }
    }
    unreadable.map_or(Ok(None), Err)
}

/// The folder or file of `dir` called `name`, whatever its case.
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.path())
}

fn descend(
    root: &Path,
    parts: &[&str],
    keys: &Arc<dyn RpfCryptoProvider>,
) -> Result<Option<Vec<u8>>, String> {
    let mut dir = root.to_path_buf();
    for (at, part) in parts.iter().enumerate() {
        let Some(next) = child(&dir, part) else {
            return Ok(None);
        };
        if next.is_dir() {
            dir = next;
            continue;
        }
        let rest = &parts[at + 1..];
        if rest.is_empty() {
            return std::fs::read(&next)
                .map(Some)
                .map_err(|e| format!("cannot read {}: {e}", next.display()));
        }
        if !is_archive_segment(part) {
            return Ok(None);
        }
        let archive = RpfArchive::open_with(&next, Arc::clone(keys))
            .map_err(|e| format!("cannot open {}: {e}", next.display()))?;
        return inside(&Arc::new(archive), rest);
    }
    Ok(None)
}

/// The file `rest` names inside `archive`, going into a nested archive when the path goes through one.
fn inside(archive: &Arc<RpfArchive>, rest: &[&str]) -> Result<Option<Vec<u8>>, String> {
    if let Some(entry) = archive.find(&rest.join("/")) {
        if archive
            .node_for_entry(entry)
            .is_none_or(|node| node.is_dir())
        {
            return Ok(None);
        }
        return archive
            .read_entry(entry)
            .map(Some)
            .map_err(|e| format!("cannot read {} from its archive: {e}", rest.join("/")));
    }
    for (at, part) in rest.iter().enumerate().take(rest.len().saturating_sub(1)) {
        if !is_archive_segment(part) {
            continue;
        }
        let Some(entry) = archive.find(&rest[..=at].join("/")) else {
            continue;
        };
        let nested = archive
            .open_nested(entry)
            .map_err(|e| format!("cannot open {part}: {e}"))?;
        return inside(&Arc::new(nested), &rest[at + 1..]);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use or_io::container::rpf::crypto::NoOpCryptoProvider;
    use or_io::container::rpf::RpfBuilder;

    use super::*;

    fn keys() -> Arc<dyn RpfCryptoProvider> {
        Arc::new(NoOpCryptoProvider::new())
    }

    fn archive(entries: &[(&str, &[u8])]) -> RpfBuilder {
        let mut builder = RpfBuilder::default();
        for (path, body) in entries {
            builder.insert_with(path, *body, false).expect("an entry");
        }
        builder
    }

    fn save(path: &Path, builder: &RpfBuilder) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        builder.save(path).expect("an archive");
    }

    /// `GTAV/x64/pack.rpf` holding `a.ytd`, `models/b.ydr` and a stored `inner.rpf` of `c.ydd`.
    fn install(dir: &Path) -> PathBuf {
        let root = dir.join("GTAV");
        let inner = archive(&[("c.ydd", b"deep")]).to_bytes().expect("inner");
        let pack = archive(&[
            ("a.ytd", b"textures"),
            ("models/b.ydr", b"drawable"),
            ("inner.rpf", &inner),
        ]);
        save(&root.join("x64/pack.rpf"), &pack);
        std::fs::write(root.join("x64/readme.txt"), b"loose").unwrap();
        root
    }

    fn read(root: &Path, path: &str) -> Result<Option<Vec<u8>>, String> {
        read_virtual_file(root, path, &keys())
    }

    #[test]
    fn a_file_in_an_archive_a_nested_archive_and_a_folder_is_read_from_the_installs_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = install(dir.path());

        let want = |path: &str, bytes: &[u8]| {
            assert_eq!(read(&root, path), Ok(Some(bytes.to_vec())), "{path}");
        };
        want("x64/pack.rpf/a.ytd", b"textures");
        want("x64/pack.rpf/models/b.ydr", b"drawable");
        want("x64/pack.rpf/inner.rpf/c.ydd", b"deep");
        want("x64/readme.txt", b"loose");
        want("X64\\PACK.RPF\\A.YTD", b"textures");
    }

    #[test]
    fn a_path_from_an_explorer_opened_above_the_install_is_found_from_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = install(dir.path());

        assert_eq!(
            read(&root, "GTAV/x64/pack.rpf/a.ytd"),
            Ok(Some(b"textures".to_vec()))
        );
    }

    #[test]
    fn a_file_the_install_does_not_hold_is_none_and_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = install(dir.path());

        for path in [
            "x64/pack.rpf/gone.ytd",
            "x64/pack.rpf/models",
            "x64/pack.rpf/inner.rpf/gone.ydd",
            "x64/gone.rpf/a.ytd",
            "pack.rpf/a.ytd",
            "",
        ] {
            assert_eq!(read(&root, path), Ok(None), "{path:?}");
        }
    }

    #[test]
    fn an_archive_that_will_not_open_is_an_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = install(dir.path());
        std::fs::write(root.join("x64/broken.rpf"), b"not an archive").unwrap();

        let why = read(&root, "x64/broken.rpf/a.ytd").unwrap_err();

        assert!(why.contains("broken.rpf"), "{why}");
    }
}

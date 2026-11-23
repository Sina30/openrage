//! Every file of one extension a retail install ships, loose or inside an
//! archive at any depth, for the suites whose format no corpus stages.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! Each install's own executable supplies the keys and the bundled snapshot
//! the NG ones (D18).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

/// Each install `OPENRAGE_GAME_ROOT` names, or `None` with the skip recorded.
pub fn roots(what: &str) -> Option<Vec<PathBuf>> {
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return None;
    }
    Some(roots)
}

fn provider(keys: &Arc<Gta5Keys>) -> Arc<dyn RpfCryptoProvider> {
    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys)))
}

fn walk_archive(
    archive: &Arc<RpfArchive>,
    label: &str,
    suffix: &str,
    depth: u32,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let path = format!("{label}/{}", node.path());
        let i = node.entry_index();
        if name.ends_with(".rpf") && depth < MAX_NEST_DEPTH {
            match archive.open_nested(i) {
                Ok(nested) => walk_archive(&Arc::new(nested), &path, suffix, depth + 1, visit),
                Err(e) => visit(path, Err(e.to_string())),
            }
        } else if name.ends_with(suffix) {
            visit(path, archive.read_entry(i).map_err(|e| e.to_string()));
        }
    }
}

/// Every `.{ext}` under `root`, each labelled by its path; an archive that will not open is visited as an error.
pub fn every(root: &Path, ext: &str, visit: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
    let suffix = format!(".{ext}");
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lower = label.to_ascii_lowercase();
            if lower.ends_with(&suffix) {
                visit(label, std::fs::read(&path).map_err(|e| e.to_string()));
            } else if lower.ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(&keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, &suffix, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

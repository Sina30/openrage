//! Copy, cut and paste: files between archives and folders and with Explorer.
//!
//! ⚠ The system clipboard is the desktop's, so every test reads and writes
//! `platform::clipboard::fake` instead.

mod commands;
mod fields;
mod files;
mod records;

use std::path::{Path, PathBuf};

use super::every_add_lands_where_the_tree_shows_it::opened;
use super::{AppState, DocPayload, DocumentId, RpfNodeId};

/// `files` written into a new folder `mods` under `root`, opened with Open Folder.
fn folder_of(root: &Path, files: &[(&str, Vec<u8>)]) -> (AppState, DocumentId) {
    let folder = root.join("mods");
    std::fs::create_dir_all(&folder).expect("the folder");
    for (name, bytes) in files {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's folder");
        }
        std::fs::write(path, bytes).expect("a file");
    }
    let mut state = AppState::new();
    let doc = opened(&mut state, &folder);
    (state, doc)
}

/// The names of the rows directly under `folder`, sorted.
pub(super) fn names_under(state: &AppState, doc: DocumentId, folder: RpfNodeId) -> Vec<String> {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rpf
        .session
        .children_of(folder)
        .into_iter()
        .filter_map(|id| rpf.session.node(id).map(|n| n.name.clone()))
        .collect();
    names.sort();
    names
}

/// Take away what a copy wrote out for Explorer, once a test has looked at it.
pub(super) fn forget_offered(offered: &[PathBuf]) {
    let scratch = (super::config_root())
        .expect("the run's own folder, where a test's copy stages")
        .join("openrage-clipboard");
    for path in offered.iter().filter(|p| p.starts_with(&scratch)) {
        if let Some(copy) = path.parent() {
            let _ = std::fs::remove_dir_all(copy);
        }
    }
}

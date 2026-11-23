//! The question a delete in the archive tree asks before anything goes: what, how much, and where to.

use std::path::Path;

use or_io::container::rpf::RpfEntry;

use super::document::{DocumentId, RpfNodeId};
use super::rpf_node::{ArchiveSource, RpfNodeBacking};
use super::rpf_session::RpfEditSession;
use super::{AppState, DocPayload, PendingRetailEdit};
use crate::platform::recycle_bin;

/// Where a deleted row goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteFrom {
    /// A file or folder on disk, sent to the Recycle Bin where the platform has one.
    Disk,
    /// A row the session holds, gone from the file named here once that is saved.
    Session(Option<String>),
}

/// A delete the user has been asked about and has not answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDelete {
    pub doc_id: DocumentId,
    pub node_id: RpfNodeId,
    pub name: String,
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    pub from: DeleteFrom,
}

impl PendingDelete {
    pub fn question(&self) -> String {
        match self.from {
            DeleteFrom::Disk if recycle_bin::RECOVERABLE => {
                tr!(
                    "Move “{name}” to the {bin}?",
                    name = self.name,
                    bin = crate::i18n::tr(recycle_bin::NAME)
                )
            }
            DeleteFrom::Disk => tr!("Delete “{name}” permanently?", name = self.name),
            DeleteFrom::Session(_) => tr!("Delete “{name}”?", name = self.name),
        }
    }

    /// How many files and folders go, and how much they hold.
    pub fn contents(&self) -> String {
        let files = count(self.files, "file");
        let held = match self.folders {
            0 => files,
            n => format!("{files} in {}", count(n, "folder")),
        };
        format!("{held}, {}", or_job::human_bytes(self.bytes))
    }

    /// What happens to them, and whether it can be taken back.
    pub fn consequence(&self) -> String {
        match &self.from {
            DeleteFrom::Disk if recycle_bin::RECOVERABLE => {
                tr!(
                    "You can restore it from the {name}.",
                    name = crate::i18n::tr(recycle_bin::NAME)
                )
            }
            DeleteFrom::Disk => tr!("This cannot be undone.").to_string(),
            DeleteFrom::Session(Some(file)) => tr!(
                "It leaves {file} when the archive is saved; until then the file on disk keeps it.",
                file = file
            ),
            DeleteFrom::Session(None) => {
                tr!("Nothing on disk changes until it is saved.").to_string()
            }
        }
    }

    /// The button that says yes.
    pub fn verb(&self) -> &'static str {
        match self.from {
            DeleteFrom::Disk if recycle_bin::RECOVERABLE => {
                if cfg!(windows) {
                    tr!("Move to Recycle Bin")
                } else {
                    tr!("Move to Trash")
                }
            }
            DeleteFrom::Disk => tr!("Delete permanently"),
            DeleteFrom::Session(_) => tr!("Delete"),
        }
    }

    fn done(&self) -> String {
        match self.from {
            DeleteFrom::Disk if recycle_bin::RECOVERABLE => {
                tr!(
                    "Moved “{name}” to the {bin}",
                    name = self.name,
                    bin = crate::i18n::tr(recycle_bin::NAME)
                )
            }
            _ => tr!("Deleted “{name}”", name = self.name),
        }
    }

    /// What a delete says once the Recycle Bin's stand-in has left the file where it was.
    fn left_in_place(&self) -> String {
        let name = &self.name;
        tr!(
            "The {bin}'s stand-in was asked to take “{name}”; nothing was moved",
            name = name,
            bin = crate::i18n::tr(recycle_bin::NAME)
        )
    }
}

fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

impl AppState {
    /// Ask whether to delete a row, saying what goes; `Err` when it cannot be deleted at all.
    pub(super) fn ask_delete(
        &mut self,
        doc_id: DocumentId,
        node_id: RpfNodeId,
    ) -> Result<(), String> {
        let file = self
            .affected_disk_path(doc_id, node_id)
            .filter(|p| !p.is_dir())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let doc = self
            .document(doc_id)
            .ok_or_else(|| tr!("Document {doc_id} not found", doc_id = doc_id))?;
        let DocPayload::Rpf(rpf) = &doc.payload else {
            return Err(tr!("Document is not an RPF").to_string());
        };
        let node = rpf
            .session
            .node(node_id)
            .ok_or_else(|| tr!("RPF node {node_id} does not exist", node_id = node_id))?;
        if node.parent.is_none() {
            return Err(
                tr!("The folder or archive a tab shows is closed, not deleted").to_string(),
            );
        }
        let ((files, folders, bytes), from) = match node.backing.fs_path() {
            Some(path) => (on_disk(path), DeleteFrom::Disk),
            None => (in_session(&rpf.session, node_id), DeleteFrom::Session(file)),
        };
        self.pending_delete = Some(PendingDelete {
            doc_id,
            node_id,
            name: node.name.clone(),
            files,
            folders,
            bytes,
            from,
        });
        Ok(())
    }

    /// Answer the pending delete; a yes asks the edit gate again, since the mode can change while the question is up.
    pub fn answer_delete(&mut self, confirmed: bool) {
        let Some(asked) = self.pending_delete.take() else {
            return;
        };
        if !confirmed {
            return;
        }
        let (doc_id, node_id) = (asked.doc_id, asked.node_id);
        let target = self.affected_disk_path(doc_id, node_id);
        let action = PendingRetailEdit::DeleteNode { doc_id, node_id };
        if self
            .gate_tree_edit(target.as_deref(), action, "delete")
            .is_err()
        {
            return;
        }
        let path = self.disk_path_for_node(doc_id, node_id);
        match self.rpf_delete_node_inner(doc_id, node_id) {
            // ⚠ Only the Recycle Bin's stand-in, a self-check's, leaves a path it was asked to take.
            Ok(_) if path.is_some_and(|p| std::fs::symlink_metadata(p).is_ok()) => {
                self.show_toast(asked.left_in_place())
            }
            Ok(_) => self.show_toast(asked.done()),
            Err(e) => self.show_error_toast(e),
        }
    }
}

/// Files, folders and bytes at or under `path`; a link counts as itself and is not followed.
///
/// ⚠ Walks on the UI thread, so a folder of tens of thousands of files stalls the frame that asks.
fn on_disk(path: &Path) -> (usize, usize, u64) {
    let (mut files, mut folders, mut bytes) = (0, 0, 0u64);
    let mut stack = vec![path.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&at) else {
            continue;
        };
        if meta.is_dir() {
            folders += 1;
            let entries = std::fs::read_dir(&at).into_iter().flatten().flatten();
            stack.extend(entries.map(|entry| entry.path()));
        } else {
            files += 1;
            bytes += meta.len();
        }
    }
    (files, folders, bytes)
}

/// Files, folders and bytes at or under a row of the session; a nested archive is one file of its stored size.
fn in_session(session: &RpfEditSession, id: RpfNodeId) -> (usize, usize, u64) {
    let (mut files, mut folders, mut bytes) = (0, 0, 0u64);
    let mut stack = vec![id];
    while let Some(at) = stack.pop() {
        let Some(node) = session.node(at) else {
            continue;
        };
        match &node.backing {
            RpfNodeBacking::Archive {
                source:
                    ArchiveSource::Nested {
                        parent_archive_id,
                        parent_entry_index,
                        ..
                    },
                ..
            } => {
                files += 1;
                bytes += session
                    .archive(*parent_archive_id)
                    .and_then(|slot| slot.archive.entries.get(*parent_entry_index as usize))
                    .map_or(0, |entry| match entry {
                        RpfEntry::Binary(b) => u64::from(b.uncompressed_size),
                        _ => 0,
                    });
            }
            _ if node.is_dir() => {
                folders += 1;
                stack.extend(session.children_of(at));
            }
            _ => {
                files += 1;
                bytes += session.size_for(at).unwrap_or(0);
            }
        }
    }
    (files, folders, bytes)
}

//! Undo and redo through each document's own history, which the Edit menu, its keys and the History panel move through.

use std::path::{Path, PathBuf};

use super::{
    AppState, Change, DocPayload, DocumentId, EditDirection, EditHistory, GateOutcome,
    PendingRetailEdit, PendingRetailEditPrompt,
};
use crate::ui::commands::focused_document;

impl AppState {
    /// Count `change`, already made to `doc`, as the newest step of its history.
    pub fn record(&mut self, doc: DocumentId, label: impl Into<String>, mut change: Change) {
        // ⚠ A payload edit can land while a gesture's image edit is unread, as a click
        // ending a typed value does: it is taken back out, the image read, and made again.
        let unread = self.document(doc).is_some_and(|d| d.unread());
        if unread && !change.patches_the_image() && self.flip(doc, &mut change).is_ok() {
            self.catch_up(doc);
            if self.flip(doc, &mut change).is_err() {
                tracing::warn!("an edit lost its place in the payload read back from the image");
                return;
            }
        }
        if let Some(document) = self.document_mut(doc) {
            document.record(label.into(), change);
        }
        self.rebuild_after_an_edit(doc);
    }

    /// Read `doc`'s payload back from its image if an image edit has not reached it, and forget the previews drawn from the old payload.
    pub(super) fn catch_up(&mut self, doc: DocumentId) {
        if self.document_mut(doc).is_some_and(|d| d.catch_up()) {
            self.texture_cache.forget_document(doc);
            self.rebuild_after_an_edit(doc);
        }
    }

    /// The history of the document the focused panel shows, which Undo and Redo move through.
    pub fn focused_history(&self) -> Option<&EditHistory> {
        self.document(focused_document(self)?)
            .map(|doc| &doc.history)
    }

    pub fn undo(&mut self) -> Option<String> {
        self.step(focused_document(self)?, EditDirection::Undo)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.step(focused_document(self)?, EditDirection::Redo)
    }

    /// Undo or redo `doc`'s steps until it stands at step `target`.
    pub fn go_to_step(&mut self, doc: DocumentId, target: u64) {
        while let Some(way) = self.document(doc).and_then(|d| d.history.way_to(target)) {
            if self.step(doc, way).is_none() {
                return;
            }
        }
    }

    /// End every edit a step is taking in, once the pointer is up and nothing holds the keyboard, read back each image an edit left unread, and build each one a view asked for.
    pub fn seal_edits(&mut self) {
        for doc in &mut self.documents {
            doc.history.seal();
            doc.build_the_asked_image();
        }
        let unread: Vec<DocumentId> = (self.documents.iter())
            .filter(|d| d.unread())
            .map(|d| d.id)
            .collect();
        for doc in unread {
            self.catch_up(doc);
        }
    }

    /// Undo or redo `doc`'s next step whole; one that cannot finish changes nothing, says why, and leaves the history.
    pub(super) fn step(&mut self, doc: DocumentId, direction: EditDirection) -> Option<String> {
        let writes = self.may_write(doc, direction)?;
        let mut step = self.document_mut(doc)?.history.take(direction)?;
        if let Err(why) = self.flip(doc, &mut step.change) {
            let said = (self.document_mut(doc))
                .map_or(why.clone(), |d| d.could_not_step(step, direction, &why));
            tracing::warn!("{said}");
            self.show_error_toast(said);
            return None;
        }
        let label = step.label.clone();
        let stale = step.change.stale_trees();
        if let Some(document) = self.document_mut(doc) {
            document.stepped(step, direction);
        }
        // A tree that gave a file up cannot take its row back, so it reads the folder again
        for tree in stale {
            let root = match self.document(tree).map(|d| &d.payload) {
                Some(DocPayload::Rpf(rpf)) => rpf.session.root_id(),
                _ => continue,
            };
            if let Err(why) = self.refresh_folder(tree, root) {
                tracing::warn!("a tree the undo left stale could not be read again: {why}");
            }
        }
        for path in &writes {
            self.file_replaced(path);
        }
        // ⚠ Decoded previews are keyed by name, so a step that swapped a texture's pixels would leave the old ones shown.
        self.texture_cache.forget_document(doc);
        self.rebuild_after_an_edit(doc);
        Some(label)
    }

    /// What the next step `direction` of `doc` writes on disk when it may write it, which no job may read and the edit gate must let; a refusal is shown, and the step stays.
    fn may_write(&mut self, doc: DocumentId, direction: EditDirection) -> Option<Vec<PathBuf>> {
        let Some(step) = self.document(doc).and_then(|d| d.history.next(direction)) else {
            return Some(Vec::new());
        };
        let writes: Vec<_> = (step.change.on_disk().into_iter())
            .map(Path::to_path_buf)
            .collect();
        let then = match direction {
            EditDirection::Undo => tr!("undo it"),
            EditDirection::Redo => tr!("redo it"),
        };
        let permitted = writes.iter().all(|path| {
            if let Err(why) = self.no_job_holds(path, then) {
                self.show_error_toast(why);
                return false;
            }
            match self.gate_permits(Some(path)) {
                GateOutcome::Proceed => true,
                GateOutcome::Refused => false,
                GateOutcome::Confirm(target) => {
                    // The gate asks about this write as it asks about any other, and the answer takes the step
                    self.pending_retail_edit = Some(PendingRetailEditPrompt {
                        action: PendingRetailEdit::Step {
                            doc_id: doc,
                            direction,
                        },
                        target_path: target,
                        verb: match direction {
                            EditDirection::Undo => "undo",
                            EditDirection::Redo => "redo",
                        },
                    });
                    false
                }
            }
        });
        permitted.then_some(writes)
    }

    fn flip(&mut self, doc: DocumentId, change: &mut Change) -> Result<(), String> {
        let gone = || crate::i18n::tr(super::edit_history::GONE).to_string();
        match change {
            Change::Pose(other) => {
                std::mem::swap(&mut self.anim_edits, other);
                self.anim.invalidate();
                Ok(())
            }
            Change::ClipFields { clip_key, other } => {
                let now = self.clip_field_snapshot(doc, *clip_key).ok_or_else(gone)?;
                if !self.apply_clip_fields(doc, *clip_key, other) {
                    return Err(gone());
                }
                *other = now;
                self.anim.invalidate();
                Ok(())
            }
            change => (self.document_mut(doc))
                .ok_or_else(|| tr!("its document is closed").to_string())?
                .flip(change),
        }
    }

    fn rebuild_after_an_edit(&mut self, doc: DocumentId) {
        self.sim.pending_rebuild = true;
        self.sim.manager.snapshots.clear();
        // ⚠ A GTA VI drawable's tile caches a pack's texture under the drawable, which no edit of the pack names.
        if matches!(
            self.document(doc).map(|d| &d.payload),
            Some(DocPayload::Ttpf(_))
        ) {
            for held in &self.documents {
                if matches!(held.payload, DocPayload::Gta6Drawable(_)) {
                    self.texture_cache.forget_document(held.id);
                }
            }
        }
    }
}

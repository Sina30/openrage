//! A document's side of its history: recording a step, flipping one, and marking where that leaves it.

use std::path::Path;
use std::sync::Arc;

use super::{ArchiveId, DocPayload, Document};
use crate::state::edit_history::{flip_whole, Change, EditDirection, Step, GONE};

// ⚠ Per thread, because the test binary runs tests on parallel threads.
#[cfg(test)]
thread_local! {
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Document {
    /// Count `change`, already made, as the newest step of this document's history; an image edit reaches the payload at the next `catch_up`.
    pub fn record(&mut self, label: String, change: Change) {
        let patch = change.patches_the_image();
        self.unread |= patch;
        // The payload no longer matches the image an edit of it leaves, which goes to the step.
        let image = if patch { None } else { self.resource.take() };
        self.history.push(label, change, image);
        self.touch(patch);
    }

    /// What the steps of this document's history keep, besides the image the document holds.
    pub fn history_bytes(&self) -> usize {
        self.history.held_bytes(self.resource.as_ref())
    }

    /// Whether the image holds an edit the payload has not been read back from.
    pub(in crate::state) fn unread(&self) -> bool {
        self.unread
    }

    /// Read the payload back from the image if it holds an edit the payload lacks; `true` when it read.
    ///
    /// ⚠ A payload edit drops the image and a save clears the patched mark, so
    /// an image edit the payload does not hold is lost by either: each reads
    /// back first.
    pub(in crate::state) fn catch_up(&mut self) -> bool {
        if !self.unread {
            return false;
        }
        self.read_back();
        true
    }

    /// How many times this thread has read a payload back from its image.
    #[cfg(test)]
    pub fn reads_on_this_thread() -> usize {
        READS.with(|reads| reads.get())
    }

    /// Read the payload again from the image; where the reader refuses the image, the payload stays as it was.
    pub(super) fn read_back(&mut self) {
        #[cfg(test)]
        READS.with(|reads| reads.set(reads.get() + 1));
        self.unread = false;
        match self.read_image() {
            Ok(payload) => {
                self.payload = payload;
                self.payload_epoch = self.payload_epoch.wrapping_add(1);
            }
            Err(why) => {
                tracing::warn!(document = %self.name, %why, "the edited image does not read back");
            }
        }
    }

    fn read_image(&self) -> Result<DocPayload, String> {
        use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7};

        let image =
            (self.resource.as_ref()).ok_or(tr!("the document holds no image of its bytes"))?;
        let handler = crate::state::payload_handlers::handler_for_payload(&self.payload)
            .ok_or(tr!("no reader is registered for this document"))?;
        // Stored, not deflated: the reader inflates it at once.
        let bytes = Rsc7::reseal(&image.data, &image.seal, Compression::none())
            .map_err(|e| e.to_string())?;
        (handler.parse)(self.path.as_deref().unwrap_or(Path::new("")), &bytes)
    }

    /// Keep `step`, just undone or redone, and mark the document saved exactly where its file stands.
    pub(in crate::state) fn stepped(&mut self, mut step: Step, direction: EditDirection) {
        let patch = step.change.patches_the_image();
        if !patch {
            // An edit of the contents swaps the image as well: each side has the one its grid steps were made on.
            step.image = std::mem::replace(&mut self.resource, step.image.take());
        }
        self.history.put_back(step, direction);
        self.touch(patch);
        if self.history.at_saved() {
            self.dirty = false;
            self.patched = false;
            // ⚠ An archive an undo touched is marked unsaved though it holds what its file does.
            if let DocPayload::Rpf(rpf) = &mut self.payload {
                for archive in rpf.session.dirty_archive_ids() {
                    rpf.session.clear_archive_dirty(archive);
                }
            }
        }
    }

    /// The file now holds this document as it stood at step `at`: saved if it still stands there.
    pub fn saved_as_of(&mut self, at: u64) {
        if self.history.at() == at {
            self.mark_saved();
        } else {
            // ⚠ An edit after the save began leaves the document unsaved, but the file is rewritten all the same.
            self.opened_as(None);
            self.history.saved_at(at);
        }
    }

    /// Count a change made on disk at once, which no step holds; `reached_an_archive` when it reached an archive the tree had opened.
    ///
    /// ⚠ A step names rows by id, only at or under an opened archive. A rename
    /// or move keeps every id, and `moved_on_disk` points the tree's archives
    /// at their new files, but not rows a step took out, which still read the
    /// old ones, as after a write of the archive; a delete takes the rows.
    pub(in crate::state) fn changed_on_disk(&mut self, reached_an_archive: bool, deleted: bool) {
        if reached_an_archive && (deleted || self.history.reads_an_archive()) {
            self.history.clear();
        }
        self.touch(false);
    }

    /// Take what the file on disk now holds as this document, saved.
    ///
    /// ⚠ The steps that hold a value of the old contents go, and the images the others hold: an
    /// undo across the reload would write that value, or the old image, over the new contents,
    /// and a save would then write it into the file the recook answers `Unchanged` for.
    pub(in crate::state) fn reloaded(
        &mut self,
        payload: DocPayload,
        resource: Option<Arc<super::ResourceImage>>,
    ) {
        self.payload = payload;
        self.unread = false;
        self.set_resource(resource);
        self.history.forget_old_contents();
        self.mark_saved();
    }

    pub(super) fn touch(&mut self, patch: bool) {
        self.dirty = self.unwritten_archives().unwrap_or(true);
        self.patched |= patch;
        self.edits = self.edits.wrapping_add(1);
        // ⚠ A patch leaves the payload as it was until it is read back: a drag writes the image every frame.
        if !patch {
            self.payload_epoch = self.payload_epoch.wrapping_add(1);
        }
    }

    /// Counts each change of the payload, which what is drawn from it is keyed on: an image edit counts once read back.
    pub fn payload_epoch(&self) -> u64 {
        self.payload_epoch
    }

    /// Swap what `change` holds with what this document holds, all of it or, saying why, none.
    pub(in crate::state) fn flip(&mut self, change: &mut Change) -> Result<(), String> {
        let gone = || crate::i18n::tr(GONE).to_string();
        match change {
            Change::Meta(edits) => {
                let DocPayload::Meta(meta) = &mut self.payload else {
                    return Err(gone());
                };
                let Some(parsed) = meta.parsed.as_mut() else {
                    return Err(gone());
                };
                if !crate::state::meta_edit::flip_all(edits, &mut parsed.instance) {
                    return Err(gone());
                }
                meta.after_visual_edit_in_place();
                Ok(())
            }
            Change::Bytes { at, other } => {
                let Some(image) = self.resource.as_mut() else {
                    return Err(gone());
                };
                let data = &mut Arc::make_mut(image).data;
                match data.get_mut(*at..).and_then(|d| d.get_mut(..other.len())) {
                    Some(bytes) => bytes.swap_with_slice(other),
                    None => return Err(gone()),
                }
                self.read_back();
                Ok(())
            }
            Change::Image { other } => {
                let Some(image) = self.resource.as_mut() else {
                    return Err(gone());
                };
                std::mem::swap(image, other);
                self.read_back();
                Ok(())
            }
            Change::Part { parts, .. } => {
                let marked = self.unsaved_archives();
                let payload = &mut self.payload;
                let Err(at) = flip_whole(parts, |part| part.swap(payload)) else {
                    return Ok(());
                };
                // ⚠ The parts flipped back marked the archives they touched unsaved again.
                if let (DocPayload::Rpf(rpf), Some(marked)) = (&mut self.payload, marked) {
                    rpf.session.mark_unsaved_only(&marked);
                }
                Err(parts.get(at).map_or_else(gone, |part| part.why_not()))
            }
            Change::MoveShader { owner, from, to } => {
                let Some(drawable) = super::drawable_mut(&mut self.payload, *owner) else {
                    return Err(gone());
                };
                let count = drawable
                    .shader_group
                    .as_deref()
                    .map_or(0, |g| g.shaders.len());
                if *from >= count || *to >= count {
                    return Err(gone());
                }
                drawable.move_shader(*to, *from);
                std::mem::swap(from, to);
                Ok(())
            }
            Change::Pose(_) | Change::ClipFields { .. } => Err(gone()),
        }
    }

    /// The archives of a tree marked unsaved.
    fn unsaved_archives(&self) -> Option<Vec<ArchiveId>> {
        match &self.payload {
            DocPayload::Rpf(rpf) => Some(rpf.session.dirty_archive_ids()),
            _ => None,
        }
    }

    /// Forget `step`, which could not finish for `why` and changed nothing, keeping the saved mark true; the sentence it returns stands in the History panel.
    ///
    /// ⚠ A tree's steps name rows by id, so each step before it still finds
    /// its own. Every other step names its place by path or index, which the
    /// change the document keeps may have moved, so those go as well.
    pub(in crate::state) fn could_not_step(
        &mut self,
        step: Step,
        direction: EditDirection,
        why: &str,
    ) -> String {
        let alone = matches!(self.payload, DocPayload::Rpf(_));
        let (verb, left) = match (direction, alone) {
            (EditDirection::Undo, true) => (tr!("Undo"), tr!("it leaves the history")),
            (EditDirection::Redo, true) => (
                tr!("Redo"),
                tr!("it and the steps after it leave the history"),
            ),
            (EditDirection::Undo, false) => (tr!("Undo"), tr!("the history is emptied")),
            (EditDirection::Redo, false) => (tr!("Redo"), tr!("the history is emptied")),
        };
        let said = tr!(
            "{verb} of “{step}” changed nothing, and {left}: {why}",
            verb = verb,
            left = left,
            why = why,
            step = step.label
        );
        self.history
            .dropped(step.id, direction, alone, said.clone());
        said
    }
}

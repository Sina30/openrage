//! A GTA VI animation resource's edits, each a step of its document's history: a key of a
//! channel, a clip's window and rate through the clip fields GTA V's clips edit, a group entry's
//! window, a clip's name, and a track held over the whole clip, added or removed. The sections
//! they leave are what a save writes.

use std::sync::Arc;

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::anim::{AnimEdit, AnimError, AnimResource, ClipPlay};

use super::{AppState, Change, ClipFieldSnapshot, DocPayload, DocumentId};

/// The GTA VI animation row's `edited_sections`: the resource's sections once an edit is made.
pub(super) fn edited_sections(payload: &DocPayload) -> Option<Result<Rpf9Sections, String>> {
    let DocPayload::Gta6Anim(resource) = payload else {
        return None;
    };
    resource
        .edited
        .then(|| resource.sections.as_deref().cloned())?
        .map(Ok)
}

fn resource(payload: &mut DocPayload) -> Option<&mut Arc<AnimResource>> {
    match payload {
        DocPayload::Gta6Anim(resource) => Some(resource),
        _ => None,
    }
}

impl AppState {
    /// `edit` made in document `doc`'s GTA VI animation resource; the resource as it stood before.
    fn edit_gta6_anim(
        &mut self,
        doc: DocumentId,
        edit: &AnimEdit,
    ) -> Result<Arc<AnimResource>, String> {
        self.replace_gta6_anim(doc, |held| or_io::gta6::anim::edit(held, edit))
    }

    /// Document `doc`'s GTA VI animation resource replaced by what `make` makes of it; the
    /// resource as it stood before.
    pub(super) fn replace_gta6_anim(
        &mut self,
        doc: DocumentId,
        make: impl FnOnce(&AnimResource) -> Result<AnimResource, AnimError>,
    ) -> Result<Arc<AnimResource>, String> {
        let gone = || crate::i18n::tr(super::edit_history::GONE).to_string();
        let held =
            resource(&mut self.document_mut(doc).ok_or_else(gone)?.payload).ok_or_else(gone)?;
        let edited = make(held).map_err(|e| e.to_string())?;
        Ok(std::mem::replace(held, Arc::new(edited)))
    }

    /// Sets a key of a GTA VI animation as a step of `doc`'s history; a refusal is shown.
    pub fn edit_gta6_key(&mut self, doc: DocumentId, edit: AnimEdit) -> bool {
        let AnimEdit::Key {
            animation,
            dof,
            component,
            frame,
            ..
        } = edit
        else {
            return false;
        };
        match self.edit_gta6_anim(doc, &edit) {
            Ok(before) => {
                let key = format!(
                    "gta6 key {}.{}.{dof}.{component}.{frame}",
                    animation.set, animation.animation
                );
                self.record(doc, tr!("Edit a key"), Change::part(key, before, resource));
                true
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }

    /// Sets a GTA VI group entry's window as a step of `doc`'s history; a refusal is shown.
    pub fn edit_gta6_entry(&mut self, doc: DocumentId, edit: AnimEdit) -> bool {
        let AnimEdit::EntryWindow {
            clip, group, entry, ..
        } = edit
        else {
            return false;
        };
        match self.edit_gta6_anim(doc, &edit) {
            Ok(before) => {
                let key = format!("gta6 entry {clip}.{group}.{entry}");
                let change = Change::part(key, before, resource);
                self.record(doc, tr!("Edit a group entry's window"), change);
                true
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }

    /// Renames GTA VI clip `clip` as a step of `doc`'s history; a refusal is shown.
    pub fn rename_gta6_clip(&mut self, doc: DocumentId, clip: usize, name: &str) -> bool {
        let renamed = |held: &AnimResource| or_io::gta6::anim::rename_clip(held, clip, name);
        match self.replace_gta6_anim(doc, renamed) {
            Ok(before) => {
                let change = Change::part(format!("gta6 rename {clip}"), before, resource);
                self.record(doc, tr!("Rename a clip"), change);
                true
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }

    /// Holds a track over the whole clip, adds or removes one, as a step of its own; a refusal is shown.
    pub fn relay_gta6_anim(&mut self, doc: DocumentId, edit: AnimEdit) -> bool {
        let label = match edit {
            AnimEdit::Pose { .. } => tr!("Hold a track over the whole clip"),
            AnimEdit::AddDof { .. } => tr!("Add a track"),
            AnimEdit::RemoveDof { .. } => tr!("Remove a track"),
            _ => return false,
        };
        match self.edit_gta6_anim(doc, &edit) {
            Ok(before) => {
                let mut change = Change::part(String::new(), before, resource);
                if let Change::Part { key, .. } = &mut change {
                    *key = None;
                }
                self.record(doc, label, change);
                true
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }

    /// Says, as an edited GTA VI animation resource is saved, which of its words are kept as read.
    pub(crate) fn say_the_animation_hashes_are_kept(&mut self, doc: DocumentId) {
        let edited =
            |d: &super::Document| matches!(&d.payload, DocPayload::Gta6Anim(r) if r.edited);
        if self.document(doc).is_some_and(edited) {
            self.show_warning_toast(tr!("Each edited block keeps the hash at its start as it was read: no rule for it is known, so the game may refuse the saved file. An edited track list is signed again."));
        }
    }

    /// GTA VI's clip fields: clip `clip`, by its place in the dictionary, when it plays one
    /// animation. It states no loop flag anyone found.
    pub(super) fn gta6_clip_field_snapshot(
        &self,
        doc: DocumentId,
        clip: u32,
    ) -> Option<ClipFieldSnapshot> {
        let DocPayload::Gta6Anim(resource) = &self.document(doc)?.payload else {
            return None;
        };
        let clip = resource.clips.get(clip as usize)?;
        let ClipPlay::One {
            start, end, rate, ..
        } = clip.play
        else {
            return None;
        };
        Some(ClipFieldSnapshot {
            name: clip.name.clone(),
            start_time: start,
            end_time: end,
            rate,
            looped: false,
        })
    }

    /// Writes `fields`' window and rate to GTA VI clip `clip`; its name, which its key hashes,
    /// is renamed apart, and no loop flag is written.
    pub(super) fn apply_gta6_clip_fields(
        &mut self,
        doc: DocumentId,
        clip: u32,
        fields: &ClipFieldSnapshot,
    ) -> bool {
        let edit = AnimEdit::ClipWindow {
            clip: clip as usize,
            start: fields.start_time,
            end: fields.end_time,
            rate: fields.rate,
        };
        match self.edit_gta6_anim(doc, &edit) {
            Ok(_) => true,
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "gta6_anim_edit_tests.rs"]
pub(super) mod tests;

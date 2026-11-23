//! Where a save of a document opened from an archive goes: back into the
//! archive, to a loose file, or into the mods overlay's copy of the archive.

use std::path::{Path, PathBuf};

use or_io::decl::Repair;

use super::document_io::{atomic_write_with_backup, serialize_payload};
use super::extract_job::PendingExportFixes;
use super::overlay_copy::write_into_place;
use super::prefs::ExportFixes;
use super::{
    retail, AppState, ArchiveId, DocPayload, DocumentId, DocumentKind, GateOutcome,
    PendingRetailEdit, PendingRetailEditPrompt, RpfEntryBinding,
};

/// Where the user sends a save of an archive entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveDestination {
    /// Back into the archive it was opened from.
    Archive,
    /// A loose file at this path; the archive keeps its bytes.
    LooseFile(PathBuf),
    /// Into the mods overlay's copy of the archive; the shipped one keeps its bytes.
    Overlay,
}

/// A save of an archive entry waiting on where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveDestinationChoice {
    pub doc_id: DocumentId,
    pub name: String,
    /// The archive on disk the entry goes back into; `None` while only an archive in memory holds it.
    pub archive: Option<PathBuf>,
    /// The overlay copy an overlay save writes; `None` for an archive outside an install.
    pub overlay: Option<PathBuf>,
}

/// The file on disk that holds an entry.
enum Holder {
    /// An archive of a mounted install, rebuilt from its own records with every archive nested in it.
    Mounted(ArchiveId, PathBuf),
    /// An `.rpf` opened on its own, written whole.
    Standalone(PathBuf),
}

impl Holder {
    fn path(&self) -> &Path {
        match self {
            Holder::Mounted(_, path) | Holder::Standalone(path) => path,
        }
    }
}

impl AppState {
    /// Ask where a save of the document goes; false when it was not opened from an archive.
    pub(super) fn offer_save_destination(&mut self, doc_id: DocumentId) -> bool {
        let Some(binding) = self.document(doc_id).and_then(|d| d.rpf_entry_binding) else {
            return false;
        };
        let name = self
            .document(binding.parent_doc_id)
            .and_then(|d| match &d.payload {
                DocPayload::Rpf(rpf) => rpf.session.node(binding.entry_id),
                _ => None,
            })
            .map(|n| n.name.clone())
            .unwrap_or_default();
        let archive = self.holder_of(binding).map(|h| h.path().to_path_buf());
        let overlay = archive.as_deref().and_then(|p| self.overlay_for(p));
        self.save_destination = Some(SaveDestinationChoice {
            doc_id,
            name,
            archive,
            overlay,
        });
        true
    }

    pub fn cancel_save_destination(&mut self) {
        self.save_destination = None;
    }

    /// Answer the open choice with `dest`.
    pub fn choose_save_destination(&mut self, dest: SaveDestination) {
        if let Some(choice) = self.save_destination.take() {
            self.save_entry_to(choice.doc_id, dest);
        }
    }

    /// Save an archive entry's document into `dest`, if the edit gate permits it.
    pub(super) fn save_entry_to(&mut self, doc_id: DocumentId, dest: SaveDestination) {
        let Some(binding) = self.document(doc_id).and_then(|d| d.rpf_entry_binding) else {
            return;
        };
        let holder = self.holder_of(binding);
        let target = match &dest {
            SaveDestination::Archive => holder.map(|h| h.path().to_path_buf()),
            SaveDestination::LooseFile(path) => Some(path.clone()),
            SaveDestination::Overlay => {
                let Some(overlay) = holder.and_then(|h| self.overlay_for(h.path())) else {
                    self.show_error_toast(tr!(
                        "Not saved: this archive is not inside a game install"
                    ));
                    return;
                };
                Some(overlay)
            }
        };
        match self.gate_permits(target.as_deref()) {
            GateOutcome::Refused => return,
            GateOutcome::Confirm(target_path) => {
                self.pending_retail_edit = Some(PendingRetailEditPrompt {
                    action: PendingRetailEdit::SaveEntry { doc_id, dest },
                    target_path,
                    verb: tr!("save to"),
                });
                return;
            }
            GateOutcome::Proceed => {}
        }
        self.save_entry_to_inner(doc_id, dest, None);
    }

    /// The tail of [`save_entry_to`](Self::save_entry_to) once the gate is
    /// satisfied; `fixes` is the export prompt's answer, `None` until given.
    pub(super) fn save_entry_to_inner(
        &mut self,
        doc_id: DocumentId,
        dest: SaveDestination,
        fixes: Option<bool>,
    ) {
        let Some(binding) = self.document(doc_id).and_then(|d| d.rpf_entry_binding) else {
            return;
        };
        if let Some(DocPayload::Meta(m)) = self.document_mut(doc_id).map(|d| &mut d.payload) {
            m.ensure_text_current();
        }
        let Some(source) = self.save_source_for(doc_id) else {
            return;
        };
        let repair = match (fixes, self.prefs.export_fixes) {
            (Some(true), _) | (None, ExportFixes::Apply) => Repair::Lossless,
            _ => Repair::Refuse,
        };
        let serialized =
            source.into_bytes(self.prefs.rpf_output_mode, self.keys.provider(), repair);
        let overlay = dest == SaveDestination::Overlay;
        let saved = match dest {
            SaveDestination::LooseFile(path) => {
                self.save_entry_loose(doc_id, binding, path, serialized, fixes)
            }
            SaveDestination::Archive | SaveDestination::Overlay => {
                serialized.and_then(|(bytes, repairs)| {
                    let written = self.save_entry_into_archive(doc_id, binding, bytes, overlay)?;
                    self.report_repairs(doc_id, &repairs);
                    Ok(written)
                })
            }
        };
        match saved {
            Ok(Some(written)) => {
                self.show_toast(tr!("Saved to {written}", written = written.display()))
            }
            Ok(None) => {}
            Err(e) => {
                let name = self.document(doc_id).map(|d| d.title()).unwrap_or_default();
                let e = format!("{name}: {e}");
                self.set_error(tr!("Could not save the file"), e.clone());
                self.show_error_toast(tr!("Save failed: {e}", e = e));
            }
        }
    }

    /// `Ok(None)` while the export prompt is asking.
    ///
    /// A written loose file marks the document saved, though the archive keeps
    /// its bytes: closing it loses nothing.
    /// ⚠ An unedited document whose canonical form is refused still asks, so
    /// the answer can repair it or keep the original bytes.
    fn save_entry_loose(
        &mut self,
        doc_id: DocumentId,
        binding: RpfEntryBinding,
        path: PathBuf,
        canonical: Result<(Vec<u8>, Vec<String>), String>,
        fixes: Option<bool>,
    ) -> Result<Option<PathBuf>, String> {
        self.no_job_reads(&path, "save")?;
        // ⚠ Only a document never edited has original bytes to keep: an edit
        // exists only in the canonical form, and one saved as a loose file is
        // marked saved while the archive still holds the bytes it opened from.
        let edited = self.document(doc_id).is_none_or(|d| d.edit_epoch() != 0);
        let original = (!edited)
            .then(|| self.entry_bytes(binding))
            .flatten()
            .filter(|original| canonical.as_ref().map(|(c, _)| c) != Ok(original));
        let kept = match original {
            None => None,
            Some(original) => {
                let answer = fixes.or(match self.prefs.export_fixes {
                    ExportFixes::Apply => Some(true),
                    ExportFixes::Keep => Some(false),
                    ExportFixes::Ask => None,
                });
                match answer {
                    Some(true) => None,
                    Some(false) => Some(original),
                    None => {
                        self.pending_export_fixes =
                            Some(PendingExportFixes::SaveEntry { doc_id, path });
                        return Ok(None);
                    }
                }
            }
        };
        let (bytes, repairs) = match kept {
            Some(original) => (original, Vec::new()),
            None => canonical?,
        };
        atomic_write_with_backup(&path, &bytes)
            .map_err(|e| tr!("Write '{path}': {e}", e = e, path = path.display()))?;
        self.file_replaced(&path);
        if let Some(doc) = self.document_mut(doc_id) {
            doc.mark_saved();
        }
        self.report_repairs(doc_id, &repairs);
        Ok(Some(path))
    }

    /// Tells the user each count a save of the document rewrote from its run.
    pub(super) fn report_repairs(&mut self, doc_id: DocumentId, repairs: &[String]) {
        if repairs.is_empty() {
            return;
        }
        let name = self.document(doc_id).map(|d| d.title()).unwrap_or_default();
        for repair in repairs {
            tracing::info!("Repaired {name}: {repair}");
        }
        self.show_toast(tr!(
            "Repaired {count} count(s) in {name}: {repairs}",
            name = name,
            count = repairs.len(),
            repairs = repairs.join("; ")
        ));
    }

    fn save_entry_into_archive(
        &mut self,
        doc_id: DocumentId,
        binding: RpfEntryBinding,
        bytes: Vec<u8>,
        overlay: bool,
    ) -> Result<Option<PathBuf>, String> {
        let holder = self.holder_of(binding);
        let dest = match (&holder, overlay) {
            (_, false) => None,
            (Some(h), true) => Some(
                self.overlay_for(h.path())
                    .ok_or(tr!("This archive is not inside a game install"))?,
            ),
            (None, true) => return Err(tr!("This archive is not on disk").to_string()),
        };
        self.update_rpf_entry_bytes(binding.parent_doc_id, binding.entry_id, bytes)?;
        let written = match holder {
            Some(holder) => Some(self.write_holder(binding.parent_doc_id, holder, dest)?),
            None => {
                self.show_info_toast(tr!("Updated in its archive; save that archive to write it"));
                None
            }
        };
        if let Some(doc) = self.document_mut(doc_id) {
            doc.mark_saved();
        }
        Ok(written)
    }

    /// Write the archive that holds an entry to its own path, or to `dest`, which it then points at.
    fn write_holder(
        &mut self,
        parent: DocumentId,
        holder: Holder,
        dest: Option<PathBuf>,
    ) -> Result<PathBuf, String> {
        let dest = dest.filter(|d| d != holder.path());
        match (holder, dest) {
            (Holder::Mounted(archive_id, _), None) => self.save_one_archive(parent, archive_id),
            (Holder::Mounted(archive_id, base), Some(dest)) => {
                let (bytes, _) = self.archive_bytes(parent, archive_id, Some(&dest))?;
                write_into_place(&dest, &bytes)?;
                self.note_overlay_base(&base, &dest);
                self.expander.files_replaced();
                self.file_replaced(&dest);
                if let Some(doc) = self.document_mut(parent) {
                    if let DocPayload::Rpf(rpf) = &mut doc.payload {
                        // ⚠ Left on the shipped path, the next overlay save finds this copy and refuses it.
                        if let Some(slot) = rpf.session.archives.get_mut(&archive_id) {
                            slot.fs_path = Some(dest.clone());
                        }
                    }
                    doc.archive_written(archive_id);
                }
                Ok(dest)
            }
            (Holder::Standalone(path), dest) => {
                let doc = self
                    .document(parent)
                    .ok_or_else(|| tr!("Document {parent} not found", parent = parent))?;
                let bytes = serialize_payload(
                    &doc.payload,
                    self.prefs.rpf_output_mode,
                    self.keys.provider(),
                )?;
                let written = match dest {
                    Some(dest) => {
                        self.no_job_reads(&dest, "save")?;
                        write_into_place(&dest, &bytes)?;
                        self.note_overlay_base(&path, &dest);
                        dest
                    }
                    None => {
                        self.no_job_reads(&path, "save")?;
                        atomic_write_with_backup(&path, &bytes).map_err(|e| {
                            tr!("Write '{path}': {e}", e = e, path = path.display())
                        })?;
                        path
                    }
                };
                self.file_replaced(&written);
                if let Some(doc) = self.document_mut(parent) {
                    doc.path = Some(written.clone());
                    doc.mark_saved();
                }
                Ok(written)
            }
        }
    }

    /// The file on disk an entry is written back into, if a save of it reaches disk.
    fn holder_of(&self, binding: RpfEntryBinding) -> Option<Holder> {
        let doc = self.document(binding.parent_doc_id)?;
        let DocPayload::Rpf(rpf) = &doc.payload else {
            return None;
        };
        if let Some((archive_id, path)) = rpf.session.archive_on_disk_around(binding.entry_id) {
            return Some(Holder::Mounted(archive_id, path));
        }
        let standalone = doc.kind == DocumentKind::Rpf && doc.rpf_entry_binding.is_none();
        doc.path
            .clone()
            .filter(|_| standalone)
            .map(Holder::Standalone)
    }

    /// Where an overlay save of the archive at `path` writes: the archive itself once it is the overlay's.
    fn overlay_for(&self, path: &Path) -> Option<PathBuf> {
        if retail::classify(path, &self.prefs) == retail::PathClass::Overlay {
            return Some(path.to_path_buf());
        }
        retail::overlay_copy_target(path, &self.prefs)
    }

    fn entry_bytes(&self, binding: RpfEntryBinding) -> Option<Vec<u8>> {
        match &self.document(binding.parent_doc_id)?.payload {
            DocPayload::Rpf(rpf) => rpf.session.bytes_for(binding.entry_id).map(|b| b.to_vec()),
            _ => None,
        }
    }
}

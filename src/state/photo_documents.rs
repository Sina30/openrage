//! Files held as documents with no load job, tab or history, for a photograph of a model.

use std::path::Path;

use super::{document_io, AppState, Document, DocumentId, DocumentKind};

impl AppState {
    /// `bytes` read as the file at `path` and held as a document that is not made active.
    pub fn hold_bytes(&mut self, path: &Path, bytes: &[u8]) -> Result<DocumentId, String> {
        let kind = DocumentKind::from_path(path).ok_or("not a kind of file OpenRage opens")?;
        let payload = document_io::parse_payload(path, kind, bytes, self.keys.provider())?;
        let id = self.alloc_doc_id();
        self.documents
            .push(Document::new(id, Some(path.to_path_buf()), payload));
        Ok(id)
    }

    /// `bytes` read as the resource file at `path` of `kind`, held by nobody: a photograph keeps what it parsed itself.
    pub fn parse_resource(
        &self,
        path: &Path,
        kind: DocumentKind,
        bytes: &[u8],
    ) -> Result<or_io::Asset, String> {
        match document_io::parse_payload(path, kind, bytes, self.keys.provider())? {
            super::DocPayload::Resource(asset) => Ok(asset),
            _ => Err("not a resource file".into()),
        }
    }

    /// Drop documents [`Self::hold_bytes`] held, and what texture resolution kept about them.
    pub fn let_go(&mut self, ids: &[DocumentId]) {
        self.documents.retain(|doc| !ids.contains(&doc.id));
        for id in ids {
            self.forget_texture_resolution(*id);
        }
        if self.active_document.is_some_and(|id| ids.contains(&id)) {
            self.active_document = None;
        }
    }
}

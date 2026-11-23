//! Records of a metadata array copied and pasted after the selection in another file, or deleted.

use or_io::meta::Value;

use super::super::meta_edit::{value_at, MetaEdit};
use super::super::presets::{record_of, MetaStep};
use super::{AppState, Clipboard, Held};
use crate::state::{Change, DocPayload, DocumentId};

/// Copied records, each whole.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Records {
    items: Vec<Value>,
}

impl AppState {
    /// The array of `doc` holding the selected records, and their indices, oldest first.
    fn selected_records(&self, doc: DocumentId) -> Result<(Vec<MetaStep>, Vec<usize>), String> {
        let (array, indices) = (self.records_selected.of(doc))
            .ok_or("Select a record first: tick its box in the form")?;
        Ok((array.to_vec(), indices.iter().copied().collect()))
    }

    /// The array at `path` in `doc`'s metadata.
    fn records_at(&mut self, doc: DocumentId, path: &[MetaStep]) -> Option<&mut Vec<Value>> {
        let DocPayload::Meta(meta) = &mut self.document_mut(doc)?.payload else {
            return None;
        };
        match value_at(&mut meta.parsed.as_mut()?.instance, path)? {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Copy the records selected in `doc`'s metadata form.
    pub fn copy_records(&mut self, doc: DocumentId) -> Result<String, String> {
        let (array, indices) = self.selected_records(doc)?;
        let items = self
            .records_at(doc, &array)
            .ok_or(tr!("The selected records are no longer in the file"))?;
        let items: Vec<Value> = (indices.iter().filter_map(|&at| items.get(at)))
            .filter(|item| record_of(item).is_some())
            .cloned()
            .collect();
        let said = tr!("Copied {count} record(s)", count = items.len());
        self.panels.get::<Clipboard>().held = Some(Held::Records(Records { items }));
        Ok(said)
    }

    /// Paste the copied records after the last record selected in `doc`, refusing a record of a type its array does not hold with a sentence.
    pub fn paste_records(&mut self, doc: DocumentId) -> Result<String, String> {
        let Some(Held::Records(copied)) = self.held().cloned() else {
            return Err(tr!("Nothing to paste: copy records first").into());
        };
        let (array, indices) = self.selected_records(doc)?;
        let items = self
            .records_at(doc, &array)
            .ok_or(tr!("The selected records are no longer in the file"))?;
        let holds: Vec<String> = items
            .iter()
            .filter_map(record_of)
            .map(|r| r.type_name.clone())
            .collect();
        let stranger = (copied.items.iter().filter_map(record_of))
            .find(|record| !holds.contains(&record.type_name));
        if let Some(stranger) = stranger {
            let mut held = holds.clone();
            held.sort();
            held.dedup();
            let named = array.iter().rev().find_map(|step| match step {
                MetaStep::Field(name) | MetaStep::Nth(name, _) => Some(name.as_str()),
                MetaStep::Index(_) => None,
            });
            return Err(tr!("`{named}` holds {held}, so a {type_name} cannot go into it, and nothing was pasted", named = named.unwrap_or(tr!("This array")), held = held.join(" and "), type_name = stranger.type_name));
        }
        let after = indices.last().map_or(items.len(), |&last| last + 1);
        let mut edits = Vec::new();
        for (i, item) in copied.items.iter().cloned().enumerate() {
            items.insert(after + i, item);
            edits.push(MetaEdit::Element {
                path: array.clone(),
                at: after + i,
                other: None,
            });
        }
        let pasted = edits.len();
        self.after_records_changed(doc);
        let label = tr!("Paste {pasted} record(s)", pasted = pasted);
        self.record(doc, label.clone(), Change::Meta(edits));
        (self.records_selected).set(doc, &array, after..after + pasted);
        Ok(label)
    }

    /// Take the records selected in `doc` out of their array, as one step.
    pub fn delete_records(&mut self, doc: DocumentId) -> Result<String, String> {
        let (array, indices) = self.selected_records(doc)?;
        let items = self
            .records_at(doc, &array)
            .ok_or(tr!("The selected records are no longer in the file"))?;
        // ⚠ Last first, so each index still names the record it was selected as.
        let mut edits = Vec::new();
        for &at in indices.iter().rev() {
            if at >= items.len() {
                continue;
            }
            edits.push(MetaEdit::Element {
                path: array.clone(),
                at,
                other: Some(items.remove(at)),
            });
        }
        let label = tr!("Delete {count} record(s)", count = edits.len());
        self.after_records_changed(doc);
        self.record(doc, label.clone(), Change::Meta(edits));
        self.records_selected.set(doc, &array, []);
        Ok(label)
    }

    fn after_records_changed(&mut self, doc: DocumentId) {
        if let Some(DocPayload::Meta(meta)) = self.document_mut(doc).map(|d| &mut d.payload) {
            meta.after_visual_edit_in_place();
        }
    }

    /// Whether copied records are held for a paste.
    pub fn records_to_paste(&self) -> bool {
        matches!(self.held(), Some(Held::Records(_)))
    }
}

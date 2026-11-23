//! A field of the inspector's grid copied whole and pasted over a field of its type.
//!
//! ⚠ A field is read off the grid's labels, `Owner > Part` and `Run [i]`: the
//! parts of a struct that holds nothing nested, or of a short array, go
//! together, as a vector or a colour does, and any other row goes alone.

use or_editor::{Edit, EditKind, Summary};
use or_io::desc::{Kind, ScalarValue};

use super::{AppState, Clipboard, DocumentId, Held};

/// A field's value: its name, and each part's name, kind and value.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FieldValue {
    name: String,
    parts: Vec<(String, EditKind, ScalarValue)>,
}

/// The field a label names a part of, and the part's name there.
fn owner_of(label: &str) -> Option<(&str, &str)> {
    if let Some((run, _)) = label.rsplit_once(" [").filter(|(_, i)| i.ends_with(']')) {
        return Some((run, &label[run.len() + 1..]));
    }
    label.rsplit_once(" > ")
}

/// The field row `at` of `summary` belongs to, by its label and rows: a small struct or a short array whole, else the row alone.
fn field_rows(summary: &Summary, at: usize) -> (String, Vec<usize>) {
    let Some(row) = summary.rows.get(at) else {
        return (String::new(), Vec::new());
    };
    let alone = (row.label.clone(), vec![at]);
    let Some((owner, _)) = owner_of(&row.label) else {
        return alone;
    };
    let part = |label: &str| owner_of(label).is_some_and(|(o, _)| o == owner);
    let nested = summary.rows.iter().any(|r| {
        let below = r.label.strip_prefix(owner);
        below.is_some_and(|rest| rest.starts_with(" > ")) && !part(&r.label)
    });
    if nested {
        return alone;
    }
    let rows = (summary.rows.iter().enumerate())
        .filter(|(_, r)| part(&r.label))
        .map(|(i, _)| i)
        .collect();
    (owner.to_string(), rows)
}

/// The field row `at` belongs to: the name it is shown by, and each editable part's name and edit.
fn parts_of(summary: &Summary, at: usize) -> (String, Vec<(String, Edit)>) {
    let (field, rows) = field_rows(summary, at);
    let alone = summary.rows.get(at).is_some_and(|r| r.label == field);
    let parts = (rows.iter().filter_map(|&i| summary.rows.get(i)))
        .filter_map(|r| {
            let part = match owner_of(&r.label) {
                Some((_, part)) if !alone => part.to_string(),
                _ => String::new(),
            };
            Some((part, r.edit?))
        })
        .collect();
    (shown(&field), parts)
}

fn shown(field: &str) -> String {
    field.rsplit(" > ").next().unwrap_or(field).to_string()
}

/// The name of the field row `at` of `summary` belongs to, as its menu says it.
pub fn field_name(summary: &Summary, at: usize) -> String {
    shown(&field_rows(summary, at).0)
}

/// A field's parts as a sentence says them: `X, Y, Z, W (float)`, or `one 32-bit integer`.
fn described(parts: &[(String, EditKind)]) -> String {
    let mut kinds: Vec<String> = parts
        .iter()
        .map(|(_, kind)| match kind {
            EditKind::Bool => tr!("yes or no").to_string(),
            EditKind::F32 => "float".to_string(),
            EditKind::F64 => "double".to_string(),
            EditKind::Int(w) | EditKind::Enum(w) => tr!("{bits}-bit integer", bits = w.size() * 8),
        })
        .collect();
    kinds.dedup();
    let kinds = kinds.join(" and ");
    match parts {
        [(part, _)] if part.is_empty() => format!("one {kinds}"),
        _ => {
            let names: Vec<&str> = parts.iter().map(|(part, _)| part.as_str()).collect();
            format!("{} ({kinds})", names.join(", "))
        }
    }
}

impl AppState {
    /// Copy the field of `doc`'s grid that row `at` of `summary`, read from `doc`, belongs to.
    pub fn copy_field(
        &mut self,
        doc: DocumentId,
        summary: &Summary,
        at: usize,
    ) -> Result<String, String> {
        let (name, parts) = parts_of(summary, at);
        if parts.is_empty() {
            return Err(tr!(
                "Nothing in {name} can be edited, so nothing was copied",
                name = name
            ));
        }
        let data = &self
            .document(doc)
            .and_then(|d| d.resource.as_ref())
            .ok_or(tr!("The document holds no bytes to copy from"))?
            .data;
        let parts = (parts.into_iter())
            .map(|(part, edit)| {
                let kind = Kind::of_scalar(super::super::pack_edit::scalar(edit.kind));
                let bytes = data.get(edit.offset..edit.offset + kind.size())?;
                Some((part, edit.kind, kind.decode(bytes)?))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                tr!(
                    "{name} could not be read, so nothing was copied",
                    name = name
                )
            })?;
        let said = tr!("Copied {name}", name = name);
        self.panels.get::<Clipboard>().held = Some(Held::Field(FieldValue { name, parts }));
        Ok(said)
    }

    /// Paste the copied field into `doc` over the one row `at` of `summary`, read from `doc`, belongs to, refusing a field of another type with a sentence.
    pub fn paste_field(
        &mut self,
        doc: DocumentId,
        summary: &Summary,
        at: usize,
    ) -> Result<String, String> {
        let Some(Held::Field(copied)) = self.held().cloned() else {
            return Err(tr!("Nothing to paste: copy a field first").into());
        };
        let (name, parts) = parts_of(summary, at);
        let here: Vec<(String, EditKind)> = (parts.iter())
            .map(|(part, edit)| (part.clone(), edit.kind))
            .collect();
        let there: Vec<(String, EditKind)> = (copied.parts.iter())
            .map(|(part, kind, _)| (part.clone(), *kind))
            .collect();
        if here != there {
            return Err(tr!(
                "{name} holds {here}, and the copied {name_2} holds {there}, so nothing was pasted",
                name = name,
                here = described(&here),
                name_2 = copied.name,
                there = described(&there)
            ));
        }
        let writes: Vec<(Edit, ScalarValue)> = (parts.iter().zip(&copied.parts))
            .map(|((_, edit), (_, _, value))| (*edit, value.clone()))
            .collect();
        let label = tr!(
            "Paste {name_2} into {name}",
            name = name,
            name_2 = copied.name
        );
        if !self.patch_scalars(doc, label.clone(), &writes) {
            return Err(tr!(
                "{name} did not take the value, so nothing was pasted",
                name = name
            ));
        }
        Ok(label)
    }

    /// Whether a field's value is held for a paste.
    pub fn field_to_paste(&self) -> bool {
        matches!(self.held(), Some(Held::Field(_)))
    }
}

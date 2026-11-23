//! The documents of a project that only this machine can name, kept beside the project and out of the file a repository shares.
//!
//! A document outside the project's folder has no path another machine could use: `store_path` can
//! only write it absolute, which puts a user name and a drive letter into a shared file. It goes to
//! `machine.json` in the project's own folder of `.openrage` instead (`private_dir`) and comes back, open
//! as it was, when the project is read here.

use std::io;
use std::path::Path;

use or_project::snapshot::{private_dir, PRIVATE_DIR};
use or_project::{project_folder, relative_inside, store_path};
use serde_json::{json, Value};

use super::{decode_document, LoadedProject, ProjectDocument, ProjectFile, PROJECT_SCHEMA};

const FILE: &str = "machine.json";

/// A document kept apart, with whether its tab was open and was the one in front.
pub(super) struct Apart {
    doc: ProjectDocument,
    open: bool,
    active: bool,
}

/// The project as the shared file holds it, and the documents it cannot name.
pub(super) fn apart(project: &ProjectFile, folder: &Path) -> (ProjectFile, Vec<Apart>) {
    let mut shared = ProjectFile {
        documents: Vec::new(),
        open: Vec::new(),
        active: None,
        ..project.clone()
    };
    let mut apart = Vec::new();
    // ⚠ `open` and `active` count the documents, so each is carried to the place its document has now.
    let mut place: Vec<Option<usize>> = Vec::new();
    for (index, doc) in project.documents.iter().enumerate() {
        let here = std::path::absolute(&doc.path).unwrap_or_else(|_| doc.path.clone());
        if relative_inside(&here, folder).is_some() {
            place.push(Some(shared.documents.len()));
            shared.documents.push(doc.clone());
        } else {
            place.push(None);
            apart.push(Apart {
                doc: doc.clone(),
                open: project.open.contains(&index),
                active: project.active == Some(index),
            });
        }
    }
    let placed = |index: &usize| place.get(*index).copied().flatten();
    shared.open = project.open.iter().filter_map(placed).collect();
    shared.active = project.active.as_ref().and_then(placed);
    (shared, apart)
}

/// Write the documents kept apart for the project file `project`, or remove the file when there are none, so a document the project dropped does not come back.
pub(super) fn keep(project: &Path, apart: &[Apart]) -> io::Result<()> {
    let folder = project_folder(project);
    let dir = private_dir(project);
    let file = dir.join(FILE);
    if apart.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let documents: Vec<Value> = (apart.iter())
        .map(|kept| {
            let mut entry = kept.doc.extra.clone();
            entry.insert(
                "path".into(),
                Value::String(store_path(&kept.doc.path, &folder)),
            );
            if let Some(kind) = kept.doc.kind.and_then(|k| serde_json::to_value(k).ok()) {
                entry.insert("kind".into(), kind);
            }
            if kept.open {
                entry.insert("open".into(), Value::Bool(true));
            }
            if kept.active {
                entry.insert("active".into(), Value::Bool(true));
            }
            Value::Object(entry)
        })
        .collect();
    let bytes =
        serde_json::to_vec_pretty(&json!({ "documents": documents })).map_err(io::Error::other)?;
    std::fs::create_dir_all(&dir)?;
    // ⚠ Everything in the folder is this machine's, so the folder ignores itself.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, b"*\n")?;
    }
    or_project::replace_file(&file, &bytes)
}

/// Add the documents kept apart for the project file `project` to `loaded`, open as they were; one the file names already is not added twice.
pub(super) fn join(project: &Path, loaded: &mut LoadedProject) {
    let folder = project_folder(project);
    let file = private_dir(project).join(FILE);
    // ⚠ The file used to be one for the folder, whatever project files it held: the first project file read here takes it, and a second is not given the first's documents.
    let older = folder.join(PRIVATE_DIR).join(FILE);
    if !file.exists() && older.is_file() {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&older, &file);
    }
    let Ok(bytes) = std::fs::read(&file) else {
        return;
    };
    let Ok(Value::Object(top)) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    let Some(Value::Array(entries)) = top.get("documents") else {
        return;
    };
    let mut unread = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Value::Object(fields) = entry else {
            continue;
        };
        let mut fields = fields.clone();
        let flag = |fields: &mut serde_json::Map<String, Value>, key: &str| {
            fields.remove(key) == Some(Value::Bool(true))
        };
        let (open, active) = (flag(&mut fields, "open"), flag(&mut fields, "active"));
        let Some(doc) = decode_document(
            index,
            &Value::Object(fields),
            PROJECT_SCHEMA,
            &folder,
            &folder,
            &mut unread,
        ) else {
            continue;
        };
        let project = &mut loaded.project;
        if project.documents.iter().any(|held| held.path == doc.path) {
            continue;
        }
        project.documents.push(doc);
        let at = project.documents.len() - 1;
        if open {
            project.open.push(at);
        }
        if active && project.active.is_none() {
            project.active = Some(at);
        }
    }
}

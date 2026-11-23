//! The `.openrage-proj` file: which documents a project holds, written so the folder can travel.
//!
//! ⚠ **Read field by field, through [`serde_json::Value`].** A derived
//! `Deserialize` turns one unreadable field into an unreadable file. Each field
//! here that fails to read costs itself and a line in
//! [`LoadedProject::findings`], and a key this build does not know is carried
//! and written back rather than dropped.

use std::path::{Path, PathBuf};

use or_project::{resolve_member, store_path, Container};
use serde_json::{Map, Value};

use super::document::DocumentKind;
use crate::ui::dock::Workspace;

pub use or_project::{project_folder, shadow_root, PROJECT_EXTENSION};

/// The schema this build writes; [`or_project::SCHEMA`] says what each one added.
pub const PROJECT_SCHEMA: u32 = or_project::SCHEMA;

mod machine;

const SHOWN_KEY: &str = "shown_workspaces";
const KNOWN_TOP: [&str; 8] = [
    "schema",
    "workspace",
    "active",
    "open",
    "documents",
    SHOWN_KEY,
    or_project::container::MODULES_KEY,
    or_project::container::OUTPUTS_KEY,
];
const KNOWN_DOCUMENT: [&str; 2] = ["path", "kind"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectFile {
    pub workspace: Workspace,
    /// Index into [`Self::documents`].
    pub active: Option<usize>,
    /// Indices into [`Self::documents`] of the ones open as tabs, in tab order.
    pub open: Vec<usize>,
    /// The project's members, open or not.
    pub documents: Vec<ProjectDocument>,
    /// The workspace buttons the strip shows, in order; none says every one is.
    pub shown: Option<Vec<Workspace>>,
    /// The content the project holds and the builds it declares.
    pub container: Container,
    /// Top-level keys this build does not read, written back as they came.
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDocument {
    /// Where the file is on this machine: already resolved against the project folder.
    pub path: PathBuf,
    pub kind: Option<DocumentKind>,
    /// Keys on this entry this build does not read, written back as they came.
    pub extra: Map<String, Value>,
}

impl ProjectDocument {
    pub fn new(path: PathBuf, kind: Option<DocumentKind>) -> Self {
        Self {
            path,
            kind,
            extra: Map::new(),
        }
    }
}

/// What an open project holds beyond its tabs: its members and the keys this build does not read.
///
/// ⚠ **A project is rebuilt from this and the open documents on every save**,
/// so a member whose tab was closed, or a key an older build does not read,
/// lives here or is gone at the next save.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Carried {
    pub top: Map<String, Value>,
    pub members: Vec<ProjectDocument>,
    pub container: Container,
    /// The workspace buttons the strip shows; none says every one is.
    pub shown: Option<Vec<Workspace>>,
    /// The workspace the file states the project opens on, which a save writes as it is: the one on screen is the machine's.
    pub workspace: Workspace,
    /// The project changed in a way the file does not hold: a member removed, a build added, a button pinned.
    pub changed: bool,
}

impl Carried {
    pub fn from_project(project: &ProjectFile) -> Self {
        Self {
            top: project.extra.clone(),
            members: project.documents.clone(),
            container: project.container.clone(),
            shown: project.shown.clone(),
            workspace: project.workspace,
            changed: false,
        }
    }

    /// The index of the member at `path`, which is added if it is not one yet.
    pub fn member(&mut self, path: &Path, kind: DocumentKind) -> usize {
        let index = match self.members.iter().position(|m| m.path == path) {
            Some(index) => index,
            None => {
                self.members
                    .push(ProjectDocument::new(path.to_path_buf(), None));
                self.members.len() - 1
            }
        };
        self.members[index].kind = Some(kind);
        index
    }
}

#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub project: ProjectFile,
    /// The schema the file declared, before it was migrated.
    pub schema: u32,
    /// One sentence per field that could not be read and what was used instead.
    pub findings: Vec<String>,
}

pub fn load_project(path: &Path) -> std::io::Result<LoadedProject> {
    let bytes = std::fs::read(path)?;
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut loaded = decode(&bytes, path, &cwd)?;
    machine::join(path, &mut loaded);
    Ok(loaded)
}

/// Write the project at `path`: the shared file, which names no path of this machine, and beside it the files only this machine can name.
///
/// ⚠ The file is not written when it would read the same, so a save that changed nothing leaves
/// its bytes and its date alone for a repository and an editor to see.
pub fn save_project(path: &Path, project: &ProjectFile) -> std::io::Result<()> {
    let folder = project_folder(path);
    let (shared, apart) = machine::apart(project, &folder);
    let value = encode(&shared, path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if !std::fs::read(path).is_ok_and(|old| old == bytes) {
        or_project::replace_file(path, &bytes)?;
    }
    // ⚠ Best effort: the files that are not the project's cost the tabs they were, never the save.
    if let Err(e) = machine::keep(path, &apart) {
        tracing::warn!(error = %e, "the files outside the project's folder were not kept beside it");
    }
    Ok(())
}

/// Read a project file's bytes.
///
/// `cwd` is what a schema-1 relative path was relative to: that schema wrote
/// whatever path the process held, so the folder cannot be assumed for it.
pub fn decode(bytes: &[u8], project_path: &Path, cwd: &Path) -> std::io::Result<LoadedProject> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let Value::Object(mut top) = value else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            tr!("a project file is a JSON object"),
        ));
    };
    let mut findings = Vec::new();

    let schema = match top.get("schema") {
        None => 1,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => n,
            None => {
                findings.push(tr!(
                    "`schema` is {v}, not a number; read as schema 1",
                    v = v
                ));
                1
            }
        },
    };
    if schema > PROJECT_SCHEMA {
        findings.push(tr!(
            "written by a newer OpenRage (schema {schema}); saving writes schema \
             {project_schema} and keeps the fields this version does not read",
            schema = schema,
            project_schema = PROJECT_SCHEMA
        ));
    }

    let workspace = match top.get("workspace") {
        None => Workspace::default(),
        Some(v) => serde_json::from_value(v.clone()).unwrap_or_else(|_| {
            findings.push(tr!(
                "`workspace` is {v}, which names no workspace; using {default}",
                v = v,
                default = Workspace::default().label()
            ));
            Workspace::default()
        }),
    };

    let active = match top.get("active") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64().and_then(|n| usize::try_from(n).ok()) {
            Some(n) => Some(n),
            None => {
                findings.push(tr!(
                    "`active` is {v}, not an index; no document is active",
                    v = v
                ));
                None
            }
        },
    };

    let folder = project_folder(project_path);
    let mut documents = Vec::new();
    // ⚠ Stored indices count every entry, and an unreadable one is skipped,
    // so each stored index maps to its entry's place in `documents`.
    let mut kept: Vec<Option<usize>> = Vec::new();
    match top.get("documents") {
        None | Some(Value::Null) => {}
        Some(Value::Array(entries)) => {
            for (i, entry) in entries.iter().enumerate() {
                let doc = decode_document(i, entry, schema, &folder, cwd, &mut findings);
                kept.push(doc.map(|doc| {
                    documents.push(doc);
                    documents.len() - 1
                }));
            }
        }
        Some(v) => findings.push(tr!(
            "`documents` is {v}, not a list; the project holds none",
            v = v
        )),
    }
    let kept_at = |i: usize| kept.get(i).copied().flatten();
    let active = active.and_then(kept_at);

    // Schema 2 wrote only the open tabs, so each of its documents was open.
    let open = if schema < 3 {
        (0..documents.len()).collect()
    } else {
        match top.get("open") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(entries)) => entries
                .iter()
                .filter_map(|v| {
                    let index = v.as_u64().and_then(|n| usize::try_from(n).ok());
                    if index.is_none() {
                        findings.push(tr!("`open` entry {v} is not an index; skipped", v = v));
                    }
                    index.and_then(kept_at)
                })
                .collect(),
            Some(v) => {
                findings.push(tr!("`open` is {v}, not a list; no document opens", v = v));
                Vec::new()
            }
        }
    };

    let shown = match top.get(SHOWN_KEY) {
        None | Some(Value::Null) => None,
        Some(Value::Array(names)) => {
            let known: Vec<Workspace> = names
                .iter()
                .filter_map(|name| {
                    let read = serde_json::from_value(name.clone()).ok();
                    if read.is_none() {
                        findings.push(tr!("`{shown_key}` names {name}, which is no workspace this build knows; skipped", shown_key = SHOWN_KEY, name = name));
                    }
                    read
                })
                .collect();
            Some(known).filter(|known| !known.is_empty())
        }
        Some(v) => {
            findings.push(tr!(
                "`{shown_key}` is {v}, not a list; every workspace button is shown",
                shown_key = SHOWN_KEY,
                v = v
            ));
            None
        }
    };

    let container = Container::read(&top, &mut findings);
    top.retain(|key, _| !KNOWN_TOP.contains(&key.as_str()));
    Ok(LoadedProject {
        project: ProjectFile {
            workspace,
            active,
            open,
            documents,
            shown,
            container,
            extra: top,
        },
        schema,
        findings,
    })
}

fn decode_document(
    index: usize,
    entry: &Value,
    schema: u32,
    folder: &Path,
    cwd: &Path,
    findings: &mut Vec<String>,
) -> Option<ProjectDocument> {
    let Value::Object(fields) = entry else {
        findings.push(tr!(
            "document {index} is {entry}, not an entry; skipped",
            index = index,
            entry = entry
        ));
        return None;
    };
    let Some(stored) = fields.get("path").and_then(Value::as_str) else {
        findings.push(tr!(
            "document {index} has no readable `path`; skipped",
            index = index
        ));
        return None;
    };
    let path = resolve_member(stored, schema, folder, cwd);

    let kind = match fields.get("kind") {
        None | Some(Value::Null) => None,
        Some(v) => serde_json::from_value(v.clone()).ok().or_else(|| {
            findings.push(tr!(
                "document {index} (`{stored}`) has kind {v}, which this build does \
                 not know; it is read from the extension instead",
                index = index,
                stored = stored,
                v = v
            ));
            None
        }),
    };

    let mut extra = fields.clone();
    extra.retain(|key, _| !KNOWN_DOCUMENT.contains(&key.as_str()));
    Some(ProjectDocument { path, kind, extra })
}

/// The key of a member entry that states the id of the source folder it names.
pub const SOURCE_ID: &str = "id";

/// The id the folder at `path` states in its `asset.json`, when it is a source folder that is there.
fn source_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    or_source::asset::parse_folder_name(name)?;
    let text = std::fs::read_to_string(path.join(or_source::asset::MANIFEST)).ok()?;
    Some(or_source::asset::AssetManifest::parse(&text).ok()?.id)
}

/// Write a project as JSON, with every path inside its folder made relative.
pub fn encode(project: &ProjectFile, project_path: &Path) -> serde_json::Result<Value> {
    let folder = project_folder(project_path);
    let documents = project
        .documents
        .iter()
        .map(|doc| {
            let mut entry = doc.extra.clone();
            entry.insert("path".into(), Value::String(store_path(&doc.path, &folder)));
            if let Some(kind) = doc.kind {
                entry.insert("kind".into(), serde_json::to_value(kind)?);
            }
            // ⚠ A source folder is stated by its id as well as its path, so a project can follow it when its folder moves; the path stays the fallback.
            if let Some(id) = source_id(&doc.path) {
                entry.insert(SOURCE_ID.into(), Value::String(id));
            }
            Ok(Value::Object(entry))
        })
        .collect::<serde_json::Result<Vec<_>>>()?;

    let mut top = project.extra.clone();
    top.insert("schema".into(), PROJECT_SCHEMA.into());
    top.insert("workspace".into(), serde_json::to_value(project.workspace)?);
    if let Some(active) = project.active {
        top.insert("active".into(), active.into());
    }
    top.insert("open".into(), serde_json::to_value(&project.open)?);
    top.insert("documents".into(), Value::Array(documents));
    if let Some(shown) = &project.shown {
        top.insert(SHOWN_KEY.into(), serde_json::to_value(shown)?);
    }
    project.container.write(&mut top);
    Ok(Value::Object(top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(path: &Path, body: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn stored_paths(file: &Path) -> Vec<String> {
        let value: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        value["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["path"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn a_project_round_trips_through_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("proj").join("my.openrage-proj");
        let outside = dir.path().join("elsewhere").join("b.ytd");
        let project = ProjectFile {
            workspace: Workspace::Model,
            active: Some(1),
            open: vec![0, 1],
            documents: vec![
                ProjectDocument::new(
                    dir.path().join("proj/models/props/a.ydr"),
                    Some(DocumentKind::Ydr),
                ),
                ProjectDocument::new(outside.clone(), None),
            ],
            shown: None,
            container: Container::default(),
            extra: Map::new(),
        };

        save_project(&file, &project).unwrap();
        let loaded = load_project(&file).unwrap();

        assert_eq!(loaded.schema, PROJECT_SCHEMA);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.project, project);
        assert_eq!(
            stored_paths(&file),
            vec!["models/props/a.ydr".to_string()],
            "inside the folder is relative with `/` on every host; a file outside it is not in the shared file"
        );
        assert!(
            std::fs::read_to_string(or_project::snapshot::private_dir(&file).join("machine.json"))
                .unwrap()
                .contains("b.ytd"),
            "it is kept beside the project, for this machine"
        );
    }

    #[test]
    fn a_project_folder_moved_elsewhere_still_opens_its_documents() {
        let dir = tempfile::tempdir().unwrap();
        let before = dir.path().join("machine-a").join("proj");
        write(&before.join("models/a.ydr"), b"drawable");
        write(&before.join("textures/a.ytd"), b"dictionary");
        let project = ProjectFile {
            documents: vec![
                ProjectDocument::new(before.join("models/a.ydr"), Some(DocumentKind::Ydr)),
                ProjectDocument::new(before.join("textures/a.ytd"), Some(DocumentKind::Ytd)),
            ],
            ..ProjectFile::default()
        };
        save_project(&before.join("p.openrage-proj"), &project).unwrap();

        let after = dir.path().join("machine-b").join("unzipped").join("proj");
        copy_tree(&before, &after);
        std::fs::remove_dir_all(dir.path().join("machine-a")).unwrap();

        let loaded = load_project(&after.join("p.openrage-proj")).unwrap();
        let paths: Vec<&Path> = loaded
            .project
            .documents
            .iter()
            .map(|d| d.path.as_path())
            .collect();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], after.join("models/a.ydr"));
        assert_eq!(paths[1], after.join("textures/a.ytd"));
        let found = paths.iter().filter(|p| p.is_file()).count();
        assert_eq!(found, 2, "every document is found at its new home");
    }

    /// ⚠ The fixture is what schema 1's derived `Serialize` wrote: every path
    /// as the process held it, and `kind: null` rather than an absent key.
    #[test]
    fn a_schema_one_file_still_opens_and_is_rewritten_relative() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("proj");
        let file = folder.join("old.openrage-proj");
        let inside = folder.join("models").join("a.ydr");
        let cwd = dir.path().join("where-the-editor-ran");
        let schema_one = json!({
            "schema": 1,
            "workspace": "Texture",
            "active": 0,
            "documents": [
                { "path": inside, "kind": "Ydr" },
                { "path": "loose/b.ydr", "kind": null },
            ],
        });
        write(
            &file,
            serde_json::to_string_pretty(&schema_one)
                .unwrap()
                .as_bytes(),
        );

        let loaded = decode(&std::fs::read(&file).unwrap(), &file, &cwd).unwrap();
        assert_eq!(loaded.schema, 1);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.project.workspace, Workspace::Texture);
        assert_eq!(loaded.project.active, Some(0));
        assert_eq!(loaded.project.documents[0].path, inside);
        assert_eq!(loaded.project.documents[0].kind, Some(DocumentKind::Ydr));
        assert_eq!(
            loaded.project.documents[1].path,
            cwd.join("loose").join("b.ydr"),
            "a schema-1 relative path meant the process's directory, not the folder"
        );

        save_project(&file, &loaded.project).unwrap();
        let rewritten: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(rewritten["schema"], json!(PROJECT_SCHEMA));
        assert_eq!(
            stored_paths(&file),
            vec!["models/a.ydr".to_string()],
            "saving migrates: inside the folder becomes relative, and a file outside it is kept beside the project, not in it"
        );
        let reread = load_project(&file).unwrap();
        assert_eq!(reread.project.documents, loaded.project.documents);
    }

    #[test]
    fn a_schema_two_file_migrates_with_every_document_open() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let schema_two = json!({
            "schema": 2,
            "active": 1,
            "documents": [{ "path": "a.ydr" }, { "path": "b.ydr" }],
        });

        let loaded = decode(schema_two.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.schema, 2);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);

        let written = encode(&loaded.project, &file).unwrap();
        assert_eq!(written["schema"], json!(PROJECT_SCHEMA));
        assert_eq!(
            written["open"],
            json!([0, 1]),
            "schema 2 held only the open tabs, so every member was open"
        );
        assert_eq!(written["active"], json!(1));
    }

    #[test]
    fn an_open_index_follows_its_entry_past_one_that_was_skipped() {
        let text = json!({
            "schema": 3,
            "active": 2,
            "open": [2, 0, "x"],
            "documents": [7, { "path": "a.ydr" }, { "path": "b.ydr" }],
        });
        let loaded = decode(
            text.to_string().as_bytes(),
            Path::new("p.x"),
            Path::new("."),
        )
        .unwrap();
        assert_eq!(loaded.project.documents.len(), 2);
        assert_eq!(
            loaded.project.open,
            vec![1],
            "entry 0 was skipped, so it opens nothing"
        );
        assert_eq!(loaded.project.active, Some(1));
        assert_eq!(loaded.findings.len(), 2, "{:?}", loaded.findings);
    }

    #[test]
    fn one_unreadable_field_costs_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let text = json!({
            "schema": 2,
            "workspace": null,
            "active": "two",
            "documents": [
                { "path": "a.ydr", "kind": "Ydr" },
                { "path": 7 },
                12,
                { "path": "b.ydr", "kind": "NoSuchKind" },
            ],
        });

        let loaded = decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();

        let p = &loaded.project;
        assert_eq!(p.workspace, Workspace::default());
        assert_eq!(p.active, None);
        assert_eq!(p.documents.len(), 2, "{:?}", p.documents);
        assert_eq!(p.documents[0].path, dir.path().join("a.ydr"));
        assert_eq!(p.documents[0].kind, Some(DocumentKind::Ydr));
        assert_eq!(p.documents[1].path, dir.path().join("b.ydr"));
        assert_eq!(p.documents[1].kind, None);
        assert_eq!(
            loaded.findings.len(),
            5,
            "workspace, active, two unreadable entries and one kind: {:?}",
            loaded.findings
        );
    }

    #[test]
    fn a_workspace_an_older_build_named_reads_as_its_replacement() {
        for (named, now) in [
            ("Geometry", Workspace::Model),
            ("Explorer", Workspace::Texture),
            ("Text", Workspace::Data),
            ("Meta", Workspace::Data),
            ("Particle", Workspace::Particle),
        ] {
            let text = json!({ "schema": 3, "workspace": named, "documents": [] });
            let loaded = decode(
                text.to_string().as_bytes(),
                Path::new("p.openrage-proj"),
                Path::new("."),
            )
            .unwrap();
            assert!(loaded.findings.is_empty(), "{named}: {:?}", loaded.findings);
            assert_eq!(loaded.project.workspace, now, "{named}");
        }
    }

    #[test]
    fn keys_this_build_does_not_read_are_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let text = json!({
            "schema": PROJECT_SCHEMA + 1,
            "camera_per_document": true,
            "documents": [{ "path": "a.ydr", "camera": { "yaw": 0.5 } }],
        });

        let loaded = decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.schema, PROJECT_SCHEMA + 1);
        assert_eq!(loaded.findings.len(), 1, "{:?}", loaded.findings);
        assert!(loaded.findings[0].contains("newer"));

        let written = encode(&loaded.project, &file).unwrap();
        assert_eq!(written["camera_per_document"], json!(true));
        assert_eq!(written["documents"][0]["camera"], json!({ "yaw": 0.5 }));
        assert_eq!(written["documents"][0]["path"], json!("a.ydr"));
    }

    #[test]
    fn a_file_with_no_schema_is_read_as_schema_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("proj").join("p.openrage-proj");
        let text = json!({ "documents": [{ "path": "rel.ydr" }] });
        let loaded = decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.schema, 1);
        assert_eq!(loaded.project.documents[0].path, dir.path().join("rel.ydr"));
    }

    #[test]
    fn something_that_is_not_a_project_is_an_error_rather_than_a_panic() {
        let file = Path::new("p.openrage-proj");
        for text in ["", "not json", "[]", "null", "3"] {
            assert!(
                decode(text.as_bytes(), file, Path::new(".")).is_err(),
                "`{text}` is not a project"
            );
        }
    }

    #[test]
    fn an_active_index_past_the_documents_that_survived_is_dropped() {
        let text = json!({ "schema": 2, "active": 1, "documents": [{ "path": "a.ydr" }, 5] });
        let loaded = decode(
            text.to_string().as_bytes(),
            Path::new("p.x"),
            Path::new("."),
        )
        .unwrap();
        assert_eq!(loaded.project.documents.len(), 1);
        assert_eq!(loaded.project.active, None);
    }

    /// ⚠ The fixture is what the build before the container wrote for a project of today, so
    /// it must read as it always did and gain only the container's keys when it is saved.
    #[test]
    fn a_project_of_today_reads_unchanged_and_migrates_when_saved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("today.openrage-proj");
        let schema_three = json!({
            "schema": 3,
            "workspace": "Model",
            "active": 1,
            "open": [1, 0],
            "documents": [
                { "path": "models/a.ydr", "kind": "Ydr", "camera": { "yaw": 0.5 } },
                { "path": "textures/b.ytd", "kind": "Ytd" },
            ],
            "camera_per_document": true,
        });

        let loaded = decode(schema_three.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.schema, 3);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.project.documents.len(), 2);
        assert_eq!(loaded.project.open, vec![1, 0]);
        assert_eq!(loaded.project.active, Some(1));
        assert_eq!(loaded.project.container, Container::default());

        let written = encode(&loaded.project, &file).unwrap();
        assert_eq!(written["schema"], json!(PROJECT_SCHEMA));
        for (key, value) in schema_three.as_object().unwrap() {
            if key != "schema" {
                assert_eq!(&written[key], value, "`{key}` changed in the migration");
            }
        }
        assert_eq!(
            written["modules"],
            json!([{ "id": "files", "type": "files" }])
        );
        assert_eq!(written["outputs"], json!([]));
    }

    #[test]
    fn a_container_another_program_declared_survives_a_save_of_the_editor() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let declared = json!({
            "schema": PROJECT_SCHEMA,
            "modules": [
                { "id": "files", "type": "files" },
                { "id": "uniforms", "type": "widgets", "path": "modules/uniforms" },
            ],
            "outputs": [
                { "id": "fivem", "kind": "fivem-resource", "name": "pack", "take": ["files"],
                  "max_bytes": 1024, "split_at": 100 },
            ],
            "documents": [],
        });
        write(&file, declared.to_string().as_bytes());

        let loaded = load_project(&file).unwrap();
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.project.container.outputs[0].name, "pack");
        assert!(
            loaded.project.extra.is_empty(),
            "the container's keys are read as the container, not carried a second time"
        );

        save_project(&file, &loaded.project).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["modules"], declared["modules"]);
        assert_eq!(saved["outputs"], declared["outputs"]);
    }

    #[test]
    fn the_buttons_a_project_shows_survive_its_file_and_an_unknown_one_costs_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let project = ProjectFile {
            shown: Some(vec![
                Workspace::Project,
                Workspace::Texture,
                Workspace::Model,
            ]),
            ..ProjectFile::default()
        };

        save_project(&file, &project).unwrap();
        let loaded = load_project(&file).unwrap();
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.project.shown, project.shown);
        let stored: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            stored["shown_workspaces"],
            json!(["Project", "Texture", "Model"])
        );

        let text = json!({
            "schema": PROJECT_SCHEMA,
            "shown_workspaces": ["Texture", "Cinema", 7, "Project"],
            "documents": [],
        });
        let loaded = decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(
            loaded.project.shown,
            Some(vec![Workspace::Texture, Workspace::Project]),
            "the names this build knows are kept, in order"
        );
        assert_eq!(loaded.findings.len(), 2, "{:?}", loaded.findings);
        assert!(
            loaded.project.extra.is_empty(),
            "a key this build reads is not carried a second time"
        );

        let text = json!({ "schema": PROJECT_SCHEMA, "shown_workspaces": "all", "documents": [] });
        let loaded = decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.project.shown, None, "every button is shown");
        assert_eq!(loaded.findings.len(), 1, "{:?}", loaded.findings);

        let none = ProjectFile::default();
        let written = encode(&none, &file).unwrap();
        assert!(
            written.get("shown_workspaces").is_none(),
            "a project that pinned nothing writes no key"
        );
    }
}

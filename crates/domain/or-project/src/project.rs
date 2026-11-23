//! A project file as a program other than the editor reads it, and a new one as such a program writes it.
//!
//! The editor owns every other key (`workspace`, `active`, `open` and each document's own
//! keys), and reads and writes them itself; this reads the members and the container, and a
//! file it writes holds only what the editor reads back as an empty session.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::container::{unique_id, Container, Module, ModuleKind};
use crate::links::refuse_member;
use crate::paths::{project_folder, resolve_member, resolved, store_path};
use crate::plan::{Presence, Source};

/// The `kind` an older build wrote for the install, which is the editor's own mount and no member.
const INSTALL_KIND: &str = "GameRoot";

/// The schema this build writes: 1 stored every path as the process saw it; 2 stores a path
/// inside the project folder relative to it; 3 lists which documents are open apart from
/// which belong to the project; 4 adds the container's `modules` and `outputs`.
pub const SCHEMA: u32 = 4;

/// A file the project holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Where the file is on this machine: already resolved against the project folder.
    pub path: PathBuf,
    pub kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub path: PathBuf,
    /// The schema the file declared, before it was migrated.
    pub schema: u32,
    pub container: Container,
    pub members: Vec<Member>,
    /// One sentence per field that could not be read and what was used instead.
    pub findings: Vec<String>,
}

impl Project {
    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let cwd = std::env::current_dir().unwrap_or_default();
        Self::decode(&bytes, path, &cwd)
    }

    /// Read a project file's bytes; `cwd` is what a schema-1 relative path was relative to.
    pub fn decode(bytes: &[u8], path: &Path, cwd: &Path) -> io::Result<Self> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let value: Value = serde_json::from_slice(bytes).map_err(invalid)?;
        let Value::Object(top) = value else {
            return Err(invalid("a project file is a JSON object"));
        };
        let mut findings = Vec::new();

        let schema = match top.get("schema") {
            None => 1,
            Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
                Some(n) => n,
                None => {
                    findings.push(format!("`schema` is {v}, not a number; read as schema 1"));
                    1
                }
            },
        };
        if schema > SCHEMA {
            findings.push(format!(
                "written by a newer OpenRage (schema {schema}); what this version does not read is ignored"
            ));
        }

        let container = Container::read(&top, &mut findings);
        let folder = project_folder(path);
        let mut members = Vec::new();
        match top.get("documents") {
            None | Some(Value::Null) => {}
            Some(Value::Array(entries)) => {
                for (index, entry) in entries.iter().enumerate() {
                    match member(index, entry, schema, &folder, cwd) {
                        Ok(member) => members.push(member),
                        Err(finding) => findings.push(finding),
                    }
                }
            }
            Some(v) => findings.push(format!(
                "`documents` is {v}, not a list; the project holds none"
            )),
        }
        Ok(Self {
            path: path.to_path_buf(),
            schema,
            container,
            members,
            findings,
        })
    }

    /// Write a new project file at `path` holding `files`, refusing to replace one that exists.
    pub fn create(path: &Path, container: &Container, files: &[PathBuf]) -> io::Result<()> {
        Self::create_with(path, container, files, Map::new())
    }

    /// [`Self::create`] with more keys in the file: `extra` is written as it is, under the keys the file always has.
    pub fn create_with(
        path: &Path,
        container: &Container,
        files: &[PathBuf],
        extra: Map<String, Value>,
    ) -> io::Result<()> {
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            ));
        }
        let folder = project_folder(path);
        let documents = files
            .iter()
            .map(|file| json!({ "path": store_path(file, &folder) }))
            .collect();
        let mut top = extra;
        top.insert("schema".into(), SCHEMA.into());
        top.insert("open".into(), Value::Array(Vec::new()));
        top.insert("documents".into(), Value::Array(documents));
        container.write(&mut top);
        let bytes = serde_json::to_vec_pretty(&Value::Object(top)).map_err(invalid)?;
        replace_file(path, &bytes)
    }

    pub fn folder(&self) -> PathBuf {
        project_folder(&self.path)
    }

    /// Every file the project holds, as the plan needs to know of it: the install is no source.
    pub fn sources(&self) -> Vec<Source> {
        self.members
            .iter()
            .filter(|m| m.kind.as_deref() != Some(INSTALL_KIND))
            .map(|m| Source {
                path: m.path.clone(),
                presence: match std::fs::metadata(&m.path) {
                    Err(_) => Presence::Missing,
                    Ok(meta) if meta.is_dir() => Presence::Folder,
                    Ok(meta) => Presence::File { bytes: meta.len() },
                },
            })
            .collect()
    }
}

/// Read the project file at `path`, let `change` edit its container, and write it back with every other key as it was.
///
/// ⚠ A key this build does not know is carried, as the editor carries it, and the file is written
/// under this build's schema because the container's keys are what it declares. Nothing is written
/// when `change` refuses.
pub fn edit_container<T>(
    path: &Path,
    change: impl FnOnce(&mut Container) -> Result<T, String>,
) -> Result<T, String> {
    edit_file(path, |_, container| change(container))
}

/// What adding files to a project did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Added {
    pub added: usize,
    /// Files the project held already.
    pub held: usize,
    /// One sentence for each file a build could not place, with what to do about it.
    pub refused: Vec<String>,
}

/// Add `files` to the project at `path` as documents of its Files content, which every build takes when it is made here.
///
/// ⚠ A file outside the project's folder has no place in a build, and one inside linked content is
/// already read through the link, so neither is added: each is refused with the reason. Nothing is
/// written when no file was added.
pub fn add_files(path: &Path, files: &[PathBuf]) -> Result<Added, String> {
    let loaded = Project::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut done = Added::default();
    let mut fresh: Vec<&PathBuf> = Vec::new();
    for file in files {
        let wanted = resolved(file);
        if fresh.iter().any(|f| resolved(f) == wanted)
            || loaded.members.iter().any(|m| resolved(&m.path) == wanted)
        {
            done.held += 1;
        } else if let Some(why) = refuse_member(&loaded.container, path, file) {
            done.refused.push(why);
        } else {
            fresh.push(file);
        }
    }
    done.added = fresh.len();
    if fresh.is_empty() {
        return Ok(done);
    }
    let folder = project_folder(path);
    edit_file(path, |top, container| {
        let mut documents = match top.remove("documents") {
            Some(Value::Array(documents)) => documents,
            _ => Vec::new(),
        };
        documents.extend(
            fresh
                .iter()
                .map(|file| json!({ "path": store_path(file, &folder) })),
        );
        top.insert("documents".into(), Value::Array(documents));
        if !container
            .modules
            .iter()
            .any(|m| m.kind == ModuleKind::Files && !m.is_link())
        {
            let id = unique_id("files", container.modules.iter().map(|m| m.id.as_str()));
            container.modules.push(Module::files(&id));
            for output in &mut container.outputs {
                output.take.push(id.clone());
            }
        }
        Ok(())
    })?;
    Ok(done)
}

/// [`edit_container`], with the file's other top-level keys in reach of `change` too.
pub fn edit_file<T>(
    path: &Path,
    change: impl FnOnce(&mut Map<String, Value>, &mut Container) -> Result<T, String>,
) -> Result<T, String> {
    let shown = path.display();
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let Ok(Value::Object(mut top)) = serde_json::from_slice::<Value>(bytes) else {
        return Err(format!("{shown} is not a project file"));
    };
    let mut container = Container::read(&top, &mut Vec::new());
    let done = change(&mut top, &mut container)?;
    container.write(&mut top);
    top.insert("schema".into(), SCHEMA.into());
    let bytes = serde_json::to_vec_pretty(&Value::Object(top)).map_err(|e| e.to_string())?;
    replace_file(path, &bytes).map_err(|e| format!("{shown}: {e}"))?;
    Ok(done)
}

fn member(
    index: usize,
    entry: &Value,
    schema: u32,
    folder: &Path,
    cwd: &Path,
) -> Result<Member, String> {
    let Value::Object(fields) = entry else {
        return Err(format!(
            "document {index} is {entry}, not an entry; skipped"
        ));
    };
    let Some(stored) = fields.get("path").and_then(Value::as_str) else {
        return Err(format!("document {index} has no readable `path`; skipped"));
    };
    Ok(Member {
        path: resolve_member(stored, schema, folder, cwd),
        kind: fields
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn invalid(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

/// Write `bytes` to `path` through a temporary file beside it, so a failed write leaves the old file.
pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{ModuleKind, Output};

    fn write(path: &Path, body: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn a_created_project_loads_with_its_members_and_its_container() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("pack");
        let outside = dir.path().join("elsewhere").join("b.ytd");
        write(&folder.join("stream/a.ydr"), b"a");
        write(&outside, b"b");
        let mut container = Container::default();
        container
            .outputs
            .push(Output::fivem_resource("fivem", "pack", &["files"]));
        let file = folder.join("pack.openrage-proj");

        Project::create(
            &file,
            &container,
            &[folder.join("stream/a.ydr"), outside.clone()],
        )
        .unwrap();
        let loaded = Project::load(&file).unwrap();

        assert_eq!(loaded.schema, SCHEMA);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.container, container);
        let paths: Vec<&Path> = loaded.members.iter().map(|m| m.path.as_path()).collect();
        assert_eq!(
            paths,
            [folder.join("stream/a.ydr").as_path(), outside.as_path()]
        );
        let stored: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(stored["documents"][0]["path"], json!("stream/a.ydr"));
        assert_eq!(
            stored["documents"][1]["path"],
            json!(outside.to_string_lossy()),
            "inside the folder is relative with `/` on every host; outside stays absolute"
        );
        assert_eq!(stored["schema"], json!(SCHEMA));
    }

    #[test]
    fn editing_the_container_of_a_file_keeps_every_other_key_and_a_refusal_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let text = json!({
            "schema": 3, "workspace": "Model", "camera_per_document": true,
            "documents": [{ "path": "a.ydr", "camera": { "yaw": 0.5 } }],
        });
        write(&file, text.to_string().as_bytes());

        let refused: Result<(), String> =
            edit_container(&file, |_| Err("no, said the change".to_string()));
        assert_eq!(refused.unwrap_err(), "no, said the change");
        assert_eq!(std::fs::read(&file).unwrap(), text.to_string().as_bytes());

        edit_container(&file, |container| {
            container
                .outputs
                .push(Output::fivem_resource("fivem", "pack", &["files"]));
            Ok(())
        })
        .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["schema"], json!(SCHEMA));
        assert_eq!(saved["camera_per_document"], json!(true));
        assert_eq!(saved["documents"], text["documents"]);
        assert_eq!(saved["workspace"], json!("Model"));
        assert_eq!(saved["outputs"][0]["name"], json!("pack"));
        assert_eq!(
            saved["modules"],
            json!([{ "id": "files", "type": "files" }])
        );

        write(&file, b"not json");
        assert!(edit_container(&file, |_| Ok(())).is_err());
    }

    #[test]
    fn a_project_that_exists_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        write(&file, b"{}");
        let refused = Project::create(&file, &Container::default(), &[]).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&file).unwrap(), b"{}");
    }

    #[test]
    fn a_schema_three_file_reads_unchanged_with_its_documents_as_one_files_module() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.openrage-proj");
        let schema_three = json!({
            "schema": 3,
            "workspace": "Model",
            "active": 0,
            "open": [0],
            "documents": [{ "path": "models/a.ydr", "kind": "Ydr" }, { "path": "b.ytd" }],
        });

        let loaded =
            Project::decode(schema_three.to_string().as_bytes(), &file, dir.path()).unwrap();

        assert_eq!(loaded.schema, 3);
        assert!(loaded.findings.is_empty(), "{:?}", loaded.findings);
        assert_eq!(loaded.container.modules.len(), 1);
        assert_eq!(loaded.container.modules[0].kind, ModuleKind::Files);
        assert!(loaded.container.outputs.is_empty());
        assert_eq!(
            loaded.members,
            [
                Member {
                    path: dir.path().join("models").join("a.ydr"),
                    kind: Some("Ydr".into())
                },
                Member {
                    path: dir.path().join("b.ytd"),
                    kind: None
                },
            ]
        );
    }

    #[test]
    fn a_schema_one_path_is_read_from_where_the_editor_ran() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("where-the-editor-ran");
        let file = dir.path().join("proj").join("old.openrage-proj");
        let text = json!({ "schema": 1, "documents": [{ "path": "loose/b.ydr", "kind": null }] });
        let loaded = Project::decode(text.to_string().as_bytes(), &file, &cwd).unwrap();
        assert_eq!(loaded.members[0].path, cwd.join("loose").join("b.ydr"));
    }

    #[test]
    fn a_newer_file_opens_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let newer = json!({
            "schema": SCHEMA + 1,
            "camera_per_document": true,
            "modules": [{ "id": "uniforms", "type": "widgets" }, { "id": "files", "type": "files" }],
            "outputs": [{ "id": "dlc", "kind": "singleplayer-dlc", "name": "x", "take": [] }],
            "documents": [{ "path": "a.ydr", "camera": { "yaw": 0.5 } }],
        });

        let loaded = Project::decode(newer.to_string().as_bytes(), &file, dir.path()).unwrap();

        assert_eq!(loaded.schema, SCHEMA + 1);
        assert_eq!(loaded.findings.len(), 1, "{:?}", loaded.findings);
        assert!(
            loaded.findings[0].contains("newer"),
            "{:?}",
            loaded.findings
        );
        assert_eq!(loaded.members.len(), 1);
        assert_eq!(loaded.container.modules.len(), 2);
        assert_eq!(loaded.container.outputs.len(), 1);
    }

    #[test]
    fn something_that_is_not_a_project_is_an_error_rather_than_a_panic() {
        let file = Path::new("p.openrage-proj");
        for text in ["", "not json", "[]", "null", "3"] {
            assert!(
                Project::decode(text.as_bytes(), file, Path::new(".")).is_err(),
                "`{text}` is not a project"
            );
        }
    }

    #[test]
    fn one_unreadable_member_costs_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        let text = json!({
            "schema": 3,
            "documents": [{ "path": "a.ydr" }, { "path": 7 }, 12, { "path": "b.ydr" }],
        });
        let loaded = Project::decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        assert_eq!(loaded.members.len(), 2);
        assert_eq!(loaded.findings.len(), 2, "{:?}", loaded.findings);
    }

    #[test]
    fn the_install_is_no_source_and_a_source_knows_what_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.openrage-proj");
        write(&dir.path().join("a.ydr"), b"four");
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        let text = json!({
            "schema": 3,
            "documents": [
                { "path": "a.ydr" }, { "path": "gone.ydr" }, { "path": "folder" },
                { "path": "install", "kind": "GameRoot" },
            ],
        });
        let loaded = Project::decode(text.to_string().as_bytes(), &file, dir.path()).unwrap();
        let presence: Vec<Presence> = loaded.sources().into_iter().map(|s| s.presence).collect();
        assert_eq!(
            presence,
            [
                Presence::File { bytes: 4 },
                Presence::Missing,
                Presence::Folder
            ]
        );
    }
}

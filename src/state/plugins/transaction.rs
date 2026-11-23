//! Files a plugin writes together, in the project: the check of the set, its application as one step of History, and Undo and Redo over it.
//!
//! A plugin reaches here two ways, `write_files` in a run and a suggested fix a person accepted. Both are
//! a [`Transaction`], checked as a set before any file is written, applied all together or not at all,
//! and recorded as one step of the project's History that keeps each file as it was and as it became, so
//! Undo and Redo move all of them together (`a_plugin_writes_files_together`).
//!
//! ⚠ A project holds sources: a plugin may not write a game file into it, nor the project's own files
//! (`.openrage`, `build`, `.git`, the project file), and a `wardrobe.json` or another `.json` must read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use or_plugin::{FileWrite, SaveCheck, SuggestedFix, Transaction};
use or_project::{project_folder, replace_file, BUILD_DIR, PROJECT_EXTENSION};
use serde_json::{json, Value};

use crate::state::AppState;

/// The key of the attachment that marks a step as a plugin's files.
const KEY: &str = "plugin_files";

/// How a plugin's set is checked against the project it would be written into.
#[derive(Debug, Clone)]
pub(in crate::state) struct ProjectFiles {
    folder: PathBuf,
}

impl ProjectFiles {
    /// The check for the open project, none while no project is open.
    pub(in crate::state) fn of(state: &AppState) -> Option<Self> {
        let project = state.project_path.as_deref()?;
        Some(Self {
            folder: project_folder(project),
        })
    }

    /// The first thing wrong with writing `files`, none when the set can be written.
    pub(in crate::state) fn problem(&self, files: &[(&str, &[u8])]) -> Option<String> {
        files
            .iter()
            .find_map(|(path, bytes)| self.problem_with(path, bytes))
    }

    /// Why `path` is not a place a plugin writes, none when it is: inside the project, and neither a place the project keeps for itself, a project file nor a game file.
    ///
    /// ⚠ The check of the paths a step kept for Undo and Redo as well, since those are joined onto the project's folder.
    pub(in crate::state) fn place_problem(path: &str) -> Option<String> {
        let one = Transaction {
            files: vec![FileWrite {
                path: path.to_string(),
                bytes: Vec::new(),
            }],
        };
        if let Some(why) = one.problem() {
            return Some(why);
        }
        let extension = extension_of(path);
        if path.split('/').any(is_kept_for_itself) {
            return Some(format!(
                "`{path}` is in a folder the project keeps for itself, and a plugin does not write there"
            ));
        }
        if extension == PROJECT_EXTENSION {
            return Some(format!(
                "`{path}` is a project file, and a plugin does not write one"
            ));
        }
        // ⚠ A game file is not what a project holds: the source it comes from is.
        if or_io::gta5::formats::format(&extension).is_some() || extension == "rpf" {
            return Some(format!(
                "`{path}` is a game file, and a project holds sources: a plugin writes the source folder it comes from"
            ));
        }
        None
    }

    fn problem_with(&self, path: &str, bytes: &[u8]) -> Option<String> {
        if let Some(why) = Self::place_problem(path) {
            return Some(why);
        }
        let extension = extension_of(path);
        let name = path.rsplit('/').next().unwrap_or(path);
        if name == or_wardrobe::model::FILE {
            if let Err(e) = or_wardrobe::Wardrobe::from_json(bytes, path) {
                return Some(format!("`{path}` would not read as a pack: {e}"));
            }
        } else if extension == "json" && serde_json::from_slice::<Value>(bytes).is_err() {
            return Some(format!("`{path}` would not read as JSON"));
        }
        let target = self.folder.join(path);
        if target.is_dir() {
            return Some(format!("`{path}` is a folder, and a plugin writes files"));
        }
        let mut above = target.parent();
        while let Some(parent) = above.filter(|p| p.starts_with(&self.folder) && *p != self.folder)
        {
            if parent.is_file() {
                return Some(format!(
                    "`{path}` would be inside `{}`, which is a file",
                    parent.display()
                ));
            }
            above = parent.parent();
        }
        None
    }
}

fn extension_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// The folders whose names the project keeps for itself, in lower case, wherever they stand in a path.
const KEPT: [&str; 2] = [".openrage", ".git"];

/// Whether `part` names a folder the project keeps for itself, however the file system spells it.
///
/// ⚠ NTFS and a default Mac fold case, so `.GIT` is `.git`; NTFS also answers to `GIT~1` and `OPENRA~1` for
/// the two dotted names. A nested `.git` is a repository of its own, and `build` is never read back as content.
fn is_kept_for_itself(part: &str) -> bool {
    let part = part.to_lowercase();
    let short_name_of_a_kept_one = part.split_once('~').is_some_and(|(stem, n)| {
        !stem.is_empty()
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
            && KEPT.iter().any(|kept| kept[1..].starts_with(stem))
    });
    part == BUILD_DIR || KEPT.contains(&part.as_str()) || short_name_of_a_kept_one
}

/// What a plugin's call is checked against: the open document for the file it replaces, and the project for the files it writes together.
pub(in crate::state) struct PluginCheck {
    pub document: Option<Box<dyn SaveCheck>>,
    pub files: Option<ProjectFiles>,
}

const NO_DOCUMENT_CHECK: &str = "this format has no save check, so a plugin may not change it";

impl SaveCheck for PluginCheck {
    fn why_the_document_would_be_invalid(&self, document: &[u8]) -> Option<String> {
        match &self.document {
            Some(check) => check.why_the_document_would_be_invalid(document),
            None => Some(NO_DOCUMENT_CHECK.to_string()),
        }
    }

    fn why_the_file_would_be_invalid(&self, file: &[u8]) -> Option<String> {
        match &self.document {
            Some(check) => check.why_the_file_would_be_invalid(file),
            None => Some(NO_DOCUMENT_CHECK.to_string()),
        }
    }

    fn why_the_files_would_be_invalid(&self, files: &[(&str, &[u8])]) -> Option<String> {
        match &self.files {
            Some(check) => check.problem(files),
            None => Some("no project is open, so there is nowhere to write files".to_string()),
        }
    }
}

/// What writing a plugin's files together came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    pub files: usize,
    /// Whether a step of History holds them, so that Undo takes them back; none when the project keeps no history.
    pub undoable: bool,
}

impl Applied {
    /// The sentence for a person: how many files were written together, and whether Undo takes them back.
    pub fn said(&self) -> String {
        let noun = if self.files == 1 { "file" } else { "files" };
        let undo = if self.undoable {
            "Undo in the project takes them back as one step"
        } else {
            "No history is kept for this project, so nothing takes them back"
        };
        format!("{} {noun} written together. {undo}", self.files)
    }
}

/// A file a plugin wrote in one step, as the step keeps it.
#[derive(Debug, Clone)]
pub(in crate::state) struct Written {
    path: PathBuf,
    shown: String,
    had_before: bool,
    before: PathBuf,
    after: PathBuf,
}

/// Where a file stands now: its bytes, none when it is not there.
fn on_disk(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// Put `bytes` at `path`, or take the file away when there are none.
fn put(path: &Path, bytes: Option<&[u8]>) -> std::io::Result<()> {
    match bytes {
        Some(bytes) => replace_file(path, bytes),
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

impl AppState {
    /// Write the files of `set` into the open project together and keep them as one step of History called `label`; how many.
    ///
    /// ⚠ All or none: the set is checked as a whole first, and a write that fails puts back every file written before it
    /// and the folders made for them, and records no step.
    pub fn apply_plugin_files(
        &mut self,
        label: &str,
        set: &Transaction,
    ) -> Result<Applied, String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        let check = ProjectFiles::of(self)
            .ok_or("No project is open, so there is nowhere to write the files")?;
        let files: Vec<(&str, &[u8])> = set
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.bytes.as_slice()))
            .collect();
        if let Some(why) = set.problem().or_else(|| check.problem(&files)) {
            return Err(format!("No file was written: {why}"));
        }
        let targets: Vec<PathBuf> = set
            .files
            .iter()
            .map(|f| check.folder.join(&f.path))
            .collect();
        let before: Vec<Option<Vec<u8>>> = targets.iter().map(|t| on_disk(t)).collect();
        let made = folders_to_make(&check.folder, &targets);

        for (at, file) in set.files.iter().enumerate() {
            if let Err(e) = replace_file(&targets[at], &file.bytes) {
                for undone in (0..=at).rev() {
                    let _ = put(&targets[undone], before[undone].as_deref());
                    let _ = std::fs::remove_file(with_tmp(&targets[undone]));
                }
                for folder in made.iter().rev() {
                    let _ = std::fs::remove_dir(folder);
                }
                return Err(format!(
                    "No file was written: `{}` could not be written ({e})",
                    file.path
                ));
            }
        }

        // ⚠ A project whose history cannot be kept has written files nothing takes back, and says so.
        let step = self.record_step(label);
        if let Some(n) = step {
            self.keep_what_a_step_wrote(n, set, &before);
        }
        self.workbench.want_scan();
        Ok(Applied {
            files: set.files.len(),
            undoable: step.is_some(),
        })
    }

    /// The files of a suggested fix, each with whether it replaces a file the project has now.
    pub fn plugin_fix_files(&self, fix: &SuggestedFix) -> Vec<(String, bool)> {
        let folder = self.project_path.as_deref().map(project_folder);
        (fix.files.files.iter())
            .map(|file| {
                let replaces = folder
                    .as_ref()
                    .is_some_and(|folder| folder.join(&file.path).is_file());
                (file.path.clone(), replaces)
            })
            .collect()
    }

    /// Write the files of a fix a person accepted together, as one step of History, and say what came of it.
    pub fn apply_plugin_fix(&mut self, plugin: &str, fix: &SuggestedFix) {
        match self.apply_plugin_files(&format!("{plugin}: {}", fix.label), &fix.files) {
            Ok(applied) => self.show_info_toast(format!("{}: {}", fix.label, applied.said())),
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Keep with step `n` each file of `set` as it was and as it became, so Undo and Redo can move them together.
    ///
    /// ⚠ A copy that cannot be kept only costs the Undo, which then says so: the files are written already.
    fn keep_what_a_step_wrote(&mut self, n: u64, set: &Transaction, before: &[Option<Vec<u8>>]) {
        let Some(aside) = self.workbench.step_aside(n) else {
            return;
        };
        let kept = (|| -> std::io::Result<()> {
            let _ = std::fs::remove_dir_all(&aside);
            std::fs::create_dir_all(&aside)?;
            for (k, file) in set.files.iter().enumerate() {
                std::fs::write(aside.join(format!("{k}.after")), &file.bytes)?;
                if let Some(bytes) = &before[k] {
                    std::fs::write(aside.join(format!("{k}.before")), bytes)?;
                }
            }
            let listed: Vec<Value> = set
                .files
                .iter()
                .enumerate()
                .map(|(k, file)| json!({ "path": file.path, "before": before[k].is_some() }))
                .collect();
            self.workbench
                .attach_to_step(n, &serde_json::to_vec(&json!({ KEY: listed }))?)
        })();
        if let Err(why) = kept {
            tracing::warn!(%why, "what a plugin wrote was not kept with its step");
        }
    }

    /// The files step `n` is the writing of, none when it is no plugin's.
    ///
    /// ⚠ The record is a file in the project's private folder, and its paths are joined onto the project's folder, so each is
    /// checked as a plugin's path is: a record that names a place a plugin may not write moves nothing.
    pub(in crate::state) fn plugin_files_of(&self, n: u64) -> Result<Vec<Written>, String> {
        let (Some(project), Some(aside), Some(bytes)) = (
            self.project_path.as_deref(),
            self.workbench.step_aside(n),
            self.workbench.step_attachment(n),
        ) else {
            return Ok(Vec::new());
        };
        let folder = project_folder(project);
        let Ok(Value::Object(attached)) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(Vec::new());
        };
        let Some(Value::Array(listed)) = attached.get(KEY) else {
            return Ok(Vec::new());
        };
        let mut files = Vec::new();
        for (k, entry) in listed.iter().enumerate() {
            let (Some(shown), Some(had_before)) = (
                entry.get("path").and_then(Value::as_str),
                entry.get("before").and_then(Value::as_bool),
            ) else {
                continue;
            };
            if let Some(why) = ProjectFiles::place_problem(shown) {
                return Err(format!(
                    "Not moved, and nothing was changed: the step names a file that is not a place a plugin writes. {why}"
                ));
            }
            files.push(Written {
                path: folder.join(shown),
                shown: shown.to_string(),
                had_before,
                before: aside.join(format!("{k}.before")),
                after: aside.join(format!("{k}.after")),
            });
        }
        Ok(files)
    }
}

/// The folders on the way to `targets` that do not exist yet, the outer ones first.
fn folders_to_make(root: &Path, targets: &[PathBuf]) -> Vec<PathBuf> {
    let mut made: Vec<PathBuf> = Vec::new();
    for target in targets {
        let mut missing = Vec::new();
        let mut above = target.parent();
        while let Some(parent) = above.filter(|p| p.starts_with(root) && *p != root) {
            if parent.exists() {
                break;
            }
            missing.push(parent.to_path_buf());
            above = parent.parent();
        }
        for folder in missing.into_iter().rev() {
            if !made.contains(&folder) {
                made.push(folder);
            }
        }
    }
    made
}

fn with_tmp(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Why going over `steps` would not leave the files as they were written, none when it would.
///
/// ⚠ Read in the order the move takes them, with what each step leaves in place of what the disk holds,
/// so two steps that wrote one file are checked one after the other and nothing has moved yet.
pub(in crate::state) fn plugin_files_conflict(
    steps: &[(u64, Vec<Written>)],
    going_back: bool,
) -> Option<String> {
    let mut standing: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
    for written in in_order(steps, going_back) {
        let now = standing
            .get(&written.path)
            .cloned()
            .unwrap_or_else(|| on_disk(&written.path));
        let (expected, becomes) = if going_back {
            (kept_bytes(&written.after), before_of(written))
        } else {
            (before_of(written), kept_bytes(&written.after))
        };
        let Some(expected) = expected else {
            return Some(format!(
                "Not moved, and nothing was changed: the copy of `{}` that the step kept is gone",
                written.shown
            ));
        };
        if now != expected {
            return Some(format!(
                "Not moved, and nothing was changed: `{}` is not as the plugin's step left it. Put it back by hand, or keep the change",
                written.shown
            ));
        }
        standing.insert(written.path.clone(), becomes.flatten());
    }
    None
}

/// The files of `steps` in the order a move takes them: the newest step's last file first going back, the oldest step's first file first going on.
fn in_order(steps: &[(u64, Vec<Written>)], going_back: bool) -> Vec<&Written> {
    let all = steps.iter().flat_map(|(_, files)| files.iter());
    if going_back {
        all.rev().collect()
    } else {
        all.collect()
    }
}

/// A copy a step kept: its bytes, the outer `None` when the copy is gone.
fn kept_bytes(copy: &Path) -> Option<Option<Vec<u8>>> {
    copy.is_file().then(|| std::fs::read(copy).ok())
}

/// What the file was before its step: the bytes, `Some(None)` when it was not there, and `None` when the copy is gone.
fn before_of(written: &Written) -> Option<Option<Vec<u8>>> {
    if written.had_before {
        kept_bytes(&written.before)
    } else {
        Some(None)
    }
}

/// Put the files of `steps` as they were before their steps when `going_back`, and as the steps wrote them otherwise.
pub(in crate::state) fn move_plugin_files(
    steps: &[(u64, Vec<Written>)],
    going_back: bool,
) -> Result<(), String> {
    for written in in_order(steps, going_back) {
        let bytes = if going_back {
            before_of(written)
        } else {
            kept_bytes(&written.after)
        };
        let Some(bytes) = bytes else {
            return Err(format!(
                "`{}` could not be moved: the copy the step kept is gone",
                written.shown
            ));
        };
        put(&written.path, bytes.as_deref())
            .map_err(|e| format!("`{}` could not be moved: {e}", written.shown))?;
    }
    Ok(())
}

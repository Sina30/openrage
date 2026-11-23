//! The doors into a project that are not an import's own: Add, which decompiles what it is given, and New, which starts an empty source.
//!
//! A game file that enters a project by any door becomes a source at once: Add, a drop, a pick, the Wardrobe and Replace all
//! end in `or_source::game_import`. A source is created, or imported; a project holds no compiled game file of its own.

use std::path::{Path, PathBuf};

use or_source::create::{self, KINDS};
use or_source::game_import::{kind_of, Created, FileReport, Report, Verdict};
use or_source::git;

use super::commands::Added;
use super::read_only::READ_ONLY;
use super::Question;
use crate::state::workbench::NEEDS_A_PROJECT;
use crate::state::AppState;
use crate::ui::dock::Workspace;

/// Whether the import turns `path` into a source: a game file with a source form, a source folder, an `asset.json`, or a folder, which is scanned.
fn is_imported(path: &Path) -> bool {
    path.is_dir()
        || path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(or_source::asset::MANIFEST))
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| kind_of(e).is_some())
}

impl AppState {
    /// Add `files` to the project: a game file is decompiled at once, a source or an `asset.json` is taken as it is, a folder is scanned, and every other file joins the Files content.
    ///
    /// The imports run as the job they are and report as they do; what joined the Files content is counted in the return.
    pub fn add_to_project(&mut self, files: Vec<PathBuf>) -> Added {
        let (imported, plain): (Vec<PathBuf>, Vec<PathBuf>) =
            files.into_iter().partition(|file| is_imported(file));
        if !imported.is_empty() {
            self.import_game_files(imported);
        }
        match plain.is_empty() {
            true => Added::default(),
            false => self.add_files_to_project(plain),
        }
    }

    /// Why a new source cannot be started now, none when it can.
    pub fn new_source_refused(&self) -> Option<&'static str> {
        match self.project_path {
            None => Some(crate::i18n::tr(NEEDS_A_PROJECT)),
            Some(_) => self
                .workbench
                .held_by_another
                .is_some()
                .then_some(crate::i18n::tr(READ_ONLY)),
        }
    }

    /// Ask what to call the new source of `kind`, which is then started empty under `sources/`.
    pub fn ask_for_a_new_source(&mut self, kind: &str) {
        if let Some(why) = self.new_source_refused() {
            return self.show_info_toast(why);
        }
        let Some(row) = KINDS.iter().find(|k| k.kind == kind) else {
            return self.show_info_toast(tr!(
                "A {kind} source cannot be started empty: {why_not_the_rest}",
                kind = kind,
                why_not_the_rest = create::WHY_NOT_THE_REST
            ));
        };
        self.workbench.asking = Some(Question::NewSource {
            kind: row.kind.to_string(),
            label: row.label.to_string(),
            name: row.kind.to_string(),
            error: None,
            focus: true,
        });
    }

    /// Close the question without starting anything.
    pub fn dismiss_new_source(&mut self) {
        if matches!(self.workbench.asking, Some(Question::NewSource { .. })) {
            self.workbench.asking = None;
        }
    }

    /// Start an empty source of `kind` called `name` under `sources/`; the name is said again, with a reason, when it cannot be one.
    ///
    /// The source is selected in the Assets table, where it waits to be filled, and Undo import takes it back.
    pub fn create_source(&mut self, kind: &str, name: &str) -> Result<PathBuf, String> {
        if let Some(why) = self.new_source_refused() {
            return Err(why.to_string());
        }
        let project = self
            .project_path
            .clone()
            .ok_or(crate::i18n::tr(NEEDS_A_PROJECT))?;
        let root = or_project::project_folder(&project);
        let made = create::new_source(&root, kind, name.trim())?;
        let label = KINDS
            .iter()
            .find(|k| k.kind == kind)
            .map_or("source", |k| k.label);
        let folder = made.path.clone();
        let mut report = Report::default();
        report.files.push(FileReport {
            input: tr!("New {kind}", kind = label.to_lowercase()),
            verdict: Verdict::Made {
                folder: folder.clone(),
            },
            reason: tr!("started empty as {folder}", folder = folder_name(&folder)),
        });
        report.created.push(Created {
            path: made.path,
            digest: made.digest,
        });
        let step = tr!(
            "Created the {kind} {folder}",
            kind = label.to_lowercase(),
            folder = folder_name(&folder)
        );
        self.finish_import_as(report, Some(step));
        // A new source is no import: its report has nothing to tell.
        self.workbench.imports.shown = false;
        self.select_asset(Some(folder.display().to_string()));
        self.active_workspace = Workspace::Project;
        let _ = git::update(&root);
        Ok(folder)
    }
}

/// The name of the folder `path`.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

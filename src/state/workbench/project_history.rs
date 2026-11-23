//! A project's undoable changes: each is a step kept beside the project file, and Undo, Redo and a rollback are moves along them.
//!
//! A step is the project as its file would state it, less what a session does to it (the tabs that are
//! open and the one in front): the files it holds, its content and builds, the buttons pinned and the
//! workspace the file opens on. Going to a step reads that copy back, so no step depends on another
//! and the list outlives the session (`.openrage/history`, ignored by Git).
//!
//! ⚠ **History is best effort.** If the steps cannot be written, the project goes on with no Undo and
//! says so once; a change is never held back for it.

use std::path::Path;
use std::time::SystemTime;

use or_project::snapshot::{history_dir, Snapshots};
use or_project::{project_folder, store_path};
use or_source::game_import::Created;
use or_source::git;
use serde_json::{json, Value};

use super::{leaf, Event, Workbench, HISTORY_KEPT};
use crate::state::plugins::transaction::{move_plugin_files, plugin_files_conflict};
use crate::state::project::{decode, encode, ProjectFile};
use crate::state::AppState;

/// The first step of a project that has none yet: the project as it was when it was opened.
const OPENED: &str = tr_key!("The project as it was opened");
const OUTSIDE: &str = tr_key!("The project file changed outside OpenRage");
const NOTHING_TO_UNDO: &str = tr_key!("Nothing to undo in the project yet");
const NOTHING_TO_REDO: &str = tr_key!("Nothing to redo in the project");
const NOT_KEPT: &str = tr_key!("No history is kept for this project");
const NOT_MADE_AGAIN: &str = tr_key!(
    "The sources an import made are not made again: import the files again with Import game files…"
);

/// Where the file on disk stands among the steps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Saved {
    /// As opened, before any step was kept: the first step will be it.
    #[default]
    Opened,
    Step(u64),
    /// At a step that was dropped, so no step is the file.
    Nowhere,
}

/// The steps of the open project, and what a move along them needs.
#[derive(Default)]
pub(super) struct Steps {
    begun: bool,
    store: Option<Snapshots>,
    /// The project as it stands at the step the history is at, in the bytes a step holds.
    standing: Vec<u8>,
    saved: Saved,
    opened: Option<SystemTime>,
    /// The steps could not be written, and the person was told.
    failed: bool,
    /// A move along the steps is under way, so the changes it makes are no steps of their own.
    moving: bool,
}

impl Workbench {
    /// The number of the step the project stands at; none while no step is kept.
    pub fn step_here(&self) -> Option<u64> {
        self.steps.store.as_ref()?.current().map(|step| step.n)
    }

    /// When the step the project stands at was made.
    pub fn step_here_at(&self) -> Option<SystemTime> {
        self.steps.store.as_ref()?.current().map(|step| step.at())
    }

    /// The number of the step the project file on disk holds.
    pub fn step_saved(&self) -> Option<u64> {
        match self.steps.saved {
            Saved::Step(n) => Some(n),
            Saved::Opened | Saved::Nowhere => None,
        }
    }

    /// The labels of the project's steps made since the project file was written, oldest first: what the next save writes and Don't save drops.
    pub fn unsaved_steps(&self) -> Vec<&str> {
        let here = self.step_here().unwrap_or(0);
        let saved = self.step_saved();
        (self.history.iter())
            .filter_map(|event| Some((event.step?, event.text.as_str())))
            .filter(|(n, text)| {
                *n <= here
                    && saved.is_none_or(|saved| *n > saved)
                    && *text != OPENED
                    && *text != crate::i18n::tr(OPENED)
            })
            .map(|(_, text)| text)
            .collect()
    }

    /// The folder that keeps the copies a step made, which goes with the step.
    pub(in crate::state) fn step_aside(&self, n: u64) -> Option<std::path::PathBuf> {
        Some(self.steps.store.as_ref()?.aside(n))
    }

    /// What was attached to the step `n`.
    pub(in crate::state) fn step_attachment(&self, n: u64) -> Option<Vec<u8>> {
        self.steps.store.as_ref()?.attachment(n)
    }

    /// Attach `bytes` to the step `n`, which keeps them until it is dropped.
    pub(in crate::state) fn attach_to_step(&self, n: u64, bytes: &[u8]) -> std::io::Result<()> {
        match self.steps.store.as_ref() {
            Some(store) => store.attach(n, bytes),
            None => Err(std::io::Error::other(crate::i18n::tr(NOT_KEPT))),
        }
    }

    /// What the step `n` is called.
    #[cfg(test)]
    pub fn label_of(&self, n: u64) -> Option<String> {
        let store = self.steps.store.as_ref()?;
        (store.steps().iter())
            .find(|step| step.n == n)
            .map(|step| step.label.clone())
    }

    /// Keep `after` as a new step called `label`, and a line of the history for it; what was undone is dropped.
    fn keep_step(&mut self, label: &str, after: Vec<u8>) -> Result<u64, String> {
        let Self { steps, history, .. } = self;
        let Some(store) = steps.store.as_mut() else {
            return Err(crate::i18n::tr(NOT_KEPT).to_string());
        };
        let failed = |steps: &mut Steps, why: std::io::Error| {
            steps.store = None;
            format!("{}: {why}", crate::i18n::tr(NOT_KEPT))
        };
        if store.is_empty() {
            match store.push(crate::i18n::tr(OPENED), &steps.standing) {
                Ok(n) => {
                    if steps.saved == Saved::Opened {
                        steps.saved = Saved::Step(n);
                    }
                    history.insert(
                        0,
                        Event {
                            at: steps.opened.unwrap_or_else(SystemTime::now),
                            text: crate::i18n::tr(OPENED).to_string(),
                            step: Some(n),
                        },
                    );
                }
                Err(why) => return Err(failed(steps, why)),
            }
        }
        let store = steps.store.as_mut().expect("kept above");
        let here = store.current().map(|step| step.n);
        history.retain(|event| event.step.is_none_or(|n| Some(n) <= here));
        if matches!(steps.saved, Saved::Step(n) if Some(n) > here) {
            steps.saved = Saved::Nowhere;
        }
        let n = match store.push(label, &after) {
            Ok(n) => n,
            Err(why) => return Err(failed(steps, why)),
        };
        steps.standing = after;
        if let Some(first) = store.steps().first().map(|step| step.n) {
            history.retain(|event| event.step.is_none_or(|n| n >= first));
        }
        history.push(Event {
            at: SystemTime::now(),
            text: label.to_string(),
            step: Some(n),
        });
        if history.len() > HISTORY_KEPT {
            history.remove(0);
        }
        Ok(n)
    }
}

impl AppState {
    /// The project as its file would state it without the session: what a step holds.
    fn model_bytes(&self) -> Option<Vec<u8>> {
        let path = self.project_path.as_ref()?;
        let carried = &self.project_carried;
        let file = ProjectFile {
            workspace: carried.workspace,
            active: None,
            open: Vec::new(),
            documents: carried.members.clone(),
            shown: carried.shown.clone(),
            container: carried.container.clone(),
            extra: carried.top.clone(),
        };
        serde_json::to_vec_pretty(&encode(&file, path).ok()?).ok()
    }

    /// Read the steps this project has on disk, once per project; a project file that is not the one the last step holds is a step of its own.
    ///
    /// ⚠ Nothing is written here: the folder is made when the first change is.
    pub(in crate::state) fn begin_history(&mut self) {
        // ⚠ A window that only reads the project keeps no history of it: the window that writes it keeps the steps.
        if self.workbench.steps.begun || self.workbench.held_by_another.is_some() {
            return;
        }
        let (Some(project), Some(model)) = (self.project_path.clone(), self.model_bytes()) else {
            return;
        };
        // ⚠ A project with no file yet (a path a test or a session made up) has nowhere to keep steps beside.
        if !project.is_file() {
            return;
        }
        let mut store = Snapshots::open(&history_dir(&project));
        // ⚠ The file holds the step the history stands at, or one the person undid without saving and closed: the nearest step that reads the same is where it stands, and only a file no step holds is a change made outside.
        let same = |store: &Snapshots, place: usize| store.read(place).is_ok_and(|b| b == model);
        let at = store
            .position()
            .filter(|place| same(&store, *place))
            .or_else(|| {
                (0..store.steps().len())
                    .rev()
                    .find(|place| same(&store, *place))
            });
        if let Some(place) = at.filter(|place| Some(*place) != store.position()) {
            if let Err(why) = store.go(place) {
                tracing::warn!(%why, "the place in the history was not written");
            }
        }
        let outside = at.is_none() && !store.is_empty();
        let events: Vec<Event> = (store.steps().iter())
            .map(|step| Event {
                at: step.at(),
                text: step.label.clone(),
                step: Some(step.n),
            })
            .collect();
        let steps = &mut self.workbench.steps;
        steps.begun = true;
        steps.opened = Some(SystemTime::now());
        steps.standing = model.clone();
        steps.saved = store
            .current()
            .map_or(Saved::Opened, |step| Saved::Step(step.n));
        steps.store = Some(store);
        self.workbench.history.splice(0..0, events);
        if outside {
            match self.workbench.keep_step(crate::i18n::tr(OUTSIDE), model) {
                Ok(n) => self.workbench.steps.saved = Saved::Step(n),
                Err(why) => self.history_failed(&why),
            }
        }
    }

    fn history_failed(&mut self, why: &str) {
        tracing::warn!(%why, "the project's history is not kept");
        if !std::mem::replace(&mut self.workbench.steps.failed, true) {
            self.show_warning_toast(tr!(
                "{why}. Undo is not available for this project",
                why = why
            ));
        }
    }

    /// The project was changed: keep it as a step called `label`, which is also a line of the history.
    ///
    /// ⚠ A step whose bytes read as the one before it (an import that added no file) changes nothing the
    /// file holds, so it does not make the project unsaved. A move along the steps makes none of its own.
    pub(in crate::state) fn record_step(&mut self, label: impl Into<String>) -> Option<u64> {
        let label = label.into();
        if self.workbench.steps.moving || self.workbench.held_by_another.is_some() {
            return None;
        }
        // ⚠ A history that begins only now has no earlier state to compare with, so the change stands.
        let was_begun = self.workbench.steps.begun;
        self.begin_history();
        let Some(after) = self.model_bytes() else {
            self.workbench.note(label);
            return None;
        };
        if !was_begun || after != self.workbench.steps.standing {
            self.project_carried.changed = true;
        }
        match self.workbench.keep_step(&label, after) {
            Ok(n) => Some(n),
            Err(why) => {
                self.workbench.note(label);
                if self.workbench.steps.store.is_none() && why != crate::i18n::tr(NOT_KEPT) {
                    self.history_failed(&why);
                }
                None
            }
        }
    }

    /// [`Self::record_step`] for an import, which keeps beside its step what it `created`, so the step takes back its own import whenever it is moved over.
    pub(in crate::state) fn record_import_step(
        &mut self,
        label: impl Into<String>,
        created: &[Created],
    ) {
        let Some(n) = self.record_step(label) else {
            return;
        };
        let (Some(project), Some(store)) = (
            self.project_path.as_ref(),
            self.workbench.steps.store.as_ref(),
        ) else {
            return;
        };
        if created.is_empty() {
            return;
        }
        let folder = project_folder(project);
        let made: Vec<Value> = (created.iter())
            .map(|c| json!({ "path": store_path(&c.path, &folder), "digest": c.digest }))
            .collect();
        let kept = serde_json::to_vec(&made)
            .map_err(std::io::Error::other)
            .and_then(|bytes| store.attach(n, &bytes));
        if let Err(why) = kept {
            tracing::warn!(%why, "what the import made was not kept with its step");
        }
    }

    /// What the import that is step `n` created; none for a step that is no import.
    fn import_made(&self, n: u64) -> Vec<Created> {
        let (Some(project), Some(store)) = (
            self.project_path.as_ref(),
            self.workbench.steps.store.as_ref(),
        ) else {
            return Vec::new();
        };
        let folder = project_folder(project);
        let Some(Value::Array(made)) =
            (store.attachment(n)).and_then(|b| serde_json::from_slice(&b).ok())
        else {
            return Vec::new();
        };
        (made.iter())
            .filter_map(|c| {
                Some(Created {
                    path: folder.join(c.get("path")?.as_str()?),
                    digest: c.get("digest")?.as_str()?.to_string(),
                })
            })
            .collect()
    }

    /// The project was written: the file is what the history stands at now.
    ///
    /// ⚠ The step is read again, since a save adds the files open in tabs to the project, and the next
    /// time the project is opened its file must read as the step it was left at.
    pub(in crate::state) fn project_saved(&mut self) {
        self.begin_history();
        let Some(model) = self.model_bytes() else {
            return;
        };
        let steps = &mut self.workbench.steps;
        steps.standing = model.clone();
        let Some(store) = steps.store.as_mut() else {
            return;
        };
        match store.current().map(|step| step.n) {
            Some(n) => {
                if let Err(why) = store.replace(&model) {
                    tracing::warn!(%why, "the saved step was not rewritten");
                }
                steps.saved = Saved::Step(n);
            }
            None => steps.saved = Saved::Opened,
        }
    }

    /// Whether Undo has a step to take back.
    pub fn project_can_undo(&self) -> bool {
        (self.workbench.steps.store.as_ref())
            .and_then(Snapshots::position)
            .is_some_and(|place| place > 0)
    }

    /// Whether Redo has a step to make again.
    pub fn project_can_redo(&self) -> bool {
        (self.workbench.steps.store.as_ref()).is_some_and(|store| {
            store
                .position()
                .is_some_and(|p| p + 1 < store.steps().len())
        })
    }

    /// What Undo would take back.
    pub fn project_undo_label(&self) -> Option<String> {
        let store = self.workbench.steps.store.as_ref()?;
        if store.position()? == 0 {
            return None;
        }
        store.current().map(|step| step.label.clone())
    }

    /// What Redo would make again.
    pub fn project_redo_label(&self) -> Option<String> {
        let store = self.workbench.steps.store.as_ref()?;
        store
            .steps()
            .get(store.position()? + 1)
            .map(|step| step.label.clone())
    }

    /// Take back the last step; what it was.
    pub fn undo_project(&mut self) -> Result<String, String> {
        self.settle_build_edit();
        self.begin_history();
        let store = self.workbench.steps.store.as_ref();
        let place = store
            .and_then(Snapshots::position)
            .filter(|place| *place > 0)
            .ok_or(crate::i18n::tr(NOTHING_TO_UNDO))?;
        let label = self.project_undo_label().unwrap_or_default();
        self.move_to(place - 1)?;
        self.workbench.wardrobe.trail.push(super::Side::Project);
        Ok(label)
    }

    /// Make the step that was taken back again; what it was.
    pub fn redo_project(&mut self) -> Result<String, String> {
        self.settle_build_edit();
        self.begin_history();
        let label = self
            .project_redo_label()
            .ok_or(crate::i18n::tr(NOTHING_TO_REDO))?;
        let place = (self.workbench.steps.store.as_ref())
            .and_then(Snapshots::position)
            .ok_or(crate::i18n::tr(NOTHING_TO_REDO))?;
        self.move_to(place + 1)?;
        Ok(label)
    }

    /// Stand at the step numbered `step`, undoing or redoing every step between; what it is called.
    pub fn roll_back_project_to(&mut self, step: u64) -> Result<String, String> {
        self.settle_build_edit();
        self.begin_history();
        let store = self
            .workbench
            .steps
            .store
            .as_ref()
            .ok_or(crate::i18n::tr(NOT_KEPT))?;
        let target = (store.steps().iter())
            .position(|held| held.n == step)
            .ok_or(tr!("That step is no longer kept"))?;
        let label = store.steps()[target].label.clone();
        self.move_to(target)?;
        Ok(label)
    }

    /// Stand at the step at `target`, which is not where the project stands.
    ///
    /// ⚠ Going back over an import takes back what that import created, each step its own, and refuses
    /// once a source was changed since, and then nothing moves. A copy of what it made is kept beside its step first,
    /// and going forward over it puts that back; where no copy is kept (an import of an earlier session taken back
    /// before this existed) the project moves and no more, and that is said.
    fn move_to(&mut self, target: usize) -> Result<(), String> {
        let store = self
            .workbench
            .steps
            .store
            .as_ref()
            .ok_or(crate::i18n::tr(NOT_KEPT))?;
        let here = store.position().ok_or(crate::i18n::tr(NOT_KEPT))?;
        let bytes = store
            .read(target)
            .map_err(|why| tr!("That step could not be read: {why}", why = why))?;
        let (first, past) = (here.min(target) + 1, here.max(target) + 1);
        let imports: Vec<(u64, Vec<Created>)> = (store.steps()[first..past].iter())
            .map(|step| (step.n, self.import_made(step.n)))
            .filter(|(_, made)| !made.is_empty())
            .collect();
        // ⚠ The files a plugin wrote are checked, whole, before anything moves: a file changed since leaves everything as it is.
        let mut plugin_files = Vec::new();
        for step in &store.steps()[first..past] {
            let files = self.plugin_files_of(step.n)?;
            if !files.is_empty() {
                plugin_files.push((step.n, files));
            }
        }
        if let Some(why) = plugin_files_conflict(&plugin_files, target < here) {
            return Err(why);
        }
        self.workbench.steps.moving = true;
        let moved = (if target < here {
            self.keep_aside(&imports);
            let made: Vec<Vec<Created>> = imports.iter().map(|(_, made)| made.clone()).collect();
            self.take_back_imports(&made)
        } else {
            Ok(())
        })
        .and_then(|()| self.restore_model(&bytes))
        .and_then(|()| move_plugin_files(&plugin_files, target < here));
        self.workbench.steps.moving = false;
        moved?;
        if target > here {
            let missing = self.make_imports_again(&imports);
            if missing {
                self.show_warning_toast(crate::i18n::tr(NOT_MADE_AGAIN));
            }
        }

        let steps = &mut self.workbench.steps;
        steps.standing = bytes.clone();
        let store = steps.store.as_mut().ok_or(crate::i18n::tr(NOT_KEPT))?;
        if let Err(why) = store.go(target) {
            tracing::warn!(%why, "the place in the history was not written");
        }
        let saved = match steps.saved {
            Saved::Step(n) => (store.steps().iter().position(|step| step.n == n))
                .and_then(|place| store.read(place).ok()),
            _ => None,
        };
        self.project_carried.changed = saved.is_none_or(|saved| saved != bytes);
        Ok(())
    }

    /// Keep a copy of what each import in `imports` made beside its step, so going forward over it makes it again.
    ///
    /// ⚠ A copy that cannot be kept only costs the Redo: the Undo goes on, and the Redo says what it could not make.
    fn keep_aside(&self, imports: &[(u64, Vec<Created>)]) {
        let Some(store) = self.workbench.steps.store.as_ref() else {
            return;
        };
        for (n, made) in imports {
            let aside = store.aside(*n);
            let _ = std::fs::remove_dir_all(&aside);
            for (k, created) in made.iter().enumerate() {
                if !created.path.exists() || !created.untouched() {
                    continue;
                }
                if let Err(why) = created.copy_to(&aside.join(k.to_string())) {
                    tracing::warn!(%why, "what an import made was not kept for Redo");
                }
            }
        }
    }

    /// Put back what each import in `imports` made and is not there, from the copies kept beside its step; whether something is still missing.
    fn make_imports_again(&mut self, imports: &[(u64, Vec<Created>)]) -> bool {
        let Some(store) = self.workbench.steps.store.as_ref() else {
            return imports
                .iter()
                .any(|(_, made)| made.iter().any(|c| !c.path.exists()));
        };
        let (mut missing, mut remade) = (false, false);
        let mut restored = Vec::new();
        for (n, made) in imports {
            let aside = store.aside(*n);
            for (k, created) in made.iter().enumerate() {
                if created.path.exists() {
                    continue;
                }
                match created.restore_from(&aside.join(k.to_string())) {
                    Ok(()) => {
                        remade = true;
                        restored.push(created.path.clone());
                    }
                    Err(_) => missing = true,
                }
            }
            // Empty, when everything came back; a copy that could not be put back stays for another try.
            let _ = std::fs::remove_dir(&aside);
        }
        if remade {
            self.adopt_restored_pack_sources(&restored);
            if let Some(project) = self.project_path.clone() {
                if let Err(why) = git::update(&project_folder(&project)) {
                    tracing::warn!(%why, "the ignore rules of the project are not updated");
                }
            }
            self.workbench.want_scan();
            self.workbench.findings_stale = true;
        }
        missing
    }

    /// The source folders that going back to step `step` would remove: what the imports after it created and the disk still holds.
    pub fn sources_a_roll_back_removes(&self, step: u64) -> Vec<String> {
        let Some(store) = self.workbench.steps.store.as_ref() else {
            return Vec::new();
        };
        let (Some(here), Some(target)) = (
            store.position(),
            store.steps().iter().position(|held| held.n == step),
        ) else {
            return Vec::new();
        };
        if target >= here {
            return Vec::new();
        }
        (store.steps()[target + 1..=here].iter())
            .flat_map(|held| self.import_made(held.n))
            .filter(|made| made.path.exists())
            .map(|made| leaf(&made.path))
            .collect()
    }

    /// [`Self::roll_back_project_to`], after asking when going back removes the source folders of an import.
    pub fn roll_back_project_asking(&mut self, step: u64) -> Result<(), String> {
        let sources = self.sources_a_roll_back_removes(step);
        if sources.is_empty() {
            return self.roll_back_project_to(step).map(drop);
        }
        let label = (self.workbench.steps.store.as_ref())
            .and_then(|store| store.steps().iter().find(|held| held.n == step))
            .map_or_else(String::new, |held| held.label.clone());
        self.workbench.asking = Some(super::Question::RollBack {
            step,
            label,
            sources,
        });
        Ok(())
    }

    /// Take back what the imports `made` created, newest first.
    ///
    /// ⚠ Nothing is removed while any source was changed since its import, or the move would stop with
    /// the newer imports gone; a source that is gone already was taken back.
    fn take_back_imports(&mut self, made: &[Vec<Created>]) -> Result<(), String> {
        let present: Vec<Vec<Created>> = (made.iter().rev())
            .map(|created| {
                created
                    .iter()
                    .filter(|c| c.path.exists())
                    .cloned()
                    .collect()
            })
            .filter(|created: &Vec<Created>| !created.is_empty())
            .collect();
        let changed: Vec<String> = (present.iter().flatten())
            .filter(|c| !c.untouched())
            .map(|c| leaf(&c.path))
            .collect();
        if !changed.is_empty() {
            return Err(tr!("Not undone, and nothing was removed: {changed} changed since the import. Remove what you do not want by hand", changed = changed.join(", ")));
        }
        present
            .iter()
            .try_for_each(|created| self.take_back_made(created))
    }

    /// Put the project the bytes of a step state back in place of the one held.
    fn restore_model(&mut self, bytes: &[u8]) -> Result<(), String> {
        let project = self.project_path.clone().ok_or(tr!("No project is open"))?;
        let loaded = decode(bytes, &project, Path::new("."))
            .map_err(|why| tr!("That step could not be read: {why}", why = why))?;
        let restored = loaded.project;
        let carried = &mut self.project_carried;
        carried.members = restored.documents;
        carried.container = restored.container;
        carried.shown = restored.shown;
        carried.workspace = restored.workspace;
        carried.top = restored.extra;
        let container = &self.project_carried.container;
        if (self.workbench.selected_content.as_deref())
            .is_some_and(|id| container.module(id).is_none())
        {
            self.workbench.selected_content = None;
        }
        if (self.workbench.selected_output.as_deref())
            .is_some_and(|id| container.output(id).is_none())
        {
            self.workbench.selected_output = None;
        }
        self.workbench.want_scan();
        self.workbench.findings_stale = true;
        Ok(())
    }

    /// Say that the project opens on the workspace on screen, which is the only way a save writes one.
    pub fn open_project_here(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let workspace = self.active_workspace;
        if self.project_path.is_none() || self.project_carried.workspace == workspace {
            return;
        }
        self.project_carried.workspace = workspace;
        self.record_step(tr!(
            "The project opens on {workspace}",
            workspace = workspace.label()
        ));
    }
}

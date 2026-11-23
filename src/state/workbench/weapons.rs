//! The Weapons workspace's state: the weapons of a weapons content as the last scan read them, the weapon selected, and the edits made to
//! its metadata with the Undo that takes each back.
//!
//! ⚠ **Thin by design.** Every rule, every read and every write of a weapon's files is `or-weapons`'s, through the typed family of
//! `or-io`; this keeps what the scan read, what is selected and the steps Undo moves along, and asks for a change.

use std::path::{Path, PathBuf};

use or_io::meta::typed::weapon::CWeaponInfo;
use or_project::{Container, Module, ModuleKind};
use or_source::game_import::{Created, Report, Verdict, Watch};
use or_weapons::assemble::Choice;
use or_weapons::{Depth, Facts, Finding, Weapon, Weapons};

use super::pack_import::{self, each_file, made, refused, PackJob};
use super::{ViewMode, Workbench};
use crate::state::AppState;
use crate::ui::dock::Workspace;

/// What the scan read of one weapons content.
#[derive(Debug, Clone)]
pub struct WeaponsRead {
    /// The id of the content.
    pub module: String,
    pub folder: PathBuf,
    /// The folder is another project's, which alone writes it.
    pub linked: bool,
    pub read: Result<WeaponsPack, String>,
}

/// A weapons content read: its model, what the disk said of its sources and what is wrong with it.
#[derive(Debug, Clone)]
pub struct WeaponsPack {
    pub model: Weapons,
    pub facts: Facts,
    pub findings: Vec<Finding>,
}

/// Every weapons content of `container`, read in the project in `project_folder`.
pub fn read_all(container: &Container, project_folder: &Path) -> Vec<WeaponsRead> {
    (container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Weapons)
        .map(|module| WeaponsRead {
            module: module.id.clone(),
            folder: or_weapons::module_folder(project_folder, module).unwrap_or_default(),
            linked: module.is_link(),
            read: or_weapons::planned(project_folder, module, Depth::Open)
                .map(|(model, facts, planned)| WeaponsPack {
                    model,
                    facts,
                    findings: planned.findings,
                })
                .map_err(|e| e.to_string()),
        })
        .collect()
}

/// A file as an edit left it and as it was.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// A change to the model of the pack, taken back by what it was and not by a copy of the file: the pack may have gained weapons since.
#[derive(Debug, Clone)]
pub enum ModelChange {
    Renamed {
        id: String,
        from: String,
        to: String,
    },
    Removed {
        weapon: Weapon,
        at: usize,
    },
}

/// One change Undo takes back: what it was called, the files it wrote and the changes it made to the model of the pack.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub changes: Vec<FileChange>,
    pub model: Vec<ModelChange>,
}

/// How many steps Undo keeps.
const KEPT: usize = 50;

/// The Weapons workspace's own state, reset with the project.
#[derive(Default)]
pub struct WeaponsWork {
    /// The weapons content the workspace shows; the first when none is chosen.
    pub module: Option<String>,
    /// The id of the weapon selected.
    pub selected: Option<String>,
    /// The text the list is filtered by.
    pub filter: String,
    /// The changes Undo can take back, newest last.
    pub undo: Vec<Step>,
    /// What Undo took back and Redo can make again, newest last; a new change empties it.
    pub redo: Vec<Step>,
    /// The part picked for each bone of the weapon selected; a bone with none picked wears its default.
    pub choice: Choice,
    /// How the assembled weapon is drawn.
    pub view: ViewMode,
    /// The assembled weapon's scene and camera.
    pub scene: super::SceneWork,
    /// Bumped whenever a weapon's files change, so what was read of them is read again.
    pub generation: u64,
    /// The name being typed for the weapon selected, with the name it has, kept until the field is left.
    pub renaming: Option<(String, String)>,
    /// The component being added: the attach point it goes to and the name typed.
    pub adding: Option<(usize, String)>,
}

impl Workbench {
    /// The weapons contents as the last scan read them.
    pub fn weapons_read(&self) -> &[WeaponsRead] {
        self.scan.as_ref().map_or(&[], |scan| &scan.weapons)
    }
}

/// The files under `paths`: a folder is walked for the files in it, a bounded way down.
fn files_under(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    const DEPTH: usize = 8;
    const FILES: usize = 2000;
    fn walk(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if out.len() >= FILES {
            return;
        }
        if path.is_dir() {
            if depth == 0 {
                return;
            }
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for entry in entries {
                walk(&entry, depth - 1, out);
            }
        } else {
            out.push(path.to_path_buf());
        }
    }
    let mut out = Vec::new();
    for path in paths {
        walk(&path, DEPTH, &mut out);
    }
    out
}

impl AppState {
    /// Whether the project holds weapons content.
    pub fn has_weapons(&self) -> bool {
        (self.project_carried.container.modules.iter()).any(|m| m.kind == ModuleKind::Weapons)
    }

    /// The weapons content the workspace shows: the one chosen, else the first.
    pub fn weapons_pack(&self) -> Option<&WeaponsRead> {
        let packs = self.workbench.weapons_read();
        let chosen = self.workbench.weapons.module.as_deref();
        chosen
            .and_then(|id| packs.iter().find(|p| p.module == id))
            .or_else(|| packs.first())
    }

    /// The weapon selected and the content it is in; none when nothing is selected or the pack could not be read.
    pub fn selected_weapon(&self) -> Option<(&WeaponsRead, &Weapon)> {
        let pack = self.weapons_pack()?;
        let id = self.workbench.weapons.selected.as_deref()?;
        let weapon = pack.read.as_ref().ok()?.model.weapon(id)?;
        Some((pack, weapon))
    }

    /// Select the weapon `id` of the content `module`, or none.
    pub fn select_weapon(&mut self, module: &str, id: Option<String>) {
        let work = &mut self.workbench.weapons;
        if work.module.as_deref() != Some(module) || work.selected != id {
            work.choice.clear();
            work.renaming = None;
            work.adding = None;
        }
        work.module = Some(module.to_string());
        work.selected = id;
    }

    /// Make the project hold weapons: a `weapons` folder with an empty model, a content for it, and a place for it in every build.
    pub fn add_weapon_content(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(project) = self.project_path.clone() else {
            return;
        };
        let taken: Vec<&str> = (self.project_carried.container.modules.iter())
            .map(|m| m.id.as_str())
            .collect();
        let id = super::commands::unique("weapons", taken.into_iter());
        let module = Module::weapons(&id, &id);
        let folder = or_project::project_folder(&project).join(&id);
        let refused = or_project::links::refuse_write(
            &self.project_carried.container,
            &project,
            &folder.join(or_weapons::FILE),
        );
        if let Err(why) = refused {
            self.show_error_toast(why);
            return;
        }
        if let Err(why) =
            or_weapons::adopt::start_module(&or_project::project_folder(&project), &module)
        {
            self.show_error_toast(why);
            return;
        }
        let container = &mut self.project_carried.container;
        container.modules.push(module);
        for output in &mut container.outputs {
            output.take.push(id.clone());
        }
        self.workbench.want_scan();
        self.workbench.weapons.module = Some(id.clone());
        self.record_step(tr!("Added the weapons content {id}", id = id));
        self.active_workspace = Workspace::Weapons;
        self.show_toast(tr!(
            "Weapons added: drop models, textures and weapon files on the Weapons screen to fill it"
        ));
    }

    /// The folder of the weapons content to write, none having said why when the window only reads the project or the content is linked.
    fn weapons_folder_to_write(&mut self) -> Option<(String, PathBuf)> {
        if self.refused_as_read_only() {
            return None;
        }
        // ⚠ The content as the project file states it, not as the scan read it: a pack just made has not been scanned yet.
        let chosen = self.workbench.weapons.module.as_deref();
        let modules = &self.project_carried.container.modules;
        let module = chosen
            .and_then(|id| {
                modules
                    .iter()
                    .find(|m| m.id == id && m.kind == ModuleKind::Weapons)
            })
            .or_else(|| modules.iter().find(|m| m.kind == ModuleKind::Weapons));
        let (Some(module), Some(project)) = (module, self.project_path.as_ref()) else {
            self.show_warning_toast(tr!(
                "The project holds no weapons: Project ▸ Add a weapon pack… adds some"
            ));
            return None;
        };
        if module.is_link() {
            let why = tr!("These weapons are linked from another project, which alone edits them: open that project to change them");
            self.show_warning_toast(why);
            return None;
        }
        let folder = or_weapons::module_folder(&or_project::project_folder(project), module);
        match folder {
            Ok(folder) => Some((module.id.clone(), folder)),
            Err(why) => {
                self.show_error_toast(why.to_string());
                None
            }
        }
    }

    /// Add the game files `paths` (and the files in the folders among them) to the weapons: each becomes a source, and the weapons their metadata defines join the pack, made by the import job.
    ///
    /// ⚠ Not in the frame: a source is extracted, compiled, read again and compared, which takes seconds for a model.
    pub fn add_weapons(&mut self, paths: Vec<PathBuf>) {
        let Some((module, folder)) = self.weapons_folder_to_write() else {
            return;
        };
        let files = files_under(paths);
        if files.is_empty() {
            self.show_toast(tr!("No weapon file was found among those dropped"));
            return;
        }
        self.start_pack_import(
            ModuleKind::Weapons,
            module,
            folder,
            PackJob::Add { files, into: None },
        );
    }

    /// The import of weapon files or of a new weapon ended: its report is the import report, its step takes back what it made, and the Weapons screen shows the pack.
    pub(super) fn finish_weapons_import(
        &mut self,
        module: String,
        new: Option<String>,
        report: Report,
    ) {
        let [made, _, _, refused] = super::imports::counts(&report);
        let folder = (self.workbench.weapons_read().iter())
            .find(|pack| pack.module == module)
            .map(|pack| pack.folder.clone())
            .or_else(|| self.weapons_folder_of(&module));
        let model = folder.as_deref().and_then(|f| Weapons::read(f).ok());
        let created: Vec<String> = pack_import::pack_sources(
            folder.as_deref().unwrap_or(Path::new("")),
            report.created.iter().map(|c| &c.path),
        );
        let is_metadata = |source: &String| source.to_ascii_lowercase().ends_with(".metadata.src");
        let defined: Vec<&Weapon> = (model.iter().flat_map(|m| m.weapons.iter()))
            .filter(|w| {
                w.sources
                    .iter()
                    .any(|s| created.contains(s) && is_metadata(s))
            })
            .collect();
        let unplaced = created
            .iter()
            .filter(|source| !is_metadata(source))
            .filter(|source| {
                !(model.iter().flat_map(|m| m.weapons.iter())).any(|w| w.sources.contains(source))
            })
            .count();
        let first_defined = defined.first().map(|w| w.id.clone());
        let first_refused = || {
            (report.files.iter())
                .find(|file| file.verdict == Verdict::Refused)
                .map(|file| format!("{} {}", file.input, file.reason))
                .unwrap_or_default()
        };
        let step = match (made, refused) {
            (0, _) => None,
            (_, 0) if new.is_some() => Some(tr!(
                "Added the weapon {name}",
                name = new.as_deref().unwrap_or_default()
            )),
            (made, 0) => Some(tr!(
                "Added {made} and {weapons} to the weapons",
                made = crate::state::front_door::counted(made, "source"),
                weapons = crate::state::front_door::counted(defined.len(), "weapon")
            )),
            (made, refused) => Some(tr!(
                "Added {made} and {weapons}; {refused} could not be added: {first}",
                made = crate::state::front_door::counted(made, "source"),
                weapons = crate::state::front_door::counted(defined.len(), "weapon"),
                refused = refused,
                first = first_refused()
            )),
        };
        let selected = match &new {
            Some(name) => (model.iter().flat_map(|m| m.weapons.iter()))
                .find(|w| w.name.eq_ignore_ascii_case(name))
                .map(|w| w.id.clone()),
            None => first_defined,
        };
        self.workbench.weapons.generation += 1;
        if selected.is_some() {
            self.select_weapon(&module, selected);
        } else {
            self.workbench.weapons.module = Some(module);
        }
        self.active_workspace = Workspace::Weapons;
        if unplaced > 0 {
            self.show_warning_toast(tr!(
                "{sources} belong to no weapon yet: pick the weapon they are for",
                sources = crate::state::front_door::counted(unplaced, "source")
            ));
        }
        self.finish_import_as(report, step);
    }

    /// The folder of the weapons content `module` as the project file states it.
    fn weapons_folder_of(&self, module: &str) -> Option<PathBuf> {
        let project = self.project_path.as_ref()?;
        let module = (self.project_carried.container.modules.iter())
            .find(|m| m.id == module && m.kind == ModuleKind::Weapons)?;
        or_weapons::module_folder(&or_project::project_folder(project), module).ok()
    }

    /// A weapon of a name nobody holds yet in a metadata source of its own, made by the import job and selected when it ends.
    pub fn new_weapon(&mut self) {
        let Some((module, folder)) = self.weapons_folder_to_write() else {
            return;
        };
        let held: Vec<String> = self
            .weapons_pack()
            .and_then(|p| p.read.as_ref().ok())
            .map(|p| {
                p.model
                    .weapons
                    .iter()
                    .map(|w| w.name.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let mut n = 1;
        let name = loop {
            let name = if n == 1 {
                "WEAPON_NEW".to_string()
            } else {
                format!("WEAPON_NEW_{n}")
            };
            if !held.contains(&name.to_ascii_lowercase()) {
                break name;
            }
            n += 1;
        };
        self.start_pack_import(ModuleKind::Weapons, module, folder, PackJob::New(name));
    }

    /// Take the weapon selected out of the pack: its sources stay in the folder, where the rules say no weapon names them.
    pub fn remove_selected_weapon(&mut self) {
        let Some((module, folder)) = self.weapons_folder_to_write() else {
            return;
        };
        let Some((_, weapon)) = self.selected_weapon() else {
            return;
        };
        let (id, name) = (weapon.id.clone(), weapon.name.clone());
        let Ok(mut model) = Weapons::read(&folder) else {
            return;
        };
        let Some(at) = model.weapons.iter().position(|w| w.id == id) else {
            return;
        };
        let removed = model.weapons.remove(at);
        if let Err(e) = model.write(&folder) {
            self.show_error_toast(e.to_string());
            return;
        }
        self.push_weapons_step(Step {
            label: tr!("Removed the weapon {name}", name = name).to_string(),
            changes: Vec::new(),
            model: vec![ModelChange::Removed {
                weapon: removed,
                at,
            }],
        });
        self.select_weapon(&module, None);
    }

    /// Change the weapon selected with `change`, as one step Undo takes back.
    pub fn edit_selected_weapon(&mut self, label: String, change: impl FnOnce(&mut CWeaponInfo)) {
        let Some((_, folder)) = self.weapons_folder_to_write() else {
            return;
        };
        let Some((pack, weapon)) = self.selected_weapon() else {
            return;
        };
        let Some(source) = pack
            .read
            .as_ref()
            .ok()
            .and_then(|read| or_weapons::rules::defining_source(weapon, &read.facts))
        else {
            self.show_warning_toast(tr!(
                "No metadata of this weapon defines it, so there is nothing to change"
            ));
            return;
        };
        let (name, source) = (weapon.name.clone(), folder.join(source));
        match or_weapons::edit::edit_weapon(&source, &name, change) {
            Ok(edited) if edited.changed() => {
                self.push_weapons_step(Step {
                    label,
                    changes: vec![FileChange {
                        path: edited.file,
                        before: Some(edited.before),
                        after: Some(edited.after),
                    }],
                    model: Vec::new(),
                });
            }
            Ok(_) => {}
            Err(why) => self.show_error_toast(why),
        }
    }

    /// Give the weapon selected the name `new`, in its model and in the metadata that defines it, as one step.
    pub fn rename_selected_weapon(&mut self, new: &str) {
        let new = new.trim();
        let Some((_, folder)) = self.weapons_folder_to_write() else {
            return;
        };
        let Some((pack, weapon)) = self.selected_weapon() else {
            return;
        };
        if new.is_empty() || new == weapon.name {
            return;
        }
        let taken = pack.read.as_ref().is_ok_and(|read| {
            read.model
                .weapons
                .iter()
                .any(|w| w.id != weapon.id && w.name.eq_ignore_ascii_case(new))
        });
        if taken {
            self.show_warning_toast(tr!(
                "Another weapon is called {name}: the game resolves a name once",
                name = new
            ));
            return;
        }
        let (id, old) = (weapon.id.clone(), weapon.name.clone());
        let source = pack
            .read
            .as_ref()
            .ok()
            .and_then(|read| or_weapons::rules::defining_source(weapon, &read.facts))
            .map(|s| folder.join(s));
        let mut changes = Vec::new();
        if let Some(source) = source {
            let new = new.to_string();
            match or_weapons::edit::edit_weapon(&source, &old, |w| w.name = new) {
                Ok(edited) if edited.changed() => changes.push(FileChange {
                    path: edited.file,
                    before: Some(edited.before),
                    after: Some(edited.after),
                }),
                Ok(_) => {}
                Err(why) => {
                    self.show_error_toast(why);
                    return;
                }
            }
        }
        let Ok(mut model) = Weapons::read(&folder) else {
            return;
        };
        if let Some(weapon) = model.weapon_mut(&id) {
            weapon.name = new.to_string();
        }
        if let Err(e) = model.write(&folder) {
            self.show_error_toast(e.to_string());
            return;
        }
        self.push_weapons_step(Step {
            label: tr!("Renamed {old} to {new}", old = old, new = new).to_string(),
            changes,
            model: vec![ModelChange::Renamed {
                id,
                from: old,
                to: new.to_string(),
            }],
        });
    }

    fn push_weapons_step(&mut self, step: Step) {
        let work = &mut self.workbench.weapons;
        work.undo.push(step);
        if work.undo.len() > KEPT {
            work.undo.remove(0);
        }
        work.redo.clear();
        work.generation += 1;
        self.workbench.want_scan();
    }

    /// Put the files of `step` as they were (`undo`) or as the step left them.
    fn apply_weapons_step(&mut self, step: &Step, undo: bool) -> Result<(), String> {
        for change in step.changes.iter().rev() {
            let wanted = if undo { &change.before } else { &change.after };
            match wanted {
                Some(bytes) => std::fs::write(&change.path, bytes)
                    .map_err(|e| format!("{}: {e}", change.path.display()))?,
                None => {
                    let _ = std::fs::remove_file(&change.path);
                }
            }
        }
        if !step.model.is_empty() {
            self.change_weapons_model(&step.model, undo)?;
        }
        self.workbench.want_scan();
        self.workbench.weapons.generation += 1;
        Ok(())
    }

    /// Take back the last change to a weapon.
    pub fn undo_weapons_change(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(step) = self.workbench.weapons.undo.pop() else {
            return;
        };
        match self.apply_weapons_step(&step, true) {
            Ok(()) => {
                self.show_toast(tr!("Undid: {what}", what = step.label.as_str()));
                self.workbench.weapons.redo.push(step);
            }
            Err(why) => {
                self.workbench.weapons.undo.push(step);
                self.show_error_toast(why);
            }
        }
    }

    /// Make the change Undo took back again.
    pub fn redo_weapons_change(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(step) = self.workbench.weapons.redo.pop() else {
            return;
        };
        match self.apply_weapons_step(&step, false) {
            Ok(()) => {
                self.show_toast(tr!("Did again: {what}", what = step.label.as_str()));
                self.workbench.weapons.undo.push(step);
            }
            Err(why) => {
                self.workbench.weapons.redo.push(step);
                self.show_error_toast(why);
            }
        }
    }
}

impl AppState {
    /// Make the changes of `model` to the model of the pack the project holds in its own folder, back (`undo`) or again, by what each was.
    fn change_weapons_model(&mut self, model: &[ModelChange], undo: bool) -> Result<(), String> {
        let (_, folder) = self
            .weapons_folder_to_write()
            .ok_or_else(|| tr!("The weapons of the project cannot be written").to_string())?;
        let mut pack = Weapons::read(&folder).map_err(|e| e.to_string())?;
        for change in model.iter().rev() {
            match (change, undo) {
                (ModelChange::Renamed { id, from, .. }, true) => {
                    if let Some(weapon) = pack.weapon_mut(id) {
                        weapon.name = from.clone();
                    }
                }
                (ModelChange::Renamed { id, to, .. }, false) => {
                    if let Some(weapon) = pack.weapon_mut(id) {
                        weapon.name = to.clone();
                    }
                }
                (ModelChange::Removed { weapon, at }, true) => {
                    if pack.weapon(&weapon.id).is_none() {
                        pack.weapons
                            .insert((*at).min(pack.weapons.len()), weapon.clone());
                    }
                }
                (ModelChange::Removed { weapon, .. }, false) => {
                    pack.weapons.retain(|w| w.id != weapon.id);
                }
            }
        }
        pack.write(&folder).map_err(|e| e.to_string())
    }

    /// A source folder that Undo took away is no source of a weapon: each pack goes back to what it held before the files were added.
    pub(super) fn forget_removed_weapon_sources(&mut self, created: &[Created]) {
        self.edit_weapon_packs(|folder| {
            let gone = pack_import::pack_sources(
                folder,
                created
                    .iter()
                    .map(|made| &made.path)
                    .filter(|path| !path.exists()),
            );
            if gone.is_empty() {
                return false;
            }
            or_weapons::import::forget(folder, &gone).unwrap_or(false)
        });
    }

    /// A source folder that Redo put back is a source of its pack again, and the weapons its metadata defines with it.
    pub(super) fn adopt_restored_weapon_sources(&mut self, restored: &[PathBuf]) {
        self.edit_weapon_packs(|folder| {
            let dirs: Vec<PathBuf> =
                pack_import::pack_sources(folder, restored.iter().filter(|p| p.is_dir()))
                    .into_iter()
                    .map(|name| folder.join(name))
                    .collect();
            !dirs.is_empty() && or_weapons::import::learn(folder, &dirs).is_ok()
        });
    }

    /// Let `edit` change each weapons content the project holds in its own folder; it says whether it did, and the scan is asked for again then.
    fn edit_weapon_packs(&mut self, edit: impl Fn(&Path) -> bool) {
        let Some(project) = self.project_path.as_deref() else {
            return;
        };
        let root = or_project::project_folder(project);
        let mut changed = false;
        for module in (self.project_carried.container.modules.iter())
            .filter(|m| m.kind == ModuleKind::Weapons && !m.is_link())
        {
            if let Ok(folder) = or_weapons::module_folder(&root, module) {
                changed |= edit(&folder);
            }
        }
        if changed {
            self.workbench.weapons.generation += 1;
            self.workbench.want_scan();
        }
    }
}

/// The work of the import job for weapons: each of `files` becomes a source of the pack in `folder`, one at a time, so that the job names the file in hand and a stop keeps what is made; then the pack learns of the sources.
///
/// ⚠ A file with no source form, or that cannot be read, is refused in the report with its sentence and the others still come in.
pub(super) fn add_to_pack(folder: &Path, files: &[PathBuf], watch: &Watch<'_>) -> Report {
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut report = each_file(files, watch, |path, name, report| {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return refused(report, name, format!("cannot be read: {e}")),
        };
        let incoming = or_weapons::import::Incoming {
            name,
            bytes: &bytes,
            from_game: false,
            virtual_path: None,
        };
        match or_weapons::import::add_one(folder, &incoming) {
            Ok(dir) => {
                sources.push(dir.clone());
                made(report, name, dir);
            }
            Err(why) => refused(report, name, why),
        }
    });
    if let Err(why) = or_weapons::import::learn(folder, &sources) {
        report
            .notes
            .push(format!("the pack of weapons was not updated: {why}"));
    }
    report
}

/// The work of the import job for a new weapon: a metadata source of its own for `name`, and the weapon in the pack in `folder`.
pub(super) fn new_in_pack(folder: &Path, name: &str) -> Report {
    let mut report = Report::default();
    match or_weapons::import::new_weapon(folder, name) {
        Ok((_, dir)) => made(&mut report, name, dir),
        Err(why) => refused(&mut report, name, why),
    }
    report
}

/// What a weapons content holds as the last scan read it, in words.
pub fn held_by(workbench: &Workbench, module: &Module) -> String {
    match workbench
        .weapons_read()
        .iter()
        .find(|p| p.module == module.id)
    {
        None => tr!("Reading…").to_string(),
        Some(pack) => match &pack.read {
            Ok(read) => super::held(&module.kind, read.model.weapons.len()),
            Err(_) => tr!("could not be read").to_string(),
        },
    }
}

/// How many weapons the model of a weapons content holds as the disk says, none when its file cannot be read.
pub fn on_disk(project: &Path, module: &Module) -> Option<usize> {
    let folder = or_weapons::module_folder(&or_project::project_folder(project), module).ok()?;
    Weapons::read(&folder).ok().map(|m| m.weapons.len())
}

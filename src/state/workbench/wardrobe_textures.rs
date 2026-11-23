//! The Wardrobe's texture steps as the app runs them: normal and specular maps, a texture budget with its plan shown before it is accepted, completing the
//! materials of the models, and a folder an external editor saves into bound to a slot of a texture.
//!
//! ⚠ **Nothing here makes a map, chooses a size or completes a material.** `or-wardrobe` and `or-source` do, and this asks for it off the UI thread, shows
//! what it said and carries it to the Log. A budget is a plan until it is accepted: the plan is measured on a worker and changes no byte, and Accept carries
//! out the plan that was shown.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use or_source::budget::Options as BudgetOptions;
use or_source::maps::Options as MapOptions;
use or_wardrobe::edit::Watch;
use or_wardrobe::mesh::Op;
use or_wardrobe::settings::Target;
use or_wardrobe::slots::{self, Binding, MapSlot};
use or_wardrobe::{budget as pack_budget, maps as pack_maps};

use super::wardrobe::Done;
use super::Question;
use crate::state::AppState;

/// How long a bound folder stays quiet before its newest picture is put in: an editor saves in several writes.
const QUIET: Duration = Duration::from_millis(400);

/// What the maps window is set to; the strength is in tenths so that a question stays comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapsDraft {
    pub normal: bool,
    pub specular: bool,
    pub strength: u32,
}

impl Default for MapsDraft {
    fn default() -> Self {
        Self {
            normal: true,
            specular: true,
            strength: 20,
        }
    }
}

impl MapsDraft {
    pub fn asks(&self) -> bool {
        self.normal || self.specular
    }

    fn options(&self) -> MapOptions {
        MapOptions {
            normal: self.normal,
            specular: self.specular,
            strength: self.strength as f32 / 10.0,
            only: Vec::new(),
        }
    }
}

/// A budget plan as the window shows it, with what it was made for.
#[derive(Debug, Clone)]
pub struct PlannedBudget {
    pub module: String,
    pub garments: Option<Vec<String>>,
    pub folder: PathBuf,
    pub plan: pack_budget::Plan,
}

impl PlannedBudget {
    /// Every texture the plan changes, with the source it is in, in the order the window lists them.
    pub fn choices(&self) -> impl Iterator<Item = (&str, &pack_budget::Choice)> {
        self.plan
            .sources
            .iter()
            .flat_map(|s| s.choices.iter().map(move |c| (s.source.as_str(), c)))
    }
}

/// The pictures of one choice: the texture as it is and as the plan would store it, at the size it is now.
pub type Pictures = (usize, Arc<[or_io::codec::texture::RgbaImage; 2]>);

/// The two pictures of a choice, as it is and as the plan would store it, or why they were not made.
type Looked = Result<[or_io::codec::texture::RgbaImage; 2], String>;

/// What the texture steps keep between frames.
#[derive(Default)]
pub struct TextureDesk {
    /// The memory the budget is to fit, in megabytes; 0 until the window first opens.
    pub target_mb: f32,
    planning: Option<Receiver<Result<PlannedBudget, String>>>,
    pub plan: Option<PlannedBudget>,
    /// Why the last plan was not made.
    pub failed: Option<String>,
    /// The choice of the plan shown side by side.
    pub chosen: usize,
    looking: Option<(usize, Receiver<Looked>)>,
    pub pictures: Option<Pictures>,
    watches: Watches,
}

impl TextureDesk {
    pub fn is_planning(&self) -> bool {
        self.planning.is_some()
    }

    /// The bindings of the folders the editor saves into, with the content each is in.
    pub fn bindings(&self) -> &[(String, PathBuf, Binding)] {
        &self.watches.bound
    }
}

/// The folders bound to slots, watched for the editor's saves.
#[derive(Default)]
struct Watches {
    /// Each binding, with the content and the folder of the pack it is in.
    bound: Vec<(String, PathBuf, Binding)>,
    /// The generation of the scan the bindings were last read at.
    read_at: Option<u64>,
    watcher: Option<WatcherState>,
    /// The newest picture each binding last put in, so that a save is put in once.
    delivered: HashMap<(String, usize, MapSlot), std::time::SystemTime>,
}

struct WatcherState {
    events: Receiver<notify::Result<notify::Event>>,
    _watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    /// Folders that changed and when they last did.
    pending: HashMap<PathBuf, Instant>,
}

impl AppState {
    /// Ask what the maps are to be, for the garments picked or the whole pack.
    pub fn ask_to_make_maps(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let picked = self.picked_garments();
        let garments = (!picked.is_empty()).then_some(picked);
        let draft = MapsDraft::default();
        let lines = self.maps_words(&module, garments.as_deref(), &draft);
        self.workbench.asking = Some(Question::Maps {
            module,
            garments,
            draft,
            lines,
        });
    }

    /// What the maps `draft` asks for would be, before they are made: nothing is copied or written.
    pub fn maps_words(
        &self,
        module: &str,
        garments: Option<&[String]>,
        draft: &MapsDraft,
    ) -> Vec<String> {
        let Some(folder) = (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .map(|p| p.folder.clone())
        else {
            return Vec::new();
        };
        match pack_maps::plan(&folder, garments, &draft.options()) {
            Ok(report) if report.lines().is_empty() => {
                vec![tr!("Nothing would be made.").to_string()]
            }
            Ok(report) => report.lines(),
            Err(why) => vec![why.to_string()],
        }
    }

    /// Make the maps of the garments `garments` (the whole pack where none) of the content `module`: the garments name copies of their sources that hold the pictures, and Undo takes it back.
    pub fn make_maps(&mut self, module: String, garments: Option<Vec<String>>, draft: MapsDraft) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let options = draft.options();
        let watch = Watch::default();
        let work = watch.clone();
        self.wardrobe_job_watched(
            tr!("Making normal and specular maps"),
            &module,
            watch,
            Box::new(move || {
                match pack_maps::generate(&folder, garments.as_deref(), &options, &work) {
                    Ok((report, revision)) => {
                        let n = report.maps();
                        let said = match n {
                            0 => tr!("No map was made: every diffuse has its maps, or none is named as one").to_string(),
                            n => tr!("Made {n} map(s) as new pictures in copies of the sources; the garments name the copies and the sources they were are untouched", n = n),
                        };
                        Done::revised(said, ("maps", tr!("Made {n} map(s)", n = n)), revision)
                            .with_notes(report.lines())
                    }
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Open the budget window for the garments picked, or the whole pack.
    pub fn ask_to_budget(&mut self) {
        let Some(module) = self.wardrobe_module_id() else {
            return;
        };
        let picked = self.picked_garments();
        let garments = (!picked.is_empty()).then_some(picked);
        let desk = &mut self.workbench.wardrobe.textures;
        if desk.target_mb <= 0.0 {
            desk.target_mb = 64.0;
        }
        desk.plan = None;
        desk.pictures = None;
        desk.failed = None;
        self.workbench.asking = Some(Question::Budget { module, garments });
    }

    /// Measure the textures and plan a budget that fits `target_mb`; nothing is written. The plan arrives when the worker is done.
    pub fn plan_budget(&mut self, module: String, garments: Option<Vec<String>>) {
        let Some(read) = (self.workbench.packs().iter())
            .find(|p| p.module == module)
            .and_then(|p| p.read.as_ref().ok().map(|r| (r, p.folder.clone())))
        else {
            return;
        };
        let (read, folder) = read;
        let options = BudgetOptions {
            legacy: read.wardrobe.rules.holds(Target::Legacy),
            ..BudgetOptions::default()
        };
        let target = (f64::from(self.workbench.wardrobe.textures.target_mb.max(0.01))
            * 1024.0
            * 1024.0) as u64;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let planned = pack_budget::plan(
                &folder,
                garments.as_deref(),
                target,
                &options,
                &Watch::default(),
            )
            .map(|plan| PlannedBudget {
                module,
                garments,
                folder,
                plan,
            })
            .map_err(|e| e.to_string());
            let _ = tx.send(planned);
        });
        let desk = &mut self.workbench.wardrobe.textures;
        desk.planning = Some(rx);
        desk.plan = None;
        desk.pictures = None;
        desk.failed = None;
    }

    /// Carry the plan on show out: the garments name copies of the sources cut to it, and Undo takes it back.
    pub fn accept_budget(&mut self) {
        let Some(shown) = self.workbench.wardrobe.textures.plan.clone() else {
            return;
        };
        let Some(folder) = self.folder_to_edit(&shown.module) else {
            return;
        };
        let legacy = (self.workbench.packs().iter())
            .find(|p| p.module == shown.module)
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.rules.holds(Target::Legacy));
        let options = BudgetOptions {
            legacy,
            ..BudgetOptions::default()
        };
        let watch = Watch::default();
        let work = watch.clone();
        self.workbench.wardrobe.textures.plan = None;
        self.workbench.wardrobe.textures.pictures = None;
        self.wardrobe_job_watched(
            tr!("Fitting the textures to a budget"),
            &shown.module.clone(),
            watch,
            Box::new(move || {
                match pack_budget::accept(&folder, shown.garments.as_deref(), &shown.plan, &options, &work) {
                    Ok((report, revision)) => {
                        let n = report.textures();
                        let said = match n {
                            0 => tr!("No texture was changed: they are not as the plan measured them").to_string(),
                            n => tr!("Fitted {n} texture(s) to the budget; the garments name lighter copies and the sources they were are untouched", n = n),
                        };
                        Done::revised(said, ("budget", tr!("Fitted {n} texture(s) to a budget", n = n)), revision)
                            .with_notes(report.lines())
                    }
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Show the texture of choice `index` of the plan as it is and as the plan would store it, side by side.
    pub fn look_at_budget_choice(&mut self, index: usize) {
        let desk = &mut self.workbench.wardrobe.textures;
        desk.chosen = index;
        let Some(shown) = desk.plan.as_ref() else {
            return;
        };
        let Some((source, choice)) = shown.choices().nth(index) else {
            return;
        };
        let (dir, ladder) = (
            shown
                .folder
                .join(source.replace('/', std::path::MAIN_SEPARATOR_STR)),
            choice.ladder.clone(),
        );
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let both = (|| {
                Ok([
                    or_source::budget::picture(&dir, &ladder, 0, PICTURE)?,
                    or_source::budget::picture(&dir, &ladder, 1, PICTURE)?,
                ])
            })();
            let _ = tx.send(both);
        });
        desk.looking = Some((index, rx));
    }

    /// Complete the materials of the models of the garments `garments` of `module` (every garment where none): the constants a shader declares that a material lacks are stated at the values it declares, as one step of Undo.
    pub fn complete_materials(&mut self, module: String, garments: Option<Vec<String>>) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let watch = Watch::default();
        let work = watch.clone();
        let label = Op::Materials.label();
        self.wardrobe_job_watched(
            label,
            &module,
            watch,
            Box::new(move || {
                match or_wardrobe::mesh::run_many(
                    &folder,
                    garments.as_deref(),
                    &Op::Materials,
                    None,
                    &work,
                ) {
                    Ok((report, _)) if !report.changed() && !report.refused.is_empty() => {
                        Done::said(Err((report.refused.iter())
                            .map(|r| format!("{}: {}", r.file, r.why))
                            .collect::<Vec<_>>()
                            .join("; ")))
                    }
                    Ok((report, _)) if !report.changed() => Done::said(Ok(report.sentence())),
                    Ok((report, revision)) => Done::revised(
                        report.sentence(),
                        ("materials", label.to_string()),
                        revision,
                    )
                    .with_notes(report.lines()),
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Bind the folder `folder` to a slot of a texture of a garment of `module`: what the editor saves there becomes the picture of the slot.
    pub fn bind_watch_folder(
        &mut self,
        module: &str,
        garment: String,
        texture: usize,
        slot: MapSlot,
        folder: PathBuf,
    ) {
        let Some(pack) = self.folder_to_edit(module) else {
            return;
        };
        let binding = Binding {
            folder: folder.display().to_string(),
            garment,
            texture,
            slot,
        };
        match slots::bind(&pack, binding) {
            Ok(()) => {
                self.workbench.wardrobe.textures.watches.read_at = None;
                self.toasts.success(tr!(
                    "{folder} is watched: a picture saved there becomes the {word} of the texture",
                    folder = folder.display(),
                    word = slot.word()
                ));
            }
            Err(why) => self.toasts.error(why.to_string()),
        }
    }

    /// Let go of a binding.
    pub fn unbind_watch_folder(&mut self, module: &str, binding: &Binding) {
        let Some(pack) = self.folder_to_edit(module) else {
            return;
        };
        if let Err(why) = slots::unbind(&pack, binding) {
            self.toasts.error(why.to_string());
        }
        self.workbench.wardrobe.textures.watches.read_at = None;
    }

    /// Take what the workers sent, and put in what an editor saved in a bound folder.
    pub(super) fn poll_textures(&mut self) {
        let desk = &mut self.workbench.wardrobe.textures;
        if let Some(rx) = &desk.planning {
            match rx.try_recv() {
                Ok(Ok(plan)) => {
                    desk.plan = Some(plan);
                    desk.planning = None;
                    desk.chosen = 0;
                    self.look_at_budget_choice(0);
                }
                Ok(Err(why)) => {
                    desk.failed = Some(why);
                    desk.planning = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => desk.planning = None,
            }
        }
        let desk = &mut self.workbench.wardrobe.textures;
        if let Some((index, rx)) = &desk.looking {
            match rx.try_recv() {
                Ok(Ok(both)) => {
                    desk.pictures = Some((*index, Arc::new(both)));
                    desk.looking = None;
                }
                Ok(Err(why)) => {
                    tracing::warn!(%why, "a texture of the budget could not be shown");
                    desk.looking = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => desk.looking = None,
            }
        }
        self.poll_watch_folders();
    }

    /// Whether a plan is being measured or a picture of it is being made, which wants a frame again.
    pub fn is_planning_a_budget(&self) -> bool {
        let desk = &self.workbench.wardrobe.textures;
        desk.planning.is_some() || desk.looking.is_some()
    }

    fn poll_watch_folders(&mut self) {
        let generation = self.workbench.wardrobe.generation;
        let packs: Vec<(String, PathBuf)> = (self.workbench.packs().iter())
            .filter(|p| !p.linked)
            .map(|p| (p.module.clone(), p.folder.clone()))
            .collect();
        let watches = &mut self.workbench.wardrobe.textures.watches;
        if watches.read_at != Some(generation) {
            watches.bound = packs
                .iter()
                .flat_map(|(module, folder)| {
                    slots::bindings(folder)
                        .into_iter()
                        .map(move |b| (module.clone(), folder.clone(), b))
                })
                .collect();
            watches.read_at = Some(generation);
        }
        if watches.bound.is_empty() {
            watches.watcher = None;
            return;
        }
        let wanted: HashSet<PathBuf> = watches
            .bound
            .iter()
            .map(|(_, _, b)| PathBuf::from(&b.folder))
            .collect();
        let stale = watches.watcher.as_ref().is_none_or(|w| w.watched != wanted);
        if stale {
            watches.watcher = start_watching(&wanted);
        }
        let Some(state) = watches.watcher.as_mut() else {
            return;
        };
        let now = Instant::now();
        for event in state.events.try_iter().flatten() {
            if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                for path in event.paths {
                    if let Some(parent) = path.parent() {
                        state.pending.insert(parent.to_path_buf(), now);
                    }
                }
            }
        }
        let due: Vec<PathBuf> = state
            .pending
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= QUIET)
            .map(|(folder, _)| folder.clone())
            .collect();
        for folder in &due {
            state.pending.remove(folder);
        }
        for folder in due {
            let bound: Vec<(String, PathBuf, Binding)> = self
                .workbench
                .wardrobe
                .textures
                .watches
                .bound
                .iter()
                .filter(|(_, _, b)| Path::new(&b.folder) == folder)
                .cloned()
                .collect();
            for (module, pack, binding) in bound {
                self.put_in_saved_picture(&module, &pack, &binding);
            }
        }
    }

    /// Put the newest picture saved in the folder of `binding` into its slot, once for each save.
    fn put_in_saved_picture(&mut self, module: &str, pack: &std::path::Path, binding: &Binding) {
        let Some(file) = slots::newest(binding) else {
            return;
        };
        let Some(saved) = std::fs::metadata(&file).and_then(|m| m.modified()).ok() else {
            return;
        };
        let key = (binding.garment.clone(), binding.texture, binding.slot);
        let delivered = &self.workbench.wardrobe.textures.watches.delivered;
        if delivered.get(&key).is_some_and(|last| *last >= saved) {
            return;
        }
        match slots::deliver(pack, binding, &file) {
            Ok(done) => {
                // Only a save that went in is not tried again: one that did not is tried at the folder's next change.
                self.workbench
                    .wardrobe
                    .textures
                    .watches
                    .delivered
                    .insert(key, saved);
                self.workbench.note(tr!(
                    "Put {file} into the {word} of {source}",
                    file = file.display(),
                    word = binding.slot.word(),
                    source = done.source
                ));
                // The source folder is watched by the live recook, which cooks it now.
                self.workbench.want_scan();
            }
            Err(why) => self.toasts.warning(tr!(
                "{file} was saved in {folder} (content {module}) and was not put in: {why}",
                module = module,
                why = why,
                file = file.display(),
                folder = binding.folder
            )),
        }
    }
}

/// The side of the pictures a budget choice is shown with.
const PICTURE: u32 = 256;

fn start_watching(folders: &HashSet<PathBuf>) -> Option<WatcherState> {
    let (sender, events) = channel();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = sender.send(event);
        },
        notify::Config::default(),
    )
    .map_err(|e| tracing::warn!(error = %e, "no file watcher, so a bound folder is not watched"))
    .ok()?;
    for folder in folders {
        if let Err(e) = watcher.watch(folder, RecursiveMode::NonRecursive) {
            tracing::warn!(folder = %folder.display(), error = %e, "a bound folder is not watched");
        }
    }
    Some(WatcherState {
        events,
        _watcher: watcher,
        watched: folders.clone(),
        pending: HashMap::new(),
    })
}

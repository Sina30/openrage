//! Importing into the open project from the app: what a picker or an explorer row hands over, the job that runs on a worker, the report it leaves and Undo.
//!
//! ⚠ **Nothing here decompiles.** `or_source::game_import` does, and it only adds: this picks the
//! inputs, runs it off the UI thread, puts a kept file among the project's files and hands every new
//! source to the cook that is already watching the project. A source comes in as it is, and a new
//! source is `or_source::create`'s.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_job::{Cancel, Progress};
use or_project::{project_folder, shadow_root, ModuleKind};
use or_source::asset::{Source, MANIFEST};
use or_source::game_import::{
    import_watched, Created, Facts, FileReport, Input, Options, Report, Skeleton, Verdict, Watch,
};
use or_source::{create, git};
use or_title::vfs::is_archive_segment;

use super::leaf;
use super::pack_import::PackJob;
use crate::state::jobs::{JobKind, RunningJob};
use crate::state::retail::{self, PathClass};
use crate::state::toasts::ToastAction;
use crate::state::{
    AppState, DocPayload, DocumentId, DocumentKind, RpfNodeBacking, RpfNodeId, RpfSessionNode,
};
use crate::ui::dock::Workspace;

/// Why an import command waits for a project.
pub const NEEDS_A_PROJECT: &str = tr_key!("Open or create a project first");

const NEEDS_ROWS: &str = tr_key!("Select the files to import in Files or in the centre first");
const NEEDS_FILES: &str =
    tr_key!("A whole archive, a folder inside one and a folder of the game are imported by selecting the files in them");
const NEEDS_ENTRIES: &str = tr_key!("An archive is not imported whole: open it in Files and use Import into project on the files in it");

/// One thing to import, owned so that a worker can read it.
#[derive(Debug, Clone)]
pub enum Item {
    /// A file, or a folder walked recursively, on disk.
    Path {
        path: PathBuf,
        from_game: bool,
        /// The install the path is in, said when it came out of the game.
        build: Option<String>,
    },
    /// A file inside an archive, read before the worker starts, under the name and the path it has there.
    Entry {
        name: String,
        bytes: Arc<Vec<u8>>,
        virtual_path: String,
        from_game: bool,
        build: Option<String>,
    },
}

/// What the worker does.
enum Work {
    Import {
        items: Vec<Item>,
        /// The skeleton of each freemode ped the game folder gave, by the model name of the ped, for the ped components among the items.
        skeletons: Vec<(&'static str, Skeleton)>,
    },
    Images {
        images: Vec<PathBuf>,
    },
    /// A job on the pack in `folder`, which belongs to the content `module` of `kind`: game files made its sources, or a new source.
    Pack {
        kind: ModuleKind,
        module: String,
        folder: PathBuf,
        job: PackJob,
    },
}

struct Running {
    doing: String,
    /// What the job says until the worker has counted its files.
    detail: String,
    receiver: Receiver<Report>,
    /// Set by Cancel; the worker reads it between files.
    cancel: Cancel,
    progress: Progress,
    /// Whether the worker reads `cancel`: adding images is one copy that cannot stop.
    stoppable: bool,
    /// The content the import adds sources to, whose screen shows once it ends, and the name of the one new source when that is the job.
    into: Option<(ModuleKind, String, Option<String>)>,
}

/// What the last import left, which its window shows and Undo import takes back.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub report: Report,
    /// What Undo import did or why it refused, once it was pressed.
    pub undone: Option<Result<String, String>>,
}

/// The imports of the open project: the one running and the last report.
#[derive(Default)]
pub struct ImportWork {
    running: Option<Running>,
    report: Option<ImportReport>,
    /// The report's window is open.
    pub shown: bool,
    /// How many imports have ended, so the toast of one can tell whether a later import came after it.
    serial: u64,
}

impl ImportWork {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn report(&self) -> Option<&ImportReport> {
        self.report.as_ref()
    }
}

/// How many files were made, already sources, kept and refused.
pub fn counts(report: &Report) -> [usize; 4] {
    let mut counts = [0; 4];
    for file in &report.files {
        counts[match file.verdict {
            Verdict::Made { .. } => 0,
            Verdict::AlreadyASource { .. } => 1,
            Verdict::Kept { .. } => 2,
            Verdict::Refused => 3,
        }] += 1;
    }
    counts
}

/// What an import did, in one line: `7 files: 5 became sources, 1 kept as it is, 1 refused`, with no count that is nought.
pub fn said(report: &Report) -> String {
    let [made, already, kept, refused] = counts(report);
    let part = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        part(made, tr!("became a source"), tr!("became sources")),
        part(
            already,
            tr!("was already a source"),
            tr!("were already sources"),
        ),
        part(kept, tr!("kept as it is"), tr!("kept as they are")),
        part(refused, "refused", "refused"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let files = crate::state::front_door::counted(report.files.len(), "file");
    match parts.is_empty() {
        true => files,
        false => format!("{files}: {}", parts.join(", ")),
    }
}

impl Work {
    fn run(self, root: &Path, watch: &Watch<'_>) -> Report {
        match self {
            Self::Import { items, skeletons } => {
                // A ped component's skin is bound to the skeleton of its ped, which is in another file: the one of the ped its path or its name states.
                let skeleton_of = |label: &str| {
                    let label = label.to_ascii_lowercase();
                    (skeletons.iter())
                        .find(|(ped, _)| label.contains(ped))
                        .map(|(_, skeleton)| skeleton.clone())
                        .unwrap_or_default()
                };
                let facts = |from_game: bool,
                             build: &Option<String>,
                             virtual_path: Option<String>,
                             label: &str| Facts {
                    from_game,
                    skeleton: skeleton_of(
                        &virtual_path.clone().unwrap_or_else(|| label.to_string()),
                    ),
                    virtual_path,
                    game_build: build.clone().filter(|_| from_game),
                };
                let inputs: Vec<Input<'_>> = items
                    .iter()
                    .map(|item| match item {
                        Item::Path {
                            path,
                            from_game,
                            build,
                        } => Input::Path {
                            path,
                            facts: facts(*from_game, build, None, &path.display().to_string()),
                        },
                        Item::Entry {
                            name,
                            bytes,
                            virtual_path,
                            from_game,
                            build,
                        } => Input::Bytes {
                            name,
                            bytes,
                            facts: facts(*from_game, build, Some(virtual_path.clone()), name),
                        },
                    })
                    .collect();
                import_watched(root, &inputs, &Options::default(), watch)
            }
            Self::Images { images } => author(root, &images),
            Self::Pack {
                kind, folder, job, ..
            } => super::pack_import::run(&kind, &folder, job, watch),
        }
    }
}

impl AppState {
    /// Why an import cannot start now, none when it can.
    pub fn import_refused(&self) -> Option<&'static str> {
        if self.project_path.is_none() {
            return Some(crate::i18n::tr(NEEDS_A_PROJECT));
        }
        if self.workbench.held_by_another.is_some() {
            return Some(crate::i18n::tr(super::read_only::READ_ONLY));
        }
        self.workbench
            .imports
            .is_running()
            .then_some(tr!("An import is already running"))
    }

    /// Whether `path` is a file of the game as shipped: inside an install, outside its mods overlay.
    fn is_the_games(&self, path: &Path) -> bool {
        retail::classify(path, &self.prefs) == PathClass::ShippedGame
    }

    /// The build of the install `path` is in, for the source that comes out of it.
    ///
    /// ⚠ The install that holds the file, not the mounted one: a file of an Enhanced install is
    /// Enhanced's while Legacy is mounted.
    fn build_of(&self, path: &Path) -> Option<String> {
        retail::game_of(path, &self.prefs).map(|game| game.label().to_string())
    }

    /// Why the files and folders at `paths` cannot be imported now, none when they can.
    pub fn import_paths_refused(&self, paths: &[PathBuf]) -> Option<&'static str> {
        // ⚠ The same hazard as a whole archive in `rows_cannot_be_imported`: a file read whole and kept, gigabytes and all.
        self.import_refused().or_else(|| {
            (paths.iter())
                .any(|path| {
                    (path.file_name().and_then(|n| n.to_str())).is_some_and(is_archive_segment)
                })
                .then_some(crate::i18n::tr(NEEDS_ENTRIES))
        })
    }

    /// Import the files and folders at `paths`, which a picker or a drop named, into the open project.
    pub fn import_game_files(&mut self, paths: Vec<PathBuf>) {
        if let Some(why) = self.import_paths_refused(&paths) {
            self.show_info_toast(why);
            return;
        }
        let items = paths
            .into_iter()
            .map(|path| {
                let from_game = self.is_the_games(&path);
                Item::Path {
                    build: self.build_of(&path).filter(|_| from_game),
                    from_game,
                    path,
                }
            })
            .collect();
        self.start_import(items);
    }

    /// Why the rows of `doc` cannot be imported, none when they can.
    pub fn rows_cannot_be_imported(
        &self,
        doc: DocumentId,
        rows: &[RpfNodeId],
    ) -> Option<&'static str> {
        if let Some(why) = self.import_refused() {
            return Some(why);
        }
        if rows.is_empty() {
            return Some(crate::i18n::tr(NEEDS_ROWS));
        }
        let Some(DocPayload::Rpf(rpf)) = self.document(doc).map(|d| &d.payload) else {
            return Some(crate::i18n::tr(NEEDS_ROWS));
        };
        let importable = |node: &RpfSessionNode| match &node.backing {
            RpfNodeBacking::DiskFile { .. }
            | RpfNodeBacking::InMemoryFile { .. }
            | RpfNodeBacking::ArchiveFile { .. } => true,
            // ⚠ A walk reads every file whole and keeps the ones with no source form: a folder of the game holds archives of gigabytes.
            RpfNodeBacking::DiskFolder { fs_path, .. } => !self.is_the_games(fs_path),
            _ => false,
        };
        let all = rows
            .iter()
            .all(|&row| rpf.session.node(row).is_some_and(importable));
        (!all).then_some(crate::i18n::tr(NEEDS_FILES))
    }

    /// Import the rows of `doc` into the open project: a file or a folder on disk where it is, and an archive entry from its bytes, with the path it has in the archive.
    ///
    /// ⚠ An entry's bytes are read here, on the UI thread, since the archive's reader is the session's;
    /// that is one entry at a time, as opening one reads it. Nothing is written to the archive.
    pub fn import_rows(&mut self, doc: DocumentId, rows: &[RpfNodeId]) {
        if let Some(why) = self.rows_cannot_be_imported(doc, rows) {
            self.show_info_toast(why);
            return;
        }
        let in_the_game = self.document(doc).is_some_and(|d| {
            d.kind == DocumentKind::GameRoot
                || d.path
                    .as_deref()
                    .is_some_and(|path| self.is_the_games(path))
        });
        let opened = self.document(doc).and_then(|d| d.path.clone());
        let mut items = Vec::new();
        let mut unread = Vec::new();
        if let Some(DocPayload::Rpf(rpf)) = self.document(doc).map(|d| &d.payload) {
            for &row in rows {
                let Some(node) = rpf.session.node(row) else {
                    continue;
                };
                match &node.backing {
                    RpfNodeBacking::DiskFile { fs_path, .. }
                    | RpfNodeBacking::DiskFolder { fs_path, .. } => {
                        let from_game = self.is_the_games(fs_path);
                        items.push(Item::Path {
                            build: self.build_of(fs_path).filter(|_| from_game),
                            from_game,
                            path: fs_path.clone(),
                        });
                    }
                    _ => match (rpf.session.bytes_for(row), rpf.session.path_of(row)) {
                        (Some(bytes), Some(virtual_path)) => {
                            // ⚠ The archive's file decides, not the folder that was opened: a folder above the install holds its archives too.
                            let game_file = (rpf.session.archive_on_disk_around(row))
                                .map(|(_, archive)| archive)
                                .filter(|archive| self.is_the_games(archive));
                            let from_game = in_the_game || game_file.is_some();
                            let build = (game_file.as_deref().or(opened.as_deref()))
                                .and_then(|path| self.build_of(path))
                                .filter(|_| from_game);
                            items.push(Item::Entry {
                                name: node.name.clone(),
                                bytes,
                                virtual_path,
                                from_game,
                                build,
                            });
                        }
                        _ => unread.push(node.name.clone()),
                    },
                }
            }
        }
        if let Some(first) = unread.first() {
            self.show_error_toast(tr!(
                "{first} could not be read from its archive, so it was not imported",
                first = first
            ));
        }
        self.start_import(items);
    }

    /// Write `images` into a new textures source of the project, as a person authored it.
    pub fn add_images_as_textures_source(&mut self, images: Vec<PathBuf>) {
        if let Some(why) = self.import_refused() {
            self.show_info_toast(why);
            return;
        }
        let detail = tr!("{count} image(s)", count = images.len());
        self.run_import(
            Work::Images { images },
            tr!("Adding images as a textures source").to_string(),
            detail,
        );
    }

    /// Run `job` on the pack `folder` of the content `module` of `kind` as the import job, and show the screen of the content when it ends.
    pub(super) fn start_pack_import(
        &mut self,
        kind: ModuleKind,
        module: String,
        folder: PathBuf,
        job: PackJob,
    ) {
        let detail = tr!("Reading what is to be imported").to_string();
        self.run_import(
            Work::Pack {
                kind,
                module,
                folder,
                job,
            },
            tr!("Importing into the project").to_string(),
            detail,
        );
    }

    fn start_import(&mut self, items: Vec<Item>) {
        if items.is_empty() {
            return;
        }
        let detail = tr!("Reading what is to be imported").to_string();
        let skeletons = or_wardrobe::Ped::ALL
            .into_iter()
            .filter_map(|ped| {
                let bones = self.workbench.wardrobe.dressed.bones(ped)?;
                Some((ped.model(), Skeleton::of(bones.to_vec())))
            })
            .collect();
        self.run_import(
            Work::Import { items, skeletons },
            tr!("Importing into the project").to_string(),
            detail,
        );
    }

    fn run_import(&mut self, work: Work, doing: String, detail: String) {
        if let Some(why) = self.import_refused() {
            self.show_info_toast(why);
            return;
        }
        let Some(project) = self.project_path.clone() else {
            return;
        };
        let root = project_folder(&project);
        let stoppable = matches!(work, Work::Import { .. } | Work::Pack { .. });
        let into = match &work {
            Work::Pack {
                kind, module, job, ..
            } => Some((
                kind.clone(),
                module.clone(),
                match job {
                    PackJob::New(name) => Some(name.clone()),
                    PackJob::Add { .. } => None,
                },
            )),
            _ => None,
        };
        let (cancel, progress) = (Cancel::new(), Progress::new());
        let (stop, told) = (cancel.clone(), progress.clone());
        let (tx, receiver) = channel();
        std::thread::spawn(move || {
            let cancelled = || stop.is_set();
            let say = |done: usize, total: usize, name: &str| {
                told.reached(done as u64, total as u64);
                told.now_on(name);
            };
            let watch = Watch {
                cancelled: Some(&cancelled),
                progress: Some(&say),
            };
            let _ = tx.send(work.run(&root, &watch));
        });
        self.workbench.imports.running = Some(Running {
            doing,
            detail,
            receiver,
            cancel,
            progress,
            stoppable,
            into,
        });
    }

    /// The import running, as the job list draws it.
    pub fn project_import_job(&self) -> Option<RunningJob> {
        let running = self.workbench.imports.running.as_ref()?;
        let snap = running.progress.snapshot();
        let counted = snap.total > 0;
        Some(RunningJob {
            kind: JobKind::ProjectImport,
            doing: running.doing.clone(),
            fraction: counted.then(|| snap.fraction()),
            detail: match counted {
                true => tr!(
                    "{attempted} of {total} files  {item}",
                    attempted = snap.attempted,
                    total = snap.total,
                    item = snap.item
                ),
                false => running.detail.clone(),
            },
            stopping: running.cancel.is_set(),
        })
    }

    /// Ask the import running to stop after the file in hand: what it made stays, and its report lists the rest.
    pub fn cancel_import(&mut self) {
        let stoppable = self.workbench.imports.running.as_ref().map(|running| {
            if running.stoppable {
                running.cancel.set();
            }
            running.stoppable
        });
        if stoppable == Some(false) {
            self.show_info_toast(tr!(
                "This import finishes the images it was given; its report has Undo import"
            ));
        }
    }

    /// Take the report of the import that ended: its kept files join the project, its sources are cooked, and its window opens.
    pub(super) fn poll_imports(&mut self) {
        let Some(running) = self.workbench.imports.running.take() else {
            return;
        };
        let report = match running.receiver.try_recv() {
            Ok(report) => report,
            Err(TryRecvError::Empty) => {
                self.workbench.imports.running = Some(running);
                return;
            }
            // ⚠ A worker that panicked drops its sender: what it wrote is on disk, and the scan lists it.
            Err(TryRecvError::Disconnected) => {
                self.workbench.want_scan();
                self.show_error_toast(tr!(
                    "The import stopped before it was done: sources/ holds what it made"
                ));
                return;
            }
        };
        match running.into {
            Some((ModuleKind::Particles, module, new)) => {
                self.finish_particles_import(module, new, report)
            }
            Some((ModuleKind::Weapons, module, new)) => {
                self.finish_weapons_import(module, new, report)
            }
            Some((ModuleKind::Maps, module, _)) => self.finish_maps_import(module, report),
            _ => self.finish_import(report),
        }
    }

    fn finish_import(&mut self, report: Report) {
        self.finish_import_as(report, None);
    }

    /// [`Self::finish_import`] with the History step called `step` when one is given, as a new source is not an import of files.
    pub(super) fn finish_import_as(&mut self, mut report: Report, step: Option<String>) {
        let kept: Vec<PathBuf> = report
            .files
            .iter()
            .filter_map(|file| match &file.verdict {
                Verdict::Kept {
                    copied_to: Some(path),
                } => Some(path.clone()),
                _ => None,
            })
            .collect();
        let held = self.add_files_without_a_step(kept);
        report.notes.extend(held.refused);

        // ⚠ Dated a second ago, so the cook's quiet time has already passed.
        let then = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        for file in &report.files {
            if let Verdict::Made { folder } = &file.verdict {
                if let Some(cooks) = self.workbench.cooks.as_mut() {
                    cooks.watch(folder);
                }
                self.workbench.file_saved(folder.join(MANIFEST), then);
            }
        }

        let said = said(&report);
        // ⚠ The window opens only for an import with something to read: a file refused or kept as it is, one left out, one stopped. One that worked is a line and a button.
        let to_read = report.files.is_empty()
            || report.cancelled
            || !report.left_out.is_empty()
            || !report.notes.is_empty()
            || counts(&report)[2..].iter().any(|&n| n > 0);
        // ⚠ One step for the whole import, which Undo takes back with Undo import: the files it kept and the sources it made.
        if report.cancelled {
            // The window says it stopped and has Undo import, so no toast says it a second time.
            self.record_import_step(
                tr!("Stopped the import after {said}", said = said),
                &report.created,
            );
        } else {
            let new_source = step.is_some();
            let label = step.unwrap_or_else(|| tr!("Imported {said}", said = said));
            self.record_import_step(label.clone(), &report.created);
            // ⚠ The window has Undo import itself, so it is shown alone; the toast is for the import that needs no reading.
            if !to_read && !report.created.is_empty() {
                let serial = self.workbench.imports.serial + 1;
                let toast = crate::state::toasts::Toast::success(label)
                    .with_action(ToastAction::UndoImport(serial));
                // A new source has no report worth reading.
                self.toasts.push(match new_source {
                    true => toast,
                    false => toast.with_also(ToastAction::ShowReport),
                });
            } else if !to_read {
                self.show_info_toast(tr!(
                    "Already in the project: {files}",
                    files = crate::state::front_door::counted(report.files.len(), "file")
                ));
            }
        }
        self.workbench.imports.serial += 1;
        self.workbench.want_scan();
        self.workbench.findings_stale = true;
        self.workbench.imports.report = Some(ImportReport {
            report,
            undone: None,
        });
        self.workbench.imports.shown = to_read;
    }

    /// Whether the last import made something that Undo import can still take back.
    pub fn import_can_be_undone(&self) -> bool {
        (self.workbench.imports.report()).is_some_and(|shown| {
            !shown.report.created.is_empty() && !matches!(shown.undone, Some(Ok(_)))
        })
    }

    /// The Undo import of the toast an import raised: it takes that import back, unless another ended after it.
    pub(super) fn undo_import_of_toast(&mut self, serial: u64) {
        if serial != self.workbench.imports.serial {
            self.show_info_toast(tr!(
                "Another import ended after this one: the History goes back to before it"
            ));
            return;
        }
        self.undo_last_import();
    }

    /// Undo import as a command does it: with the report window shut, a toast says how it went.
    pub fn undo_last_import(&mut self) {
        self.undo_import();
        match self
            .workbench
            .imports
            .report()
            .and_then(|shown| shown.undone.clone())
        {
            Some(Ok(said)) => self.show_toast(said),
            Some(Err(why)) => self.show_error_toast(why),
            None => {}
        }
    }

    /// Show the import in the Assets table: its first source, else the first file it kept.
    pub fn show_import_in_assets(&mut self) {
        let Some(shown) = self.workbench.imports.report() else {
            return;
        };
        let files = &shown.report.files;
        let first = |wanted: fn(&Verdict) -> Option<&Path>| {
            files.iter().find_map(|file| wanted(&file.verdict))
        };
        let key = first(|verdict| match verdict {
            Verdict::Made { folder } => Some(folder.as_path()),
            _ => None,
        })
        .or_else(|| {
            first(|verdict| match verdict {
                Verdict::AlreadyASource { folder } => Some(folder.as_path()),
                _ => None,
            })
        })
        .or_else(|| {
            first(|verdict| match verdict {
                Verdict::Kept {
                    copied_to: Some(path),
                } => Some(path.as_path()),
                _ => None,
            })
        })
        .map(|path| path.display().to_string());
        self.select_asset(key);
        self.active_workspace = Workspace::Project;
    }

    /// Take back the last import: remove exactly what it created, once none of it was changed.
    ///
    /// ⚠ All or nothing. A folder whose digest is not the one the import left is somebody's work now,
    /// and removing the others would leave a half of the import that no report describes.
    pub fn undo_import(&mut self) {
        let Some(shown) = self.workbench.imports.report.as_ref() else {
            return;
        };
        if matches!(shown.undone, Some(Ok(_))) {
            return;
        }
        let created = shown.report.created.clone();
        let outcome = self.undo_created(&created);
        if let Some(shown) = self.workbench.imports.report.as_mut() {
            shown.undone = Some(outcome);
        }
    }

    /// Take back what `created` holds as Undo import does, for a move along the History; the report's window marks its import taken back once none of it is left.
    pub(super) fn take_back_made(&mut self, created: &[Created]) -> Result<(), String> {
        let outcome = self.undo_created(created);
        if let Some(shown) = self.workbench.imports.report.as_mut() {
            if shown.report.created.iter().all(|c| !c.path.exists()) {
                shown.undone = Some(outcome.clone());
            }
        }
        outcome.map(|_| ())
    }

    fn undo_created(&mut self, created: &[Created]) -> Result<String, String> {
        if created.is_empty() {
            return Err(
                tr!("This import created nothing, so there is nothing to take back").to_string(),
            );
        }
        let changed: Vec<String> = created
            .iter()
            .filter(|c| !c.untouched())
            .map(|c| leaf(&c.path))
            .collect();
        if !changed.is_empty() {
            return Err(tr!("Not undone, and nothing was removed: {changed} changed since the import. Remove what you do not want by hand", changed = changed.join(", ")));
        }
        let Some(project) = self.project_path.clone() else {
            return Err(crate::i18n::tr(NEEDS_A_PROJECT).to_string());
        };
        let shadow = shadow_root(&project);
        let ids: Vec<String> = created
            .iter()
            .filter_map(|c| Source::open(&c.path).ok())
            .map(|source| source.manifest.id)
            .collect();
        let mut removed = 0;
        let mut failed = Vec::new();
        for c in created {
            match c.undo() {
                Ok(true) => removed += 1,
                Ok(false) => failed.push(tr!(
                    "{path} changed while it was taken back",
                    path = leaf(&c.path)
                )),
                Err(e) => failed.push(format!("{}: {e}", c.path.display())),
            }
        }
        // ⚠ The cook's output of a source that is gone would go on resolving textures for the viewport.
        for id in ids {
            let _ = std::fs::remove_dir_all(shadow.join(id));
        }
        let gone: HashSet<&Path> = created.iter().map(|c| c.path.as_path()).collect();
        self.project_carried
            .members
            .retain(|member| !gone.contains(member.path.as_path()));
        self.forget_removed_pack_sources(created);
        if let Err(e) = git::update(&project_folder(&project)) {
            failed.push(tr!(
                "the ignore rules of the project are not updated: {e}",
                e = e
            ));
        }
        self.workbench.want_scan();
        self.workbench.findings_stale = true;
        self.record_step(tr!(
            "Took back the import: {removed} folder(s) and file(s) removed",
            removed = removed
        ));
        match failed.first() {
            Some(first) => Err(tr!(
                "Took back {removed}, but {first}",
                removed = removed,
                first = first
            )),
            None => Ok(tr!(
                "Took back the import: {removed} folder(s) and file(s) removed",
                removed = removed
            )),
        }
    }
}

/// `<name>.textures.src` under `sources/` from `images`, written as a person authored it: a free name, the images copied in, and no DDS.
///
/// ⚠ A picture that is none, or is named like another, is refused by name and the others still make the source.
fn author(root: &Path, images: &[PathBuf]) -> Report {
    let mut report = Report::default();
    let (usable, refused) = create::sort_images(images);
    for (image, reason) in refused {
        report.files.push(FileReport {
            input: image.display().to_string(),
            verdict: Verdict::Refused,
            reason,
        });
    }
    if usable.is_empty() {
        return report;
    }
    let paths: Vec<PathBuf> = usable.into_iter().map(|(path, _)| path).collect();
    match create::textures_from_images(root, &create::default_name(&paths), &paths) {
        Ok(made) => {
            report.files.push(FileReport {
                input: tr!("{count} image(s)", count = paths.len()),
                verdict: Verdict::Made {
                    folder: made.path.clone(),
                },
                reason: tr!(
                    "became {path} from {count} image(s)",
                    path = leaf(&made.path),
                    count = paths.len()
                ),
            });
            report.created.push(made);
        }
        Err(why) => report.files.push(FileReport {
            input: tr!("{count} image(s)", count = paths.len()),
            verdict: Verdict::Refused,
            reason: why,
        }),
    }
    report
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use or_job::{Cancel, Progress};
    use or_source::game_import::Watch;

    use super::*;
    use crate::state::importing_game_files_from_the_app::{project, sources_of, write, ytd};

    fn running(progress: &Progress, cancel: &Cancel, stoppable: bool) -> Running {
        let (_sender, receiver) = channel();
        Running {
            doing: "Importing into the project".into(),
            detail: "Reading the folder".into(),
            receiver,
            cancel: cancel.clone(),
            progress: progress.clone(),
            stoppable,
            into: None,
        }
    }

    #[test]
    fn a_running_import_says_how_far_it_has_got_and_cancel_asks_it_to_stop() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _) = project(dir.path());
        let (progress, cancel) = (Progress::new(), Cancel::new());
        state.workbench.imports.running = Some(running(&progress, &cancel, true));

        let job = state.project_import_job().expect("a job");
        assert_eq!(job.fraction, None, "the walk has not counted its files yet");
        assert_eq!(job.detail, "Reading the folder");

        progress.reached(3, 12);
        progress.now_on("lamp.ytd");
        let job = state.project_import_job().expect("a job");
        assert_eq!(job.fraction, Some(0.25));
        assert!(
            job.detail.contains("3 of 12") && job.detail.contains("lamp.ytd"),
            "{}",
            job.detail
        );
        assert!(!job.stopping && !cancel.is_set());

        state.cancel_job(JobKind::ProjectImport);
        assert!(cancel.is_set(), "the worker reads this flag between files");
        assert!(state.project_import_job().is_some_and(|job| job.stopping));
    }

    #[test]
    fn an_import_that_cannot_stop_does_not_say_it_is_stopping() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _) = project(dir.path());
        let (progress, cancel) = (Progress::new(), Cancel::new());
        state.workbench.imports.running = Some(running(&progress, &cancel, false));

        state.cancel_job(JobKind::ProjectImport);

        assert!(!cancel.is_set());
        assert!(state.project_import_job().is_some_and(|job| !job.stopping));
    }

    #[test]
    fn a_stopped_import_keeps_what_it_made_lists_the_rest_and_undo_import_takes_back_only_that() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, folder) = project(dir.path());
        let props = dir.path().join("downloads/props");
        for n in 0..5u8 {
            write(
                &props.join(format!("prop_{n}.ytd")),
                &ytd(&format!("prop_{n}"), n + 1),
            );
        }
        let mine = folder.join("sources/mine.textures.src");
        write(&mine.join("asset.json"), b"{}");

        let asked = Cell::new(0);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() > 2
        };
        let watch = Watch {
            cancelled: Some(&stop),
            progress: None,
        };
        let work = Work::Import {
            items: vec![Item::Path {
                path: props,
                from_game: false,
                build: None,
            }],
            skeletons: Vec::new(),
        };
        let report = work.run(&folder, &watch);
        state.finish_import(report);

        let shown = state.workbench.imports.report().expect("a report");
        assert!(shown.report.cancelled);
        assert_eq!(counts(&shown.report)[0], 2, "two were made before the stop");
        assert_eq!(shown.report.left_out.len(), 3);
        assert!(state.workbench.imports.shown, "the report window opens");
        assert!(
            state.toasts.iter().all(|t| !t.message.contains("stopped")),
            "the window says it stopped, so no toast says it again: {:?}",
            state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
        assert!(
            (state.workbench.history().iter()).any(|e| e.text.contains("Stopped")),
            "{:?}",
            state.workbench.history()
        );

        assert!(state.import_can_be_undone());
        state.undo_import();
        let shown = state.workbench.imports.report().expect("a report");
        assert!(matches!(shown.undone, Some(Ok(_))), "{:?}", shown.undone);
        assert_eq!(sources_of(&folder), ["mine.textures.src"]);
    }
}

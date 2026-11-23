//! Reopening a built pack from the app: the question's answer, the job that runs on a worker, and the report it leaves.
//!
//! ⚠ **Nothing here reads a pack.** `or_wardrobe::reopen` and `or_wardrobe::gctproject` do, and all they write is a new
//! project folder; this asks where, runs them off the UI thread with the Stop their `Watch` gives, and keeps what they said.
//! The pack is never written and the folder is new, so the result has no Undo: deleting the folder takes it back.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use or_project::container::is_name;
use or_wardrobe::edit::{Error as EditError, Watch};
use or_wardrobe::reopen::{Error, Options, Reopened};
use or_wardrobe::{gctproject, reopen};

use super::Question;
use crate::state::front_door::new_project::default_place;
use crate::state::jobs::{file_name, JobKind, Outcome, RunningJob, Then};
use crate::state::AppState;

/// Why a pack cannot be reopened while one is being.
const BUSY: &str = tr_key!("A pack is already being imported");

struct Running {
    built: PathBuf,
    into: PathBuf,
    /// The question that started it, which comes back with the reason when the pack is refused.
    asked: Question,
    receiver: Receiver<Result<Reopened, Error>>,
    watch: Watch,
}

/// What the last reopening made, which its window shows.
#[derive(Debug, Clone)]
pub struct ReopenReport {
    /// The pack that was read.
    pub built: PathBuf,
    /// The new project folder.
    pub into: PathBuf,
    pub made: Reopened,
}

/// The reopening of a built pack: the one running and the last report.
///
/// ⚠ Kept when the project changes: it belongs to the pack that was read and the folder it made, not to the project that was open.
#[derive(Default)]
pub struct ReopenWork {
    running: Option<Running>,
    /// The packs of a drop that holds several, asked about one after the other once the question on screen is answered and the pack before is read.
    waiting: VecDeque<PathBuf>,
    report: Option<ReopenReport>,
    /// The report's window is open.
    pub shown: bool,
}

impl ReopenWork {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn report(&self) -> Option<&ReopenReport> {
        self.report.as_ref()
    }
}

/// The extension of `path` in lower case.
fn extension_of(path: &Path) -> String {
    (path.extension())
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Whether a dropped file is a built pack that asks to be reopened: a singleplayer DLC, an `.oiv` or a grzyClothTool project.
///
/// ⚠ A `.rpf` is an archive the explorer opens, so it asks only where a drop already means "put this in the project", and
/// only when it is named `dlc.rpf` as every singleplayer DLC is: the game's own archives and a mod's are not packs of this kind.
pub fn asks_to_reopen(path: &Path, on_the_project: bool) -> bool {
    match extension_of(path).as_str() {
        "oiv" | gctproject::EXTENSION => true,
        "rpf" => {
            on_the_project
                && (path.file_name()).is_some_and(|name| name.eq_ignore_ascii_case("dlc.rpf"))
        }
        _ => false,
    }
}

/// What the file is, as the question says it.
pub fn what_it_is(built: &Path) -> &'static str {
    match extension_of(built).as_str() {
        "oiv" => tr!("an OIV package"),
        gctproject::EXTENSION => tr!("a grzyClothTool project"),
        _ => tr!("a singleplayer DLC"),
    }
}

/// The folder name the project is offered under: the pack file's name, or the folder it is in for the `dlc.rpf` every DLC is called.
fn default_name(built: &Path) -> String {
    let stem = built.file_stem().map(|s| s.to_string_lossy().into_owned());
    let folder = (built.parent())
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned());
    let named = match stem.as_deref() {
        Some("dlc") => folder.or(stem),
        _ => stem,
    };
    named
        .filter(|name| is_name(name))
        .unwrap_or_else(|| "imported".into())
}

/// The folder the project is made in, or why `name` in `place` cannot be one.
pub fn destination(place: &str, name: &str) -> Result<PathBuf, String> {
    let (place, name) = (place.trim(), name.trim());
    if !is_name(name) {
        return Err(tr!(
            "{name} cannot be a folder name here: it is the name of the project's folder",
            name = format!("{name:?}")
        ));
    }
    let place = Path::new(place);
    if place.as_os_str().is_empty() {
        return Err(tr!("Say which folder the project's folder is made in").into());
    }
    if !place.is_dir() {
        return Err(tr!("{place} is not a folder", place = place.display()));
    }
    let into = place.join(name);
    let holds_something = std::fs::read_dir(&into).is_ok_and(|mut d| d.next().is_some());
    if holds_something || into.is_file() {
        return Err(tr!(
            "{folder} already holds something: a pack is imported into a folder of its own",
            folder = into.display()
        ));
    }
    Ok(into)
}

fn run(built: &Path, into: &Path, options: &Options, watch: &Watch) -> Result<Reopened, Error> {
    match extension_of(built) == gctproject::EXTENSION {
        true => gctproject::reopen_watched(built, into, options, watch),
        false => reopen::reopen_watched(built, into, options, watch),
    }
}

impl AppState {
    /// Why a pack cannot be reopened now, none when it can.
    pub fn reopen_refused(&self) -> Option<&'static str> {
        self.workbench
            .reopen
            .is_running()
            .then_some(crate::i18n::tr(BUSY))
    }

    /// Ask whether the pack `built` is to be reopened as a project, and where.
    pub fn ask_to_reopen(&mut self, built: PathBuf) {
        if let Some(why) = self.reopen_refused() {
            self.show_info_toast(why);
            return;
        }
        let place = default_place(&self.prefs).display().to_string();
        let name = default_name(&built);
        self.workbench.asking = Some(Question::Reopen {
            built,
            place,
            name,
            pack: String::new(),
            error: None,
            focus: true,
        });
    }

    /// Ask about `built` now, or once the question on screen is answered when one is.
    pub(in crate::state) fn ask_in_turn(&mut self, built: PathBuf) {
        match self.workbench.asking {
            Some(_) => self.workbench.reopen.waiting.push_back(built),
            None => self.ask_to_reopen(built),
        }
    }

    /// Ask about the next pack that waits, once nothing is asked and no pack is being read.
    fn ask_about_the_next_waiting(&mut self) {
        if self.workbench.asking.is_some() || self.workbench.reopen.is_running() {
            return;
        }
        if let Some(built) = self.workbench.reopen.waiting.pop_front() {
            self.ask_to_reopen(built);
        }
    }

    /// Answer the question with `asked`, the fields as they stand: start reopening, or say what is wrong with them.
    pub fn answer_reopen(&mut self, asked: Question) {
        let Question::Reopen { place, name, .. } = &asked else {
            return;
        };
        if let Some(why) = self.reopen_refused() {
            self.show_info_toast(why);
            return;
        }
        match destination(place, name) {
            Ok(into) => self.begin_reopen(asked, into, Watch::default()),
            Err(why) => self.workbench.asking = Some(with_error(asked, why)),
        }
    }

    /// Read the pack `asked` names on a worker and make the project in `into`, which `watch` can stop.
    pub(in crate::state) fn begin_reopen(&mut self, asked: Question, into: PathBuf, watch: Watch) {
        let Question::Reopen { built, pack, .. } = &asked else {
            return;
        };
        let options = Options {
            pack: Some(pack.trim().to_string()).filter(|p| !p.is_empty()),
        };
        let (path, to, told) = (built.clone(), into.clone(), watch.clone());
        let (tx, receiver) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(run(&path, &to, &options, &told));
        });
        self.workbench.reopen.running = Some(Running {
            built: built.clone(),
            into,
            asked,
            receiver,
            watch,
        });
        self.workbench.asking = None;
    }

    /// The reopening running, as the job list draws it.
    pub fn project_reopen_job(&self) -> Option<RunningJob> {
        let running = self.workbench.reopen.running.as_ref()?;
        let seen = running.watch.progress.snapshot();
        let detail = match (seen.total, seen.attempted) {
            (0, 0) => tr!("Opening the pack").to_string(),
            (0, read) => tr!("{read} file(s) read  {item}", read = read, item = seen.item),
            (total, read) => tr!(
                "{read} of {total} files  {item}",
                read = read,
                total = total,
                item = seen.item
            ),
        };
        Some(RunningJob {
            kind: JobKind::ProjectReopen,
            doing: tr!(
                "Importing {built} as a project",
                built = file_name(&running.built)
            ),
            fraction: (seen.total > 0).then(|| seen.fraction()),
            detail,
            stopping: running.watch.cancel.is_set(),
        })
    }

    /// Ask the reopening running to stop: nothing is made, and the pack is not touched.
    pub fn cancel_reopen(&mut self) {
        if let Some(running) = &self.workbench.reopen.running {
            running.watch.cancel.set();
        }
    }

    /// Take the result of the reopening that ended: its report opens, or the question comes back with the reason.
    pub(super) fn poll_reopen(&mut self) {
        self.ask_about_the_next_waiting();
        let Some(running) = self.workbench.reopen.running.take() else {
            return;
        };
        let outcome = match running.receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => {
                self.workbench.reopen.running = Some(running);
                return;
            }
            // ⚠ A worker that panicked drops its sender: what it wrote is in the folder, which the sentence names.
            Err(TryRecvError::Disconnected) => {
                let said = tr!(
                    "Importing the pack stopped before it was done: {running} may hold part of it",
                    running = running.into.display()
                );
                self.jobs
                    .record(JobKind::ProjectReopen, Outcome::Failed, &said);
                self.show_error_toast(said);
                return;
            }
        };
        self.finish_reopen(running, outcome);
    }

    fn finish_reopen(&mut self, running: Running, outcome: Result<Reopened, Error>) {
        let Running {
            built, into, asked, ..
        } = running;
        let kind = JobKind::ProjectReopen;
        match outcome {
            Ok(made) => {
                let said = tr!(
                    "Imported {built} as a project: {garments} garment(s), {textures} texture(s)",
                    built = file_name(&built),
                    garments = made.garments,
                    textures = made.textures
                );
                self.jobs
                    .record(kind, Outcome::Done(Then::OpenFolder(into.clone())), &said);
                self.workbench.note(said);
                self.workbench.reopen.report = Some(ReopenReport { built, into, made });
                self.workbench.reopen.shown = true;
            }
            Err(Error::Edit(EditError::Stopped)) => {
                let said = tr!("Stopped: nothing was made, and the pack was not changed");
                self.jobs
                    .record(kind, Outcome::Stopped(Then::Nothing), said);
                self.show_info_toast(said);
            }
            Err(why) => {
                self.jobs.record(kind, Outcome::Failed, why.to_string());
                match self.workbench.asking {
                    None => self.workbench.asking = Some(with_error(asked, why.to_string())),
                    Some(_) => self.show_error_toast(tr!(
                        "{built} was not imported: {why}",
                        why = why,
                        built = file_name(&built)
                    )),
                }
            }
        }
    }

    /// Open the project the last reopening made, asking first when another is open.
    pub fn open_reopened_project(&mut self) {
        let Some(file) = (self.workbench.reopen.report()).map(|r| r.made.project.clone()) else {
            return;
        };
        self.workbench.reopen.shown = false;
        self.open_project_asking(file);
    }
}

/// `question` with `why` as the reason it was asked again.
fn with_error(question: Question, why: String) -> Question {
    match question {
        Question::Reopen {
            built,
            place,
            name,
            pack,
            focus,
            ..
        } => Question::Reopen {
            built,
            place,
            name,
            pack,
            error: Some(why),
            focus,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Prefs;

    fn running(watch: &Watch) -> Running {
        let (_sender, receiver) = channel();
        Running {
            built: "D:/packs/dlc.rpf".into(),
            into: "D:/projects/dlc".into(),
            asked: Question::Reopen {
                built: "D:/packs/dlc.rpf".into(),
                place: "D:/projects".into(),
                name: "dlc".into(),
                pack: String::new(),
                error: None,
                focus: false,
            },
            receiver,
            watch: watch.clone(),
        }
    }

    #[test]
    fn a_running_reopen_says_how_many_files_it_has_read_and_stop_asks_it_to_stop() {
        let mut state = AppState::from_prefs(Prefs::default());
        let watch = Watch::default();
        state.workbench.reopen.running = Some(running(&watch));

        let job = state.project_reopen_job().expect("a job");
        assert_eq!(job.fraction, None, "the pack has not been counted yet");
        assert_eq!(job.detail, "Opening the pack");
        assert!(job.doing.contains("dlc.rpf"), "{}", job.doing);

        watch.progress.reached(3, 0);
        watch.progress.now_on("ped/lamp.ytd");
        let job = state.project_reopen_job().expect("a job");
        assert!(
            job.detail.contains("3 file(s) read") && job.detail.contains("ped/lamp.ytd"),
            "a pack that does not say how many files it holds is counted as it goes: {}",
            job.detail
        );

        // A project file knows its count, so it has a bar.
        watch.progress.reached(1, 4);
        let job = state.project_reopen_job().expect("a job");
        assert_eq!(job.fraction, Some(0.25));
        assert!(job.detail.contains("1 of 4"), "{}", job.detail);

        assert!(!job.stopping && !watch.cancel.is_set());
        state.cancel_job(JobKind::ProjectReopen);
        assert!(
            watch.cancel.is_set(),
            "the worker reads this flag between files"
        );
        assert!(state.project_reopen_job().is_some_and(|job| job.stopping));
    }

    #[test]
    fn a_worker_that_panicked_says_which_folder_may_hold_part_of_the_project() {
        let mut state = AppState::from_prefs(Prefs::default());
        state.workbench.reopen.running = Some(running(&Watch::default()));

        // The sender was dropped without a result.
        state.poll_reopen();

        assert!(!state.workbench.reopen.is_running());
        assert!(
            state.toasts.iter().any(|t| t.message.contains("projects")),
            "{:?}",
            state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
    }
}

//! Checking a source against the game: whether the file it was taken from is still the file the install holds.
//!
//! ⚠ **Only when asked, and never per scan.** The check reads the install's archives, so a person
//! asks for one source, a worker reads, and the answer is kept for as long as the project is open.
//! `or_source::origin_check` judges; this finds the install and says what it found.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use or_source::asset::Source;
use or_source::origin_check::{check, Installed, Verdict};
use or_title::install::read_virtual_file;

use super::scan::Kept;
use super::{leaf, Workbench};
use crate::state::AppState;

/// The installs this machine has, as the label a source's `origin.game_build` states and the folder, the active game's first.
type Installs = Vec<(String, PathBuf)>;

struct Running {
    source: PathBuf,
    receiver: Receiver<Verdict>,
}

/// The check running, and what the checks of this session found, by the folder of the source.
#[derive(Default)]
pub struct Originals {
    running: Option<Running>,
    verdicts: HashMap<PathBuf, Verdict>,
}

impl Originals {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// What the last check of the source in `folder` found.
    pub fn verdict_of(&self, folder: &Path) -> Option<&Verdict> {
        self.verdicts.get(folder)
    }

    /// Whether a check found the game's file for the source in `folder` to be another one now.
    pub fn is_changed(&self, folder: &Path) -> bool {
        self.verdict_of(folder).is_some_and(Verdict::is_changed)
    }
}

/// The root of the install a source was taken from: the one named by `game_build`, else the first.
fn root_for<'a>(installs: &'a Installs, game_build: Option<&str>) -> Option<&'a Path> {
    let named = game_build.and_then(|build| installs.iter().find(|(label, _)| label == build));
    named.or(installs.first()).map(|(_, root)| root.as_path())
}

impl Workbench {
    pub fn originals(&self) -> &Originals {
        &self.originals
    }
}

impl AppState {
    /// The install folders set in the preferences, the active game's first.
    fn installs(&self) -> Installs {
        let mut found: Installs = or_title::Game::all()
            .filter_map(|game| {
                let root = self.prefs.game_paths.get(game)?;
                Some((game.label().to_string(), root.clone()))
            })
            .collect();
        if let Some(active) = self.prefs.active_game.map(|game| game.label()) {
            found.sort_by_key(|(label, _)| label != active);
        }
        found
    }

    /// Why Check against the game cannot run on `row` now, none when it can.
    pub fn check_refused(&self, row: &super::AssetRow) -> Option<&'static str> {
        if row.kept != Kept::Source {
            return Some(tr!("Only a source folder has an original to check"));
        }
        if !row.from_game {
            return Some(tr!(
                "Only a source taken from the game has a game file to check against"
            ));
        }
        self.workbench
            .originals
            .is_running()
            .then_some(tr!("A check is already running"))
    }

    /// Compare the file the source at `key` was taken from with the install's now, on a worker.
    pub fn check_against_game(&mut self, key: &str) {
        let Some(row) = self.workbench.row(key) else {
            return;
        };
        let (source, refused) = (row.path.clone(), self.check_refused(row));
        if let Some(why) = refused {
            self.show_info_toast(why);
            return;
        }
        let installs = self.installs();
        if installs.is_empty() {
            self.show_info_toast(tr!(
                "Set the game's install folder first: the check reads the game's file from it"
            ));
            return;
        }
        let keys = self.keys.provider();
        let (tx, receiver) = channel();
        let folder = source.clone();
        std::thread::spawn(move || {
            let verdict = match Source::open(&folder) {
                Err(problem) => Verdict::Unchecked(problem.to_string()),
                Ok(source) => {
                    let game_build = source.manifest.origin.game_build.as_deref();
                    check(&source.manifest, |path| {
                        let Some(root) = root_for(&installs, game_build) else {
                            return Installed::Unreadable(tr!("no install is set").to_string());
                        };
                        match read_virtual_file(root, path, &keys) {
                            Ok(Some(bytes)) => Installed::Found(bytes),
                            Ok(None) => Installed::Missing,
                            Err(why) => Installed::Unreadable(why),
                        }
                    })
                }
            };
            let _ = tx.send(verdict);
        });
        self.workbench.note(tr!(
            "Checking {source} against the game",
            source = leaf(&source)
        ));
        self.workbench.originals.running = Some(Running { source, receiver });
    }

    /// Take the answer of the check that ended: a line in the history, a toast, and the rows and problems read afresh.
    pub(super) fn poll_originals(&mut self) {
        let Some(running) = self.workbench.originals.running.take() else {
            return;
        };
        let verdict = match running.receiver.try_recv() {
            Ok(verdict) => verdict,
            Err(TryRecvError::Empty) => {
                self.workbench.originals.running = Some(running);
                return;
            }
            // ⚠ A worker that panicked drops its sender; nothing was compared, and it is said so.
            Err(TryRecvError::Disconnected) => {
                Verdict::Unchecked(tr!("the check stopped before it was done").to_string())
            }
        };
        let said = format!("{}: {}", leaf(&running.source), verdict.sentence());
        self.workbench.note(tr!("Checked {said}", said = said));
        match &verdict {
            Verdict::Unchanged => self.show_toast(said),
            Verdict::Changed { .. } | Verdict::Missing { .. } => self.show_warning_toast(said),
            Verdict::Unchecked(_) => self.show_info_toast(said),
        }
        self.workbench
            .originals
            .verdicts
            .insert(running.source, verdict);
        self.workbench.generation += 1;
        self.workbench.findings_stale = true;
    }
}

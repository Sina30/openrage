//! Building a DLC pack from a folder in the app: a question shows what
//! `openrage-cli dlc create` packs and refuses before anything is written, a
//! job builds it, and the line `dlclist.xml` needs waits to be copied.
//!
//! ⚠ The pack is built by `or_cli::create_dlc_pack`, the call the command
//! makes, so a folder cannot pack differently from the app and the command.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use or_io::container::rpf::RpfEncryption;
use or_job::{Cancel, Progress};
use or_title::dlc::{DlcPackSpec, PackDocuments};

use super::jobs::{JobKind, Outcome, RunningJob, Then};
use super::retail::{classify, PathClass};
use super::{AppState, GateOutcome, RpfOutputMode};

const IN_AN_INSTALL: &str = tr_key!(
    "A DLC pack is built outside the game, as openrage-cli dlc create \
                             builds it: choose a folder outside the install, then copy the \
                             pack's folder in and add its line to dlclist.xml yourself"
);

/// The pack a folder's Create DLC pack… describes, waiting on Create or Cancel.
pub struct PackQuestion {
    pub folder: PathBuf,
    /// What the command packs of `folder`; the question sets its name, type and order.
    pub spec: DlcPackSpec,
    pub encryption: RpfOutputMode,
    /// Why the command refuses the pack as it reads at the last [`recheck`](Self::recheck).
    pub refused: Vec<String>,
    /// Puts the caret in the name once the question first draws.
    pub focus_name: bool,
}

impl PackQuestion {
    /// Ask the command again what it refuses, after an answer changed.
    pub fn recheck(&mut self) {
        self.refused = or_cli::dlc_refusals(&self.spec, encryption(self.encryption));
    }

    /// The line `dlclist.xml` needs for the pack, as the command prints it.
    pub fn line(&self) -> String {
        dlclist_line(&self.spec.dlclist_entry())
    }
}

/// A pack just built, until its window is closed.
pub struct Built {
    pub folder: PathBuf,
    pub line: String,
}

type Building = Result<(PathBuf, PackDocuments), String>;

struct Running {
    cancel: Cancel,
    progress: Progress,
    receiver: Receiver<Building>,
    name: String,
    /// The folder the pack is built from, every file of which it reads.
    folder: PathBuf,
}

/// The question, the one pack building and the pack it left.
#[derive(Default)]
pub struct DlcPacks {
    pub asked: Option<PackQuestion>,
    running: Option<Running>,
    pub built: Option<Built>,
}

impl DlcPacks {
    /// Run `work`, which builds the pack `name` of `folder`, as the one build.
    pub(super) fn start(
        &mut self,
        name: String,
        folder: PathBuf,
        work: impl FnOnce(Cancel, Progress) -> Building + Send + 'static,
    ) {
        let (cancel, progress) = (Cancel::new(), Progress::new());
        let (tx, receiver) = channel();
        let (worker_cancel, worker_progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work(worker_cancel, worker_progress));
        });
        self.running = Some(Running {
            cancel,
            progress,
            receiver,
            name,
            folder,
        });
    }

    /// The build running, named, and the folder it reads.
    pub fn reads(&self) -> Option<(String, &[PathBuf])> {
        let running = self.running.as_ref()?;
        let name = tr!("the build of the DLC pack {name}", name = running.name);
        Some((name, std::slice::from_ref(&running.folder)))
    }

    /// The pack building, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let running = self.running.as_ref()?;
        let snap = running.progress.snapshot();
        Some(RunningJob {
            kind: JobKind::DlcPack,
            doing: tr!("Building the DLC pack {name}", name = running.name),
            fraction: (snap.total > 0).then(|| snap.fraction()),
            detail: tr!(
                "{size} written of {size_2} in the folder",
                size = or_job::human_bytes(snap.attempted),
                size_2 = or_job::human_bytes(snap.total)
            ),
            stopping: running.cancel.is_set(),
        })
    }

    /// ⚠ Stops at the build's next write, which removes the archive it began.
    pub fn cancel(&self) {
        if let Some(running) = &self.running {
            running.cancel.set();
        }
    }
}

/// The archive encryption `mode` names, as the command's `--encryption` takes it.
fn encryption(mode: RpfOutputMode) -> RpfEncryption {
    match mode {
        RpfOutputMode::Open => RpfEncryption::Open,
        RpfOutputMode::Aes => RpfEncryption::Aes,
        RpfOutputMode::Ng => RpfEncryption::Ng,
    }
}

fn dlclist_line(entry: &str) -> String {
    format!("<Item>{entry}</Item>", entry = entry)
}

impl AppState {
    /// Ask what to pack `folder` as, showing what the command packs and refuses, with its defaults.
    ///
    /// ponytail: lists the folder on the UI thread, as the command does before
    /// it builds; a job of its own if a folder of many thousand files stalls it.
    pub fn ask_dlc_pack(&mut self, folder: PathBuf) {
        if self.dlc_pack.running.is_some() {
            self.show_error_toast(tr!("A DLC pack is already being built"));
            return;
        }
        // ⚠ No name, as the command has none until `--name` gives it one.
        let spec = match or_cli::dlc_pack_of_folder(&folder, "") {
            Ok(spec) => spec,
            Err(why) => {
                self.show_error_toast(why);
                return;
            }
        };
        let mut asked = PackQuestion {
            folder,
            spec,
            encryption: RpfOutputMode::Open,
            refused: Vec::new(),
            focus_name: true,
        };
        asked.recheck();
        self.dlc_pack.asked = Some(asked);
    }

    /// Build the asked pack as `<dest>/<name>/dlc.rpf` on a worker, as the command builds it.
    ///
    /// ⚠ Never into a game install, its overlay included: installing a pack
    /// is copying its folder in and adding its line, a step of its own. The
    /// gate is asked all the same, as every write asks it. A refusal keeps the
    /// question, so another folder can be chosen.
    pub fn create_dlc_pack(&mut self, dest: PathBuf) {
        let Some(asked) = &self.dlc_pack.asked else {
            return;
        };
        let archive = dest.join(&asked.spec.name).join("dlc.rpf");
        if self.dlc_pack.running.is_some() {
            self.show_error_toast(tr!("A DLC pack is already being built"));
            return;
        }
        if classify(&archive, &self.prefs) != PathClass::Outside {
            self.show_warning_toast(crate::i18n::tr(IN_AN_INSTALL));
            return;
        }
        if archive.exists() {
            self.show_error_toast(tr!(
                "`{archive}` is already there; choose another folder",
                archive = archive.display()
            ));
            return;
        }
        match self.gate_permits(Some(&archive)) {
            GateOutcome::Proceed => {}
            // ⚠ Only an install asks, and an install was refused above.
            GateOutcome::Refused | GateOutcome::Confirm(_) => return,
        }
        let Some(asked) = self.dlc_pack.asked.take() else {
            return;
        };
        let (name, encryption) = (asked.spec.name.clone(), encryption(asked.encryption));
        let folder = asked.folder.clone();
        self.dlc_pack.start(name, folder, move |cancel, progress| {
            // ponytail: bytes written against the folder's size, which
            // compression and padding make approximate.
            let total = (asked.spec.files.iter())
                .filter_map(|file| std::fs::metadata(&file.source).ok())
                .map(|meta| meta.len())
                .sum();
            or_cli::create_dlc_pack(&asked.spec, &dest, encryption, &mut |written| {
                progress.reached(written, total);
                !cancel.is_set()
            })
        });
    }

    /// Where a finished build lands: the pack's window and its line, or why it stopped.
    pub(super) fn poll_dlc_pack(&mut self) {
        let received = match &self.dlc_pack.running {
            Some(running) => running.receiver.try_recv(),
            None => return,
        };
        if matches!(received, Err(TryRecvError::Empty)) {
            return;
        }
        let Some(running) = self.dlc_pack.running.take() else {
            return;
        };
        let (outcome, said) = match received {
            Ok(Ok((archive, docs))) => {
                let folder = archive
                    .parent()
                    .map_or_else(PathBuf::new, Path::to_path_buf);
                let line = dlclist_line(&docs.dlclist_entry);
                let said = tr!(
                    "Built {archive}; dlclist.xml needs {line}",
                    line = line,
                    archive = archive.display()
                );
                self.dlc_pack.built = Some(Built {
                    folder: folder.clone(),
                    line,
                });
                (Outcome::Done(Then::OpenFolder(folder)), said)
            }
            Ok(Err(_)) if running.cancel.is_set() => (
                Outcome::Stopped(Then::Nothing),
                tr!("Stopped before it finished; the archive it began is removed").to_string(),
            ),
            Ok(Err(why)) => {
                self.show_error_toast(tr!(
                    "{name} was not built: {why}",
                    why = why,
                    name = running.name
                ));
                (Outcome::Failed, why)
            }
            Err(_) => (
                Outcome::Failed,
                tr!("It stopped on an internal error; no pack was built").to_string(),
            ),
        };
        self.jobs.record(JobKind::DlcPack, outcome, said);
    }
}

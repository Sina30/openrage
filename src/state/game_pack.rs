//! The game's shaders as a SPIR-V pack, which the helper program `or-shader-tool` bakes: where the helper is, the process that runs it, what it says, and whether the pack on disk is the one it would write now.
//!
//! ⚠ The helper carries the C++ translator and the editor never loads it: it is a process, and the editor reads what it wrote.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use or_io::gta5::shaders::spirv_pack::{SpirvPack, MANIFEST_FILE};
use or_shader_tool::pack_protocol::Event;

use super::jobs::{JobKind, RunningJob};

/// The variable that names the helper program, for one built elsewhere.
pub const HELPER_ENV: &str = "OPENRAGE_SHADER_TOOL";

/// The variable that names a folder holding a pack baked elsewhere, which is read and never baked over.
pub const PACK_ENV: &str = "OPENRAGE_GAME_PACK";

/// What the pack of the game's shaders is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePackStatus {
    /// There is none, and none is being baked.
    Missing,
    /// There is no helper to bake one: it is not beside the editor, or it was built without its translator.
    NoHelper(String),
    /// The helper is running: reading the install while `total` is nought, then translating.
    Baking {
        done: usize,
        total: usize,
    },
    /// The pack on disk is the one the helper would write, with this many programs translated.
    Ready {
        translated: usize,
    },
    Failed {
        message: String,
    },
}

impl GamePackStatus {
    pub fn short_label(&self) -> String {
        match self {
            Self::Missing => "Game shaders not baked".into(),
            Self::NoHelper(why) => format!("Game shaders cannot be baked: {why}"),
            Self::Baking { total: 0, .. } => "Reading the game's shaders".into(),
            Self::Baking { done, total } => format!("Baking the game's shaders: {done} of {total}"),
            Self::Ready { translated } => format!("Game shaders ready ({translated} programs)"),
            Self::Failed { message } => format!("Baking the game's shaders failed: {message}"),
        }
    }
}

/// What happened since the last look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePackEvent {
    /// A pack is on disk at the root: `baked` where a bake just wrote it, else found to be current.
    Ready {
        root: PathBuf,
        baked: bool,
    },
    Failed {
        message: String,
    },
    Stopped,
    Progress,
}

/// The helper program: `OPENRAGE_SHADER_TOOL`, else beside the running editor.
pub fn helper_path() -> Option<PathBuf> {
    helper_in(
        std::env::var_os(HELPER_ENV).map(PathBuf::from),
        std::env::current_exe().ok().as_deref(),
    )
}

/// The helper program a `named` path or the folder of `editor` holds, the named first, where it is a file.
fn helper_in(named: Option<PathBuf>, editor: Option<&Path>) -> Option<PathBuf> {
    let beside = editor.and_then(|exe| {
        let name = format!("or-shader-tool{}", std::env::consts::EXE_SUFFIX);
        Some(exe.parent()?.join(name))
    });
    named.into_iter().chain(beside).find(|p| p.is_file())
}

/// Who the helper says would write a pack now: the translator, the legalizer and the pack's version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub translator: String,
    pub legalizer: String,
    pub pack_version: u32,
}

/// The last line a process said on a stream, to say why it failed.
fn last_line(text: &[u8]) -> Option<String> {
    String::from_utf8_lossy(text)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(windows)]
fn command(helper: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(helper);
    // A console window flashing up for the helper would be a window the editor did not mean to open
    command.creation_flags(0x0800_0000);
    command
}

#[cfg(not(windows))]
fn command(helper: &Path) -> Command {
    Command::new(helper)
}

/// Ask `helper` who would write a pack now; `Err` says why it cannot, such as that it was built without its translator.
pub fn identity_of(helper: &Path) -> Result<Identity, String> {
    let output = command(helper)
        .arg("pack-identity")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", helper.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(Event::Identity {
            translator,
            legalizer,
            pack_version,
        }) = Event::parse(line)
        {
            return Ok(Identity {
                translator,
                legalizer,
                pack_version,
            });
        }
    }
    Err(last_line(&output.stderr)
        .unwrap_or_else(|| format!("{} did not say who it is", helper.display())))
}

/// A cheap name for the install's build: the size and age of its executable, which a patch changes.
pub fn install_fingerprint(root: &Path) -> String {
    ["GTA5.exe", "GTA5_Enhanced.exe", "PlayGTAV.exe"]
        .iter()
        .find_map(|name| std::fs::metadata(root.join(name)).ok())
        .map_or_else(String::new, |meta| {
            let age = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            format!("{:x}-{age:x}", meta.len())
        })
}

fn read_pack(root: &Path) -> Option<SpirvPack> {
    serde_json::from_slice(&std::fs::read(root.join(MANIFEST_FILE)).ok()?).ok()
}

/// The pack at `root`, when it was written by who `identity` names, from an install of the build `fingerprint` names, and translated something; the number of programs it translated.
pub fn current_pack(root: &Path, identity: &Identity, fingerprint: &str) -> Option<usize> {
    let pack = read_pack(root)?;
    let current = pack.schema_version == identity.pack_version
        && pack.produced_by.translator == identity.translator
        && pack.produced_by.legalizer == identity.legalizer
        && pack.source_build_hash == fingerprint;
    let count = pack.translated();
    (current && count > 0).then_some(count)
}

/// Programs translated by the pack at `root`, whoever wrote it and whenever; `None` where it holds none.
pub fn translated_in(root: &Path) -> Option<usize> {
    let count = read_pack(root)?.translated();
    (count > 0).then_some(count)
}

enum Message {
    Event(Event),
    /// The helper exited: how, and the last it said on its error stream.
    Ended(Result<(), String>),
}

/// The helper running one bake.
struct Job {
    messages: Receiver<Message>,
    child: Arc<Mutex<Child>>,
    cancelled: bool,
    join: Option<JoinHandle<()>>,
}

/// The helper told to bake the pack of `install` into `root`, saying so a line at a time.
fn bake_command(helper: &Path, install: &Path, root: &Path, fingerprint: &str) -> Command {
    let mut bake = command(helper);
    bake.arg("bake-pack")
        .arg(install)
        .arg("--out")
        .arg(root)
        .arg("--build-hash")
        .arg(fingerprint)
        .arg("--progress");
    bake
}

impl Job {
    /// Run `command` as the bake, its output read as the helper's is.
    fn spawn(mut command: Command) -> Result<Self, String> {
        let program = command.get_program().to_string_lossy().into_owned();
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        let stdout = child.stdout.take().ok_or("the helper has no output")?;
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        let (sender, messages) = channel();
        let waiting = Arc::clone(&child);
        let join = std::thread::Builder::new()
            .name("game-shader-bake".into())
            .spawn(move || {
                // The error stream is read as it fills, or a helper that said much would stop on a full pipe
                let said = std::thread::spawn(move || {
                    let mut kept = Vec::new();
                    if let Some(stderr) = stderr {
                        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                            kept.push(line);
                            if kept.len() > 20 {
                                kept.remove(0);
                            }
                        }
                    }
                    kept
                });
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(event) = Event::parse(&line) {
                        let _ = sender.send(Message::Event(event));
                    }
                }
                let status = waiting
                    .lock()
                    .map_err(|_| "the helper could not be waited for".to_string())
                    .and_then(|mut child| child.wait().map_err(|e| e.to_string()));
                let said = said.join().unwrap_or_default();
                let ended = match status {
                    Ok(status) if status.success() => Ok(()),
                    Ok(status) => Err(said
                        .iter()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("the helper stopped with {status}"))),
                    Err(e) => Err(e),
                };
                let _ = sender.send(Message::Ended(ended));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            messages,
            child,
            cancelled: false,
            join: Some(join),
        })
    }

    /// Stop the helper, which ends its output and so the thread that reads it.
    fn cancel(&mut self) {
        self.cancelled = true;
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// The pack of the game's shaders: whether there is one, and the helper that bakes it.
pub struct GamePackController {
    root: PathBuf,
    /// Whether the pack is one the owner baked elsewhere and named, which is read and never baked over.
    named: bool,
    status: GamePackStatus,
    job: Option<Job>,
    auto_attempted: bool,
}

impl GamePackController {
    /// The pack under OpenRage's configuration `root`, or where `OPENRAGE_GAME_PACK` says one is.
    pub fn new(config_root: &Path) -> Self {
        match std::env::var_os(PACK_ENV) {
            Some(root) => Self::at(PathBuf::from(root), true),
            None => Self::at(config_root.join("shader-pack").join("game"), false),
        }
    }

    fn at(root: PathBuf, named: bool) -> Self {
        Self {
            root,
            named,
            status: GamePackStatus::Missing,
            job: None,
            auto_attempted: false,
        }
    }

    /// Where the pack is read from and baked to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn status(&self) -> &GamePackStatus {
        &self.status
    }

    pub fn is_baking(&self) -> bool {
        self.job.is_some()
    }

    /// The bake in flight, as the job list draws it.
    pub fn job(&self) -> Option<RunningJob> {
        let job = self.job.as_ref()?;
        let GamePackStatus::Baking { done, total } = self.status else {
            return None;
        };
        Some(RunningJob {
            kind: JobKind::GamePackBake,
            doing: "Baking the game's shaders for the viewport".into(),
            fraction: (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0)),
            detail: if total > 0 {
                format!("{done} of {total} programs")
            } else {
                "reading the install".into()
            },
            stopping: job.cancelled,
        })
    }

    pub fn cancel(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.cancel();
        }
    }

    /// Bake the pack of the install at `install` now, as the editor does once a game is set: not if one is running, and not if a bake was tried this run.
    pub fn maybe_auto_bake(&mut self, install: &Path) -> Option<GamePackEvent> {
        self.auto(|controller| controller.start(install, false))
    }

    /// Run `begin` as the one automatic bake of a run, which a bake in flight or one already tried this run puts off.
    fn auto(
        &mut self,
        begin: impl FnOnce(&mut Self) -> Option<GamePackEvent>,
    ) -> Option<GamePackEvent> {
        if self.is_baking() || self.auto_attempted {
            return None;
        }
        self.auto_attempted = true;
        begin(self)
    }

    /// Bake the pack of the install at `install`, even where the one on disk is current.
    pub fn rebake(&mut self, install: &Path) -> Option<GamePackEvent> {
        self.auto_attempted = true;
        self.cancel();
        self.job = None;
        self.start(install, true)
    }

    fn start(&mut self, install: &Path, force: bool) -> Option<GamePackEvent> {
        // ⚠ A bake takes the processor for a while and reads an install, so a self-check starts none
        if crate::platform::standing_in() {
            tracing::info!(
                "the game shader bake's stand-in was asked to bake {}",
                install.display()
            );
            return None;
        }
        self.start_with(helper_path(), install, force)
    }

    /// [`Self::start`] with the helper named, which a test stands in for.
    fn start_with(
        &mut self,
        helper: Option<PathBuf>,
        install: &Path,
        force: bool,
    ) -> Option<GamePackEvent> {
        // A pack named by the variable is the owner's to keep: it is read and never written over
        if self.named {
            self.status = match translated_in(&self.root) {
                Some(translated) => GamePackStatus::Ready { translated },
                None => GamePackStatus::Missing,
            };
            return None;
        }
        let Some(helper) = helper else {
            self.status =
                GamePackStatus::NoHelper("or-shader-tool is not beside the editor".into());
            return None;
        };
        let identity = match identity_of(&helper) {
            Ok(identity) => identity,
            Err(why) => {
                self.status = GamePackStatus::NoHelper(why);
                return None;
            }
        };
        let fingerprint = install_fingerprint(install);
        if !force {
            if let Some(translated) = current_pack(&self.root, &identity, &fingerprint) {
                self.status = GamePackStatus::Ready { translated };
                return Some(GamePackEvent::Ready {
                    root: self.root.clone(),
                    baked: false,
                });
            }
        }
        tracing::info!(root = %self.root.display(), "baking the game's shaders");
        match self.begin(bake_command(&helper, install, &self.root, &fingerprint)) {
            Ok(()) => None,
            Err(message) => Some(GamePackEvent::Failed { message }),
        }
    }

    /// Run `command` as the one bake, which a test stands in for as the job list's own test does.
    pub(super) fn begin(&mut self, command: Command) -> Result<(), String> {
        match Job::spawn(command) {
            Ok(job) => {
                self.status = GamePackStatus::Baking { done: 0, total: 0 };
                self.job = Some(job);
                Ok(())
            }
            Err(message) => {
                self.status = GamePackStatus::Failed {
                    message: message.clone(),
                };
                Err(message)
            }
        }
    }

    /// Take what the helper has said since the last look.
    pub fn poll(&mut self) -> Option<GamePackEvent> {
        let job = self.job.as_mut()?;
        let mut told = None;
        loop {
            match job.messages.try_recv() {
                Ok(Message::Event(Event::Progress { done, total })) => {
                    self.status = GamePackStatus::Baking { done, total };
                    told = Some(GamePackEvent::Progress);
                }
                Ok(Message::Event(_)) => {}
                Ok(Message::Ended(ended)) => {
                    let cancelled = job.cancelled;
                    self.job = None;
                    return Some(match (ended, cancelled) {
                        (_, true) => {
                            self.status = GamePackStatus::Missing;
                            GamePackEvent::Stopped
                        }
                        (Ok(()), false) => match translated_in(&self.root) {
                            Some(translated) => {
                                self.status = GamePackStatus::Ready { translated };
                                GamePackEvent::Ready {
                                    root: self.root.clone(),
                                    baked: true,
                                }
                            }
                            None => {
                                let message = "the helper wrote no pack".to_string();
                                self.status = GamePackStatus::Failed {
                                    message: message.clone(),
                                };
                                GamePackEvent::Failed { message }
                            }
                        },
                        (Err(message), false) => {
                            self.status = GamePackStatus::Failed {
                                message: message.clone(),
                            };
                            GamePackEvent::Failed { message }
                        }
                    });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = GamePackStatus::Failed {
                        message: "the helper's output ended without a word".into(),
                    };
                    return Some(GamePackEvent::Failed {
                        message: "the helper's output ended without a word".into(),
                    });
                }
            }
        }
        told
    }
}

impl Default for GamePackController {
    /// ⚠ Under `config_root`, which a self-check points at its scratch folder.
    fn default() -> Self {
        let root =
            super::prefs::config_root().unwrap_or_else(|| std::env::temp_dir().join("openrage"));
        Self::new(&root)
    }
}

#[cfg(test)]
mod tests;

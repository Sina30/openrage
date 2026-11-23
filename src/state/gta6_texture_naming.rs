//! The texture naming rules run on a worker as a row of the job list, a stem at a time with its
//! progress and Cancel: each stem a drawable states makes thousands of names to hash. The index of
//! texture packs by key runs the same way.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use or_job::{Cancel, Progress};

use super::jobs::{JobKind, Outcome, RunningJob, Then};
use super::AppState;

/// How a naming run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NamingEnded {
    /// How many keys it named.
    Named(usize),
    /// How many texture packs it indexed by their keys, and how many of them it read, the rest being as kept.
    Indexed(usize, usize),
    /// How many scanned texture dictionaries it indexed by their textures' keys.
    Dictionaries(usize),
    /// How many lines of a names file named a key.
    Loaded(usize),
    /// How many bodies of an install's archives it read for their textures, and how many keys it named.
    Harvested(usize, usize),
    /// A harvest of the names an install's archives spell: the names the vocabulary holds after
    /// it, those new to it, the bodies that state their own name, and the pack keys named.
    Names {
        names: usize,
        new: usize,
        labelled: usize,
        keys: usize,
    },
    Stopped,
}

pub(super) struct TextureNaming {
    doing: String,
    cancel: Cancel,
    progress: Progress,
    ended: Arc<Mutex<Option<NamingEnded>>>,
}

impl TextureNaming {
    /// Learn the names the rules give from `stems` for any of `keys`, on a worker.
    pub(super) fn start(stems: Vec<String>, keys: HashSet<u64>) -> Self {
        let doing = tr!("Naming GTA VI textures from their drawables' names").to_string();
        Self::run(doing, move |cancel, progress| {
            let mut named = 0;
            for (done, stem) in stems.iter().enumerate() {
                if cancel.is_set() {
                    return NamingEnded::Stopped;
                }
                progress.reached(done as u64, stems.len() as u64);
                named += super::ttpf_document::learn_rule_names(stem, &keys);
            }
            NamingEnded::Named(named)
        })
    }

    /// Run `work` on a worker as a naming run, which the job list says is `doing`.
    pub(super) fn run(
        doing: String,
        work: impl FnOnce(&Cancel, &Progress) -> NamingEnded + Send + 'static,
    ) -> Self {
        let naming = Self {
            doing,
            cancel: Cancel::new(),
            progress: Progress::new(),
            ended: Arc::default(),
        };
        let (cancel, progress, ended) = (
            naming.cancel.clone(),
            naming.progress.clone(),
            Arc::clone(&naming.ended),
        );
        std::thread::spawn(move || {
            let outcome = work(&cancel, &progress);
            *ended.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
        });
        naming
    }

    fn job(&self) -> Option<RunningJob> {
        if self.ended().is_some() {
            return None;
        }
        Some(RunningJob {
            kind: JobKind::TextureNames,
            doing: self.doing.clone(),
            fraction: Some(self.progress.snapshot().fraction()),
            detail: String::new(),
            stopping: self.cancel.is_set(),
        })
    }

    fn ended(&self) -> Option<NamingEnded> {
        *self.ended.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Drop for TextureNaming {
    fn drop(&mut self) {
        self.cancel.set();
    }
}

impl AppState {
    /// The naming runs still running, as the job list draws them.
    pub(super) fn texture_naming_jobs(&self) -> impl Iterator<Item = RunningJob> + '_ {
        self.texture_naming.iter().filter_map(TextureNaming::job)
    }

    /// Ask every naming run to stop; each says it stopped once its worker sees the stop.
    pub(super) fn stop_texture_naming(&self) {
        self.texture_naming.iter().for_each(|n| n.cancel.set());
    }

    /// Say in the job list how each naming run that ended went, and drop it.
    pub(super) fn poll_texture_naming(&mut self) {
        let mut ended = Vec::new();
        self.texture_naming.retain(|n| match n.ended() {
            Some(e) => {
                ended.push(e);
                false
            }
            None => true,
        });
        for e in ended {
            let (outcome, said) = match e {
                NamingEnded::Named(count) => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI texture key named from a drawable's name",
                        "{count} GTA VI texture keys named from drawables' names",
                        count
                    ),
                ),
                NamingEnded::Indexed(count, read) if read < count => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI texture pack indexed by its keys, {read} read and the rest from the kept index",
                        "{count} GTA VI texture packs indexed by their keys, {read} read and the rest from the kept index",
                        count,
                        read = read
                    ),
                ),
                NamingEnded::Indexed(count, _) => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI texture pack indexed by its keys",
                        "{count} GTA VI texture packs indexed by their keys",
                        count
                    ),
                ),
                NamingEnded::Dictionaries(count) => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI texture dictionary indexed by its textures' keys",
                        "{count} GTA VI texture dictionaries indexed by their textures' keys",
                        count
                    ),
                ),
                NamingEnded::Loaded(count) => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI texture key named from the names file",
                        "{count} GTA VI texture keys named from the names file",
                        count
                    ),
                ),
                NamingEnded::Harvested(count, named) => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI body read for its textures' keys and names, {named} keys named",
                        "{count} GTA VI bodies read for their textures' keys and names, {named} keys named",
                        count,
                        named = named
                    ),
                ),
                NamingEnded::Names {
                    names,
                    new,
                    labelled,
                    keys,
                } => (
                    Outcome::Done(Then::Nothing),
                    tr_n!(
                        "{count} GTA VI name held, {new} new; {labelled} bodies state their own name and {keys} texture keys are named",
                        "{count} GTA VI names held, {new} new; {labelled} bodies state their own name and {keys} texture keys are named",
                        names,
                        new = new,
                        labelled = labelled,
                        keys = keys
                    ),
                ),
                NamingEnded::Stopped => (
                    Outcome::Stopped(Then::Nothing),
                    tr!("Stopped before it finished").to_string(),
                ),
            };
            self.jobs.record(JobKind::TextureNames, outcome, said);
        }
    }
}

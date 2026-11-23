//! The recent projects as the start page's cards: what each holds and how it is built.
//!
//! ⚠ **Read on a worker.** A project file may sit on a network drive, so the start page draws what
//! the last read left and asks for another when the list changes.

use crate::state::front_door::KindWords as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use or_project::{build_root, built_resources, ModuleKind, Project};

use super::words::counted;
use crate::state::workbench::files_held;
use crate::ui::task_screens::{self, TaskScreen};

/// One recent project as the start page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub path: PathBuf,
    pub name: String,
    /// The project file is there.
    pub found: bool,
    /// What it holds, one badge each: `Files (12)`.
    pub content: Vec<String>,
    /// How its builds stand, in a few words.
    pub builds: String,
    /// Why the file could not be read, when it could not.
    pub unreadable: Option<String>,
}

/// Read the card of the project at `path`.
#[cfg(test)]
pub fn read_card(path: &Path) -> Card {
    read_card_for(path, &task_screens::all())
}

/// Read the card of the project at `path`, saying the content of each of `screens` in the words of its row.
fn read_card_for(path: &Path, screens: &[&TaskScreen]) -> Card {
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let mut card = Card {
        path: path.to_path_buf(),
        name,
        found: path.is_file(),
        content: Vec::new(),
        builds: String::new(),
        unreadable: None,
    };
    if !card.found {
        return card;
    }
    match Project::load(path) {
        Ok(project) => {
            // ⚠ The rows the Assets table lists, counted by the rule the scan reads them by.
            let members: Vec<PathBuf> = (project.members.iter())
                .filter(|member| !matches!(member.kind.as_deref(), Some("GameRoot" | "Folder")))
                .map(|member| member.path.clone())
                .collect();
            let held = files_held(
                &project.container,
                &or_project::project_folder(path),
                &members,
            );
            for module in &project.container.modules {
                let screen = screens.iter().find(|row| row.kind == module.kind);
                card.content.push(match (&module.kind, screen) {
                    (_, Some(screen)) => badge(screen, path, module),
                    (ModuleKind::Files, _) => tr!("Files · {held}", held = counted(held, "file")),
                    (ModuleKind::AddonPed, _) if module.is_link() => {
                        tr!("Addon ped · linked").to_string()
                    }
                    (ModuleKind::AddonPed, _) => match peds_of(path, module) {
                        Some(n) => tr!("Addon ped · {n}", n = counted(n, "ped")),
                        None => tr!("Addon ped").to_string(),
                    },
                    (ModuleKind::Unknown(word), _) => word.clone(),
                    // A kind with a screen has its row above.
                    (kind, _) => kind.words(),
                });
            }
            card.builds = build_state(path, &project.container.outputs);
        }
        Err(e) => card.unreadable = Some(e.to_string()),
    }
    card
}

/// What a card says of a content that has a screen of its own: its name, and how many it holds as the disk says, or that it is linked.
fn badge(screen: &TaskScreen, project: &Path, module: &or_project::Module) -> String {
    if module.is_link() {
        return tr!(
            "{content} · linked",
            content = crate::i18n::tr(screen.content)
        );
    }
    match (screen.on_disk)(project, module) {
        Some(n) => format!(
            "{} · {}",
            crate::i18n::tr(screen.content),
            counted(n, screen.count)
        ),
        None => crate::i18n::tr(screen.content).to_string(),
    }
}

/// How the builds of a project stand: written to `build/` with their marker, or not.
fn build_state(project: &Path, outputs: &[or_project::Output]) -> String {
    if outputs.is_empty() {
        return tr!("No build declared").into();
    }
    let built = built_resources(&build_root(project)).unwrap_or_default();
    let written = outputs
        .iter()
        .filter(|output| built.iter().any(|name| is_resource_of(name, &output.name)))
        .count();
    match (written, outputs.len()) {
        (0, _) => tr!("Not built").into(),
        (n, total) if n == total => tr!("Built").into(),
        (n, total) => tr!("{n} of {total} builds written", n = n, total = total),
    }
}

/// Whether the built resource `built` is a build of the output `output`: its own name, or `<name>_1`, `<name>_2`… when it split.
///
/// ⚠ One part written reads as written: how many parts there are is in the plan, which reads every source, and a card reads only the folder.
fn is_resource_of(built: &str, output: &str) -> bool {
    built == output
        || built
            .strip_prefix(output)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The cards of the recent projects, read off the UI thread.
#[derive(Default)]
pub struct Cards {
    cards: Vec<Card>,
    /// The paths the cards were read for.
    read_for: Vec<PathBuf>,
    reading: Option<Receiver<Vec<Card>>>,
    asked_for: Vec<PathBuf>,
    /// The disk may have changed since: read again.
    stale: bool,
}

impl Cards {
    /// Ask for the next read to see the disk as it is now.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// The card of each of `paths` that has been read, in the order of `paths`; a read starts when the list changed or the disk may have.
    pub fn of(&mut self, paths: &[PathBuf]) -> Vec<Option<&Card>> {
        if let Some(Ok(cards)) = self.reading.as_ref().map(Receiver::try_recv) {
            self.reading = None;
            self.read_for = std::mem::take(&mut self.asked_for);
            self.cards = cards;
        }
        let changed = self.read_for != paths && self.asked_for != paths;
        if self.reading.is_none() && (changed || self.stale) {
            self.stale = false;
            self.asked_for = paths.to_vec();
            let (tx, rx) = channel();
            let wanted = paths.to_vec();
            // ⚠ The rows are taken here and not on the worker, which is no thread a row was added on.
            let screens = task_screens::all();
            std::thread::spawn(move || {
                let _ = tx.send(
                    (wanted.iter())
                        .map(|path| read_card_for(path, &screens))
                        .collect(),
                );
            });
            self.reading = Some(rx);
        }
        paths
            .iter()
            .map(|path| self.cards.iter().find(|card| &card.path == path))
            .collect()
    }

    /// Whether a read is under way, so the page asks to be drawn again.
    pub fn is_reading(&self) -> bool {
        self.reading.is_some()
    }
}

/// How many peds the model of an addon ped content holds, none when its file cannot be read.
fn peds_of(project: &Path, module: &or_project::Module) -> Option<usize> {
    let folder = or_addonped::module_folder(&or_project::project_folder(project), module).ok()?;
    or_addonped::AddonPeds::read(&folder)
        .ok()
        .map(|model| model.peds.len())
}

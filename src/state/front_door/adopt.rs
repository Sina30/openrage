//! Make this a project: the offer Open folder makes, and what accepting it does.
//!
//! ⚠ **It adds a project file, and a `wardrobe.json` where it finds a clothing pack, and moves
//! nothing.** The folder keeps every file where it is, as it is; the project file is written beside
//! them and holds what is not clothing, and the content it recognised is grouped and listed.

use crate::state::front_door::KindWords as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use or_project::adopt::{analyse, Recogniser, Unplaced};

use super::new_project::project_file_in;
use super::new_project::Created;
use super::words::{counted, spoken};
use super::NewProject;
use crate::state::{AppState, DocumentKind};

/// How long what was read of a folder is trusted.
const TRUSTED: Duration = Duration::from_secs(3);

/// What the banner over an opened folder offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The folder is not a project: make it one.
    Make(PathBuf),
    /// The folder already holds this project file: open that.
    Open(PathBuf),
}

impl AppState {
    /// What can be done with the active document, which is an opened folder while no project is open.
    pub fn folder_offer(&self) -> Option<Offer> {
        if self.project_path.is_some() {
            return None;
        }
        let doc = self.active_document()?;
        if doc.kind != DocumentKind::Folder {
            return None;
        }
        let folder = doc.path.clone()?;
        Some(match self.project_held_by(&folder) {
            Some(held) => Offer::Open(held),
            None => Offer::Make(folder),
        })
    }

    /// The project file `folder` holds, read from the disk at most once in [`TRUSTED`] so a frame reads none.
    fn project_held_by(&self, folder: &Path) -> Option<PathBuf> {
        let mut seen = self.front_door.held.borrow_mut();
        if let Some((at, held)) = seen.get(folder) {
            if at.elapsed() < TRUSTED {
                return held.clone();
            }
        }
        let held = project_file_in(folder);
        seen.insert(folder.to_path_buf(), (Instant::now(), held.clone()));
        held
    }

    /// What the banner over the opened folder offers, until it is declined in this session.
    pub fn adoption_offer(&self) -> Option<Offer> {
        self.folder_offer().filter(|offer| match offer {
            Offer::Make(folder) => !self.front_door.declined.contains(folder),
            Offer::Open(project) => project
                .parent()
                .is_none_or(|folder| !self.front_door.declined.contains(folder)),
        })
    }

    /// Stop offering to make `folder` a project in this session.
    pub fn decline_adoption(&mut self, folder: &Path) {
        self.front_door.declined.insert(folder.to_path_buf());
    }

    /// Write a project file in `folder` that holds its files, and open the project, once a worker has analysed the folder.
    pub fn make_folder_a_project(&mut self, folder: &Path) -> Result<(), String> {
        let draft = NewProject::adopting(folder);
        if let Some(why) = draft.refusal() {
            return Err(why);
        }
        self.start_creating(draft);
        Ok(())
    }

    /// Tell the person what an adopted folder was recognised as and what could not be placed.
    pub(super) fn say_what_was_made(&mut self, created: &Created) {
        if created.truncated {
            self.show_warning_toast(tr!(
                "Part of the folder is not in the project: Properties lists what was left out"
            ));
        }
        if !created.found.is_empty() || created.unplaced > 0 {
            let found = if created.found.is_empty() {
                tr!("no content but files").to_string()
            } else {
                spoken(&created.found.join("; "))
            };
            let left = match created.unplaced {
                0 => String::new(),
                n => tr_n!(
                    " {files} could not be placed; Properties lists it",
                    " {files} could not be placed; Properties lists them",
                    n,
                    files = counted(n, "file")
                ),
            };
            self.show_info_toast(tr!("Adopted: {found}.{left}", found = found, left = left));
        }
    }
}

/// The kinds of content a folder is recognised as.
pub fn recognisers() -> [&'static dyn Recogniser; 5] {
    or_content::recognisers()
}

/// What a folder holds, as the dialog and the banner say it before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub folder: PathBuf,
    /// What was recognised, one line each: `Clothing: 12 model(s)…`.
    pub found: Vec<String>,
    /// The files that stay Files content.
    pub files: usize,
    pub unplaced: Vec<Unplaced>,
    pub notes: Vec<String>,
    /// The files the adoption adds beside the folder's own, inside the folder with `/`.
    pub adds: Vec<String>,
}

/// Read what `folder` holds, which writes nothing.
pub fn read_preview(folder: &Path) -> Preview {
    let adoption = analyse(folder, &recognisers());
    Preview {
        folder: folder.to_path_buf(),
        found: adoption
            .found
            .iter()
            .map(|f| spoken(&format!("{}: {}", f.module.kind.words(), f.summary)))
            .collect(),
        files: adoption.files.len(),
        adds: adoption
            .writes
            .iter()
            .filter_map(|w| w.path.strip_prefix(folder).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect(),
        unplaced: (adoption.unplaced.into_iter())
            .map(|left| Unplaced {
                why: spoken(&left.why),
                ..left
            })
            .collect(),
        notes: adoption.notes.iter().map(|note| spoken(note)).collect(),
    }
}

impl Preview {
    /// The one line the banner over an opened folder says.
    pub fn headline(&self) -> String {
        let mut parts: Vec<String> = self
            .found
            .iter()
            .map(|f| f.split(':').next().unwrap_or_default().to_string())
            .collect();
        if self.files > 0 || parts.is_empty() {
            parts.push(tr!("Files ({files})", files = self.files));
        }
        let mut line = tr!("Found: {parts}", parts = parts.join(", "));
        if !self.unplaced.is_empty() {
            line.push_str(&tr!(
                "; {count} not placed",
                count = counted(self.unplaced.len(), "file")
            ));
        }
        line
    }
}

/// The preview of the folder last asked for, read off the UI thread.
///
/// ⚠ A folder of a whole pack has thousands of files, so the dialog and the banner draw what the
/// last read left and ask for another when the folder changes.
#[derive(Default)]
pub struct Previews {
    ready: Option<Preview>,
    reading: Option<(PathBuf, Receiver<Preview>)>,
}

impl Previews {
    /// The preview of `folder` once it has been read; a read starts when it is not the folder last asked for.
    pub fn of(&mut self, folder: &Path) -> Option<&Preview> {
        if let Some((asked, receiver)) = &self.reading {
            if let Ok(preview) = receiver.try_recv() {
                if preview.folder == *asked {
                    self.ready = Some(preview);
                }
                self.reading = None;
            }
        }
        let ready_for = self.ready.as_ref().map(|p| p.folder.as_path());
        let reading_for = self.reading.as_ref().map(|(p, _)| p.as_path());
        if ready_for != Some(folder) && reading_for != Some(folder) {
            let (tx, rx) = channel();
            let wanted = folder.to_path_buf();
            self.reading = Some((wanted.clone(), rx));
            std::thread::spawn(move || {
                let _ = tx.send(read_preview(&wanted));
            });
        }
        self.ready.as_ref().filter(|p| p.folder == folder)
    }

    /// Whether a read is under way, so the panel asks to be drawn again.
    pub fn is_reading(&self) -> bool {
        self.reading.is_some()
    }

    /// Forget what was read: the folder has changed on disk.
    pub fn forget(&mut self) {
        self.ready = None;
    }
}

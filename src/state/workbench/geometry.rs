//! Replacing the geometry of a drawable or drawables source from a glTF, GLB or FBX, and the report of what the drawable had no place for.
//!
//! ⚠ **Nothing here converts or compiles.** `or_source::models::import` does, and it writes nothing
//! unless the source still compiles with the model in it; this picks the drawable of a dictionary,
//! runs the import off the UI thread, says what came of it and hands the folder to the cook that is
//! already watching the project, whose recook then refreshes every tab open on the cooked file.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use or_source::asset::{Source, MANIFEST};
use or_source::models::{self, Imported};
use or_source::Problem;

use super::read_only::READ_ONLY;
use super::scan::{AssetRow, Kept, Seen};
use super::{leaf, Question};
use crate::state::AppState;

struct Running {
    /// The source's folder.
    source: PathBuf,
    name: String,
    /// The model file, as the report names it.
    from: String,
    receiver: Receiver<Result<Imported, Vec<Problem>>>,
}

/// The import of a model into a source that is running.
#[derive(Default)]
pub struct GeometryWork {
    running: Option<Running>,
}

impl GeometryWork {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

impl AppState {
    /// Why the geometry of `row` cannot be replaced now, none when it can.
    pub fn replace_geometry_refused(&self, row: &AssetRow) -> Option<&'static str> {
        if self.workbench.held_by_another.is_some() {
            return Some(crate::i18n::tr(READ_ONLY));
        }
        let is_a_model = matches!(row.source_kind.as_deref(), Some("drawable" | "drawables"));
        if row.kept != Kept::Source || !is_a_model {
            return Some(tr!(
                "Only a drawable or a drawables source has geometry to replace"
            ));
        }
        if matches!(row.seen, Seen::Unreadable(_)) {
            return Some(tr!("This source cannot be read: Problems says why"));
        }
        self.workbench
            .geometry
            .is_running()
            .then_some(tr!("A model is already being put into a source"))
    }

    /// Put the model in `file` into the source at `key`: a dictionary with several drawables first asks which one.
    pub fn start_replacing_geometry(&mut self, key: &str, file: PathBuf) {
        let Some(row) = self.workbench.row(key) else {
            return;
        };
        let (folder, refused) = (row.path.clone(), self.replace_geometry_refused(row));
        let dictionary = row.source_kind.as_deref() == Some("drawables");
        if let Some(why) = refused {
            self.show_info_toast(why);
            return;
        }
        let entries = Source::open(&folder).and_then(|source| models::entries(&source));
        match entries {
            Err(problem) => self.show_error_toast(problem.to_string()),
            Ok(entries) if entries.is_empty() && dictionary => {
                self.show_error_toast(tr!("The dictionary lists no drawable to put a model into"));
            }
            Ok(entries) if entries.is_empty() => self.replace_geometry(key, None, file),
            Ok(mut entries) if entries.len() == 1 => {
                self.replace_geometry(key, Some(entries.remove(0)), file);
            }
            Ok(entries) => {
                self.workbench.asking = Some(Question::ReplaceGeometry {
                    key: key.to_string(),
                    file,
                    entries,
                    chosen: 0,
                });
            }
        }
    }

    /// Import `file` into the source at `key`, into the drawable `entry` of a dictionary, on a worker.
    pub fn replace_geometry(&mut self, key: &str, entry: Option<String>, file: PathBuf) {
        let Some(row) = self.workbench.row(key) else {
            return;
        };
        if let Some(why) = self.replace_geometry_refused(row) {
            self.show_info_toast(why);
            return;
        }
        let (source, name) = (row.path.clone(), row.name.clone());
        let from = leaf(&file);
        let (tx, receiver) = channel();
        let folder = source.clone();
        std::thread::spawn(move || {
            let done = Source::open(&folder)
                .map_err(|problem| vec![problem])
                .and_then(|source| models::import(&source, entry.as_deref(), &file));
            let _ = tx.send(done);
        });
        self.workbench.note(tr!(
            "Putting {from} into the geometry of {name}",
            from = from,
            name = name
        ));
        self.workbench.geometry.running = Some(Running {
            source,
            name,
            from,
            receiver,
        });
    }

    /// Take the report of the import that ended: the cook is asked for, and a window says what the drawable had no place for or why nothing changed.
    pub(super) fn poll_geometry(&mut self) {
        let Some(running) = self.workbench.geometry.running.take() else {
            return;
        };
        let done = match running.receiver.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => {
                self.workbench.geometry.running = Some(running);
                return;
            }
            // ⚠ A worker that panicked drops its sender: the import writes a file only after it compiled, whole or not at all.
            Err(TryRecvError::Disconnected) => Err(vec![Problem::new(tr!(
                "the import stopped before it was done, and the source is as it was"
            ))]),
        };
        let Running {
            source, name, from, ..
        } = running;
        let (replaced, lines) = match done {
            Ok(imported) => (true, imported.lines()),
            Err(problems) => (false, problems.iter().map(Problem::to_string).collect()),
        };
        if replaced {
            // ⚠ Dated a second ago, so the cook's quiet time has already passed.
            let then = Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now);
            self.workbench.file_saved(source.join(MANIFEST), then);
            self.workbench.note(tr!(
                "Replaced the geometry of {name} from {from}",
                name = name,
                from = from
            ));
            self.workbench.want_scan();
        } else {
            let first = lines.first().cloned().unwrap_or_default();
            self.workbench.note(tr!(
                "The geometry of {name} was not replaced from {from}: {first}",
                name = name,
                from = from,
                first = first
            ));
        }
        if self.workbench.asking.is_none() {
            self.workbench.asking = Some(Question::Geometry {
                name,
                from,
                replaced,
                lines,
            });
        } else if !replaced {
            self.show_error_toast(tr!(
                "{name} was not changed: {lines}",
                name = name,
                lines = lines.join("; ")
            ));
        }
    }
}

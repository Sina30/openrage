//! The import job of a content that keeps its sources in a folder of its own (particles, weapons): game files made sources of the pack
//! one at a time, or one new source, as the import worker runs them, so each is one step of History that Undo takes back with the
//! sources it made and the place they had in the pack, and Redo makes again.
//!
//! ⚠ A pack's model (`effects.json`, `weapons.json`) is not part of the project file, so the History does not hold it: what Undo takes
//! away is the source folders, and this puts the model right after (`forget_removed_pack_sources`), as Redo puts it right after the
//! folders come back (`adopt_restored_pack_sources`).

use std::path::{Path, PathBuf};

use or_project::ModuleKind;
use or_source::game_import::{tree_digest, Created, FileReport, LeftOut, Report, Verdict, Watch};

use crate::state::AppState;

/// How many files a stopped import lists as not reached, as `or_source::game_import` lists them.
const LISTED: usize = 1_000;

/// What the job does to the pack.
pub(super) enum PackJob {
    /// Make a source of each of these game files, which join the set `into` names where the content has sets.
    Add {
        files: Vec<PathBuf>,
        into: Option<String>,
    },
    /// Make one new source called this.
    New(String),
}

/// Run `job` on the pack of the `kind` of content in `folder`.
pub(super) fn run(kind: &ModuleKind, folder: &Path, job: PackJob, watch: &Watch<'_>) -> Report {
    match (kind, job) {
        (ModuleKind::Particles, PackJob::Add { files, .. }) => {
            super::particles::add_to_pack(folder, &files, watch)
        }
        (ModuleKind::Particles, PackJob::New(name)) => super::particles::new_in_pack(folder, &name),
        (ModuleKind::Weapons, PackJob::Add { files, .. }) => {
            super::weapons::add_to_pack(folder, &files, watch)
        }
        (ModuleKind::Maps, PackJob::Add { files, into }) => {
            super::maps::add_to_pack(folder, &files, into.as_deref(), watch)
        }
        (ModuleKind::Weapons, PackJob::New(name)) => super::weapons::new_in_pack(folder, &name),
        (other, _) => {
            let mut report = Report::default();
            report
                .notes
                .push(format!("{other:?} content has no import job"));
            report
        }
    }
}

/// Run `one` for each of `files` with its name, stopping between files when the watch says so; what was made stays and the rest is listed.
pub(super) fn each_file(
    files: &[PathBuf],
    watch: &Watch<'_>,
    mut one: impl FnMut(&Path, &str, &mut Report),
) -> Report {
    let mut report = Report::default();
    for (done, path) in files.iter().enumerate() {
        if watch.cancelled.is_some_and(|stop| stop()) {
            let rest = &files[done..];
            report.cancelled = true;
            report.notes.push(format!(
                "Stopped after {done} of {} file(s): the {} not reached were not imported; the first {} are listed",
                files.len(),
                rest.len(),
                rest.len().min(LISTED)
            ));
            report
                .left_out
                .extend(rest.iter().take(LISTED).map(|left| LeftOut {
                    path: left.display().to_string(),
                    why: "was not reached before the import was stopped".to_string(),
                }));
            break;
        }
        let name =
            (path.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if let Some(say) = watch.progress {
            say(done, files.len(), &name);
        }
        one(path, &name, &mut report);
    }
    report
}

/// The source folder `dir` that the file `input` became.
pub(super) fn made(report: &mut Report, input: &str, dir: PathBuf) {
    match tree_digest(&dir) {
        Ok(digest) => {
            let source = dir
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            report.files.push(FileReport {
                input: input.to_string(),
                verdict: Verdict::Made {
                    folder: dir.clone(),
                },
                reason: format!("became {source}"),
            });
            report.created.push(Created { path: dir, digest });
        }
        Err(e) => report.notes.push(format!(
            "{} was made but cannot be read, so Undo cannot take it back: {e}",
            dir.display()
        )),
    }
}

/// The file `input` that could not become a source, and why.
pub(super) fn refused(report: &mut Report, input: &str, why: String) {
    report.files.push(FileReport {
        input: input.to_string(),
        verdict: Verdict::Refused,
        reason: why,
    });
}

/// The source folders of `folder` named in `paths`: only a direct child with a `.src` name is one a pack holds.
pub(super) fn pack_sources<'p>(
    folder: &Path,
    paths: impl IntoIterator<Item = &'p PathBuf>,
) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|path| {
            let inside = path.strip_prefix(folder).ok()?.to_str()?;
            (inside.to_ascii_lowercase().ends_with(".src") && !inside.contains(['/', '\\']))
                .then(|| inside.to_string())
        })
        .collect()
}

impl AppState {
    /// A source folder that Undo took away is no source of the pack that named it: each pack goes back to what it held before.
    pub(super) fn forget_removed_pack_sources(&mut self, created: &[Created]) {
        self.forget_removed_particle_sources(created);
        self.forget_removed_weapon_sources(created);
        self.forget_removed_map_sources(created);
    }

    /// A source folder that Redo put back is a source of its pack again.
    pub(super) fn adopt_restored_pack_sources(&mut self, restored: &[PathBuf]) {
        self.adopt_restored_particle_sources(restored);
        self.adopt_restored_weapon_sources(restored);
        self.adopt_restored_map_sources(restored);
    }
}

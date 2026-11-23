//! A member of the project that is gone: how Locate asks where it is now, and how a copy of a source is given an id of its own.

use std::path::{Path, PathBuf};

use super::{leaf, Kept};
use crate::platform::file_dialogs;
use crate::state::project::SOURCE_ID;
use crate::state::AppState;

/// Whether a member called like `path` is a `.src` folder, which is looked for with a folder picker where any other member is a file.
pub fn asks_for_a_folder(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| or_source::asset::parse_folder_name(name).is_some())
}

impl AppState {
    /// Point a member whose source folder is gone at the folder of the same id found elsewhere in the project, once for each scan that changed the rows.
    ///
    /// ⚠ The id is the source's own, which a rename or a move never changes; the path stays the fallback. A folder two sources share the id of, or one another member already names, is left to Locate…, since which one moved is not known.
    pub(super) fn follow_moved_members(&mut self) {
        let generation = self.workbench.generation;
        // ⚠ A window that only reads the project changes nothing of it, and the member stays missing there with its Locate…
        if self.workbench.scan.is_none()
            || self.workbench.followed == Some(generation)
            || self.workbench.held_by_another.is_some()
        {
            return;
        }
        self.workbench.followed = Some(generation);
        let held = self.member_paths();
        let rows = self.workbench.rows();
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        for member in &self.project_carried.members {
            if member.path.exists() {
                continue;
            }
            let Some(id) = member.extra.get(SOURCE_ID).and_then(|id| id.as_str()) else {
                continue;
            };
            let mut found = rows.iter().filter(|r| {
                r.kept == Kept::Source && r.id.as_deref() == Some(id) && r.path != member.path
            });
            if let (Some(row), None) = (found.next(), found.next()) {
                if !held.contains(&row.path) {
                    moves.push((member.path.clone(), row.path.clone()));
                }
            }
        }
        if moves.is_empty() {
            return;
        }
        for (old, new) in &moves {
            if let Some(member) = (self.project_carried.members.iter_mut()).find(|m| m.path == *old)
            {
                member.path.clone_from(new);
            }
        }
        self.workbench.want_scan();
        let names: Vec<String> = moves.iter().map(|(old, _)| leaf(old)).collect();
        self.record_step(match moves.len() {
            1 => tr!(
                "The project followed {names} to its new place",
                names = names.join(", ")
            ),
            n => tr!(
                "The project followed {names} to their {count} new places",
                names = names.join(", "),
                count = n
            ),
        });
    }

    /// Ask where the member at `old` is now and point the project at it: a `.src` folder asks for a folder, any other member for a file.
    pub fn locate_member_asking(&mut self, old: &Path) {
        let picked = if asks_for_a_folder(old) {
            file_dialogs::pick_source_folder()
        } else {
            file_dialogs::open_any_files().and_then(|files| files.into_iter().next())
        };
        if let Some(new) = picked {
            self.locate_member(old, new);
        }
    }

    /// Give the source in `folder`, which is a copy of another or shares its id, an id of its own.
    pub fn reid_source(&mut self, folder: &Path) {
        if self.refused_as_read_only() {
            return;
        }
        match or_source::asset::reid(folder) {
            Ok(_) => {
                self.workbench
                    .note(tr!("Gave {folder} a new id", folder = leaf(folder)));
                self.workbench.want_scan();
            }
            Err(problem) => self.show_error_toast(tr!(
                "Could not give {folder} a new id: {message}",
                folder = leaf(folder),
                message = problem.message
            )),
        }
    }
}

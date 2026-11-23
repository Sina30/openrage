//! The workspace a project was left on, kept in its private folder so the project reopens there and no machine's choice is written into the file that is shared.

use std::path::Path;

use or_project::snapshot::private_dir;

use crate::state::AppState;
use crate::ui::dock::Workspace;

const FILE: &str = "workspace.json";

/// The workspace the project at `project` was left on, when it was left on one and the file reads.
pub(in crate::state) fn remembered(project: &Path) -> Option<Workspace> {
    let bytes = std::fs::read(private_dir(project).join(FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

impl super::Workbench {
    /// Note the workspace the project opened on, so only a change from it is kept.
    pub(in crate::state) fn opened_on(&mut self, workspace: Workspace) {
        self.workspace_seen = Some(workspace);
    }
}

impl AppState {
    /// Keep the workspace on screen as the one the open project is left on, once it changes from the one it opened on.
    ///
    /// ⚠ The first look only notes the workspace: opening a project writes nothing. A folder that cannot be written is let be, since this is a convenience.
    pub(in crate::state) fn remember_workspace(&mut self) {
        let Some(project) = self.project_path.as_ref() else {
            return;
        };
        if self.workbench.held_by_another.is_some() {
            return;
        }
        let now = self.active_workspace;
        match self.workbench.workspace_seen.replace(now) {
            None => {}
            Some(before) if before == now => {}
            Some(_) => {
                let private = private_dir(project);
                let written = serde_json::to_vec(&now)
                    .map_err(std::io::Error::other)
                    .and_then(|bytes| {
                        std::fs::create_dir_all(&private)?;
                        // ⚠ Everything in the folder is this machine's, so the folder ignores itself.
                        let ignore = private.join(".gitignore");
                        if !ignore.exists() {
                            std::fs::write(ignore, b"*\n")?;
                        }
                        std::fs::write(private.join(FILE), bytes)
                    });
                if let Err(why) = written {
                    tracing::warn!(%why, "the workspace the project was left on was not kept");
                }
            }
        }
    }
}

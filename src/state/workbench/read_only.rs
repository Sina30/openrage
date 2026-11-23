//! A project another window has open is read here and written there: the lock, the sentence that says so, and the refusal every change of the project makes.

use crate::state::AppState;

/// What a change of the project says when it is refused here and the window that holds it is not named.
pub const READ_ONLY: &str = tr_key!(
    "This project is open in another OpenRage window, which writes it; this window only reads it"
);

impl AppState {
    /// Take the right to write the project that has just been opened, or note that another window has it.
    ///
    /// ⚠ After the workbench started over for the project: that is what lets go of the lock of the project this window had.
    pub(in crate::state) fn hold_the_open_project(&mut self) {
        let Some(project) = self.project_path.clone() else {
            return;
        };
        // ⚠ No folder to keep the lock file in is no lock, as a lock the system cannot give is: the window writes.
        let Some(locks) = crate::state::prefs::config_root().map(|root| root.join("locks")) else {
            return;
        };
        match or_project::lock::acquire(&project, &locks) {
            Ok(lock) => self.workbench.hold(lock),
            Err(held) => {
                self.workbench.held_by_another = Some(held.to_string());
                self.show_warning_toast(tr!("{held}. Nothing can be changed here", held = held));
            }
        }
    }

    /// Why this window may not change the open project, when another window has it open.
    pub fn read_only_reason(&self) -> Option<String> {
        self.workbench.held_by_another.clone()
    }

    /// Refuse a change of the project in a window that only reads it, saying why; true when it was refused.
    pub(in crate::state) fn refused_as_read_only(&mut self) -> bool {
        match self.read_only_reason() {
            Some(why) => {
                self.show_warning_toast(why);
                true
            }
            None => false,
        }
    }
}

impl super::Workbench {
    fn hold(&mut self, lock: or_project::lock::ProjectLock) {
        self.project_lock = Some(lock);
        self.held_by_another = None;
    }
}

#[cfg(test)]
impl super::Workbench {
    /// Make this window read the project only, as if another window held it for `why`.
    pub(in crate::state) fn held_elsewhere_for_a_test(&mut self, why: &str) {
        self.held_by_another = Some(why.to_string());
    }
}

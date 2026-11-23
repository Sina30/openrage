//! A second window: another OpenRage process started on a project.
//!
//! One project lives in one window, so a second project opens in a second process that shares the
//! preferences file with the first (the save merges, see `state::prefs`).

use std::path::Path;
use std::process::Command;

/// The command that starts the program at `exe` on `project`, which it opens as a launch does.
///
/// ⚠ The project is the only argument: a launch with a path named never reopens the last project, so
/// the new window does not open the project the first window has open.
pub fn command(exe: &Path, project: &Path) -> Command {
    let mut command = Command::new(exe);
    command.arg(project);
    command
}

/// Start another OpenRage on `project`; the error says why it did not start.
///
/// ⚠ A test build and a self-check tell [`fake`] instead, so neither starts a program.
pub fn open(project: &Path) -> Result<(), String> {
    #[cfg(not(test))]
    if !super::standing_in() {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find OpenRage: {e}"))?;
        return command(&exe, project)
            .spawn()
            .map(drop)
            .map_err(|e| format!("a new window did not start: {e}"));
    }
    fake::asked(project);
    Ok(())
}

/// A stand-in for starting a program: what it was asked to open, kept and logged.
pub mod fake {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn asked(project: &Path) {
        tracing::info!(
            "the stand-in for a new window was asked to open {}",
            project.display()
        );
        if let Ok(mut opened) = OPENED.lock() {
            opened.push(project.to_path_buf());
        }
    }

    /// Whether a new window was asked for on `project` since the test binary started.
    #[cfg(test)]
    pub fn was_opened(project: &Path) -> bool {
        OPENED
            .lock()
            .is_ok_and(|opened| opened.iter().any(|p| p == project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_window_is_this_program_started_on_the_project_and_nothing_else() {
        let exe = Path::new("C:/OpenRage/openrage.exe");
        let project = Path::new("D:/mods/police/police.openrage-proj");
        let command = command(exe, project);
        assert_eq!(command.get_program(), exe.as_os_str());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [project.as_os_str()]);
    }

    #[test]
    fn a_test_build_asks_the_stand_in_and_starts_no_program() {
        let project = Path::new("D:/mods/never/started.openrage-proj");
        assert!(!fake::was_opened(project));
        open(project).expect("the stand-in answers");
        assert!(fake::was_opened(project));
    }
}

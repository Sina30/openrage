//! Whether the game reads each install's `mods` overlay, read from the install's folder once and kept until asked again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use or_title::game::{check_overlay_loader, registry, GameProfile, OverlayLoaderReport};

use super::{retail, AppState};

/// What one install's folder said, and whether a write into its overlay has warned about it yet.
pub struct Reading {
    pub profile: &'static dyn GameProfile,
    pub report: Result<OverlayLoaderReport, String>,
    warned: bool,
}

/// A reading per install folder; `None` where the folder holds no title that states a loader.
#[derive(Default)]
pub struct OverlayLoaderChecks {
    readings: BTreeMap<PathBuf, Option<Reading>>,
}

fn read(root: &Path) -> Option<Reading> {
    let profile = registry::detect(root)?;
    if profile.overlay_loaders().redirects.is_empty() {
        return None;
    }
    Some(Reading {
        profile,
        report: check_overlay_loader(profile, root).map_err(|e| e.to_string()),
        warned: false,
    })
}

/// What stops the game reading `mods`, as a modder would say it; `None` when it reads.
pub fn why_unread(profile: &dyn GameProfile, report: &OverlayLoaderReport) -> Option<String> {
    let exe = profile.executable_name();
    if report.lacks_redirect() {
        return Some(format!(
            "Nothing beside {exe} makes the game read the mods folder."
        ));
    }
    if report.lacks_loader() {
        let plugins: Vec<&str> = report.redirects.iter().map(|r| r.file).collect();
        return Some(format!(
            "{} stands beside {exe}, but nothing starts it: the game needs an ASI loader named {}.",
            plugins.join(" and "),
            report.rules.loader_files.join(" or "),
        ));
    }
    None
}

impl AppState {
    /// What the install at `root` needs for the game to read its overlay, read on first ask.
    pub fn overlay_loader(&mut self, root: &Path) -> Option<&Reading> {
        (self.overlay_loader.readings)
            .entry(root.to_path_buf())
            .or_insert_with(|| read(root))
            .as_ref()
    }

    /// Look in `root`'s folder again, after the user has added or removed a file there.
    pub fn recheck_overlay_loader(&mut self, root: &Path) {
        self.overlay_loader.readings.remove(root);
    }

    /// Warn once per install that a write into its overlay will not be read by the game.
    pub(super) fn warn_if_overlay_unread(&mut self, target: &Path) {
        if retail::classify(target, &self.prefs) != retail::PathClass::Overlay {
            return;
        }
        let Some(root) = retail::install_root_of(target, &self.prefs).map(Path::to_path_buf) else {
            return;
        };
        let Some(reading) = self
            .overlay_loader
            .readings
            .entry(root)
            .or_insert_with_key(|root| read(root))
        else {
            return;
        };
        let Ok(report) = &reading.report else {
            return;
        };
        let Some(why) = why_unread(reading.profile, report).filter(|_| !reading.warned) else {
            return;
        };
        let said = format!(
            "{} will not read what is written into mods. {why} Settings, Editing says what to add.",
            reading.profile.label()
        );
        reading.warned = true;
        self.show_warning_toast(said);
    }
}

#[cfg(test)]
mod tests {
    use or_title::Game;

    use super::*;
    use crate::state::{EditMode, GateOutcome, ToastType};

    fn install_of(files: &[&str]) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("scratch");
        for name in files {
            std::fs::write(dir.path().join(name), b"").expect("a stand-in file");
        }
        let mut state = AppState::default();
        state
            .prefs
            .game_paths
            .set(Game::GtaVLegacy, Some(dir.path().to_path_buf()));
        state.prefs.active_game = Some(Game::GtaVLegacy);
        state.prefs.edit_mode = EditMode::Overlay;
        (dir, state)
    }

    fn warnings(state: &AppState) -> Vec<String> {
        (state.toasts.iter())
            .filter(|t| t.toast_type == ToastType::Warning)
            .map(|t| t.message.clone())
            .collect()
    }

    fn write_into_overlay(state: &mut AppState, root: &Path) {
        let target = root.join("mods").join("update").join("update.rpf");
        assert_eq!(state.gate_permits(Some(&target)), GateOutcome::Proceed);
    }

    #[test]
    fn a_write_into_the_overlay_of_an_install_the_game_will_not_read_warns_once() {
        let (dir, mut state) = install_of(&["GTA5.exe"]);

        write_into_overlay(&mut state, dir.path());
        let first = warnings(&state);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(first[0].contains("GTA V (Legacy) will not read what is written into mods"));
        assert!(first[0].contains("Nothing beside GTA5.exe"), "{first:?}");

        write_into_overlay(&mut state, dir.path());
        assert_eq!(
            warnings(&state).len(),
            1,
            "a second write does not repeat it"
        );

        state.recheck_overlay_loader(dir.path());
        write_into_overlay(&mut state, dir.path());
        assert_eq!(
            warnings(&state).len(),
            2,
            "a check that still finds nothing warns again"
        );
    }

    #[test]
    fn an_install_the_game_reads_is_not_warned_about() {
        let (dir, mut state) = install_of(&["GTA5.exe", "OpenIV.asi", "dinput8.dll"]);
        write_into_overlay(&mut state, dir.path());
        assert_eq!(warnings(&state), Vec::<String>::new());
    }

    #[test]
    fn a_write_that_is_not_into_the_overlay_does_not_warn() {
        let (dir, mut state) = install_of(&["GTA5.exe"]);
        let elsewhere = tempfile::tempdir().expect("scratch");
        for target in [
            elsewhere.path().join("mods").join("x.rpf"),
            dir.path().join("mods_").join("x.rpf"),
            dir.path().join("update").join("update.rpf"),
        ] {
            state.gate_permits(Some(&target));
        }
        assert_eq!(warnings(&state), Vec::<String>::new());
    }

    #[test]
    fn a_folder_with_no_executable_is_no_install_and_not_warned_about() {
        let (dir, mut state) = install_of(&[]);
        write_into_overlay(&mut state, dir.path());
        assert_eq!(warnings(&state), Vec::<String>::new());
    }

    #[test]
    fn copying_an_archive_into_the_overlay_warns_when_the_game_will_not_read_the_copy() {
        let (dir, mut state) = install_of(&["GTA5.exe"]);
        let shipped = dir.path().join("update").join("update.rpf");
        std::fs::create_dir_all(shipped.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&shipped, b"an archive").expect("the archive");

        assert!(state.offer_overlay_copy(&shipped));
        state.answer_overlay_copy(true);
        assert_eq!(warnings(&state).len(), 1, "{:?}", warnings(&state));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while state.overlay_copy.running().is_some() && std::time::Instant::now() < deadline {
            state.poll_overlay_copy();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(dir.path().join("mods/update/update.rpf").is_file());
    }
}

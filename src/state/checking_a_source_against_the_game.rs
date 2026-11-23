//! Check against the game: a person asks for one source, a worker reads the install, and a source whose
//! original is another file now says so on its row, in its quick look and in Problems.
//!
//! ⚠ Built with `AppState::from_prefs` and a synthetic install of one unencrypted archive: no keys.

use std::path::Path;

use or_source::origin_check::Verdict;

use super::every_add_lands_where_the_tree_shows_it::opened as open_folder;
use super::importing_game_files_from_the_app::{
    imported, project, project_and_install, row, until, write, ytd,
};
use super::nested_archives_survive_a_save::archive;
use super::opening_a_ydd::node_at;
use super::workbench::{AssetState, Severity};
use super::{AppState, Prefs};
use crate::ui::commands::{self, Action};

/// A project holding `a` as a source taken from `GTAV/x64/pack.rpf/a.ytd` of an install, cooked, and selected.
fn a_source_from_the_game(dir: &Path) -> (AppState, String, std::path::PathBuf) {
    let (mut state, _, install) = project_and_install(dir);
    let doc = open_folder(&mut state, dir);
    let entry = node_at(&mut state, doc, "GTAV/x64/pack.rpf/a.ytd");
    state.import_rows(doc, &[entry]);
    imported(&mut state);
    until(&mut state, "the cook of the import", |s| {
        s.workbench.rows().iter().any(|r| r.name == "a")
            && s.workbench.state_of(row(s, "a")) == AssetState::Cooked
    });
    let key = row(&state, "a").key.clone();
    state.select_asset(Some(key.clone()));
    (state, key, install)
}

fn checked(state: &mut AppState, key: &str) -> Verdict {
    state.check_against_game(key);
    until(state, "the check", |s| {
        !s.workbench.originals().is_running()
    });
    let folder = state.workbench.row(key).expect("the row").path.clone();
    (state.workbench.originals().verdict_of(&folder).cloned()).expect("the check left a verdict")
}

#[test]
fn a_source_passes_while_the_games_file_is_the_one_it_was_taken_from_and_is_flagged_when_it_is_not()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, key, install) = a_source_from_the_game(dir.path());
    let folder = state.workbench.row(&key).unwrap().path.clone();
    assert!(state.workbench.originals().verdict_of(&folder).is_none());

    assert_eq!(checked(&mut state, &key), Verdict::Unchanged);
    assert_eq!(
        state.workbench.state_of(row(&state, "a")),
        AssetState::Cooked
    );
    assert!(state.workbench.findings().is_empty());

    // The game updates its archive: the file at that path is another file.
    write(
        &install.join("x64/pack.rpf"),
        &archive(&[("a.ytd", &ytd("road", 9))]),
    );
    // ⚠ Never per scan: a scan, a cook and the passing of time do not read the install.
    state.workbench.want_scan();
    until(&mut state, "the rows to be read again", |s| {
        s.workbench.is_scanned() && !s.workbench.originals().is_running()
    });
    assert!(!state.workbench.originals().is_changed(&folder));

    assert!(matches!(checked(&mut state, &key), Verdict::Changed { .. }));
    let current = state.workbench.state_of(row(&state, "a"));
    assert_eq!(current, AssetState::OriginalChanged);
    assert_eq!(current.label(), "The game's original changed");
    assert!(current.is_problem());
    until(&mut state, "the finding", |s| {
        !s.workbench.findings().is_empty()
    });
    let finding = &state.workbench.findings()[0];
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.text.starts_with("a: the game's original changed"),
        "{}",
        finding.text
    );
    let said: Vec<_> = (state.workbench.history().iter())
        .filter(|e| e.text.starts_with("Checked a"))
        .collect();
    assert_eq!(
        said.len(),
        2,
        "a line in the History for each check: {said:?}"
    );
}

#[test]
fn a_file_the_game_no_longer_holds_is_said_to_be_missing_and_is_not_called_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, key, install) = a_source_from_the_game(dir.path());

    write(
        &install.join("x64/pack.rpf"),
        &archive(&[("b.ytd", &ytd("other", 1))]),
    );

    assert!(
        matches!(checked(&mut state, &key), Verdict::Missing { .. }),
        "the archive no longer holds a.ytd"
    );
    assert_eq!(
        state.workbench.state_of(row(&state, "a")),
        AssetState::Cooked
    );
}

#[test]
fn only_a_source_taken_from_the_game_can_be_checked_and_an_install_must_be_set() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = project(dir.path());
    write(&dir.path().join("downloads/lamp.ytd"), &ytd("lamp", 9));
    state.import_game_files(vec![dir.path().join("downloads/lamp.ytd")]);
    imported(&mut state);
    until(&mut state, "the row", |s| {
        s.workbench.rows().iter().any(|r| r.name == "lamp")
    });
    let key = row(&state, "lamp").key.clone();
    state.select_asset(Some(key.clone()));

    let command = commands::of(&state, Action::CheckAgainstGame).expect("a command");
    assert_eq!(
        command.enabled,
        Err("Only a source taken from the game has a game file to check against"),
        "a file picked from a download is not the game's"
    );

    // A source of the game, with no install set on this machine.
    let dir = tempfile::tempdir().unwrap();
    let (mut state, key, _) = a_source_from_the_game(dir.path());
    state.prefs = Prefs::default();
    state.check_against_game(&key);
    assert!(!state.workbench.originals().is_running());
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("install folder")),
        "nothing was read, and the toast says what to set"
    );
}

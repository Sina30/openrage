//! The project's History where a review found it breaking: an import older than the last, an import of an
//! earlier session, the steps after an import, and two project files in one folder.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, like `the_project_history`.

use std::path::{Path, PathBuf};

use or_source::asset::folder_name;
use or_source::textures;

use super::importing_game_files_from_the_app::ytd;
use super::project::ProjectDocument;
use super::the_project_history::{a_project, builds, members, open, script, until, write};
use super::AppState;

/// The source `name.ytd` becomes in the project of `folder`.
fn source(folder: &Path, name: &str) -> PathBuf {
    folder
        .join("sources")
        .join(folder_name(name, textures::KIND))
}

/// Import `name.ytd` from `dir` and wait for the report; the source it made.
fn import(state: &mut AppState, dir: &Path, name: &str) -> PathBuf {
    let picked = dir.join("downloads").join(format!("{name}.ytd"));
    write(&picked, &ytd(name, 3));
    state.import_game_files(vec![picked]);
    until(state, "the import", |s| !s.workbench.imports.is_running());
    let folder = state.project_path.as_ref().unwrap().parent().unwrap();
    let made = source(folder, name);
    assert!(made.is_dir(), "{name} was not imported");
    made
}

fn first_step(state: &AppState) -> u64 {
    (state.workbench.history().iter())
        .find_map(|e| e.step)
        .expect("a first step")
}

fn last_step(state: &AppState) -> u64 {
    (state.workbench.history().iter())
        .rev()
        .find_map(|e| e.step)
        .expect("a last step")
}

#[test]
fn undo_takes_back_the_import_of_its_own_step_and_not_only_the_last_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    let hat = import(&mut state, dir.path(), "hat");

    state
        .undo_project()
        .expect("the newer import is taken back");
    assert!(!hat.exists());
    assert!(
        !state.import_can_be_undone(),
        "the report of the import that was taken back still offers Undo import"
    );
    assert!(road.is_dir(), "the older one is a step of its own");
    state
        .undo_project()
        .expect("the older import is taken back");
    assert!(
        !road.exists(),
        "Undo said the older import was taken back and left its source on disk"
    );
}

#[test]
fn a_rollback_over_two_imports_takes_back_both() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    let hat = import(&mut state, dir.path(), "hat");

    state
        .roll_back_project_to(first_step(&state))
        .expect("a rollback over two imports");
    assert!(!road.exists() && !hat.exists(), "{road:?} {hat:?}");
}

#[test]
fn a_rollback_over_two_imports_changes_nothing_when_the_older_one_was_edited() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    let hat = import(&mut state, dir.path(), "hat");
    write(&road.join("notes.txt"), b"mine");
    let here = state.workbench.step_here();

    let refused = state.roll_back_project_to(first_step(&state)).unwrap_err();
    assert!(refused.contains("road"), "{refused}");
    assert!(
        hat.is_dir() && road.is_dir(),
        "the newer import was taken back though the move was refused"
    );
    assert_eq!(state.workbench.step_here(), here);
}

#[test]
fn an_import_of_an_earlier_session_is_taken_back_and_a_source_changed_since_refuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    assert!(state.save_active_project(|_| None));
    drop(state);

    let mut state = open(&file);
    let here = state.workbench.step_here();
    write(&road.join("notes.txt"), b"mine");
    let refused = state.undo_project().unwrap_err();
    assert!(refused.contains("changed since the import"), "{refused}");
    assert!(road.is_dir(), "a source somebody edited is not removed");
    assert_eq!(state.workbench.step_here(), here, "and nothing moved");

    std::fs::remove_file(road.join("notes.txt")).unwrap();
    state.undo_project().expect("as the import left it");
    assert!(
        !road.exists(),
        "Undo after the project was opened again left the import's source on disk"
    );
}

#[test]
fn a_source_gone_already_does_not_hold_the_history_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    std::fs::remove_dir_all(&road).unwrap();

    assert!(state.undo_project().is_ok());
    assert_eq!(
        state.workbench.step_here(),
        Some(first_step(&state)),
        "there was nothing left to take back"
    );
}

#[test]
fn a_step_after_an_import_is_still_reached_once_the_history_has_gone_back_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let declared = builds(&state).len();
    let road = import(&mut state, dir.path(), "road");
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    state
        .add_build("harbor_dlc", or_project::OutputKind::SingleplayerDlc, None)
        .expect("a build");

    state
        .roll_back_project_to(first_step(&state))
        .expect("back to the start");
    assert!(!road.exists());
    assert!(members(&state).is_empty() && builds(&state).len() == declared);

    state.toasts.clear();
    state
        .roll_back_project_to(last_step(&state))
        .expect("forward over the import to the steps after it");
    assert_eq!(
        members(&state),
        ["a.lua"],
        "the file added after the import"
    );
    assert_eq!(builds(&state).len(), declared + 1, "and the build");
    assert!(
        road.is_dir(),
        "Redo makes the import's source again from the copy kept beside its step"
    );
    assert!(
        !state
            .toasts
            .iter()
            .any(|t| t.message.contains("Import game files")),
        "nothing is missing, so nothing is said: {:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

#[test]
fn redo_makes_the_sources_of_an_import_again_exactly_and_undo_can_take_them_back_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    let digest = or_source::game_import::tree_digest(&road).unwrap();

    state.undo_project().expect("the import is taken back");
    assert!(!road.exists());
    state.redo_project().expect("the import is made again");
    assert!(road.is_dir(), "the source is back");
    assert_eq!(
        or_source::game_import::tree_digest(&road).unwrap(),
        digest,
        "the same bytes in the same names"
    );

    state.undo_project().expect("and taken back again");
    assert!(!road.exists());
    state.redo_project().expect("and made again again");
    assert!(road.is_dir());
}

#[test]
fn a_step_dropped_by_a_new_change_takes_the_copy_of_its_import_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let road = import(&mut state, dir.path(), "road");
    let made = last_step(&state);
    state.undo_project().expect("taken back");
    let aside = or_project::snapshot::history_dir(&file).join(format!("{made:06}.made"));
    assert!(aside.is_dir(), "a copy is kept beside the step: {aside:?}");

    // A new change drops the step that was undone, and its copy goes with it.
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    assert!(!aside.exists(), "the copy of a step that is gone is gone");
    assert!(!road.exists());
}

#[test]
fn where_no_copy_was_kept_redo_still_says_the_import_is_not_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let road = import(&mut state, dir.path(), "road");
    let made = last_step(&state);
    state.undo_project().expect("taken back");
    std::fs::remove_dir_all(
        or_project::snapshot::history_dir(&file).join(format!("{made:06}.made")),
    )
    .unwrap();

    state.toasts.clear();
    state.redo_project().expect("the project moves");
    assert!(!road.exists());
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("Import game files")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

/// A project file `name` beside the one `file` is, holding what it holds.
fn beside(file: &Path, name: &str) -> PathBuf {
    let other = file.with_file_name(format!("{name}.openrage-proj"));
    std::fs::copy(file, &other).unwrap();
    other
}

#[test]
fn two_project_files_in_one_folder_keep_their_own_history_and_their_own_outside_files() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let folder = file.parent().unwrap().to_path_buf();
    let outside = dir.path().join("downloads").join("hat.ydd");
    write(&outside, b"hat");
    state.add_files_to_project(vec![script(&folder, "a.lua")]);
    state
        .project_carried
        .members
        .push(ProjectDocument::new(outside.clone(), None));
    state.project_carried.changed = true;
    assert!(state.save_active_project(|_| None));

    // Save project as into the same folder, where the dialog starts, and drop the file in the copy.
    let second = folder.join("harbor_v2.openrage-proj");
    state.save_project_to(second.clone()).unwrap();
    assert!(state.remove_from_project(&outside));
    assert!(state.save_active_project(|_| None));
    let in_second: Vec<String> = (state.workbench.history().iter())
        .map(|e| e.text.clone())
        .collect();
    drop(state);

    let first = open(&file);
    assert!(
        first.member_paths().contains(&outside),
        "the copy's save took the file out of the first project: {:?}",
        first.member_paths()
    );
    assert!(
        (first.workbench.history().iter()).all(|e| !e.text.contains("Removed hat.ydd")),
        "the first project lists a step only the copy made: {:?}",
        first.workbench.history()
    );
    assert!(
        in_second
            .iter()
            .all(|text| !text.contains("Added 1 file(s)")),
        "the copy lists a step the first project made: {in_second:?}"
    );
}

#[test]
fn a_machine_file_of_the_older_layout_goes_to_the_first_project_that_reads_it_and_to_no_other() {
    let dir = tempfile::tempdir().unwrap();
    let (state, file) = a_project(dir.path());
    drop(state);
    let folder = file.parent().unwrap().to_path_buf();
    let second = beside(&file, "harbor_v2");
    let outside = dir.path().join("downloads").join("hat.ydd");
    write(&outside, b"hat");
    // Where the file was kept before projects were kept apart: one for the folder.
    let older = folder.join(".openrage").join("machine.json");
    let text = serde_json::json!({ "documents": [{ "path": outside.to_string_lossy() }] });
    write(&older, &serde_json::to_vec_pretty(&text).unwrap());

    let first = open(&file);
    assert!(
        first.member_paths().contains(&outside),
        "the project that read it first lost its outside file: {:?}",
        first.member_paths()
    );
    assert!(
        !older.exists(),
        "it moved to where the project keeps its own"
    );

    let other = open(&second);
    assert!(
        !other.member_paths().contains(&outside),
        "a second project file in the folder was given the first one's file"
    );
}

#[test]
fn a_file_an_import_kept_goes_with_its_step_and_comes_back_with_it_going_forward() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = a_project(dir.path());
    let kept = file.parent().unwrap().join("files/lamp.ybn");
    let picked = dir.path().join("downloads/lamp.ybn");
    write(&picked, b"collision");
    state.import_game_files(vec![picked]);
    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });
    assert!(kept.is_file() && members(&state) == ["lamp.ybn"]);

    state.undo_project().expect("the import is taken back");
    assert!(!kept.exists(), "the copy the import made is removed");
    assert!(members(&state).is_empty());

    state.toasts.clear();
    state.redo_project().expect("forward over the import");
    assert_eq!(members(&state), ["lamp.ybn"], "the step's project is back");
    assert_eq!(
        std::fs::read(&kept).unwrap(),
        b"collision",
        "and its file is made again, byte for byte"
    );
}

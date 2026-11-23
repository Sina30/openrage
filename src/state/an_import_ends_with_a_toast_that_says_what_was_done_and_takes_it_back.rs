//! An import of game files that worked ends with a toast that says what was done and has Undo import and Show report buttons, and opens no window.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a worker for each import.

use or_source::asset::folder_name;
use or_source::textures;

use super::importing_game_files_from_the_app::{imported, project, sources_of, write, ytd};
use super::toasts::ToastAction;
use super::AppState;

/// The toast with an Undo import button, as its message and the action it carries.
fn offered(state: &AppState) -> Option<(String, ToastAction)> {
    state.toasts.iter().find_map(|t| match t.action {
        Some(action @ ToastAction::UndoImport(_)) => Some((t.message.clone(), action)),
        _ => None,
    })
}

#[test]
fn an_import_says_what_it_did_in_a_toast_and_its_button_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let picked = dir.path().join("downloads/road.ytd");
    write(&picked, &ytd("road", 3));

    state.import_game_files(vec![picked]);
    imported(&mut state);

    let (said, action) =
        offered(&state).expect("the import ends with a toast that has Undo import");
    assert!(
        said.starts_with("Imported 1 file: 1 became a source"),
        "the toast says what was done, with no count that is nought: {said}"
    );
    assert_eq!(action.label(), "Undo import");
    assert!(
        !state.workbench.imports.shown,
        "an import that worked opens no window over the table it filled"
    );
    let also = state
        .toasts
        .iter()
        .find_map(|t| t.also)
        .expect("a second button");
    assert_eq!(also.label(), "Show report");
    state.run_toast_action(also);
    assert!(
        state.workbench.imports.shown,
        "Show report opens the report"
    );
    state.workbench.imports.shown = false;
    let made = folder
        .join("sources")
        .join(folder_name("road", textures::KIND));
    assert!(made.is_dir());

    state.run_toast_action(action);

    assert!(!made.exists(), "the button took the import back");
    assert!(sources_of(&folder).is_empty(), "{:?}", sources_of(&folder));
}

#[test]
fn an_import_that_made_nothing_offers_no_undo_and_the_toast_of_an_older_import_takes_nothing_back()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let first = dir.path().join("downloads/road.ytd");
    let second = dir.path().join("downloads/path.ytd");
    write(&first, &ytd("road", 3));
    write(&second, &ytd("path", 5));

    state.import_game_files(vec![first.clone()]);
    imported(&mut state);
    let (_, old) = offered(&state).expect("the first toast");
    state.toasts.clear();

    // The same file again adds nothing: it is already a source, so no toast offers to take back what was not made.
    state.import_game_files(vec![first]);
    imported(&mut state);
    assert!(offered(&state).is_none(), "{:?}", state.toasts);

    state.import_game_files(vec![second]);
    imported(&mut state);
    let (_, newer) = offered(&state).expect("the second toast");
    assert_ne!(old, newer);

    // The first toast's button no longer takes back the report that is shown: another import ended after it.
    state.run_toast_action(old);
    assert_eq!(
        sources_of(&folder).len(),
        2,
        "nothing was taken back: {:?}",
        sources_of(&folder)
    );
    state.run_toast_action(newer);
    assert_eq!(sources_of(&folder).len(), 1);
}

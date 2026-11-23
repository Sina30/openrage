//! The mod manager in the app: reordering two conflicting mods changes the winner and the order
//! survives a restart, and a mod moves to `mods_` and back only through the edit gate and an answer.

use std::path::Path;

use super::mod_manager::PendingModMove;
use super::nested_archives_survive_a_save::errors;
use super::prefs::{load_prefs_from, save_prefs_to};
use super::{AppState, EditMode, Game, Prefs};

const SHARED: &str = "x64a.rpf/levels/gta5/props.ydr";

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// An install holding two mods that write `SHARED`.
fn install_with_two_mods() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), &format!("mods/BigProps/{SHARED}"), b"big");
    write(tmp.path(), &format!("mods/SmallProps/{SHARED}"), b"small");
    write(
        tmp.path(),
        "mods/SmallProps/x64a.rpf/levels/gta5/only_small.ydr",
        b"s",
    );
    tmp
}

fn state_for(root: &Path, mode: EditMode) -> AppState {
    let mut state = AppState::default();
    state.documents.clear();
    state
        .prefs
        .game_paths
        .set(Game::GtaVLegacy, Some(root.to_path_buf()));
    state.prefs.active_game = Some(Game::GtaVLegacy);
    state.prefs.edit_mode = mode;
    state.refresh_mods();
    state
}

fn winner(state: &AppState) -> &str {
    let conflicts = &state.mod_manager.resolution.conflicts;
    assert_eq!(conflicts.len(), 1, "the one shared entry: {conflicts:?}");
    &conflicts[0].winner
}

#[test]
fn reordering_two_conflicting_mods_changes_the_winner_and_the_order_survives_a_restart() {
    let install = install_with_two_mods();
    let mut state = state_for(install.path(), EditMode::ReadOnly);
    assert_eq!(
        winner(&state),
        "SmallProps",
        "a mod the order has not met stacks last"
    );

    state.move_mod("SmallProps", true);
    assert_eq!(winner(&state), "BigProps");
    let key = or_title::mods::install_key(install.path());
    assert_eq!(state.prefs.mod_orders[&key], ["SmallProps", "BigProps"]);

    // The preferences as the next run reads them, in a state that has never seen this one.
    let config = tempfile::tempdir().expect("tempdir");
    let file = config.path().join("prefs.json");
    save_prefs_to(&file, &state.prefs);
    let mut next = AppState::default();
    next.documents.clear();
    next.prefs = load_prefs_from(&file);
    next.refresh_mods();
    assert_eq!(winner(&next), "BigProps");

    next.move_mod("SmallProps", false);
    assert_eq!(winner(&next), "SmallProps", "and it moves back");
}

#[test]
fn an_order_the_prefs_cannot_read_costs_only_itself() {
    let prefs: Prefs =
        serde_json::from_str(r#"{"mod_orders":{"c:/a":["X","Y"],"c:/b":5,"c:/c":[1]}}"#)
            .expect("the rest of the file is read");
    assert_eq!(prefs.mod_orders.len(), 1);
    assert_eq!(prefs.mod_orders["c:/a"], ["X", "Y"]);
}

#[test]
fn a_mod_moves_to_mods_underscore_only_when_the_mode_permits_and_the_user_confirms() {
    let install = install_with_two_mods();
    let root = install.path();
    let mut state = state_for(root, EditMode::ReadOnly);

    state.ask_toggle_mod("SmallProps");
    assert_eq!(
        state.mod_manager.pending_move, None,
        "read only does not even ask"
    );
    assert!(!errors(&state).is_empty(), "and says why");
    assert!(root.join("mods/SmallProps").is_dir());

    state.set_edit_mode(EditMode::Overlay);
    state.ask_toggle_mod("SmallProps");
    let asked = state.mod_manager.pending_move.clone().expect("it asks");
    assert_eq!(
        asked,
        PendingModMove {
            name: "SmallProps".into(),
            enable: false,
            from: root.join("mods/SmallProps"),
            to: root.join("mods_/SmallProps"),
            files: 2,
        }
    );
    assert!(
        root.join("mods/SmallProps").is_dir(),
        "nothing moves before the answer"
    );

    state.answer_mod_move(false);
    assert_eq!(state.mod_manager.pending_move, None);
    assert!(
        root.join("mods/SmallProps").is_dir(),
        "No leaves it where it is"
    );

    state.ask_toggle_mod("SmallProps");
    state.answer_mod_move(true);
    assert!(!root.join("mods/SmallProps").exists());
    assert!(root.join("mods_/SmallProps").join(SHARED).is_file());
    let parked = (state.mod_manager.mods.iter())
        .find(|m| m.name == "SmallProps")
        .expect("it is still listed");
    assert_eq!(parked.place, or_title::mods::ModPlace::Parked);
    assert!(
        state.mod_manager.resolution.conflicts.is_empty(),
        "a disabled mod wins nothing"
    );

    // Enabling moves it back, and it takes the place it had, so it wins again.
    state.ask_toggle_mod("SmallProps");
    state.answer_mod_move(true);
    assert!(root.join("mods/SmallProps").join(SHARED).is_file());
    assert!(!root.join("mods_/SmallProps").exists());
    assert_eq!(winner(&state), "SmallProps");
}

#[test]
fn a_move_onto_a_folder_that_is_already_there_is_refused_and_changes_nothing() {
    let install = install_with_two_mods();
    let root = install.path();
    write(root, "mods_/SmallProps/other.ydr", b"parked earlier");
    let mut state = state_for(root, EditMode::Overlay);

    state.ask_toggle_mod("SmallProps");
    assert_eq!(state.mod_manager.pending_move, None);
    assert!(errors(&state).iter().any(|e| e.contains("already there")));
    assert!(root.join("mods/SmallProps").join(SHARED).is_file());
    assert!(root.join("mods_/SmallProps/other.ydr").is_file());
}

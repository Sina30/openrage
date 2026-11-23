//! Add clothes can be given a folder: the command that asks for one opens a dialog for a folder, and what is in it becomes
//! garments as a drop of the folder does, with Undo.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder; the dialog is the stand-in a test answers.

use super::the_wardrobe_workspace::{drop_files, garments, opened, until, C};
use crate::platform::file_dialogs;
use crate::ui::commands::{self, Action};

#[test]
fn add_a_folder_of_clothes_asks_for_a_folder_and_adds_what_is_in_it_and_undo_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    let files = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
        ],
    );
    let folder = files[0].parent().unwrap().to_path_buf();

    let command =
        commands::of(&state, Action::AddClothesFolder).expect("the command is in the table");
    assert!(command.enabled.is_ok(), "{:?}", command.enabled);
    assert!(
        command
            .hover
            .as_deref()
            .is_some_and(|h| h.contains("decompiled into a source of the project")),
        "it says it imports to the project: {:?}",
        command.hover
    );
    file_dialogs::answer_next(folder);
    commands::run_action(
        &mut state,
        &egui::Context::default(),
        Action::AddClothesFolder,
    );
    assert_eq!(
        file_dialogs::last_dialog().as_deref(),
        Some("Add a folder of clothes"),
        "it asked for a folder"
    );
    until(&mut state, "the garments of the folder", |s| {
        garments(s) == 2
    });

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 0);
}

#[test]
fn a_dialog_that_picks_nothing_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    commands::run_action(
        &mut state,
        &egui::Context::default(),
        Action::AddClothesFolder,
    );
    for _ in 0..20 {
        state.poll_workbench();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(garments(&state), 0);
    assert!(state.workbench.wardrobe.undo_label().is_none());
}

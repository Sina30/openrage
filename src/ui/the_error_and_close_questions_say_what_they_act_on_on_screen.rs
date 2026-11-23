//! The two questions the shell asks outside the project screens: the error of a file that will not open, and the close that would drop
//! unsaved work. Each says which file, in plain words, takes Escape and has the one button that goes on in front of the keyboard.
//!
//! Read through AccessKit from whole frames of the shell, in the smallest window the editor has.

use std::time::{Duration, Instant};

use super::frames_of_the_shell::{key, Shell};
use super::the_front_door_on_screen::{a_first_launch, a_project, click_on, names, shows};
use crate::state::front_door::Template;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

const SMALLEST: (u32, u32) = (1280, 720);

/// A first launch asked to open a file whose bytes are no game file, with the error window up.
fn a_file_that_will_not_open(dir: &std::path::Path) -> Shell {
    let bad = dir.join("broken_hat.ydd");
    std::fs::write(&bad, [0x0C, 0xE2, 0x66, 0xF1, 0, 0, 0, 0]).unwrap();
    let mut shell = a_first_launch();
    shell.state.load_file(bad);
    let until = Instant::now() + Duration::from_secs(30);
    while !shell.state.show_error_dialog {
        assert!(Instant::now() < until, "the file never failed to open");
        shell.state.poll_load_result();
        shell.draw_sized(SMALLEST, Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
    shell.settled_sized(SMALLEST);
    shell
}

#[test]
fn a_file_that_will_not_open_is_named_said_in_plain_words_and_keeps_the_library_for_the_details() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_file_that_will_not_open(dir.path());
    let said = names(&shell);
    assert!(
        shows(&shell, "Could not open broken_hat.ydd"),
        "the window does not say which file: {said:?}"
    );
    assert!(
        said.iter()
            .any(|n| n.contains("OpenRage could not read this file")),
        "no sentence says what the failure means: {said:?}"
    );
    for jargon in ["RSC7", "0x", "Parse error", "magic"] {
        assert!(
            !said.iter().any(|n| n.contains(jargon)),
            "the window or the bar says `{jargon}`: {said:?}"
        );
    }
    let focus = shell.focused().map(|(_, name)| name);
    assert_eq!(
        focus.as_deref(),
        Some("OK"),
        "the one button that goes on is not in front of the keyboard"
    );
    click_on(&mut shell, "Copy details");
    let copied = shell.copied.last().cloned().unwrap_or_default();
    assert!(
        copied.contains("broken_hat.ydd") && copied.contains("Parse error"),
        "the details do not keep the file and the library's own words: {copied}"
    );
    assert!(
        shell.state.show_error_dialog,
        "Copy details closed the window"
    );
    shell.draw_sized(SMALLEST, vec![key(egui::Key::Escape)]);
    assert!(
        !shell.state.show_error_dialog,
        "Escape does not close the error window"
    );
}

/// A project of loose files with one step since it was saved, and the question a close asks.
fn closing_a_project_with_a_change(dir: &std::path::Path) -> Shell {
    let file = a_project(dir, "police", Template::Files);
    let folder = file.parent().unwrap().to_path_buf();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell.settled_sized(SMALLEST);
    shell
        .state
        .add_files_to_project(vec![super::the_project_screens_on_screen::write(
            &folder.join("client.lua"),
            b"print('hello')",
        )]);
    shell.settled_sized(SMALLEST);
    assert!(
        shell.state.project_carried.changed,
        "the add changed nothing"
    );
    shell.state.close_project();
    shell.settled_sized(SMALLEST);
    shell
}

#[test]
fn closing_a_project_lists_what_is_dropped_names_the_file_once_and_escape_is_cancel() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = closing_a_project_with_a_change(dir.path());
    let said = names(&shell);
    assert!(
        shows(&shell, "Save your changes before closing the project?"),
        "{said:?}"
    );
    assert!(
        shows(&shell, "Added 1 file"),
        "the question does not say what the changes are: {said:?}"
    );
    assert!(
        shows(&shell, "Don't save drops this change"),
        "the question does not say what Don't save loses: {said:?}"
    );
    let root = dir.path().to_string_lossy().into_owned();
    assert!(
        !said.iter().any(|n| n.contains(&root)),
        "the path of the file is printed under its name: {said:?}"
    );
    let focus = shell.focused().map(|(_, name)| name);
    assert_eq!(
        focus.as_deref(),
        Some("Save"),
        "Save is not in front of the keyboard"
    );
    shell.draw_sized(SMALLEST, vec![key(egui::Key::Escape)]);
    shell.settled_sized(SMALLEST);
    assert!(
        shell.state.pending_close.is_none(),
        "Escape does not answer Cancel"
    );
    assert!(
        shell.state.project_path.is_some(),
        "Escape closed the project it was asked about"
    );
}

//! Every widget Tab reaches in the dock has a name a screen reader can speak,
//! in every workspace: the tabs, the panels' bodies and the splitters between
//! them.
//!
//! ⚠ egui_dock allocates a splitter and a tab bar's own buttons with no widget
//! info, so AccessKit knows them only as something that takes the keyboard.

use super::every_control_has_a_name_a_screen_reader_can_speak::speakable;
use super::frames_of_the_shell::{key, Shell};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::theme;

/// Tabs a window is given to come back round to where it began.
const TABS: usize = 120;

/// The widgets Tab reaches in `shell` that AccessKit has no name for, by where they are.
fn unnamed(shell: &mut Shell, what: &str) -> (usize, Vec<String>) {
    let mut reached: Vec<String> = Vec::new();
    let mut unnamed = Vec::new();
    for _ in 0..TABS {
        shell.draw(vec![key(egui::Key::Tab)]);
        let Some((rect, name)) = shell.focused().filter(|(rect, _)| rect.is_positive()) else {
            continue;
        };
        let at = format!("{rect:?}");
        if reached.contains(&at) {
            break;
        }
        reached.push(at);
        if !speakable(&name) {
            unnamed.push(format!("{what}: a stop named {name:?} at {rect:?}"));
        }
    }
    (reached.len(), unnamed)
}

#[test]
fn every_tab_stop_in_the_dock_has_a_name_a_screen_reader_can_speak() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    let mut found = Vec::new();
    let mut stops = 0;
    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        shell.settled();
        let (reached, unnamed) = unnamed(&mut shell, &format!("{workspace:?}"));
        stops += reached;
        found.extend(unnamed);
    }
    assert!(
        stops > 20,
        "Tab reached {stops} stops, so the keyboard is not being tested"
    );
    assert!(
        found.is_empty(),
        "{} stops take the keyboard and have no name:\n{}",
        found.len(),
        found.join("\n")
    );
}

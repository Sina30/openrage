//! Every control the shell draws has a name a screen reader can speak, in
//! every workspace and in Settings.
//!
//! ⚠ A name made only of icon-font glyphs is no name: those are private-use
//! characters a screen reader says nothing for.

use std::collections::BTreeMap;

use egui::accesskit::Role;

use super::frames_of_the_shell::{key, Shell};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::settings::SettingsCategory;
use crate::ui::theme;

/// Whether `role` is a control: something a user presses, ticks, picks or types in.
fn is_control(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::ComboBox
            | Role::TextInput
            | Role::Slider
            | Role::SpinButton
            | Role::Link
            | Role::ColorWell
    )
}

/// Whether `name` has a character that is not whitespace and not from the icon font.
pub(super) fn speakable(name: &str) -> bool {
    name.chars()
        .any(|c| !c.is_whitespace() && !(''..='').contains(&c))
}

/// The controls of `shell`'s last frame that have no name to speak, with where.
pub(super) fn mute(shell: &Shell, page: &str, found: &mut BTreeMap<String, usize>) {
    for widget in shell.described.iter().filter(|w| is_control(w.role)) {
        if !widget.name.as_deref().is_some_and(speakable) {
            let at = widget.bounds;
            let name = widget.name.clone().unwrap_or_default();
            *found
                .entry(format!(
                    "{page}: a {:?} named {name:?} at {at:?}",
                    widget.role
                ))
                .or_default() += 1;
        }
    }
}

#[test]
fn every_control_the_shell_draws_has_a_name_a_screen_reader_can_speak() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    let mut found = BTreeMap::new();
    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        shell.settled();
        mute(&shell, &format!("the {workspace:?} workspace"), &mut found);
    }
    shell.a_clips_timeline();
    mute(&shell, "a clip's timeline", &mut found);
    // The row's toggles and chevron are painted by hand, and only their names tell a screen reader they are there.
    for name in [
        "Collapse SKEL_Root",
        "Hide Track 0",
        "Mute Track 0",
        "Lock Track 0",
    ] {
        let present = (shell.described.iter())
            .any(|w| w.role == Role::Button && w.name.as_deref() == Some(name));
        if !present {
            found.insert(format!("a clip's timeline: no button named {name:?}"), 1);
        }
    }
    // A curve's handles are painted by hand, and are controls to a screen reader only as buttons.
    shell.an_editable_clips_timeline();
    mute(&shell, "an editable clip's timeline", &mut found);
    crate::ui::panels::command_palette::open(&mut shell.state);
    shell.settled();
    mute(&shell, "the command palette", &mut found);
    shell.draw(vec![key(egui::Key::Escape)]);
    super::the_project_screens_on_screen::mute_with_a_project_open(&mut found);
    for category in SettingsCategory::ALL {
        shell.settings(category);
        mute(
            &shell,
            &format!("Settings, {}", category.label()),
            &mut found,
        );
    }
    assert!(
        found.is_empty(),
        "{} controls have no name a screen reader can speak:
{}",
        found.len(),
        found.keys().cloned().collect::<Vec<_>>().join(
            "
"
        )
    );
}

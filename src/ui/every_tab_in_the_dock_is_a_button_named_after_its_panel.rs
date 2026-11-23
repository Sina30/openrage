//! Every tab the dock shows is a button a screen reader finds in its tab bar,
//! named after its panel, in every workspace.
//!
//! ⚠ egui_dock paints a tab's title itself and describes no widget, so AccessKit
//! gives the tab no role, and `every_control_has_a_name_a_screen_reader_can_speak`
//! only judges the roles of controls: this test looks for the buttons by name.

use egui::accesskit::Role;
use egui::{pos2, Rect};

use super::frames_of_the_shell::Shell;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::{panel_registry, theme};

/// The height `dock::draw` gives a tab bar, which sits above its leaf's viewport.
const TAB_BAR: f32 = 18.0;

#[test]
fn every_tab_in_the_dock_is_a_button_named_after_its_panel() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    let mut missing = Vec::new();
    let mut tabs = 0;
    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        shell.settled();
        let dock = shell
            .state
            .workspace_docks
            .peek(workspace)
            .expect("the workspace has a layout");
        for (_, leaf) in dock.iter_leaves() {
            let bar = Rect::from_min_max(
                pos2(leaf.viewport.min.x, leaf.viewport.min.y - TAB_BAR),
                pos2(leaf.viewport.max.x, leaf.viewport.min.y),
            );
            for tab in &leaf.tabs {
                tabs += 1;
                let title = panel_registry::title_of(tab.id(), &shell.state);
                let named = shell.described.iter().any(|widget| {
                    widget.role == Role::Button
                        && widget.name.as_deref() == Some(title.as_str())
                        && bar.contains(widget.bounds.center())
                });
                if !named {
                    missing.push(format!(
                        "{workspace:?}: no button named {title:?} in the tab bar at {bar:?}"
                    ));
                }
            }
        }
    }
    assert!(tabs > 6, "{tabs} tabs: the dock is not being tested");
    assert!(
        missing.is_empty(),
        "{} tabs are not buttons a screen reader can name:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

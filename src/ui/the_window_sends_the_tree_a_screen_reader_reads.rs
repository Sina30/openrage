//! The window sends a screen reader a tree, once the screen reader asks for it,
//! carrying the names the shell gives its controls.
//!
//! ⚠ No window opens: this reads the tree egui builds in the frame the window
//! draws, which is all the window forwards to the platform.

use egui::accesskit::{Action, ActionRequest, NodeId, Role, TreeId};
use egui_winit::accesskit_winit::WindowEvent;

use super::frames_of_the_shell::Shell;
use crate::app::accessibility::on_event;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::{panel_registry, theme};

/// The names of the buttons in the tree `shell` drew last.
fn buttons(shell: &Shell) -> Vec<String> {
    (shell.described.iter())
        .filter(|widget| widget.role == Role::Button)
        .filter_map(|widget| widget.name.clone())
        .collect()
}

#[test]
fn a_window_sends_a_screen_reader_no_tree_until_it_asks_and_then_one_with_the_names_of_the_shell() {
    let _serial = theme::serial();
    let mut shell = Shell::unattended(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    shell.settled();
    assert!(
        shell.described.is_empty(),
        "{} nodes are sent before a screen reader asks for any",
        shell.described.len()
    );

    assert!(on_event(&shell.ctx, WindowEvent::InitialTreeRequested).is_none());
    shell.settled();
    let names = buttons(&shell);
    let dock = (shell.state.workspace_docks)
        .peek(Workspace::Browse)
        .expect("the workspace has a layout");
    let mut missing = Vec::new();
    for (_, leaf) in dock.iter_leaves() {
        for tab in &leaf.tabs {
            let title = panel_registry::title_of(tab.id(), &shell.state);
            if !names.contains(&title) {
                missing.push(format!("the tab {title:?}"));
            }
        }
    }
    for name in ["Back", "Forward"] {
        if !names.iter().any(|sent| sent == name) {
            missing.push(format!("the button {name:?}"));
        }
    }
    assert!(
        missing.is_empty(),
        "the tree a screen reader is sent lacks {}, among {names:?}",
        missing.join(", ")
    );

    assert!(on_event(&shell.ctx, WindowEvent::AccessibilityDeactivated).is_none());
    shell.settled();
    assert!(
        shell.described.is_empty(),
        "the tree is still sent once the screen reader has gone"
    );
}

#[test]
fn what_a_screen_reader_asks_of_the_window_is_handed_to_its_input() {
    let shell = Shell::unattended(ThemeChoice::Dark);
    let request = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: NodeId(7),
        data: None,
    };
    let handed = on_event(&shell.ctx, WindowEvent::ActionRequested(request.clone()));
    assert_eq!(handed, Some(request));
}

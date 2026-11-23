//! The widget the keyboard is on always shows: Tab through every workspace,
//! and each widget it reaches has a ring around it, whether or not the
//! widget paints its own focus.

use egui::{Rect, Shape};

use super::frames_of_the_shell::{key, Shell};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::theme;

/// Tabs a window is given to come back round to where it began.
const TABS: usize = 120;

/// A ring in the accent's text colour, at least 2 points wide, that holds `rect` and no more than a few points more.
fn ringed(shapes: &[egui::epaint::ClippedShape], rect: Rect) -> bool {
    fn any(shape: &Shape, rect: Rect) -> bool {
        match shape {
            Shape::Vec(shapes) => shapes.iter().any(|shape| any(shape, rect)),
            Shape::Rect(ring) => {
                ring.stroke.color == theme::ACCENT_TEXT.get()
                    && ring.stroke.width >= 2.0
                    && ring.rect.contains_rect(rect)
                    && ring.rect.expand(-6.0).width() < rect.width() + 12.0
            }
            _ => false,
        }
    }
    shapes.iter().any(|clipped| any(&clipped.shape, rect))
}

/// Every widget Tab reaches in `shell` that is not ringed, by where it is and what it is called.
fn unseen(shell: &mut Shell, what: &str) -> Vec<String> {
    let mut reached = Vec::new();
    let mut unseen = Vec::new();
    for _ in 0..TABS {
        let shapes = shell.draw(vec![key(egui::Key::Tab)]);
        // A widget scrolled out of view shows no ring, and has nothing on screen to ring.
        let Some((rect, label)) = shell.focused().filter(|(rect, _)| rect.is_positive()) else {
            continue;
        };
        let at = format!("{rect:?}");
        if reached.contains(&at) {
            break;
        }
        reached.push(at);
        if !ringed(&shapes, rect) {
            unseen.push(format!("{what}: {label:?} at {rect:?}"));
        }
    }
    assert!(
        reached.len() > 3,
        "{what}: Tab reached {} widgets, so the keyboard is not being tested",
        reached.len()
    );
    unseen
}

#[test]
fn every_widget_tab_reaches_has_a_ring_around_it_in_every_workspace() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    let mut found = Vec::new();
    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        shell.settled();
        found.extend(unseen(&mut shell, &format!("{workspace:?}")));
    }
    assert!(
        found.is_empty(),
        "{} widgets take the keyboard and show nothing of it:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn a_click_takes_the_ring_away_and_the_next_tab_brings_it_back() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.settled();
    let shapes = shell.draw(vec![key(egui::Key::Tab)]);
    let (rect, label) = shell.focused().expect("Tab reaches a widget");
    assert!(ringed(&shapes, rect), "Tab to {label:?} drew no ring");

    shell.click(egui::pos2(800.0, 400.0));
    let shapes = shell.settled();
    if let Some((rect, label)) = shell.focused() {
        assert!(
            !ringed(&shapes, rect),
            "{label:?} keeps its ring after a click"
        );
    }

    let shapes = shell.draw(vec![key(egui::Key::Tab)]);
    let (rect, label) = shell.focused().expect("Tab reaches a widget again");
    assert!(ringed(&shapes, rect), "Tab to {label:?} drew no ring");
}

/// The Text panel's body in the Data workspace, with its tab clicked to the front: where the code editor is.
fn the_editors_body(shell: &mut Shell) -> Rect {
    shell.state.active_workspace = Workspace::Data;
    shell.settled();
    let tab = (shell.described.iter())
        .find(|w| w.role == egui::accesskit::Role::Button && w.name.as_deref() == Some("Text"))
        .map(|w| w.bounds.center())
        .expect("the Data workspace has a Text tab");
    shell.click(tab);
    shell.settled();
    let dock = (shell.state.workspace_docks)
        .peek(Workspace::Data)
        .expect("the workspace has a layout");
    (dock.iter_leaves())
        .find(|(_, leaf)| leaf.tabs[leaf.active.0].id() == "text_editor")
        .map(|(_, leaf)| leaf.viewport)
        .expect("the Text tab is in front")
}

/// ⚠ The code editor takes Space, Enter and the arrows as text, so they say nothing of the keyboard moving between widgets.
#[test]
fn typing_in_the_code_editor_draws_no_ring_round_it() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let body = the_editors_body(&mut shell);
    shell.click(body.center());
    let (editor, _) = shell.focused().expect("a click in the editor focuses it");
    assert!(
        editor.width() > 200.0 && editor.height() > 100.0,
        "the click focused {editor:?}, which is no editor"
    );
    for (typed, events) in [
        (
            "Space",
            vec![key(egui::Key::Space), egui::Event::Text(" ".into())],
        ),
        ("Enter", vec![key(egui::Key::Enter)]),
        ("ArrowDown", vec![key(egui::Key::ArrowDown)]),
        ("ArrowRight", vec![key(egui::Key::ArrowRight)]),
        ("ArrowUp", vec![key(egui::Key::ArrowUp)]),
        ("ArrowLeft", vec![key(egui::Key::ArrowLeft)]),
    ] {
        let shapes = shell.draw(events);
        assert_eq!(
            shell.focused().map(|(rect, _)| rect),
            Some(editor),
            "{typed} took the keyboard out of the editor"
        );
        assert!(
            !ringed(&shapes, editor),
            "{typed} typed in the editor draws a ring round it"
        );
    }

    let shapes = shell.draw(vec![key(egui::Key::Tab)]);
    let (next, label) = shell.focused().expect("Tab leaves the editor for a widget");
    assert!(
        next != editor && ringed(&shapes, next),
        "Tab out of the editor to {label:?} drew no ring"
    );
}

/// A dialog paints its own frame, so a ring drawn before the dialog is under it.
#[test]
fn a_dialog_shows_the_ring_round_the_button_the_keyboard_is_on() {
    fn in_paint_order(shape: &Shape, out: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| in_paint_order(shape, out)),
            shape => out.push(shape.clone()),
        }
    }

    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell
        .state
        .set_error("Saving failed", "The disk is full.".to_string());
    shell.settled();
    let shapes = shell.draw(vec![key(egui::Key::Tab)]);
    let (button, label) = shell.focused().expect("Tab reaches a button of the dialog");
    let mut painted = Vec::new();
    (shapes.iter()).for_each(|clipped| in_paint_order(&clipped.shape, &mut painted));
    let window = shell.ctx.global_style().visuals.window_fill();
    let frame = painted.iter().position(|shape| {
        matches!(shape, Shape::Rect(frame) if frame.fill == window && frame.rect.contains_rect(button))
    });
    let ring = painted.iter().position(|shape| {
        matches!(shape, Shape::Rect(ring) if ring.stroke.color == theme::ACCENT_TEXT.get()
            && ring.stroke.width >= 2.0 && ring.rect.contains_rect(button))
    });
    let (frame, ring) = (
        frame.expect("the dialog paints its frame round the button"),
        ring.unwrap_or_else(|| panic!("Tab to {label:?} drew no ring")),
    );
    assert!(frame < ring, "the dialog's frame is painted over the ring");
}

/// Each panel a workspace shows, with the widgets Tab reached inside its body.
fn stops_by_panel(shell: &mut Shell, workspace: Workspace) -> Vec<(String, usize)> {
    shell.state.active_workspace = workspace;
    shell.settled();
    let mut stops = Vec::new();
    let mut reached = Vec::new();
    for _ in 0..TABS {
        shell.draw(vec![key(egui::Key::Tab)]);
        let Some((rect, _)) = shell.focused() else {
            continue;
        };
        let at = format!("{rect:?}");
        if reached.contains(&at) {
            break;
        }
        reached.push(at);
        stops.push(rect);
    }
    let dock = shell
        .state
        .workspace_docks
        .peek(workspace)
        .expect("the workspace has a layout");
    dock.iter_leaves()
        .map(|(_, leaf)| {
            let panel = leaf.tabs[leaf.active.0].id().to_string();
            let body =
                Rect::from_min_max(leaf.viewport.min + egui::vec2(0.0, 22.0), leaf.viewport.max);
            let inside = stops.iter().filter(|r| body.contains(r.center())).count();
            (panel, inside)
        })
        .collect()
}

/// The panels with something to do before any file is open are reached by Tab: Files, the Document and the 3D viewport. The rest show only a line saying what to open.
#[test]
fn tab_reaches_into_the_panels_that_have_controls_before_a_file_is_open() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let _folder = shell.open_a_folder();
    for workspace in Workspace::ALL {
        for (panel, stops) in stops_by_panel(&mut shell, workspace) {
            if ["files", "document", "viewport3d"].contains(&panel.as_str()) {
                assert!(
                    stops > 0,
                    "{workspace:?}: Tab never goes into the {panel} panel"
                );
            }
        }
    }
}

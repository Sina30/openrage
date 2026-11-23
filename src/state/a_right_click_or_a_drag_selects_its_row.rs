//! A right click or the start of a drag in Files or the Files view selects the row it lands on, and
//! a drag or a drop gives its panel focus, so the keys afterwards act on that row.

use egui::PointerButton::{Primary, Secondary};

use super::navigation::Location;
use super::the_files_view_in_the_centre::{
    accessible, browsing, click_with, find, frame, labels, leaf_of, node, settled,
};
use super::undo_from_files_acts_on_what_files_shows::{
    an_archive, ctrl_z, name_of, rename_with_f2,
};
use super::{AppState, Prefs};
use crate::ui::panels::registry::ids;

/// One frame of the dock and then of what crosses the window's edge, as the app draws them.
pub(super) fn frame_with(
    ctx: &egui::Context,
    state: &mut AppState,
    input: egui::RawInput,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        ..input
    };
    labels(&ctx.run_ui(input, |ui| {
        crate::ui::dock::draw(ui, state, None);
        crate::ui::os_drop::draw(ui.ctx(), state);
    }))
}

/// A press on `from`, a move past egui's drag threshold, and a release where it ended.
fn drag(ctx: &egui::Context, state: &mut AppState, from: egui::Pos2) {
    let to = from + egui::vec2(40.0, 0.0);
    let press = |at, pressed| egui::Event::PointerButton {
        pos: at,
        button: Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [
        vec![egui::Event::PointerMoved(from), press(from, true)],
        vec![egui::Event::PointerMoved(from + egui::vec2(12.0, 0.0))],
        vec![egui::Event::PointerMoved(to)],
        vec![press(to, false)],
    ] {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        frame_with(ctx, state, input);
    }
}

/// ⚠ Files lists two archives and the selection is in the other, so neither the only tree Files
/// lists nor the selection already names the row's archive.
#[test]
fn a_right_click_on_a_row_not_selected_selects_it_and_f2_renames_it() {
    let mut state = AppState::from_prefs(Prefs::default());
    let (other, kept) = an_archive(&mut state);
    (state.rpf_rename_node(other, kept, "kept.txt")).expect("a rename in the other archive");
    let (archive, notes) = an_archive(&mut state);
    state.selection.select(other, kept);
    state.navigation.reveal = Some(Location {
        doc: archive,
        node: notes,
    });
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let at = find(&shown, leaf_of(&state, ids::FILES), "notes.txt");

    click_with(&ctx, &mut state, at, Secondary);
    assert_eq!(
        state.selection.current(),
        Some((archive, notes)),
        "the right click left notes.txt unselected"
    );
    assert_eq!(
        state.workspace_docks.focused(state.active_workspace),
        Some(ids::FILES)
    );
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, &mut state, vec![escape]);
    rename_with_f2(&ctx, &mut state, "readme.txt");
    assert_eq!(
        name_of(&state, archive, notes),
        "readme.txt",
        "F2 renamed another row"
    );

    ctrl_z(&mut state, &ctx);
    assert_eq!(
        name_of(&state, archive, notes),
        "notes.txt",
        "Ctrl+Z in Files left the rename"
    );
    assert_eq!(
        name_of(&state, other, kept),
        "kept.txt",
        "Ctrl+Z undid the other archive's rename"
    );
}

#[test]
fn a_file_dropped_onto_files_gives_files_focus() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    frame_with(&ctx, &mut state, egui::RawInput::default());
    let shown = frame_with(&ctx, &mut state, egui::RawInput::default());
    let ws = state.active_workspace;
    assert_eq!(
        state.workspace_docks.focused(ws),
        Some(ids::DOCUMENT),
        "Browse starts with its centre focused"
    );
    let at = find(&shown, leaf_of(&state, ids::FILES), "props");

    let disk = tempfile::tempdir().expect("a scratch folder");
    let dropped = disk.path().join("sign.ytd");
    std::fs::write(&dropped, b"bytes").expect("a file to drop");
    let input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(at)],
        dropped_files: vec![egui::DroppedFile {
            path: Some(dropped),
            ..Default::default()
        }],
        ..Default::default()
    };
    frame_with(&ctx, &mut state, input);
    node(&state, doc, "props/sign.ytd");
    assert_eq!(
        state.workspace_docks.focused(ws),
        Some(ids::FILES),
        "the drop left Files unfocused"
    );
}

#[test]
fn the_start_of_a_drag_selects_its_row_and_gives_its_panel_focus() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    frame_with(&ctx, &mut state, egui::RawInput::default());
    let shown = frame_with(&ctx, &mut state, egui::RawInput::default());
    let ws = state.active_workspace;

    let notes = find(&shown, leaf_of(&state, ids::FILES), "notes.txt");
    drag(&ctx, &mut state, notes);
    assert_eq!(
        state.selection.selected_ids(doc),
        [node(&state, doc, "notes.txt")],
        "a drag in Files left its row unselected"
    );
    assert_eq!(
        state.workspace_docks.focused(ws),
        Some(ids::FILES),
        "a drag in Files left it unfocused"
    );

    let shown = frame_with(&ctx, &mut state, egui::RawInput::default());
    let big = find(&shown, leaf_of(&state, ids::DOCUMENT), "big.bin");
    drag(&ctx, &mut state, big);
    assert_eq!(
        state.selection.selected_ids(doc),
        [node(&state, doc, "big.bin")],
        "a drag in the Files view left its row unselected"
    );
    assert_eq!(
        state.workspace_docks.focused(ws),
        Some(ids::DOCUMENT),
        "a drag in the Files view left it unfocused"
    );
}

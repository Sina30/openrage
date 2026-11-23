//! Edit ▸ Cut, Copy, Paste and Delete, their keys, and the Files view's menus.

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::super::the_files_view_in_the_centre::{
    accessible, browsing, click, click_with, find, frame, in_the_listing, leaf_of, listed, node,
    settled, submenu,
};
use super::*;
use crate::platform::clipboard::fake;
use crate::ui::commands::{self, Action};
use crate::ui::panels::registry::ids;
use crate::ui::shortcuts;

fn focus(state: &mut AppState, panel: &'static str) {
    let ws = state.active_workspace;
    crate::ui::dock_commands::reveal(state.workspace_docks.get_mut(ws), panel);
}

fn ctrl(state: &AppState, letter: &str, code: KeyCode, typing: bool) -> Option<Action> {
    let logical = Key::Character(letter.into());
    let mods = crate::ui::shortcuts::PRIMARY_STATE;
    shortcuts::action_for_press(
        &logical,
        PhysicalKey::Code(code),
        mods,
        typing,
        &state.prefs,
    )
}

/// ⚠ The keys arrive as winit hands them over, and run through the registry as a press does.
#[test]
fn ctrl_c_and_ctrl_v_copy_and_paste_the_rows_the_focused_panel_selects() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    let notes = node(&state, doc, "notes.txt");
    let textures = node(&state, doc, "textures");
    focus(&mut state, ids::FILES);

    for (letter, code) in [
        ("c", KeyCode::KeyC),
        ("x", KeyCode::KeyX),
        ("v", KeyCode::KeyV),
    ] {
        assert_eq!(
            ctrl(&state, letter, code, true),
            None,
            "a field with focus keeps Ctrl+{letter}"
        );
    }
    let delete = shortcuts::action_for_press(
        &Key::Named(NamedKey::Delete),
        PhysicalKey::Code(KeyCode::Delete),
        ModifiersState::empty(),
        true,
        &state.prefs,
    );
    assert_eq!(delete, None, "a field with focus keeps Delete");

    state.selection.select(doc, notes);
    let paste = ctrl(&state, "v", KeyCode::KeyV, false).expect("Ctrl+V is bound");
    assert_eq!(
        commands::of(&state, paste).map(|c| c.enabled),
        Some(Err(commands::NOTHING_TO_PASTE))
    );
    let copy = ctrl(&state, "c", KeyCode::KeyC, false).expect("Ctrl+C is bound");
    commands::run_action(&mut state, &ctx, copy);
    let offered = fake::offered();
    assert_eq!(offered.len(), 1, "Ctrl+C copied the selected row");

    state.selection.select(doc, textures);
    commands::run_action(&mut state, &ctx, paste);
    assert_eq!(names_under(&state, doc, textures), ["notes.txt"]);

    let pasted = node(&state, doc, "textures/notes.txt");
    state.selection.select(doc, pasted);
    let delete = shortcuts::action_for_press(
        &Key::Named(NamedKey::Delete),
        PhysicalKey::Code(KeyCode::Delete),
        ModifiersState::empty(),
        false,
        &state.prefs,
    )
    .expect("Delete is bound");
    commands::run_action(&mut state, &ctx, delete);
    assert!(state.pending_delete.is_some(), "Delete asks first");
    forget_offered(&offered);
}

/// ⚠ The menus are the Files view's own, opened with a right click and
/// clicked as a user does, so a menu item wired to nothing fails here.
#[test]
fn a_rows_menu_cuts_copies_and_pastes_and_the_empty_spaces_menu_pastes() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);
    let menu_item = |state: &mut AppState, row: egui::Pos2, item: &str| {
        click_with(&ctx, state, row, egui::PointerButton::Secondary);
        let mut menu = frame(&ctx, state, Vec::new());
        // A row keeps what changes it under Edit; the empty space offers Paste itself.
        if !menu.iter().any(|(text, _)| text == item) {
            menu = submenu(&ctx, state, &menu, "Edit");
        }
        click(&ctx, state, find(&menu, egui::Rect::EVERYTHING, item));
        settled(&ctx, state)
    };

    let notes = find(&shown, centre, "notes.txt");
    menu_item(
        &mut state,
        notes,
        &format!("Copy {}+C", crate::ui::shortcuts::primary_modifier_name()),
    );
    let offered = fake::offered();
    let textures = find(&shown, centre, "textures");
    menu_item(
        &mut state,
        textures,
        &format!("Paste {}+V", crate::ui::shortcuts::primary_modifier_name()),
    );
    let into = node(&state, doc, "textures");
    assert_eq!(names_under(&state, doc, into), ["notes.txt"]);

    let big = find(&shown, centre, "big.bin");
    menu_item(
        &mut state,
        big,
        &format!("Cut {}+X", crate::ui::shortcuts::primary_modifier_name()),
    );
    state.navigate_to(crate::state::navigation::Location { doc, node: into });
    settled(&ctx, &mut state);
    let empty = in_the_listing(&state);
    menu_item(
        &mut state,
        empty,
        &format!("Paste {}+V", crate::ui::shortcuts::primary_modifier_name()),
    );
    assert_eq!(listed(&state, doc), ["big.bin", "notes.txt"]);
    let root = node(&state, doc, "");
    assert!(
        !names_under(&state, doc, root).contains(&"big.bin".to_string()),
        "the cut row left the folder it was cut from"
    );
    forget_offered(&offered);
}

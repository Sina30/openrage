//! The Files view from the keyboard.

use super::*;

#[test]
fn the_arrows_move_shift_extends_ctrl_a_takes_all_and_typing_jumps_to_a_name() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);

    key(&ctx, &mut state, egui::Key::ArrowDown);
    assert_eq!(
        selected(&state, doc),
        ["props"],
        "the first press takes the first row"
    );
    key(&ctx, &mut state, egui::Key::ArrowDown);
    key(&ctx, &mut state, egui::Key::ArrowDown);
    assert_eq!(selected(&state, doc), ["big.bin"]);
    key_with(&ctx, &mut state, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    let mut extended = selected(&state, doc);
    extended.sort();
    assert_eq!(
        extended,
        ["big.bin", "textures"],
        "Shift spans from the row it left"
    );
    key(&ctx, &mut state, egui::Key::End);
    assert_eq!(selected(&state, doc), ["notes.txt"]);

    let command = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    key_with(&ctx, &mut state, egui::Key::A, command);
    assert_eq!(
        state.selection.selected_ids(doc).len(),
        4,
        "Ctrl+A takes every row"
    );

    type_text(&ctx, &mut state, "t");
    assert_eq!(
        selected(&state, doc),
        ["textures"],
        "typing jumps to the name"
    );
    type_text(&ctx, &mut state, "b");
    assert_eq!(
        selected(&state, doc),
        ["textures"],
        "letters typed together add up, and no name starts `tb`"
    );
}

#[test]
fn a_letter_typed_again_steps_on_to_the_next_name_it_starts() {
    let (mut state, doc) = browsing();
    let root = node(&state, doc, "");
    if let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) {
        rpf.session
            .add_file_copy(root, "notes2.txt", vec![1])
            .expect("a second n");
    }
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    type_text(&ctx, &mut state, "n");
    assert_eq!(selected(&state, doc), ["notes.txt"]);
    type_text(&ctx, &mut state, "n");
    assert_eq!(selected(&state, doc), ["notes2.txt"]);
}

#[test]
fn enter_goes_into_a_folder_and_backspace_comes_back_up_with_it_selected() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    key(&ctx, &mut state, egui::Key::ArrowDown);
    key(&ctx, &mut state, egui::Key::Enter);
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);

    key(&ctx, &mut state, egui::Key::Backspace);
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(
        listed(&state, doc),
        ["props", "textures", "big.bin", "notes.txt"]
    );
    assert_eq!(
        selected(&state, doc),
        ["props"],
        "the folder left is selected"
    );

    assert!(state.navigate_back(), "Back returns into the folder");
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);
    assert!(state.navigate_back());
    assert!(state.navigate_forward());
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);
}

#[test]
fn enter_on_a_file_opens_it() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    key(&ctx, &mut state, egui::Key::End);
    key(&ctx, &mut state, egui::Key::Enter);
    let notes = node(&state, doc, "notes.txt");
    let opened = |state: &AppState| {
        matches!(
            state.document(doc).map(|d| &d.payload),
            Some(DocPayload::Rpf(rpf)) if rpf.entry_documents.contains_key(&notes)
        )
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !opened(&state) && std::time::Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(opened(&state), "Enter opened nothing");
}

#[test]
fn space_shows_and_hides_the_preview_pane() {
    let (mut state, _) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    let hidden = |state: &AppState| {
        state
            .panels
            .peek::<FilesViewUi>()
            .is_some_and(|v| v.preview_hidden)
    };
    assert!(!hidden(&state), "the pane starts shown");
    key(&ctx, &mut state, egui::Key::Space);
    assert!(hidden(&state));
    key(&ctx, &mut state, egui::Key::Space);
    assert!(!hidden(&state));
}

/// F2 as winit hands it over, run through the registry as the key dispatcher runs a press.
fn f2(ctx: &egui::Context, state: &mut AppState) {
    use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
    let pressed = crate::ui::shortcuts::action_for_press(
        &Key::Named(NamedKey::F2),
        PhysicalKey::Code(KeyCode::F2),
        ModifiersState::empty(),
        false,
        &state.prefs,
    )
    .expect("F2 is bound");
    crate::ui::commands::run_action(state, ctx, pressed);
    frame(ctx, state, Vec::new());
}

#[test]
fn f2_renames_in_place_and_a_taken_name_is_refused_with_both_files_left_as_they_were() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    key(&ctx, &mut state, egui::Key::End);
    f2(&ctx, &mut state);
    let notes = node(&state, doc, "notes.txt");
    assert!(
        ui_state(&mut state).is_renaming_in_centre(doc, notes),
        "F2 edits the name where the row is"
    );
    frame(&ctx, &mut state, Vec::new());
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Text("readme.txt".into())],
    );
    key(&ctx, &mut state, egui::Key::Enter);
    assert_eq!(name_of(&state, doc, notes), "readme.txt");

    f2(&ctx, &mut state);
    frame(&ctx, &mut state, Vec::new());
    frame(&ctx, &mut state, vec![egui::Event::Text("big.bin".into())]);
    key(&ctx, &mut state, egui::Key::Enter);
    assert_eq!(
        name_of(&state, doc, notes),
        "readme.txt",
        "the taken name is refused"
    );
    assert!(node(&state, doc, "big.bin") != notes);
    assert!(
        ui_state(&mut state).is_renaming_in_centre(doc, notes),
        "the editor stays open on the refused name"
    );
}

/// A press of a named key as the editor's window handles it: a command bound to the key runs
/// and takes it, else it reaches the panels. What the press ran.
fn pressed(
    ctx: &egui::Context,
    state: &mut AppState,
    (named, code, egui_key): (
        winit::keyboard::NamedKey,
        winit::keyboard::KeyCode,
        egui::Key,
    ),
) -> Option<crate::ui::commands::Action> {
    use winit::keyboard::{Key, ModifiersState, PhysicalKey};
    let ran = crate::ui::shortcuts::action_for_press(
        &Key::Named(named),
        PhysicalKey::Code(code),
        ModifiersState::empty(),
        false,
        &state.prefs,
    );
    match ran {
        Some(action) => {
            crate::ui::commands::run_action(state, ctx, action);
            frame(ctx, state, Vec::new());
        }
        None => key(ctx, state, egui_key),
    }
    ran
}

fn delete_key() -> (
    winit::keyboard::NamedKey,
    winit::keyboard::KeyCode,
    egui::Key,
) {
    use winit::keyboard::{KeyCode, NamedKey};
    (NamedKey::Delete, KeyCode::Delete, egui::Key::Delete)
}

#[test]
fn delete_asks_before_anything_goes() {
    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    key(&ctx, &mut state, egui::Key::End);
    pressed(&ctx, &mut state, delete_key());
    let asked = state.pending_delete.clone().expect("Delete asks first");
    assert_eq!(asked.name, "notes.txt");
    assert_eq!(
        listed(&state, doc).len(),
        4,
        "nothing went before the answer"
    );
}

#[test]
fn the_files_view_deletes_on_the_key_the_user_bound_to_delete_and_no_other() {
    use crate::ui::commands::Action;
    use crate::ui::shortcuts::{force_set_binding, ShortcutMods};
    use winit::keyboard::{KeyCode, NamedKey};

    let (mut state, doc) = browsing();
    let ctx = egui::Context::default();
    settled(&ctx, &mut state);
    key(&ctx, &mut state, egui::Key::End);

    // Delete moves to F8: the key it left runs nothing, and the Files view leaves the file be.
    force_set_binding(
        &mut state.prefs,
        Action::Delete,
        ShortcutMods::none(),
        egui::Key::F8,
    );
    assert_eq!(pressed(&ctx, &mut state, delete_key()), None);
    assert!(
        state.pending_delete.is_none(),
        "the Files view asked to delete on Delete, which is bound to something else"
    );
    assert_eq!(listed(&state, doc).len(), 4);
    let f8 = (NamedKey::F8, KeyCode::F8, egui::Key::F8);
    assert_eq!(pressed(&ctx, &mut state, f8), Some(Action::Delete));
    assert_eq!(
        state
            .pending_delete
            .as_ref()
            .map(|asked| asked.name.as_str()),
        Some("notes.txt"),
        "the key Delete was bound to asks to delete"
    );
    state.pending_delete = None;
    frame(&ctx, &mut state, Vec::new());

    // And Delete bound to another command runs that command, not a delete.
    force_set_binding(
        &mut state.prefs,
        Action::ShowGrid,
        ShortcutMods::none(),
        egui::Key::Delete,
    );
    let grid = state.prefs.viewport.show_grid;
    assert_eq!(
        pressed(&ctx, &mut state, delete_key()),
        Some(Action::ShowGrid)
    );
    assert_ne!(state.prefs.viewport.show_grid, grid);
    assert!(state.pending_delete.is_none());
    assert_eq!(listed(&state, doc).len(), 4);
}

//! With Files focused, Undo, Redo and the History panel act on the install, folder or archive
//! Files shows, and never on the model active beside it.

use egui::PointerButton::{Primary, Secondary};
use or_io::desc::ScalarValue;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::destructive_file_operations_ask::node_at;
use super::every_edit_undoes::{drawable, float_field};
use super::navigation::Location;
use super::the_files_view_in_the_centre::{
    accessible, click, click_with, find, frame, leaf_of, settled, submenu,
};
use super::{AppState, DocPayload, DocumentId, DocumentKind, Prefs, RpfNodeId};
use crate::ui::commands;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use crate::ui::shortcuts;

/// `key` as winit hands it over, run through the registry as the key dispatcher runs a press.
fn press(state: &mut AppState, ctx: &egui::Context, key: Key, code: KeyCode, mods: ModifiersState) {
    let code = PhysicalKey::Code(code);
    let action = shortcuts::action_for_press(&key, code, mods, false, &state.prefs)
        .expect("the key is bound");
    commands::run_action(state, ctx, action);
}

pub(super) fn ctrl_z(state: &mut AppState, ctx: &egui::Context) {
    let z = Key::Character("z".into());
    press(
        state,
        ctx,
        z,
        KeyCode::KeyZ,
        crate::ui::shortcuts::PRIMARY_STATE,
    );
}

fn ctrl_y(state: &mut AppState, ctx: &egui::Context) {
    let y = Key::Character("y".into());
    press(
        state,
        ctx,
        y,
        KeyCode::KeyY,
        crate::ui::shortcuts::PRIMARY_STATE,
    );
}

/// The steps `doc`'s history holds done.
fn steps(state: &AppState, doc: DocumentId) -> usize {
    state.document(doc).expect("open").history.done().len()
}

pub(super) fn name_of(state: &AppState, doc: DocumentId, row: RpfNodeId) -> String {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.node(row).map(|n| n.name.clone()),
        _ => None,
    }
    .unwrap_or_default()
}

/// A drawable with one edit active in Model, and a click of `button` on `tree`'s row `row` in Files beside it.
fn a_click_in_files_beside_an_edited_model(
    state: &mut AppState,
    tree: DocumentId,
    row: RpfNodeId,
    button: egui::PointerButton,
) -> (egui::Context, DocumentId) {
    let model = drawable(state, "probe.ydr");
    let (label, edit) = float_field(state, model, 0);
    assert!(state.patch_scalar(model, &label, edit, ScalarValue::F32(137.5)));
    state.set_active_document(model);
    assert_eq!(state.active_workspace, Workspace::Model);
    state.navigation.reveal = Some(Location {
        doc: tree,
        node: row,
    });
    let ctx = accessible();
    let shown = settled(&ctx, state);
    let at = find(
        &shown,
        leaf_of(state, ids::FILES),
        &name_of(state, tree, row),
    );
    click_with(&ctx, state, at, button);
    assert_eq!(
        state.workspace_docks.focused(state.active_workspace),
        Some(ids::FILES),
        "the click left Files unfocused"
    );
    assert_eq!(
        state.active_document,
        Some(model),
        "the click took the model off screen"
    );
    (ctx, model)
}

/// `name` typed over the name being edited in place, and Enter.
fn type_name(ctx: &egui::Context, state: &mut AppState, name: &str) {
    frame(ctx, state, Vec::new());
    frame(ctx, state, Vec::new());
    frame(ctx, state, vec![egui::Event::Text(name.into())]);
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(ctx, state, vec![enter]);
}

pub(super) fn rename_with_f2(ctx: &egui::Context, state: &mut AppState, name: &str) {
    let f2 = Key::Named(NamedKey::F2);
    press(state, ctx, f2, KeyCode::F2, ModifiersState::empty());
    type_name(ctx, state, name);
}

/// An archive holding `notes.txt`, none of it on disk.
pub(super) fn an_archive(state: &mut AppState) -> (DocumentId, RpfNodeId) {
    let archive = state.new_empty_document(DocumentKind::Rpf);
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(archive).map(|d| &mut d.payload) else {
        panic!("an empty archive document holds an archive");
    };
    let root = rpf.session.root_id();
    let notes = (rpf.session)
        .add_file_copy(root, "notes.txt", b"a note".to_vec())
        .expect("a file in the archive");
    (archive, notes)
}

/// ⚠ Files lists a second archive with an edit of its own, above the one renamed in.
#[test]
fn ctrl_z_in_files_undoes_the_rename_made_there_and_keeps_the_active_models_edit() {
    let mut state = AppState::from_prefs(Prefs::default());
    let (other, kept) = an_archive(&mut state);
    (state.rpf_rename_node(other, kept, "kept.txt")).expect("a rename in the other archive");
    let (archive, notes) = an_archive(&mut state);
    let (ctx, model) = a_click_in_files_beside_an_edited_model(&mut state, archive, notes, Primary);
    rename_with_f2(&ctx, &mut state, "readme.txt");
    assert_eq!(
        name_of(&state, archive, notes),
        "readme.txt",
        "F2 renamed nothing"
    );

    ctrl_z(&mut state, &ctx);
    assert_eq!(
        name_of(&state, archive, notes),
        "notes.txt",
        "Ctrl+Z left the rename"
    );
    assert_eq!(steps(&state, model), 1, "Ctrl+Z undid the model's edit");
    assert_eq!(
        steps(&state, other),
        1,
        "Ctrl+Z undid the other archive's edit"
    );

    ctrl_y(&mut state, &ctx);
    assert_eq!(
        name_of(&state, archive, notes),
        "readme.txt",
        "Ctrl+Y left the rename undone"
    );
    assert_eq!(steps(&state, model), 1, "Ctrl+Y reached the model");
}

/// ⚠ The rename is made from the row's menu, which a right click opens, rather than with F2.
#[test]
fn ctrl_z_in_files_undoes_a_rename_from_a_rows_menu() {
    let mut state = AppState::from_prefs(Prefs::default());
    let (archive, notes) = an_archive(&mut state);
    let (ctx, model) =
        a_click_in_files_beside_an_edited_model(&mut state, archive, notes, Secondary);
    let menu = frame(&ctx, &mut state, Vec::new());
    // What changes a row is under Edit in its menu.
    let menu = submenu(&ctx, &mut state, &menu, "Edit");
    click(
        &ctx,
        &mut state,
        find(&menu, egui::Rect::EVERYTHING, "Rename F2"),
    );
    type_name(&ctx, &mut state, "readme.txt");
    assert_eq!(
        name_of(&state, archive, notes),
        "readme.txt",
        "the menu renamed nothing"
    );

    ctrl_z(&mut state, &ctx);
    assert_eq!(
        name_of(&state, archive, notes),
        "notes.txt",
        "Ctrl+Z left the rename"
    );
    assert_eq!(steps(&state, model), 1, "Ctrl+Z undid the model's edit");
}

/// ⚠ History opens in the focused panel's place and takes its focus, so it follows Files.
#[test]
fn the_history_panel_opened_from_files_lists_and_undoes_the_rename_made_there() {
    let mut state = AppState::from_prefs(Prefs::default());
    let (archive, notes) = an_archive(&mut state);
    let (ctx, model) = a_click_in_files_beside_an_edited_model(&mut state, archive, notes, Primary);
    rename_with_f2(&ctx, &mut state, "readme.txt");
    let edit = state.document(model).expect("open").history.done()[0]
        .label
        .clone();

    let history = (commands::all(&state).into_iter())
        .find(|command| command.id == "panel: history")
        .expect("the palette offers the History panel");
    commands::run(&mut state, &ctx, &history);
    let shown = frame(&ctx, &mut state, Vec::new());
    let listed = |label: &str| shown.iter().any(|(text, _)| text == label);
    assert!(
        listed("Rename notes.txt to readme.txt") && !listed(&edit),
        "the History panel lists another document's edits: {shown:?}"
    );

    ctrl_z(&mut state, &ctx);
    assert_eq!(
        name_of(&state, archive, notes),
        "notes.txt",
        "Ctrl+Z in the History panel left the rename"
    );
    assert_eq!(steps(&state, model), 1, "Ctrl+Z undid the model's edit");
}

/// ⚠ A rename on disk is made at once and is no step, so Undo there has nothing to take back.
#[test]
fn a_rename_in_a_folder_on_disk_is_no_step_and_ctrl_z_in_files_leaves_the_models_edit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("notes.txt"), b"a note").expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.open_folder(tmp.path().to_path_buf());
    let folder = state
        .active_document
        .expect("the folder opens as a document");
    let notes = node_at(&mut state, folder, "notes.txt");
    let (ctx, model) = a_click_in_files_beside_an_edited_model(&mut state, folder, notes, Primary);
    rename_with_f2(&ctx, &mut state, "readme.txt");
    assert!(
        tmp.path().join("readme.txt").is_file(),
        "F2 renamed nothing on disk"
    );

    ctrl_z(&mut state, &ctx);
    assert!(
        tmp.path().join("readme.txt").is_file(),
        "Ctrl+Z took back a rename made on disk"
    );
    assert_eq!(steps(&state, model), 1, "Ctrl+Z undid the model's edit");
}

//! A drag onto a folder copies when Ctrl is held at the drop; a selection holding the folder being
//! viewed still carries its other rows; and a drop, a copy or a rename that changes nothing writes
//! nothing, asks the edit gate nothing and records no step.

use egui::Modifiers;
use egui::PointerButton::{Primary, Secondary};

use super::a_right_click_or_a_drag_selects_its_row::frame_with;
use super::copy_cut_and_paste::{forget_offered, names_under};
use super::navigation::Location;
use super::nested_archives_survive_a_save::{archive, errors};
use super::opening_a_ydd::node_at;
use super::prefs::EditMode;
use super::the_files_view_in_the_centre::{
    accessible, browsing, click, click_with, find, frame, in_the_listing, leaf_of, listed, node,
    settled,
};
use super::two_readers_of_one_archive_across_a_save::install;
use super::undo_from_files_acts_on_what_files_shows::{ctrl_z, name_of};
use super::{AppState, DocPayload, DocumentId, RetailEditDecision};
use crate::platform::clipboard::fake;
use crate::ui::panels::registry::ids;

fn button(at: egui::Pos2, pressed: bool, modifiers: Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: Primary,
        pressed,
        modifiers,
    }
}

/// A frame of the dock with `events` and `modifiers` held.
fn held(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
    modifiers: Modifiers,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        events,
        modifiers,
        ..Default::default()
    };
    frame_with(ctx, state, input)
}

/// A press on `from`, a move past egui's drag threshold, and a release over `to` with `at_the_drop` held.
fn drag(
    ctx: &egui::Context,
    state: &mut AppState,
    from: egui::Pos2,
    to: egui::Pos2,
    at_the_drop: Modifiers,
) {
    let none = Modifiers::NONE;
    let moved = |by: f32| vec![egui::Event::PointerMoved(from + egui::vec2(by, 0.0))];
    held(
        ctx,
        state,
        vec![egui::Event::PointerMoved(from), button(from, true, none)],
        none,
    );
    held(ctx, state, moved(12.0), none);
    held(ctx, state, vec![egui::Event::PointerMoved(to)], at_the_drop);
    held(
        ctx,
        state,
        vec![button(to, false, at_the_drop)],
        at_the_drop,
    );
    held(ctx, state, Vec::new(), none);
}

/// A click on `at` with `modifiers` held.
fn click_holding(ctx: &egui::Context, state: &mut AppState, at: egui::Pos2, modifiers: Modifiers) {
    let events = vec![egui::Event::PointerMoved(at), button(at, true, modifiers)];
    held(ctx, state, events, modifiers);
    held(ctx, state, vec![button(at, false, modifiers)], modifiers);
    held(ctx, state, Vec::new(), Modifiers::NONE);
}

fn steps(state: &AppState, doc: DocumentId) -> usize {
    state.document(doc).expect("open").history.done().len()
}

/// The row `name` of the Files view's menu, clicked as a user does from a right click on `row`.
fn menu_item(ctx: &egui::Context, state: &mut AppState, row: egui::Pos2, item: &str) {
    click_with(ctx, state, row, Secondary);
    let menu = frame(ctx, state, Vec::new());
    click(ctx, state, find(&menu, egui::Rect::EVERYTHING, item));
}

/// ⚠ Ctrl is held for the last two frames only, as a user presses it during the drag.
#[test]
fn a_row_dropped_onto_a_folder_with_ctrl_held_is_copied_and_one_ctrl_z_takes_the_copy_away() {
    for panel in [ids::FILES, ids::DOCUMENT] {
        let (mut state, doc) = browsing();
        let root = node(&state, doc, "");
        let ctx = accessible();
        held(&ctx, &mut state, Vec::new(), Modifiers::NONE);
        let shown = held(&ctx, &mut state, Vec::new(), Modifiers::NONE);
        let area = leaf_of(&state, panel);

        drag(
            &ctx,
            &mut state,
            find(&shown, area, "notes.txt"),
            find(&shown, area, "props"),
            Modifiers::CTRL,
        );
        let props = node(&state, doc, "props");
        assert_eq!(
            names_under(&state, doc, props),
            ["chair.ydr", "lamp.ydr", "notes.txt"],
            "the drop in {panel} copied nothing: {:?}",
            errors(&state)
        );
        assert_eq!(
            names_under(&state, doc, root),
            ["big.bin", "notes.txt", "props", "textures"],
            "the drop in {panel} moved the row"
        );
        assert!(
            fake::offered().is_empty(),
            "the drop in {panel} took over the clipboard"
        );

        ctrl_z(&mut state, &ctx);
        assert_eq!(
            names_under(&state, doc, props),
            ["chair.ydr", "lamp.ydr"],
            "one Ctrl+Z after the drop in {panel} left the copy"
        );
        assert_eq!(
            names_under(&state, doc, root),
            ["big.bin", "notes.txt", "props", "textures"],
            "the Ctrl+Z in {panel} took the row itself"
        );
    }
}

/// ⚠ The archive's top folder is shown and selected by a click on the listing's empty space, and
/// the rows are added with Ctrl, as a user does.
#[test]
fn a_selection_holding_the_folder_being_viewed_copies_and_drags_its_other_rows() {
    let (mut state, doc) = browsing();
    let (root, textures) = (node(&state, doc, ""), node(&state, doc, "textures"));
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);

    let empty = in_the_listing(&state);
    click(&ctx, &mut state, empty);
    assert_eq!(
        state.selection.selected(doc),
        Some(root),
        "the click on the empty space did not select the folder shown"
    );
    for row in ["notes.txt", "big.bin"] {
        click_holding(&ctx, &mut state, find(&shown, centre, row), Modifiers::CTRL);
    }

    menu_item(
        &ctx,
        &mut state,
        find(&shown, centre, "notes.txt"),
        &format!("Copy {}+C", crate::ui::shortcuts::primary_modifier_name()),
    );
    let offered = fake::offered();
    assert_eq!(
        offered.len(),
        2,
        "the menu's Copy carried {} rows: {:?}",
        offered.len(),
        errors(&state)
    );
    forget_offered(&offered);

    settled(&ctx, &mut state);
    drag(
        &ctx,
        &mut state,
        find(&shown, centre, "big.bin"),
        find(&shown, centre, "textures"),
        Modifiers::NONE,
    );
    assert_eq!(
        names_under(&state, doc, textures),
        ["big.bin", "notes.txt"],
        "the drag carried {:?}",
        errors(&state)
    );
}

/// ⚠ Going into `props` selects it, and it is the folder shown; two of its rows added with Ctrl
/// are the selection, and `props` with all it holds is not.
#[test]
fn a_row_added_in_the_files_view_leaves_the_folder_being_viewed_out_of_the_selection() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let props_in_files = find(&shown, leaf_of(&state, ids::FILES), "props");
    click(&ctx, &mut state, props_in_files);
    let shown = frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);
    let props = node(&state, doc, "props");
    assert_eq!(state.selection.selected(doc), Some(props));

    let centre = leaf_of(&state, ids::DOCUMENT);
    for row in ["lamp.ydr", "chair.ydr"] {
        click_holding(&ctx, &mut state, find(&shown, centre, row), Modifiers::CTRL);
    }
    let mut chosen: Vec<String> = (state.selection.selected_ids(doc).into_iter())
        .map(|row| name_of(&state, doc, row))
        .collect();
    chosen.sort();
    assert_eq!(chosen, ["chair.ydr", "lamp.ydr"]);

    menu_item(
        &ctx,
        &mut state,
        find(&shown, centre, "lamp.ydr"),
        &format!("Copy {}+C", crate::ui::shortcuts::primary_modifier_name()),
    );
    let offered = fake::offered();
    let named: Vec<String> = offered
        .iter()
        .filter_map(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        named.len(),
        2,
        "Copy offered {named:?}: {:?}",
        errors(&state)
    );
    forget_offered(&offered);
}

#[test]
fn a_selection_holding_the_root_still_copies_the_rows_beside_it() {
    let (mut state, doc) = browsing();
    let rows = ["", "notes.txt", "big.bin"].map(|n| node(&state, doc, n));

    let said = state.copy_rows(doc, &rows, false);
    assert!(said.is_ok(), "{said:?}");
    let offered = fake::offered();
    assert_eq!(offered.len(), 2, "{offered:?}");
    forget_offered(&offered);
}

/// ⚠ `lamp.ydr` is shown in the tree under `props`, which the reveal opens, so the drop is on the
/// folder the row is in.
#[test]
fn a_row_dropped_onto_the_folder_it_sits_in_records_no_step_whether_it_moves_or_copies() {
    let (mut state, doc) = browsing();
    let props = node(&state, doc, "props");
    let lamp = node(&state, doc, "props/lamp.ydr");
    state.navigation.reveal = Some(Location { doc, node: lamp });
    let ctx = accessible();
    held(&ctx, &mut state, Vec::new(), Modifiers::NONE);
    let shown = held(&ctx, &mut state, Vec::new(), Modifiers::NONE);
    let tree = leaf_of(&state, ids::FILES);
    let before = steps(&state, doc);

    for (modifiers, what) in [(Modifiers::NONE, "move"), (Modifiers::CTRL, "copy")] {
        drag(
            &ctx,
            &mut state,
            find(&shown, tree, "lamp.ydr"),
            find(&shown, tree, "props"),
            modifiers,
        );
        assert_eq!(
            steps(&state, doc),
            before,
            "a {what} onto its own folder recorded a step"
        );
        assert_eq!(names_under(&state, doc, props), ["chair.ydr", "lamp.ydr"]);
    }
    assert_eq!(errors(&state), Vec::<String>::new());
}

/// ⚠ An install, so Read only refuses a write and Everything asks before one.
#[test]
fn a_move_or_a_rename_that_changes_nothing_asks_the_edit_gate_nothing_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(dir.path(), &[("a.rpf", archive(&[("one.txt", b"one")]))]);
    let a_row = node_at(&mut state, tree, "a.rpf");
    let one = node_at(&mut state, tree, "a.rpf/one.txt");
    let (before, bytes) = (
        steps(&state, tree),
        std::fs::read(dir.path().join("a.rpf")).expect("the archive"),
    );
    state.retail_edit_session_allow = false;

    for mode in [EditMode::ReadOnly, EditMode::Everything] {
        state.prefs.edit_mode = mode;
        assert_eq!(
            state.rpf_move_nodes(tree, &[one], a_row),
            Ok(()),
            "a move into the archive it is in, in {mode:?}"
        );
        assert_eq!(
            state.rpf_rename_node(tree, one, "one.txt"),
            Ok(()),
            "a rename of a row in an archive to its own name, in {mode:?}"
        );
        assert_eq!(
            state.rpf_rename_node(tree, a_row, "a.rpf"),
            Ok(()),
            "a rename of an archive on disk to its own name, in {mode:?}"
        );
        assert!(
            state.pending_retail_edit.is_none(),
            "the gate was asked in {mode:?}"
        );
        assert!(
            state.refused_write.is_none(),
            "a write was refused in {mode:?}"
        );
    }
    assert_eq!(errors(&state), Vec::<String>::new());
    assert_eq!(steps(&state, tree), before, "a step was recorded");
    let Some(DocPayload::Rpf(rpf)) = state.document(tree).map(|d| &d.payload) else {
        panic!("the install's tree is open");
    };
    assert!(
        !rpf.session.archive_unsaved(a_row),
        "a.rpf was marked unsaved"
    );
    assert_eq!(
        std::fs::read(dir.path().join("a.rpf")).expect("the archive"),
        bytes,
        "a.rpf was written"
    );
}

/// ⚠ An install, so Read only refuses and Everything asks; `b.rpf` is not opened before the drop,
/// which opens it, and the originals stay in `a.rpf`.
#[test]
fn rows_copied_together_ask_the_edit_gate_once_and_the_originals_stay() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("one.txt", b"one"), ("two.txt", b"two")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let rows = ["one.txt", "two.txt"].map(|n| node_at(&mut state, tree, &format!("a.rpf/{n}")));
    let (a_row, b_row) = (
        node_at(&mut state, tree, "a.rpf"),
        node_at(&mut state, tree, "b.rpf"),
    );

    state.prefs.edit_mode = EditMode::ReadOnly;
    assert_eq!(state.rpf_copy_nodes(tree, &rows, b_row), Ok(()));
    assert!(!errors(&state).is_empty(), "Read only refused in silence");
    assert!(
        state.pending_retail_edit.is_none(),
        "a refusal asks nothing"
    );
    assert_eq!(names_under(&state, tree, b_row), Vec::<String>::new());

    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    assert_eq!(state.rpf_copy_nodes(tree, &rows, b_row), Ok(()));
    assert!(state.pending_retail_edit.is_some(), "the gate did not ask");
    assert_eq!(
        names_under(&state, tree, b_row),
        Vec::<String>::new(),
        "a row was copied before the answer"
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert_eq!(
        names_under(&state, tree, b_row),
        ["kept.txt", "one.txt", "two.txt"],
        "one Yes copied: {:?}",
        errors(&state)
    );
    assert_eq!(names_under(&state, tree, a_row), ["one.txt", "two.txt"]);
}

/// ⚠ Not an install, so no gate asks: the copy lands at once.
#[test]
fn a_drag_copy_says_copied_and_three_rows_undo_as_one_step() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[
                    ("one.txt", b"one"),
                    ("two.txt", b"two"),
                    ("three.txt", b"three"),
                ]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let rows = ["one.txt", "two.txt", "three.txt"]
        .map(|n| node_at(&mut state, tree, &format!("a.rpf/{n}")));
    let b_row = node_at(&mut state, tree, "b.rpf");
    let before = steps(&state, tree);

    assert_eq!(state.rpf_copy_nodes(tree, &rows, b_row), Ok(()));
    assert_eq!(
        names_under(&state, tree, b_row).len(),
        4,
        "{:?}",
        errors(&state)
    );
    let said: Vec<_> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        said.iter().any(|m| m.starts_with("Copied 3 file(s)")),
        "a copy reads as an import: {said:?}"
    );
    assert_eq!(steps(&state, tree), before + 1, "three copies are one step");
    state.set_active_document(tree);
    assert_eq!(
        state.undo().as_deref(),
        Some("Copy 3 file(s)"),
        "the step is named for a copy"
    );
    assert_eq!(names_under(&state, tree, b_row), ["kept.txt"]);
}

/// ⚠ The selection is made in the tree, as the tree's Ctrl click makes it, and holds the folder
/// the Files view shows, which going into it selected: a drag out of the window carries the rows,
/// not the folder with all it holds.
#[test]
fn a_drag_out_of_a_selection_made_in_the_tree_that_holds_the_folder_being_viewed_carries_its_rows()
{
    let (mut state, doc) = browsing();
    let root = node(&state, doc, "");
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);

    let empty = in_the_listing(&state);
    click(&ctx, &mut state, empty);
    assert_eq!(state.selection.selected(doc), Some(root));
    for name in ["notes.txt", "big.bin"] {
        let row = node(&state, doc, name);
        state.selection.toggle(doc, row);
    }
    assert!(
        state.selection.is_selected(doc, root),
        "the tree's Ctrl clicks took the folder shown out"
    );

    settled(&ctx, &mut state);
    let from = find(&shown, centre, "big.bin");
    let none = Modifiers::NONE;
    held(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(from), button(from, true, none)],
        none,
    );
    held(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(from + egui::vec2(12.0, 0.0))],
        none,
    );
    // Past the window's edge
    held(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(egui::pos2(-40.0, 10.0))],
        none,
    );
    let carried = crate::ui::os_drop::take_drag_out(&mut state);
    let names: Vec<_> = carried
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    assert_eq!(
        names.len(),
        2,
        "the drag out carried {names:?} ({:?})",
        errors(&state)
    );
    assert!(
        names.iter().all(|n| n == "notes.txt" || n == "big.bin"),
        "{names:?}"
    );
}

/// ⚠ An install, so Everything asks: the copies are staged for the answer, and gone once it is given.
#[test]
fn a_copys_staged_files_are_gone_once_its_gate_question_is_answered() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("one.txt", b"one"), ("two.txt", b"two")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let rows = ["one.txt", "two.txt"].map(|n| node_at(&mut state, tree, &format!("a.rpf/{n}")));
    let b_row = node_at(&mut state, tree, "b.rpf");
    state.prefs.edit_mode = EditMode::Everything;

    for decision in [RetailEditDecision::No, RetailEditDecision::YesSession] {
        state.retail_edit_session_allow = false;
        assert_eq!(state.rpf_copy_nodes(tree, &rows, b_row), Ok(()));
        let staged = match state.pending_retail_edit.as_ref().map(|p| &p.action) {
            Some(super::PendingRetailEdit::ImportPaths {
                staged: Some(staged),
                ..
            }) => staged.folder().to_path_buf(),
            other => panic!("the gate did not ask to import the copies: {other:?}"),
        };
        assert!(staged.is_dir(), "the copies are not staged while it waits");
        state.apply_pending_retail_edit(decision);
        assert!(
            !staged.exists(),
            "the staged copies are left after the answer"
        );
    }
}

/// How many folders the copies are staged under now.
fn staged_copies() -> usize {
    let probe = crate::ui::os_drop::staging_dir("openrage-clipboard");
    let root = probe.parent().expect("a root").to_path_buf();
    let _ = std::fs::remove_dir_all(&probe);
    std::fs::read_dir(&root).map_or(0, |entries| entries.flatten().count())
}

/// ⚠ An install: Read only refuses outright, and nothing is asked, so nothing is left to answer.
#[test]
fn a_copy_the_gate_refuses_leaves_no_staged_file() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("one.txt", b"one"), ("two.txt", b"two")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let rows = ["one.txt", "two.txt"].map(|n| node_at(&mut state, tree, &format!("a.rpf/{n}")));
    let b_row = node_at(&mut state, tree, "b.rpf");
    state.prefs.edit_mode = EditMode::ReadOnly;
    let before = staged_copies();

    assert_eq!(state.rpf_copy_nodes(tree, &rows, b_row), Ok(()));
    assert!(
        state.pending_retail_edit.is_none(),
        "a refusal asks nothing"
    );
    assert_eq!(
        staged_copies(),
        before,
        "the refused copy left its staged files"
    );
}

/// ⚠ A question is asked while another waits: the one it replaces held copies of its own.
#[test]
fn a_copy_that_asks_while_another_question_waits_takes_the_staged_files_of_the_one_it_replaces() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("one.txt", b"one"), ("two.txt", b"two")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let one = node_at(&mut state, tree, "a.rpf/one.txt");
    let two = node_at(&mut state, tree, "a.rpf/two.txt");
    let b_row = node_at(&mut state, tree, "b.rpf");
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    let before = staged_copies();

    assert_eq!(state.rpf_copy_nodes(tree, &[one], b_row), Ok(()));
    assert!(state.pending_retail_edit.is_some(), "the gate did not ask");
    assert_eq!(staged_copies(), before + 1, "the first copy is staged");
    assert_eq!(state.rpf_copy_nodes(tree, &[two], b_row), Ok(()));
    assert_eq!(
        staged_copies(),
        before + 1,
        "the question that was replaced kept its staged files"
    );
    state.apply_pending_retail_edit(RetailEditDecision::No);
    assert_eq!(staged_copies(), before, "the last answer left staged files");
}

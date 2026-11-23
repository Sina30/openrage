//! A drag of a selected row onto a folder moves every row selected, as one step, and a drag never
//! opens a rename.

use egui::PointerButton::Primary;
use or_io::container::rpf::BodyForm;

use super::a_right_click_or_a_drag_selects_its_row::frame_with;
use super::copy_cut_and_paste::names_under;
use super::nested_archives_survive_a_save::{archive, body, errors, on_disk, save};
use super::no_save_replaces_a_file_a_job_reads::{converting, CONVERSION};
use super::opening_a_ydd::node_at;
use super::prefs::EditMode;
use super::the_files_view_in_the_centre::{accessible, browsing, find, leaf_of, node};
use super::two_readers_of_one_archive_across_a_save::install;
use super::undo_from_files_acts_on_what_files_shows::ctrl_z;
use super::{AppState, DocPayload, RetailEditDecision};
use crate::ui::panels::hierarchy::{ui_state, RENAME_TRIGGER_DELAY};
use crate::ui::panels::registry::ids;

/// A frame at `time` with `events`.
fn at(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        time: Some(time),
        events,
        ..Default::default()
    }
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A press on `from`, a move past egui's drag threshold, and a release over `to`.
fn drag_onto(ctx: &egui::Context, state: &mut AppState, from: egui::Pos2, to: egui::Pos2) {
    for events in [
        vec![egui::Event::PointerMoved(from), button(from, true)],
        vec![egui::Event::PointerMoved(from + egui::vec2(12.0, 0.0))],
        vec![egui::Event::PointerMoved(to)],
        vec![button(to, false)],
    ] {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        frame_with(ctx, state, input);
    }
}

/// ⚠ A folder and two files, and the row dragged is neither the first selected nor the focus, so
/// neither the row dragged alone nor the focus alone is what moves.
#[test]
fn three_rows_selected_and_one_dragged_onto_a_folder_all_move_and_undo_together() {
    for panel in [ids::FILES, ids::DOCUMENT] {
        let (mut state, doc) = browsing();
        let root = node(&state, doc, "");
        let [textures, big, notes] =
            ["textures", "big.bin", "notes.txt"].map(|n| node(&state, doc, n));
        state.selection.select(doc, textures);
        state.selection.toggle(doc, big);
        state.selection.toggle(doc, notes);
        let ctx = accessible();
        frame_with(&ctx, &mut state, egui::RawInput::default());
        let shown = frame_with(&ctx, &mut state, egui::RawInput::default());
        let area = leaf_of(&state, panel);

        drag_onto(
            &ctx,
            &mut state,
            find(&shown, area, "big.bin"),
            find(&shown, area, "props"),
        );
        let props = node(&state, doc, "props");
        assert_eq!(
            names_under(&state, doc, props),
            ["big.bin", "chair.ydr", "lamp.ydr", "notes.txt", "textures"],
            "the drag in {panel} moved: {:?}",
            errors(&state)
        );

        ctrl_z(&mut state, &ctx);
        assert_eq!(
            names_under(&state, doc, root),
            ["big.bin", "notes.txt", "props", "textures"],
            "one Ctrl+Z after the drag in {panel} left rows in props"
        );
    }
}

/// ⚠ The second click comes a second after the first, as a slow click does, and the drag holds
/// the row past the pause after which that click's rename opens.
#[test]
fn a_row_slow_clicked_and_then_dragged_opens_no_name_editor() {
    let (mut state, _) = browsing();
    let ctx = accessible();
    frame_with(&ctx, &mut state, egui::RawInput::default());
    let shown = frame_with(&ctx, &mut state, egui::RawInput::default());
    let notes = find(&shown, leaf_of(&state, ids::FILES), "notes.txt");
    let moved = |by: f32| egui::Event::PointerMoved(notes + egui::vec2(by, 0.0));

    for time in [1.0, 2.0] {
        frame_with(
            &ctx,
            &mut state,
            at(time, vec![moved(0.0), button(notes, true)]),
        );
        frame_with(
            &ctx,
            &mut state,
            at(time + 0.05, vec![button(notes, false)]),
        );
    }
    assert!(
        ui_state(&mut state).pending_rename.is_some(),
        "the second click scheduled no rename, so this is not a slow click"
    );
    let due = 2.05 + RENAME_TRIGGER_DELAY;
    frame_with(&ctx, &mut state, at(2.1, vec![button(notes, true)]));
    frame_with(&ctx, &mut state, at(2.15, vec![moved(12.0)]));
    frame_with(&ctx, &mut state, at(due + 0.1, vec![moved(40.0)]));
    assert!(
        ui_state(&mut state).renaming.is_none(),
        "the name editor opened in the middle of the drag"
    );

    let end = notes + egui::vec2(40.0, 0.0);
    frame_with(&ctx, &mut state, at(due + 0.2, vec![button(end, false)]));
    frame_with(&ctx, &mut state, at(due + 1.0, Vec::new()));
    assert!(
        ui_state(&mut state).renaming.is_none(),
        "the name editor opened once the drag ended"
    );
}

const MOVED: [(&str, &[u8]); 3] = [
    ("one.txt", b"one"),
    ("two.txt", b"two"),
    ("three.txt", b"three"),
];

/// ⚠ An install, so the gate refuses in Read only and asks in Everything, and `b.rpf` is not
/// opened before the drop, which opens it.
#[test]
fn rows_dropped_together_ask_the_edit_gate_once_and_save_into_the_archive_they_landed_in() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[MOVED[0], MOVED[1], MOVED[2], ("stays.txt", b"stays")]),
            ),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let rows = MOVED.map(|(name, _)| node_at(&mut state, tree, &format!("a.rpf/{name}")));
    let (a_row, b_row) = (
        node_at(&mut state, tree, "a.rpf"),
        node_at(&mut state, tree, "b.rpf"),
    );
    let a_had = names_under(&state, tree, a_row);

    state.prefs.edit_mode = EditMode::ReadOnly;
    assert!(
        state.rpf_move_nodes(tree, &rows, b_row).is_err(),
        "Read only refuses"
    );
    assert!(
        state.pending_retail_edit.is_none(),
        "a refusal asks nothing"
    );

    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    assert!(
        state.rpf_move_nodes(tree, &rows, b_row).is_err(),
        "the gate asks first"
    );
    assert_eq!(
        names_under(&state, tree, a_row),
        a_had,
        "a row moved before the answer"
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert_eq!(
        names_under(&state, tree, b_row),
        ["kept.txt", "one.txt", "three.txt", "two.txt"],
        "one Yes moved: {:?}",
        errors(&state)
    );

    save(&mut state, tree);
    let b = on_disk(&state, &dir.path().join("b.rpf"));
    for (name, bytes) in MOVED {
        assert_eq!(
            body(&b, name, BodyForm::Decoded),
            bytes,
            "{:?}",
            errors(&state)
        );
    }
    let a = on_disk(&state, &dir.path().join("a.rpf"));
    assert!(
        a.nodes
            .iter()
            .all(|n| MOVED.iter().all(|(name, _)| n.name() != *name)),
        "a.rpf kept a moved row"
    );
}

/// ⚠ In an install, so a drop the refusal misses stops at the edit gate's question.
#[test]
fn a_drop_into_a_folder_inside_a_row_it_carries_is_refused_before_the_gate_asks() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    std::fs::create_dir_all(dir.path().join("a").join("b")).expect("the folders");
    std::fs::write(dir.path().join("f.txt"), b"f").expect("a file");
    let (mut state, tree) = install(dir.path(), &[]);
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    let rows = [
        node_at(&mut state, tree, "f.txt"),
        node_at(&mut state, tree, "a"),
    ];
    let into = node_at(&mut state, tree, "a/b");

    let refused = state.rpf_move_nodes(tree, &rows, into);
    assert!(
        state.pending_retail_edit.is_none(),
        "the drop asked to write rather than refusing"
    );
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("`a` cannot go into itself")),
        "{refused:?}"
    );
    assert!(dir.path().join("f.txt").is_file(), "f.txt moved");
    assert!(dir.path().join("a").join("b").is_dir(), "a moved");
}

/// ⚠ `props` already holds a `notes.txt`, so that row stays where it was, named, and the other
/// two move.
#[test]
fn a_dropped_row_whose_name_the_folder_holds_stays_named_and_the_rest_move() {
    let (mut state, doc) = browsing();
    let (root, props) = (node(&state, doc, ""), node(&state, doc, "props"));
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("the archive is open");
    };
    (rpf.session)
        .add_file_copy(props, "notes.txt", b"props' own".to_vec())
        .expect("a file in props");
    let rows = ["textures", "big.bin", "notes.txt"].map(|n| node(&state, doc, n));

    let said = state.rpf_move_nodes(doc, &rows, props);
    assert!(
        said.as_ref()
            .is_err_and(|e| e.starts_with("Moved 2 item(s); 1 failed: notes.txt: ")
                && e.contains("already exists")),
        "{said:?}"
    );
    assert_eq!(
        names_under(&state, doc, props),
        ["big.bin", "chair.ydr", "lamp.ydr", "notes.txt", "textures"]
    );
    assert_eq!(names_under(&state, doc, root), ["notes.txt", "props"]);
}

/// ⚠ A conversion reads `a.rpf`, so its move on disk waits, named, as its delete does, and
/// `other.txt` beside it moves.
#[test]
fn a_dropped_row_a_job_reads_stays_named_and_the_rest_move() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mods = dir.path().join("mods");
    std::fs::create_dir_all(mods.join("sub")).expect("the folder dropped onto");
    std::fs::write(mods.join("other.txt"), b"other").expect("a file beside the archive");
    let (mut state, tree, read) = converting(dir.path());
    let rows = [
        node_at(&mut state, tree, "a.rpf"),
        node_at(&mut state, tree, "other.txt"),
    ];
    let sub = node_at(&mut state, tree, "sub");

    let said = state.rpf_move_nodes(tree, &rows, sub);
    assert!(
        said.as_ref().is_err_and(|e| e.contains(CONVERSION)),
        "{said:?}"
    );
    assert!(read.is_file(), "a.rpf moved while the conversion read it");
    assert!(
        mods.join("sub").join("other.txt").is_file(),
        "other.txt stayed: {said:?}"
    );
}

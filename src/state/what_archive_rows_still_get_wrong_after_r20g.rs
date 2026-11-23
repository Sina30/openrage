//! A nested archive moved into another still reads once the archive it left is
//! renamed on disk, and a rename or a move on disk waits for the extraction
//! reading it.

use or_io::container::rpf::BodyForm;

use super::a_moved_or_deleted_archive_row_leaves_nothing_behind::folder_of;
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::extract_job::{ExtractStart, TypeFilter};
use super::extract_planning::Counting;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::{archive, body, errors, nested, on_disk, save, EDITED};
use super::nothing_extracts_into_an_install::{files_under, run_to_completion};
use super::opening_a_ydd::node_at;
use super::{AppState, DocPayload, DocumentId, RpfNodeId};
use crate::platform::recycle_bin;

fn bytes_of(state: &AppState, doc: DocumentId, node: RpfNodeId) -> Option<Vec<u8>> {
    match &state.document(doc)?.payload {
        DocPayload::Rpf(rpf) => rpf.session.bytes_for(node).map(|b| b.to_vec()),
        _ => None,
    }
}

/// ⚠ `other.txt` is edited after the rename, so the save rebuilds `n.rpf` from
/// its rows and reads `inside.txt` through the archive it was moved out of.
#[test]
fn a_nested_archive_moved_into_another_reads_and_saves_once_the_one_it_left_is_renamed_on_disk() {
    let n = archive(&[("inside.txt", b"inside"), ("other.txt", b"other")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[
            ("a.rpf", archive(&[("n.rpf", &n)])),
            ("b.rpf", archive(&[("b.txt", b"b")])),
        ],
    );
    let inside = node_at(&mut state, doc, "a.rpf/n.rpf/inside.txt");
    let other = node_at(&mut state, doc, "a.rpf/n.rpf/other.txt");
    let moved = node_at(&mut state, doc, "a.rpf/n.rpf");
    let a = node_at(&mut state, doc, "a.rpf");
    node_at(&mut state, doc, "b.rpf/b.txt");
    let b = node_at(&mut state, doc, "b.rpf");

    state
        .rpf_move_node(doc, moved, b)
        .expect("the move into b.rpf");
    state
        .rpf_rename_node(doc, a, "c.rpf")
        .expect("the rename on disk");
    assert_eq!(
        bytes_of(&state, doc, inside).as_deref(),
        Some(&b"inside"[..]),
        "n.rpf reads a.rpf, which the rename took away"
    );

    state
        .update_rpf_entry_bytes(doc, other, EDITED.to_vec())
        .expect("an edit inside n.rpf");
    save(&mut state, doc);
    let saved = on_disk(&state, &dir.path().join("mods").join("b.rpf"));
    let saved = nested(&saved, "n.rpf");
    assert_eq!(
        body(&saved, "inside.txt", BodyForm::Decoded),
        b"inside",
        "{:?}",
        errors(&state)
    );
    assert_eq!(body(&saved, "other.txt", BodyForm::Decoded), EDITED);
}

/// ⚠ The extraction counts as reading until the per-frame tick takes its
/// report, so nothing here ticks it before the changes it refuses.
#[test]
fn an_archive_renamed_or_moved_while_an_extraction_reads_it_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mods = dir.path().join("mods");
    let sub = mods.join("sub");
    std::fs::create_dir_all(sub.join("loose")).expect("the folders");
    std::fs::create_dir_all(mods.join("elsewhere")).expect("the folders");
    std::fs::write(sub.join("loose").join("one.txt"), b"one").expect("a file");
    std::fs::write(sub.join("x.rpf"), archive(&[("a.txt", b"a")])).expect("x.rpf");
    let mut state = AppState::new();
    let doc = opened(&mut state, &mods);
    let x = node_at(&mut state, doc, "sub/x.rpf");
    let row = node_at(&mut state, doc, "sub");
    let elsewhere = node_at(&mut state, doc, "elsewhere");
    let out = dir.path().join("out");

    state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    let refused = [
        ("rename", state.rpf_rename_node(doc, x, "y.rpf")),
        ("move", state.rpf_move_node(doc, x, elsewhere)),
    ];
    run_to_completion(&mut state);
    for (what, done) in refused {
        let why = done
            .err()
            .unwrap_or_else(|| panic!("the {what} went ahead"));
        assert!(why.contains("extraction of sub"), "the {what}: {why}");
    }
    assert!(
        sub.join("x.rpf").is_file(),
        "x.rpf left sub during its extraction"
    );
    assert_eq!(files_under(&out), ["loose/one.txt", "x.rpf/a.txt"]);
}

/// ⚠ `loose` holds loose files only, so no archive on the extraction's list
/// names it.
#[test]
fn a_loose_folder_deleted_while_an_extraction_reads_it_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let sub = dir.path().join("mods").join("sub");
    std::fs::create_dir_all(sub.join("loose")).expect("the folders");
    std::fs::write(sub.join("loose").join("one.txt"), b"one").expect("a file");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.path().join("mods"));
    node_at(&mut state, doc, "sub/loose/one.txt");
    let loose = node_at(&mut state, doc, "sub/loose");
    let row = node_at(&mut state, doc, "sub");
    let out = dir.path().join("out");

    state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    state.rpf_delete_node(doc, loose).expect("the delete asks");
    state.answer_delete(true);
    let said = errors(&state);
    run_to_completion(&mut state);
    assert!(
        sub.join("loose").join("one.txt").is_file(),
        "loose went to the bin during the extraction of sub: {said:?}"
    );
    assert!(
        said.iter().any(|e| e.contains("extraction of sub")),
        "{said:?}"
    );
    assert_eq!(files_under(&out), ["loose/one.txt"]);

    state.rpf_delete_node(doc, loose).expect("the delete asks");
    state.answer_delete(true);
    assert!(
        bin.join("loose").is_dir(),
        "refused once the extraction was done: {:?}",
        errors(&state)
    );
}

/// How many archives the tree holds open in `doc`, and how many rows it holds.
pub(super) fn opened_in(state: &AppState, doc: DocumentId) -> (usize, usize) {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the folder's tree");
    };
    (rpf.session.archives.len(), rpf.session.all_nodes().len())
}

/// `sub` holds two archives the tree lists and has not opened, and the loose `more`.
pub(super) fn sub_of_unopened_archives(
    dir: &std::path::Path,
    more: &[(&str, Vec<u8>)],
) -> (AppState, DocumentId, RpfNodeId) {
    let sub = dir.join("mods").join("sub");
    std::fs::create_dir_all(&sub).expect("the folders");
    for (name, bytes) in more {
        std::fs::write(sub.join(name), bytes).expect("a file");
    }
    std::fs::write(sub.join("x.rpf"), archive(&[("a.txt", b"a")])).expect("x.rpf");
    let y = archive(&[("data/b.txt", b"b")]);
    std::fs::write(sub.join("y.rpf"), y).expect("y.rpf");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.join("mods"));
    node_at(&mut state, doc, "sub/y.rpf");
    let row = node_at(&mut state, doc, "sub");
    (state, doc, row)
}

#[test]
fn an_extraction_of_a_folder_holding_unopened_archives_opens_none_in_the_frame_that_starts_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(dir.path(), &[]);
    let out = dir.path().join("out");
    let before = opened_in(&state, doc);

    let started = state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(
        opened_in(&state, doc),
        before,
        "the frame that started the extraction opened archives"
    );
    assert_eq!(started, ExtractStart::Planning);
    run_to_completion(&mut state);
    assert_eq!(
        files_under(&out),
        ["x.rpf/a.txt", "y.rpf/data/b.txt"],
        "{:?}",
        errors(&state)
    );
}

#[test]
fn a_folder_deleted_while_an_extraction_is_still_opening_what_it_names_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let (mut state, doc, row) = sub_of_unopened_archives(dir.path(), &[]);
    let out = dir.path().join("out");

    let started = state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);
    state.rpf_delete_node(doc, row).expect("the delete asks");
    state.answer_delete(true);
    let said = errors(&state);
    run_to_completion(&mut state);
    assert!(
        dir.path().join("mods").join("sub").is_dir(),
        "sub went to the bin while the extraction opened it: {said:?}"
    );
    assert!(
        said.iter().any(|e| e.contains("extraction of sub")),
        "{said:?}"
    );
    assert_eq!(files_under(&out), ["x.rpf/a.txt", "y.rpf/data/b.txt"]);
}

#[test]
fn an_extraction_cancelled_while_it_opens_its_rows_writes_nothing() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(dir.path(), &[]);
    let out = dir.path().join("out");

    state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert!(state
        .running_jobs()
        .iter()
        .any(|j| j.kind == JobKind::Extraction));
    state.cancel_job(JobKind::Extraction);
    assert!(
        !state.extract_jobs.is_running(),
        "the cancel left it running"
    );
    run_to_completion(&mut state);
    assert!(!out.exists(), "a cancelled extraction wrote {out:?}");
}

/// ⚠ A row that cannot open must end the wait rather than hold it for ever.
#[test]
fn a_folder_holding_an_archive_that_will_not_open_still_extracts_the_rest() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("bad.rpf", vec![0xA5; 64])]);
    let out = dir.path().join("out");

    state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    run_to_completion(&mut state);
    assert_eq!(files_under(&out), ["x.rpf/a.txt", "y.rpf/data/b.txt"]);
    assert!(
        errors(&state).iter().any(|e| e.contains("did not open")),
        "{:?}",
        errors(&state)
    );
}

/// What a drag out of `row` hands over, asked again every frame until the rows under it have opened.
fn dragged_out(
    state: &mut AppState,
    doc: DocumentId,
    row: RpfNodeId,
    staging: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match state.stage_drag_out(doc, &[row], staging) {
            Ok(handed) => return handed,
            Err(why) => assert!(
                why.contains("opening") && std::time::Instant::now() < until,
                "{why}"
            ),
        }
        state.poll_extract_job();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn a_folder_dragged_out_hands_explorer_every_file_in_the_archives_it_holds_opened_or_not() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    let staging = dir.path().join("staging");

    let handed = dragged_out(&mut state, doc, row, &staging);
    assert_eq!(handed, [staging.join("sub")]);
    assert_eq!(
        files_under(&staging),
        ["sub/one.txt", "sub/x.rpf/a.txt", "sub/y.rpf/data/b.txt"],
        "{:?}",
        errors(&state)
    );
}

/// The counts under `row` once the click on Count types has opened the rows under it, asked as the menu asks, every frame.
pub(super) fn counted(
    state: &mut AppState,
    doc: DocumentId,
    row: RpfNodeId,
) -> Vec<(String, usize)> {
    state.count_types_under(doc, row);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let (census, counting) = state.census_of(doc, row);
        if matches!(counting, Counting::Idle { .. }) {
            return census;
        }
        assert!(std::time::Instant::now() < until, "the rows never opened");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// ⚠ The menu asks every frame it is open, so before the click the answer holds
/// only what the tree has opened, and says rows are unread.
#[test]
fn the_counts_of_only_one_type_take_every_file_under_the_row_opened_or_not() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(
        dir.path(),
        &[("one.txt", b"one".to_vec()), ("two.dat", b"two".to_vec())],
    );

    let (known, counting) = state.census_of(doc, row);
    assert_eq!(counting, Counting::Idle { unread: 2 });
    assert_eq!(known, [("dat".to_string(), 1), ("txt".to_string(), 1)]);
    assert_eq!(
        counted(&mut state, doc, row),
        [("txt".to_string(), 3), ("dat".to_string(), 1)],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ A counted folder can regain unopened rows (an archive put in it, or one a
/// defragment unloaded), so a count that finished says so and can be asked for again.
#[test]
fn the_counts_of_only_one_type_take_an_archive_added_after_the_folder_was_counted() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(dir.path(), &[]);
    assert_eq!(
        counted(&mut state, doc, row),
        [("txt".to_string(), 2)],
        "{:?}",
        errors(&state)
    );

    let z = dir.path().join("mods").join("sub").join("z.rpf");
    std::fs::write(&z, archive(&[("c.txt", b"c")])).expect("z.rpf");
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("the folder's tree");
    };
    (rpf.session)
        .add_unopened_disk_archive(row, "z.rpf".into(), z)
        .expect("the archive row");

    let (_, counting) = state.census_of(doc, row);
    assert_eq!(
        counting,
        Counting::Idle { unread: 1 },
        "the new archive is unopened, and nothing says so"
    );
    assert_eq!(
        counted(&mut state, doc, row),
        [("txt".to_string(), 3)],
        "{:?}",
        errors(&state)
    );
}

#[test]
fn a_folder_added_inside_an_archive_loses_its_new_badge_once_a_save_writes_it() {
    let badge = |state: &AppState, doc, node| {
        let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
            panic!("the folder's tree");
        };
        let row = rpf.session.node(node).expect("the row");
        crate::ui::icons::rpf_node_icon(row, false, false, true, false).badge
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(dir.path(), &[("x.rpf", archive(&[("a.txt", b"a")]))]);
    let x = node_at(&mut state, doc, "x.rpf");
    let new = state.rpf_add_folder(doc, x, "new").expect("the folder");
    let file = (state.add_file_to_row(doc, new, "n.txt", b"n".to_vec())).expect("a file in it");
    let empty = state
        .rpf_add_folder(doc, x, "empty")
        .expect("the empty folder");
    assert_eq!(
        badge(&state, doc, new),
        Some(crate::ui::icons::BadgeKind::New)
    );
    assert_eq!(
        badge(&state, doc, file),
        Some(crate::ui::icons::BadgeKind::New)
    );

    save(&mut state, doc);
    assert_eq!(
        badge(&state, doc, file),
        None,
        "the file kept its badge: {:?}",
        errors(&state)
    );
    assert_eq!(badge(&state, doc, new), None, "the folder kept its badge");
    assert_eq!(
        badge(&state, doc, empty),
        None,
        "the empty folder kept its badge"
    );
}

/// What the inspector paints for the active document, over two frames: egui lays some widgets out on the second.
fn inspector_says(state: &mut AppState) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for frame in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 900.0),
            )),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        shapes = ctx
            .run_ui(input, |ui| crate::ui::panels::inspector::draw(ui, state))
            .shapes;
    }
    let painted = super::an_edit_stays_where_it_was_made::texts(&shapes);
    painted.into_iter().map(|(text, _)| text).collect()
}

#[test]
fn the_inspector_of_a_folder_on_disk_the_tree_has_not_scanned_does_not_say_it_holds_nothing() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let sub = dir.path().join("mods").join("sub");
    std::fs::create_dir_all(&sub).expect("sub");
    std::fs::write(sub.join("one.txt"), b"one").expect("a file");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.path().join("mods"));
    let sub = node_at(&mut state, doc, "sub");
    state.selection.select(doc, sub);

    let before = inspector_says(&mut state);
    assert!(before.iter().any(|t| t == "Holds"), "{before:?}");
    assert!(
        !before.iter().any(|t| t == "0 items"),
        "an unscanned folder reads as empty: {before:?}"
    );
    assert!(
        before.iter().any(|t| t.contains("not read")),
        "it does not say it has not been read: {before:?}"
    );

    node_at(&mut state, doc, "sub/one.txt");
    let after = inspector_says(&mut state);
    assert!(after.iter().any(|t| t == "1 item"), "{after:?}");
}

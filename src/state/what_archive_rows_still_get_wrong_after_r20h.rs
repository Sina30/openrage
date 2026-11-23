//! A drag out does not hold the frame while it opens rows, and an extraction
//! opening rows holds back the files it reads and no others.

use std::path::Path;
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;

use super::extract_job::{ExtractStart, TypeFilter};
use super::extract_planning::Counting;
use super::nested_archives_survive_a_save::{archive, errors};
use super::nothing_extracts_into_an_install::{files_under, run_to_completion};
use super::opening_a_ydd::node_at;
use super::what_archive_rows_still_get_wrong_after_r20g::{
    counted, opened_in, sub_of_unopened_archives,
};
use super::{AppState, DocPayload, DocumentId, DocumentKind, RpfNodeId};

fn told(state: &AppState, what: &str) -> bool {
    state.toasts.iter().any(|t| t.message.contains(what))
}

fn root_of(state: &AppState, doc: DocumentId) -> RpfNodeId {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.root_id(),
        _ => panic!("the folder's tree"),
    }
}

/// ⚠ The first drag cannot succeed: nothing waits, so the archives are asked for
/// and not yet open. What is asserted is that it returned with none opened, and
/// that the frames after it open them and say so.
#[test]
fn a_drag_out_of_a_folder_holding_unopened_archives_opens_none_in_the_frame_that_starts_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    let staging = dir.path().join("staging");
    let before = opened_in(&state, doc);

    let said = state
        .stage_drag_out(doc, &[row], &staging)
        .expect_err("the drag waited for the archives instead of returning");
    assert!(said.contains("opening"), "{said}");
    assert_eq!(
        opened_in(&state, doc),
        before,
        "the frame that started the drag opened archives"
    );
    assert!(!staging.exists(), "a refused drag wrote {staging:?}");

    let until = Instant::now() + Duration::from_secs(30);
    while !told(&state, "drag it again") {
        assert!(Instant::now() < until, "the rows never opened");
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    }
    let handed = state
        .stage_drag_out(doc, &[row], &staging)
        .expect("the drag out stages once they are open");
    assert_eq!(handed, [staging.join("sub")]);
    assert_eq!(
        files_under(&staging),
        ["sub/one.txt", "sub/x.rpf/a.txt", "sub/y.rpf/data/b.txt"],
        "{:?}",
        errors(&state)
    );
}

/// `sub` with a folder that was listed and is gone since, so it will not list: dragged out once,
/// the failure told, and `sub`'s own files handed over by the drag after.
fn sub_dragged_out_past_a_folder_that_will_not_list(
    dir: &Path,
) -> (AppState, DocumentId, RpfNodeId) {
    let gone = dir.join("mods").join("sub").join("gone");
    std::fs::create_dir_all(&gone).expect("the folder");
    let (mut state, doc, row) = sub_of_unopened_archives(dir, &[("one.txt", b"one".to_vec())]);
    std::fs::remove_dir(&gone).expect("the folder goes after the tree listed it");

    let said = state
        .stage_drag_out(doc, &[row], &dir.join("first"))
        .expect_err("the drag waited for the rows");
    assert!(said.contains("opening"), "{said}");
    let until = Instant::now() + Duration::from_secs(30);
    while !told(&state, "drag it again") {
        assert!(Instant::now() < until, "the rows never opened");
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(told(&state, "did not open"), "{:?}", errors(&state));
    (state, doc, row)
}

const SUB_DRAGGED: [&str; 3] = ["sub/one.txt", "sub/x.rpf/a.txt", "sub/y.rpf/data/b.txt"];

/// ⚠ A folder that cannot be listed stays unopened for good, so a drag that asked
/// for it afresh, or only every other drag, would never get past it.
#[test]
fn a_drag_out_of_a_selection_holding_a_folder_that_will_not_list_hands_over_the_rest_on_every_drag_after_it_says_so(
) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_dragged_out_past_a_folder_that_will_not_list(dir.path());

    for again in 1..=4 {
        let staging = dir.path().join(format!("again{again}"));
        let handed = (state.stage_drag_out(doc, &[row], &staging))
            .unwrap_or_else(|why| panic!("drag {again} after the failure was told: {why}"));
        assert_eq!(handed, [staging.join("sub")]);
        assert_eq!(files_under(&staging), SUB_DRAGGED, "{:?}", errors(&state));
    }
    let tellings = errors(&state)
        .iter()
        .filter(|e| e.contains("did not open"))
        .count();
    assert_eq!(tellings, 1, "{:?}", errors(&state));
}

/// ⚠ Leaving the failed folder alone must not leave alone an archive the tree
/// lost since, as a defragment or a save's read-back does: its files would be
/// missing from what the drag hands over, and nothing would say so.
#[test]
fn a_drag_out_after_a_folder_failed_to_list_still_opens_an_archive_the_tree_lost_since() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_dragged_out_past_a_folder_that_will_not_list(dir.path());
    let x = node_at(&mut state, doc, "sub/x.rpf");
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("the folder's tree");
    };
    rpf.session.unload_archive(x);

    let staging = dir.path().join("again");
    let said = state
        .stage_drag_out(doc, &[row], &staging)
        .expect_err("the drag went ahead with an archive unopened");
    assert!(said.contains("opening"), "{said}");
    let until = Instant::now() + Duration::from_secs(30);
    let handed = loop {
        match state.stage_drag_out(doc, &[row], &staging) {
            Ok(handed) => break handed,
            Err(why) => assert!(why.contains("opening"), "{why}"),
        }
        assert!(Instant::now() < until, "the archive never opened");
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(handed, [staging.join("sub")]);
    assert_eq!(files_under(&staging), SUB_DRAGGED, "{:?}", errors(&state));
    assert_eq!(
        errors(&state)
            .iter()
            .filter(|e| e.contains("did not open"))
            .count(),
        1,
        "{:?}",
        errors(&state)
    );
}

/// ⚠ The menu asks every frame it is open, so asking many times is what shows
/// whether asking reads.
#[test]
fn asking_for_the_counts_of_a_folder_opens_no_archive_in_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    let before = opened_in(&state, doc);

    for _ in 0..50 {
        let (known, counting) = state.census_of(doc, row);
        assert_eq!(counting, Counting::Idle { unread: 2 });
        assert_eq!(known, [("txt".to_string(), 1)]);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!state.expander.is_listing(doc), "a read was asked for");
    assert_eq!(opened_in(&state, doc), before, "an archive was opened");
}

/// ⚠ The row is unloaded as a save's read-back, a defragment and an overlay
/// refresh unload one, once its file reads again.
#[test]
fn a_folder_counted_again_after_an_archive_that_would_not_open_is_mended_counts_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("bad.rpf", vec![0xA5; 64])]);

    assert_eq!(
        counted(&mut state, doc, row),
        [("txt".to_string(), 2)],
        "{:?}",
        errors(&state)
    );
    assert!(told(&state, "did not open"), "{:?}", errors(&state));
    assert_eq!(
        state.census_of(doc, row).1,
        Counting::Idle { unread: 0 },
        "an archive that failed offers a count it cannot make"
    );

    std::fs::write(
        dir.path().join("mods").join("sub").join("bad.rpf"),
        archive(&[("c.txt", b"c")]),
    )
    .expect("the mended archive");
    let bad = node_at(&mut state, doc, "sub/bad.rpf");
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("the folder's tree");
    };
    rpf.session.unload_archive(bad);
    assert_eq!(state.census_of(doc, row).1, Counting::Idle { unread: 1 });

    assert_eq!(
        counted(&mut state, doc, row),
        [("txt".to_string(), 3)],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ Save As is the route here: it asks `no_job_reads` first, as every save does.
fn save_as(state: &mut AppState, to: &Path) -> bool {
    let doc = state.new_empty_document(DocumentKind::Rpf);
    state.set_active_document(doc);
    let proceeded = state.save_active_document_as(to.to_path_buf());
    let until = Instant::now() + Duration::from_secs(60);
    while state.save_receiver.is_some() {
        assert!(Instant::now() < until, "the save did not finish");
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    proceeded
}

#[test]
fn a_file_saved_in_a_folder_an_extraction_is_opening_goes_ahead_unless_the_extraction_opens_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, _) = sub_of_unopened_archives(dir.path(), &[]);
    let mods = dir.path().join("mods");
    let root = root_of(&state, doc);
    let out = dir.path().join("out");

    let started =
        state.extract_rpf_node(doc, root, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);

    let beside = mods.join("saved.rpf");
    assert!(
        save_as(&mut state, &beside),
        "the save in the extraction's folder was refused: {:?}",
        errors(&state)
    );
    assert!(beside.is_file(), "nothing was written to {beside:?}");

    let opening = mods.join("sub").join("x.rpf");
    assert!(
        !save_as(&mut state, &opening),
        "a save replaced an archive the extraction was opening"
    );
    assert!(
        errors(&state).iter().any(|e| e.contains("extraction of")),
        "{:?}",
        errors(&state)
    );

    run_to_completion(&mut state);
    assert_eq!(
        files_under(&out),
        ["sub/x.rpf/a.txt", "sub/y.rpf/data/b.txt"],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ Both end by unloading the archive's row and opening it again later, which
/// would drop what the extraction has not yet planned from it. The extraction
/// has not asked for `y.rpf`, which is open already, so only `no_job_holds`,
/// which holds every file under the rows named, refuses.
#[test]
fn an_open_archive_under_an_extraction_still_opening_its_rows_is_neither_defragmented_nor_refreshed(
) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(dir.path(), &[]);
    node_at(&mut state, doc, "sub/y.rpf/data");
    let y = node_at(&mut state, doc, "sub/y.rpf");
    let on_disk = state.defrag_target(doc, y).expect("y.rpf is on disk");
    let out = dir.path().join("out");

    let started = state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);

    state.defragment_archive(doc, y);
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("extraction of sub")),
        "the defragment went ahead: {:?}",
        errors(&state)
    );
    let refresh = state.overlay_refresh_blocker(&on_disk);
    assert!(
        refresh.is_some_and(|why| why.contains("extraction of sub")),
        "the refresh would go ahead"
    );

    run_to_completion(&mut state);
    assert_eq!(
        files_under(&out),
        ["x.rpf/a.txt", "y.rpf/data/b.txt"],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ A paste's cut took `one.txt` to the Recycle Bin, and the undo writes it back under `sub`, whose
/// archives the extraction has asked for and not yet read: only `no_job_holds` refuses, as
/// `no_job_reads` asks about the archives it has asked for and no other file under them.
#[test]
fn an_undo_writing_under_an_extraction_still_opening_its_rows_waits_for_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    crate::platform::recycle_bin::fake::use_bin(&bin);
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    let one = node_at(&mut state, doc, "sub/one.txt");
    let x = node_at(&mut state, doc, "sub/x.rpf");
    state.copy_rows(doc, &[one], true).expect("the cut");
    state.paste_rows(doc, x).expect("the paste into x.rpf");
    assert!(bin.join("one.txt").is_file(), "the cut file did not leave");

    let out = dir.path().join("out");
    let started = state.extract_rpf_node(doc, row, out, BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);
    state.set_active_document(doc);
    assert_eq!(
        state.undo(),
        None,
        "the undo went ahead under the extraction"
    );
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("extraction of sub")),
        "{:?}",
        errors(&state)
    );
    assert!(
        !dir.path().join("mods/sub/one.txt").exists(),
        "the undo wrote the file back"
    );

    run_to_completion(&mut state);
    assert!(
        state.undo().is_some(),
        "the undo is refused for good: {:?}",
        errors(&state)
    );
}

/// ⚠ `mods/sub` is a mod of the install the folder document stands for, which holds loose files only:
/// the extraction has asked for no archive of it, so only `no_job_holds`, which holds every
/// file under the rows named, refuses the move to `mods_` that `no_job_reads` would let go ahead.
#[test]
fn a_mod_does_not_move_under_an_extraction_still_opening_its_rows() {
    use super::every_add_lands_where_the_tree_shows_it::opened;
    use super::{EditMode, Game};
    let dir = tempfile::tempdir().expect("a scratch directory");
    let sub = dir.path().join("mods").join("sub");
    std::fs::create_dir_all(&sub).expect("the folders");
    std::fs::write(sub.join("one.txt"), b"one").expect("a file");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.path().join("mods"));
    let row = node_at(&mut state, doc, "sub");
    state
        .prefs
        .game_paths
        .set(Game::GtaVLegacy, Some(dir.path().to_path_buf()));
    state.prefs.active_game = Some(Game::GtaVLegacy);
    state.prefs.edit_mode = EditMode::Overlay;
    state.refresh_mods();
    assert!(
        state.mod_manager.mods.iter().any(|m| m.name == "sub"),
        "`sub` is not listed as a mod: {:?}",
        state
            .mod_manager
            .mods
            .iter()
            .map(|m| &m.name)
            .collect::<Vec<_>>()
    );

    // ⚠ Not under the install, which an extraction refuses
    let elsewhere = tempfile::tempdir().expect("a scratch directory");
    let out = elsewhere.path().join("out");
    let started = state.extract_rpf_node(doc, row, out, BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);
    state.ask_toggle_mod("sub");
    assert_eq!(
        state.mod_manager.pending_move, None,
        "the move was offered under the extraction"
    );
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("extraction of sub")),
        "{:?}",
        errors(&state)
    );

    run_to_completion(&mut state);
    state.ask_toggle_mod("sub");
    assert!(
        state.mod_manager.pending_move.is_some(),
        "the move is refused for good: {:?}",
        errors(&state)
    );
}

/// ⚠ The failure was told and the rows it names are left alone, so a folder mended since would be
/// left out of every drag after it, until other rows were dragged.
#[test]
fn a_folder_mended_after_a_drag_told_it_would_not_list_is_opened_by_the_next_drag() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_dragged_out_past_a_folder_that_will_not_list(dir.path());
    let gone = dir.path().join("mods").join("sub").join("gone");
    std::fs::create_dir_all(&gone).expect("the folder is back");
    std::fs::write(gone.join("mended.txt"), b"mended").expect("a file in it");

    let staging = dir.path().join("again");
    let said = state
        .stage_drag_out(doc, &[row], &staging)
        .expect_err("the drag went ahead without the folder it was mended");
    assert!(said.contains("opening"), "{said}");
    let until = Instant::now() + Duration::from_secs(30);
    let handed = loop {
        match state.stage_drag_out(doc, &[row], &staging) {
            Ok(handed) => break handed,
            Err(why) => assert!(why.contains("opening"), "{why}"),
        }
        assert!(Instant::now() < until, "the mended folder never opened");
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(handed, [staging.join("sub")]);
    assert_eq!(
        files_under(&staging),
        [
            "sub/gone/mended.txt",
            "sub/one.txt",
            "sub/x.rpf/a.txt",
            "sub/y.rpf/data/b.txt"
        ],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ A folder that cannot be listed stays unopened for good: counted, it would be offered for
/// counting again by every menu that opens, and each click would toast the same failure.
#[test]
fn a_folder_the_count_could_not_list_is_told_once_and_left_out_of_the_unread_count() {
    use super::extract_planning::Counting;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let gone = dir.path().join("mods").join("sub").join("gone");
    std::fs::create_dir_all(&gone).expect("the folder");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    std::fs::remove_dir(&gone).expect("the folder goes after the tree listed it");
    let unread = |state: &mut AppState| match state.census_of(doc, row).1 {
        Counting::Idle { unread } => unread,
        other => panic!("a count is running: {other:?}"),
    };
    assert_eq!(
        unread(&mut state),
        3,
        "gone and the two archives are unread"
    );

    counted(&mut state, doc, row);
    assert!(told(&state, "did not open"), "{:?}", errors(&state));
    assert_eq!(
        unread(&mut state),
        0,
        "the count is offered again for a folder it already told would not list"
    );
    counted(&mut state, doc, row);
    let tellings = errors(&state)
        .iter()
        .filter(|e| e.contains("did not open"))
        .count();
    assert_eq!(tellings, 1, "{:?}", errors(&state));

    // Mended, it is unread again, and the count reads it
    std::fs::create_dir_all(&gone).expect("the folder is back");
    assert_eq!(unread(&mut state), 1, "a mended folder is not offered");
}

/// ⚠ The count advanced only while the submenu that shows it was open, since the menu's every-frame
/// `census_of` was its tick: closed mid-count, it was parked with rows in flight.
#[test]
fn a_count_goes_on_while_the_submenu_that_started_it_is_closed() {
    use super::extract_planning::Counting;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) = sub_of_unopened_archives(
        dir.path(),
        &[("one.txt", b"one".to_vec()), ("two.dat", b"two".to_vec())],
    );
    state.count_types_under(doc, row);
    // The menu is closed: only the frame's own tick runs, as no `census_of` is asked
    let until = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.census.is_some() {
        assert!(Instant::now() < until, "the count never went on");
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    }
    let (census, counting) = state.census_of(doc, row);
    assert_eq!(counting, Counting::Idle { unread: 0 });
    assert_eq!(
        census,
        [("txt".to_string(), 3), ("dat".to_string(), 1)],
        "{:?}",
        errors(&state)
    );
}

/// ⚠ A drag out of a folder holding unopened archives opens them in the background, with no bound:
/// the list says so, and its Cancel stops the opening, which the drag's toast says nothing of.
#[test]
fn a_drag_out_opening_its_rows_is_in_the_jobs_list_and_its_cancel_stops_it() {
    use super::jobs::JobKind;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, row) =
        sub_of_unopened_archives(dir.path(), &[("one.txt", b"one".to_vec())]);
    let staging = dir.path().join("staging");
    state
        .stage_drag_out(doc, &[row], &staging)
        .expect_err("the drag waits for the archives");

    let listed = state.running_jobs();
    let job = listed
        .iter()
        .find(|job| job.doing.contains("drag"))
        .unwrap_or_else(|| panic!("the opening is not in the list: {listed:?}"));
    assert_eq!(job.kind, JobKind::Extraction, "{job:?}");

    state.cancel_job(job.kind);
    assert!(
        state
            .running_jobs()
            .iter()
            .all(|job| !job.doing.contains("drag")),
        "the opening is still listed after Cancel"
    );
    for _ in 0..50 {
        state.poll_extract_job();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        !told(&state, "drag it again"),
        "a cancelled opening went on to tell the drag could be tried again"
    );
}

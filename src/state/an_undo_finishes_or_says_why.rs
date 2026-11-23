//! An undo or a redo finishes every part of its step, or changes nothing and says why, keeping
//! the history it can still undo; and undoing a cut and paste puts the cut file back.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::a_change_on_disk_keeps_the_undo_steps_beside_it::{a_folder, marked};
use super::copy_cut_and_paste::{forget_offered, names_under};
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::{archive, errors, save};
use super::opening_a_ydd::{finish_load, node_at};
use super::prefs::EditMode;
use super::the_files_view_in_the_centre::{accessible, frame};
use super::two_readers_of_one_archive_across_a_save::install;
use super::undo_from_files_acts_on_what_files_shows::name_of;
use super::{AppState, DocPayload, DocumentId, EditHistory, RetailEditDecision, RpfNodeId};
use crate::platform::{clipboard::fake, recycle_bin};
use crate::ui::commands;

/// Every row of `doc`'s tree by its path, and the archives in it marked unsaved.
fn tree(state: &AppState, doc: DocumentId) -> (Vec<String>, Vec<String>) {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the folder's tree");
    };
    let session = &rpf.session;
    let paths = |rows: Vec<RpfNodeId>| {
        let mut paths: Vec<String> = rows
            .into_iter()
            .filter_map(|r| session.path_of(r))
            .collect();
        paths.sort();
        paths
    };
    let all = session.all_nodes();
    let unsaved = (all.iter().copied())
        .filter(|&row| session.archive_unsaved(row))
        .collect();
    (paths(all), paths(unsaved))
}

fn root_of(state: &AppState, doc: DocumentId) -> RpfNodeId {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.root_id(),
        _ => panic!("{doc} is not a tree"),
    }
}

fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// What the History panel draws for the document Undo acts on.
fn history_panel(state: &mut AppState) -> Vec<String> {
    let ctx = accessible();
    let history = (commands::all(state).into_iter())
        .find(|command| command.id == "panel: history")
        .expect("the palette offers the History panel");
    commands::run(state, &ctx, &history);
    let shown = frame(&ctx, state, Vec::new());
    shown.into_iter().map(|(text, _)| text).collect()
}

/// ⚠ The paste moves `a.txt` out of `x.rpf` and `b.txt` out of `y.rpf`, so its
/// undo puts `b.txt` back before it finds `x.rpf` gone. The save leaves
/// `x.rpf` with nothing unsaved, which a Refresh drops once its file is gone.
#[test]
fn an_undo_whose_row_a_refresh_dropped_changes_nothing_says_why_and_the_older_steps_still_undo() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let x = archive(&[("a.txt", b"a"), ("kept.txt", b"kept")]);
    let y = archive(&[("b.txt", b"b"), ("kept.txt", b"kept")]);
    let z = archive(&[("z.txt", b"z")]);
    let (mut state, doc) = a_folder(dir.path(), &[("x.rpf", &x), ("y.rpf", &y), ("z.rpf", &z)]);
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let b = node_at(&mut state, doc, "y.rpf/b.txt");
    let older = node_at(&mut state, doc, "z.rpf/z.txt");
    let z_row = node_at(&mut state, doc, "z.rpf");
    (state.rpf_rename_node(doc, older, "older.txt")).expect("the older step");
    state.copy_rows(doc, &[a, b], true).expect("the cut");
    forget_offered(&fake::offered());
    state.paste_rows(doc, z_row).expect("the paste");
    save(&mut state, doc);
    std::fs::remove_file(dir.path().join("mods").join("x.rpf")).expect("x.rpf leaves the disk");
    let root = root_of(&state, doc);
    state.refresh_folder(doc, root).expect("the Refresh");

    state.set_active_document(doc);
    let before = (tree(&state, doc), marked(&state, doc));
    assert_eq!(state.undo(), None, "the paste undid with x.rpf gone");
    assert_eq!(
        (tree(&state, doc), marked(&state, doc)),
        before,
        "the undo that could not finish changed something"
    );
    let why =
        |text: &String| text.contains("Paste 2 item(s)") && text.contains("no longer in the tree");
    let said = errors(&state);
    assert!(said.iter().any(why), "no sentence says why: {said:?}");
    let drawn = history_panel(&mut state);
    assert!(
        drawn.iter().any(why),
        "the History panel does not say why: {drawn:?}"
    );

    assert_eq!(
        state.undo().as_deref(),
        Some("Rename z.txt to older.txt"),
        "the step before it no longer undoes"
    );
    assert_eq!(name_of(&state, doc, older), "z.txt");
    assert!(
        marked(&state, doc),
        "z.rpf's file holds older.txt, and the tree z.txt"
    );
    assert!(state.redo().is_some());
    assert!(
        !marked(&state, doc),
        "redone, the tree holds what every file does"
    );
}

/// `files` in a folder opened with Open Folder, `x.rpf` holding `in_x` beside them, and this thread's Recycle Bin.
fn a_folder_and_a_bin(
    dir: &Path,
    in_x: &[(&str, &[u8])],
    files: &[(&str, &[u8])],
) -> (AppState, DocumentId, RpfNodeId) {
    let x = archive(in_x);
    let mut all = vec![("x.rpf", x.as_slice())];
    all.extend_from_slice(files);
    let (mut state, doc) = a_folder(dir, &all);
    let x_row = node_at(&mut state, doc, "x.rpf");
    state.set_active_document(doc);
    (state, doc, x_row)
}

/// Cut `rows` of `doc` and paste them into `into`.
fn cut_and_paste(state: &mut AppState, doc: DocumentId, rows: &[RpfNodeId], into: RpfNodeId) {
    state.copy_rows(doc, rows, true).expect("the cut");
    state.paste_rows(doc, into).expect("the paste");
    assert!(errors(state).is_empty(), "{:?}", errors(state));
}

/// ⚠ The tree cannot move a file on disk into an archive, so the paste reads
/// it into `x.rpf` and sends it to the Recycle Bin.
#[test]
fn undoing_a_cut_and_paste_into_an_archive_puts_the_cut_file_back_where_it_was() {
    const LOOSE: &[u8] = b"the loose file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, x_row) = a_folder_and_a_bin(
        dir.path(),
        &[("kept.txt", b"kept")],
        &[("sub/loose.txt", LOOSE)],
    );
    let was = dir.path().join("mods").join("sub").join("loose.txt");
    let loose = node_at(&mut state, doc, "sub/loose.txt");
    cut_and_paste(&mut state, doc, &[loose], x_row);
    assert!(read(&was).is_none(), "the cut file stayed where it was");
    assert_eq!(names_under(&state, doc, x_row), ["kept.txt", "loose.txt"]);

    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(
        read(&was).as_deref(),
        Some(LOOSE),
        "the cut file is not back where it was: {:?}",
        errors(&state)
    );
    assert_eq!(
        name_of(&state, doc, loose),
        "loose.txt",
        "Files no longer lists it"
    );
    assert_eq!(names_under(&state, doc, x_row), ["kept.txt"]);
    assert!(
        !marked(&state, doc),
        "undone, x.rpf holds what its file does"
    );

    assert_eq!(state.redo().as_deref(), Some("Paste 1 item(s)"));
    assert!(
        read(&was).is_none(),
        "redone, the cut file stayed where it was"
    );
    assert_eq!(names_under(&state, doc, x_row), ["kept.txt", "loose.txt"]);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(read(&was).as_deref(), Some(LOOSE), "{:?}", errors(&state));
}

/// ⚠ `c.rpf` is open as a document of its own, so the paste's step is its
/// own and the cut row is the folder's, whose rows the step never holds.
#[test]
fn undoing_a_cut_into_another_documents_archive_writes_the_file_back_and_leaves_its_tree() {
    const LOOSE: &[u8] = b"the loose file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, folder) = a_folder(dir.path(), &[("loose.txt", LOOSE)]);
    let c = dir.path().join("c.rpf");
    std::fs::write(&c, archive(&[("other.txt", b"other")])).expect("c.rpf");
    let loose = node_at(&mut state, folder, "loose.txt");
    state.load_file(c);
    let c_doc = finish_load(&mut state).expect("c.rpf opens as a document");
    let c_root = root_of(&state, c_doc);
    state.copy_rows(folder, &[loose], true).expect("the cut");
    state.paste_rows(c_doc, c_root).expect("the paste");
    let was = dir.path().join("mods").join("loose.txt");
    assert!(read(&was).is_none(), "the cut file stayed where it was");

    state.set_active_document(c_doc);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(read(&was).as_deref(), Some(LOOSE), "{:?}", errors(&state));
    assert_eq!(names_under(&state, c_doc, c_root), ["other.txt"]);
}

/// ⚠ `x.rpf` already holds a folder `pack`, which the paste puts the cut
/// folder's files into, so the undo writes back only what the cut one held.
#[test]
fn undoing_the_paste_of_a_cut_folder_writes_back_the_folder_as_it_was() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc, x_row) = a_folder_and_a_bin(
        dir.path(),
        &[("pack/theirs.txt", b"theirs")],
        &[
            ("pack/ours.txt", b"ours"),
            ("pack/deeper/more.txt", b"more"),
        ],
    );
    let pack = node_at(&mut state, doc, "pack");
    let in_x = node_at(&mut state, doc, "x.rpf/pack");
    cut_and_paste(&mut state, doc, &[pack], x_row);
    let was = dir.path().join("mods").join("pack");
    assert!(!was.exists(), "the cut folder stayed where it was");

    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    let held = |path: &str| read(&was.join(path));
    assert_eq!(
        (
            held("ours.txt"),
            held("deeper/more.txt"),
            held("theirs.txt")
        ),
        (Some(b"ours".to_vec()), Some(b"more".to_vec()), None),
        "{:?}",
        errors(&state)
    );
    assert_eq!(names_under(&state, doc, in_x), ["theirs.txt"]);
    assert_eq!(
        name_of(&state, doc, pack),
        "pack",
        "Files no longer lists it"
    );
}

/// ⚠ An install, so a file beside its archives is the game as shipped, which
/// Read only never writes.
#[test]
fn undoing_a_cut_asks_the_edit_gate_before_it_writes_the_file_back() {
    const SHIPPED: &[u8] = b"a shipped file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (game, bin) = (dir.path().join("game"), dir.path().join("bin"));
    std::fs::create_dir_all(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    std::fs::create_dir_all(&game).expect("the install");
    std::fs::write(game.join("gta5.exe"), b"an install marker").expect("marker");
    std::fs::write(game.join("readme.txt"), SHIPPED).expect("a shipped file");
    let (mut state, tree) = install(&game, &[("x.rpf", archive(&[("kept.txt", b"kept")]))]);
    (state.prefs.edit_mode, state.retail_edit_session_allow) = (EditMode::Everything, true);
    let readme = node_at(&mut state, tree, "readme.txt");
    let x_row = node_at(&mut state, tree, "x.rpf");
    state.set_active_document(tree);
    cut_and_paste(&mut state, tree, &[readme], x_row);

    state.prefs.edit_mode = EditMode::ReadOnly;
    assert_eq!(
        state.undo(),
        None,
        "Read only let the undo write into the game"
    );
    assert!(read(&game.join("readme.txt")).is_none());
    let said = errors(&state);
    assert!(
        said.iter()
            .any(|e| e.contains("part of the game as shipped")),
        "the refusal does not say why: {said:?}"
    );
    assert!(
        state.focused_history().is_some_and(EditHistory::can_undo),
        "a refusal of the gate took the step away"
    );

    // A mode that permits the write, with the session not yet allowed it: the gate would ask, so the undo asks
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    state.toasts.clear();
    assert_eq!(state.undo(), None, "the undo went ahead unasked");
    let asked = state.pending_retail_edit.as_ref().expect("the undo asked");
    assert_eq!(asked.target_path, game.join("readme.txt"));
    assert_eq!(asked.verb, "undo");
    assert!(
        errors(&state).is_empty(),
        "the question came with a refusal: {:?}",
        errors(&state)
    );
    assert!(read(&game.join("readme.txt")).is_none());
    assert!(
        state.focused_history().is_some_and(EditHistory::can_undo),
        "asking took the step away"
    );

    // Yes, the answer the question was for, takes the undo through
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert_eq!(read(&game.join("readme.txt")).as_deref(), Some(SHIPPED));
    assert!(
        state.focused_history().is_some_and(EditHistory::can_redo),
        "the answer did not undo the paste"
    );
}

/// ⚠ A DLC pack built from the folder reads every file in it, the one the
/// undo wrote back included, until the per-frame tick takes what it ended with.
#[test]
fn redoing_a_cut_while_a_job_reads_its_folder_is_refused_naming_the_job() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (source, bin) = (dir.path().join("mypack_src"), dir.path().join("bin"));
    let props = source.join("x64").join("levels").join("gta5").join("props");
    std::fs::create_dir_all(&props).expect("the pack's folders");
    std::fs::create_dir_all(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    std::fs::write(props.join("models.rpf"), archive(&[("a.txt", b"a")])).expect("an archive");
    std::fs::write(props.join("props.ityp"), b"archetypes").expect("a loose file");
    let mut state = AppState::new();
    let tree = opened(&mut state, &source);
    let ityp = node_at(&mut state, tree, "x64/levels/gta5/props/props.ityp");
    let models = node_at(&mut state, tree, "x64/levels/gta5/props/models.rpf");
    state.set_active_document(tree);
    cut_and_paste(&mut state, tree, &[ityp], models);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));

    state.ask_dlc_pack(source.clone());
    if let Some(asked) = state.dlc_pack.asked.as_mut() {
        asked.spec.name = "mypack".into();
        asked.recheck();
    }
    state.create_dlc_pack(dir.path().join("built"));
    let building =
        |state: &AppState| (state.running_jobs().iter()).any(|j| j.kind == JobKind::DlcPack);
    assert!(
        building(&state),
        "the build did not start: {:?}",
        errors(&state)
    );
    assert_eq!(state.redo(), None, "the redo took a file the build reads");
    assert!(props.join("props.ityp").is_file());
    assert!(
        errors(&state).iter().any(|e| e.contains("DLC pack mypack")),
        "the refusal does not name the build: {:?}",
        errors(&state)
    );

    let until = Instant::now() + Duration::from_secs(60);
    while building(&state) {
        assert!(Instant::now() < until, "the build never ended");
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state.redo().as_deref(),
        Some("Paste 1 item(s)"),
        "{:?}",
        errors(&state)
    );
    assert!(!props.join("props.ityp").exists());
}

/// ⚠ A self-check's stand-in for the Recycle Bin leaves what it is asked to
/// take where it is, so the cut file never leaves.
#[test]
fn undoing_a_cut_the_bins_stand_in_left_in_place_puts_its_row_back_and_writes_nothing() {
    const LOOSE: &[u8] = b"the loose file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let was: PathBuf = dir.path().join("loose.txt");
    std::fs::write(dir.path().join("x.rpf"), archive(&[("kept.txt", b"kept")])).expect("x.rpf");
    std::fs::write(&was, LOOSE).expect("a loose file");
    recycle_bin::fake::leave_in_place();
    let mut state = AppState::new();
    let doc = opened(&mut state, dir.path());
    let x_row = node_at(&mut state, doc, "x.rpf");
    let loose = node_at(&mut state, doc, "loose.txt");
    state.set_active_document(doc);
    cut_and_paste(&mut state, doc, &[loose], x_row);
    assert_eq!(
        read(&was).as_deref(),
        Some(LOOSE),
        "the stand-in moved the file"
    );

    assert_eq!(
        state.undo().as_deref(),
        Some("Paste 1 item(s)"),
        "{:?}",
        errors(&state)
    );
    assert_eq!(
        name_of(&state, doc, loose),
        "loose.txt",
        "Files no longer lists it"
    );
    assert_eq!(names_under(&state, doc, x_row), ["kept.txt"]);
    assert_eq!(read(&was).as_deref(), Some(LOOSE));
}

#[test]
fn undoing_a_cut_of_an_archive_row_pasted_into_a_folder_on_disk_puts_it_back_whole() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let plain = dir.path().join("mods").join("plain");
    std::fs::create_dir_all(&plain).expect("a folder on disk");
    let x = archive(&[("a.txt", b"a"), ("kept.txt", b"kept")]);
    let (mut state, doc) = a_folder(dir.path(), &[("x.rpf", &x)]);
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let folder = node_at(&mut state, doc, "plain");
    let before = tree(&state, doc);
    state.copy_rows(doc, &[a], true).expect("the cut");
    state.paste_rows(doc, folder).expect("the paste");
    forget_offered(&fake::offered());
    assert_eq!(
        read(&plain.join("a.txt")).as_deref(),
        Some(&b"a"[..]),
        "the paste did not land on disk"
    );

    state.set_active_document(doc);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(
        tree(&state, doc).0,
        before.0,
        "the row is not back in its archive as it was"
    );
    assert!(
        read(&plain.join("a.txt")).is_none(),
        "the undo left the pasted file on disk"
    );

    // Redo cuts it again: the file is written, the row leaves its archive
    assert_eq!(state.redo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(read(&plain.join("a.txt")).as_deref(), Some(&b"a"[..]));
    let after = tree(&state, doc).0;
    assert!(after.contains(&"plain/a.txt".to_string()), "{after:?}");
    assert!(!after.contains(&"x.rpf/a.txt".to_string()), "{after:?}");

    // A file changed since the paste is not deleted by taking the paste back
    std::fs::write(plain.join("a.txt"), b"changed since").expect("an edit made elsewhere");
    assert_eq!(
        state.undo(),
        None,
        "the undo deleted a file that had changed"
    );
    assert_eq!(
        read(&plain.join("a.txt")).as_deref(),
        Some(&b"changed since"[..])
    );
    assert!(
        errors(&state)
            .iter()
            .any(|e| e.contains("has changed since")),
        "no sentence says why: {:?}",
        errors(&state)
    );
}

#[test]
fn undoing_a_cut_between_two_trees_lists_the_file_in_the_tree_it_left_without_a_refresh() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let x = archive(&[("kept.txt", b"kept")]);
    let (mut state, second) = a_folder(dir.path(), &[("x.rpf", &x)]);
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).expect("another folder");
    std::fs::write(other.join("loose.txt"), b"loose").expect("a loose file");
    let first = opened(&mut state, &other);
    let loose = node_at(&mut state, first, "loose.txt");
    let x_row = node_at(&mut state, second, "x.rpf");
    let before = tree(&state, first).0;
    assert!(before.contains(&"loose.txt".to_string()), "{before:?}");

    state.copy_rows(first, &[loose], true).expect("the cut");
    state.paste_rows(second, x_row).expect("the paste");
    assert!(errors(&state).is_empty(), "{:?}", errors(&state));
    assert!(
        read(&other.join("loose.txt")).is_none(),
        "the cut left the file"
    );
    let left = tree(&state, first).0;
    assert!(!left.contains(&"loose.txt".to_string()), "{left:?}");

    state.set_active_document(second);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(
        read(&other.join("loose.txt")).as_deref(),
        Some(&b"loose"[..]),
        "the file is not back on disk"
    );
    // The tree reads the folder again by itself, as a Refresh does, and lists the file once that lands
    let deadline = Instant::now() + Duration::from_secs(30);
    while tree(&state, first).0 != before {
        state.poll_expansions();
        assert!(
            Instant::now() < deadline,
            "the tree the file left does not list it again without a Refresh: {:?}",
            tree(&state, first).0
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        tree(&state, first).0,
        before,
        "the tree lists what it did before the cut"
    );
}

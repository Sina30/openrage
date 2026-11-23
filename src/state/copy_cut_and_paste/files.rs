//! Rows copied or cut in one archive or folder and pasted into another, or into
//! and out of Explorer.

use or_io::container::rpf::BodyForm;

use super::super::nested_archives_survive_a_save::{archive, body, errors, nested, on_disk, save};
use super::super::opening_a_ydd::{finish_load, node_at};
use super::super::prefs::EditMode;
use super::super::two_readers_of_one_archive_across_a_save::install;
use super::super::RetailEditDecision;
use super::*;
use crate::platform::{clipboard::fake, recycle_bin};

fn root_of(state: &AppState, doc: DocumentId) -> RpfNodeId {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.root_id(),
        _ => panic!("{doc} is not an archive"),
    }
}

fn bytes_of(state: &AppState, doc: DocumentId, node: RpfNodeId) -> Option<Vec<u8>> {
    match &state.document(doc)?.payload {
        DocPayload::Rpf(rpf) => rpf.session.bytes_for(node).map(|b| b.to_vec()),
        _ => None,
    }
}

/// ⚠ `b.rpf` is not opened before the paste, and `c.rpf` is open as a
/// document of its own, so the file lands in an archive of the same tree and
/// in one of another document.
#[test]
fn a_file_copied_in_one_archive_pastes_into_another_and_into_explorer() {
    const COPIED: &[u8] = b"the copied file";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(
        dir.path(),
        &[
            ("a.rpf", archive(&[("data/copied.txt", COPIED)])),
            ("b.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    let c = dir.path().join("c.rpf");
    std::fs::write(&c, archive(&[("other.txt", b"other")])).expect("c.rpf");
    let copied = node_at(&mut state, tree, "a.rpf/data/copied.txt");
    let b_row = node_at(&mut state, tree, "b.rpf");

    state.copy_rows(tree, &[copied], false).expect("the copy");
    let offered = fake::offered();
    assert_eq!(offered.len(), 1, "one file for Explorer: {offered:?}");
    assert!(offered[0].ends_with("copied.txt"), "{offered:?}");
    assert_eq!(
        std::fs::read(&offered[0]).ok().as_deref(),
        Some(COPIED),
        "Explorer pastes the bytes the archive holds"
    );

    state.paste_rows(tree, b_row).expect("the paste into b.rpf");
    save(&mut state, tree);
    state.load_file(c.clone());
    let c_doc = finish_load(&mut state).expect("c.rpf opens as a document");
    let c_root = root_of(&state, c_doc);
    state
        .paste_rows(c_doc, c_root)
        .expect("the paste into c.rpf");
    save(&mut state, c_doc);

    let mods = dir.path().join("mods");
    let b = on_disk(&state, &mods.join("b.rpf"));
    assert_eq!(
        body(&b, "copied.txt", BodyForm::Decoded),
        COPIED,
        "{:?}",
        errors(&state)
    );
    assert_eq!(body(&b, "kept.txt", BodyForm::Decoded), b"kept");
    let c_on_disk = on_disk(&state, &c);
    assert_eq!(body(&c_on_disk, "copied.txt", BodyForm::Decoded), COPIED);
    let a = on_disk(&state, &mods.join("a.rpf"));
    assert_eq!(
        body(&a, "copied.txt", BodyForm::Decoded),
        COPIED,
        "a copy leaves its file where it was"
    );
    forget_offered(&offered);
}

/// ⚠ The app copied first and Explorer after, so Explorer's files are the ones pasted.
#[test]
fn paste_takes_the_files_explorer_copied_as_an_import_does() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(dir.path(), &[("x.rpf", archive(&[("kept.txt", b"kept")]))]);
    let explorer = dir.path().join("explorer");
    std::fs::create_dir_all(explorer.join("pack")).expect("Explorer's folder");
    std::fs::write(explorer.join("loose.txt"), b"loose").expect("a file");
    std::fs::write(explorer.join("pack").join("inner.txt"), b"inner").expect("a file");
    let kept = node_at(&mut state, tree, "x.rpf/kept.txt");
    let x_row = node_at(&mut state, tree, "x.rpf");
    state
        .copy_rows(tree, &[kept], false)
        .expect("the app's copy");
    let ours = fake::offered();

    fake::copied_in_explorer(vec![explorer.join("loose.txt"), explorer.join("pack")]);
    state.paste_rows(tree, x_row).expect("the paste");
    save(&mut state, tree);

    let x = on_disk(&state, &dir.path().join("mods").join("x.rpf"));
    assert_eq!(
        body(&x, "loose.txt", BodyForm::Decoded),
        b"loose",
        "{:?}",
        errors(&state)
    );
    assert!(
        x.nodes.iter().any(|n| n.path() == "pack/inner.txt"),
        "a folder Explorer copied pastes as a folder with its files"
    );
    assert_eq!(body(&x, "inner.txt", BodyForm::Decoded), b"inner");
    assert_eq!(
        x.nodes.iter().filter(|n| n.name() == "kept.txt").count(),
        1,
        "the app's earlier copy was not pasted"
    );
    forget_offered(&ours);
}

/// ⚠ The first paste is refused, `b.rpf` holding the name, and leaves the row
/// where it was; the second lands and takes it out of `a.rpf`. A loose file
/// cut into an archive goes to the bin once it is in the archive.
#[test]
fn a_cut_row_leaves_only_once_its_paste_has_landed() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let (mut state, tree) = folder_of(
        dir.path(),
        &[
            (
                "a.rpf",
                archive(&[("moved.txt", b"moved"), ("stays.txt", b"stays")]),
            ),
            ("b.rpf", archive(&[("moved.txt", b"b's own")])),
            ("c.rpf", archive(&[("other.txt", b"other")])),
            ("loose.txt", b"loose".to_vec()),
        ],
    );
    let moved = node_at(&mut state, tree, "a.rpf/moved.txt");
    let a_row = node_at(&mut state, tree, "a.rpf");
    let b_moved = node_at(&mut state, tree, "b.rpf/moved.txt");
    let b_row = node_at(&mut state, tree, "b.rpf");
    let c_row = node_at(&mut state, tree, "c.rpf");
    let loose = node_at(&mut state, tree, "loose.txt");

    state.copy_rows(tree, &[moved], true).expect("the cut");
    assert!(
        names_under(&state, tree, a_row).contains(&"moved.txt".to_string()),
        "a cut takes nothing away before its paste"
    );
    state.paste_rows(tree, b_row).expect("the paste runs");
    assert!(
        names_under(&state, tree, a_row).contains(&"moved.txt".to_string()),
        "a refused paste leaves the cut row where it was"
    );
    assert_eq!(
        bytes_of(&state, tree, b_moved).as_deref(),
        Some(&b"b's own"[..])
    );
    assert!(
        errors(&state).iter().any(|e| e.contains("moved.txt")),
        "the refusal names the file: {:?}",
        errors(&state)
    );

    state.paste_rows(tree, c_row).expect("the paste");
    assert!(
        !names_under(&state, tree, a_row).contains(&"moved.txt".to_string()),
        "the landed paste took the row out of a.rpf"
    );
    assert!(!state.rows_to_paste(), "a cut pastes once");

    state.copy_rows(tree, &[loose], true).expect("the cut");
    state.paste_rows(tree, c_row).expect("the paste");
    assert!(
        bin.join("loose.txt").is_file(),
        "the loose file went to the bin once its paste landed: {:?}",
        errors(&state)
    );
    save(&mut state, tree);
    let mods = dir.path().join("mods");
    let c = on_disk(&state, &mods.join("c.rpf"));
    assert_eq!(body(&c, "moved.txt", BodyForm::Decoded), b"moved");
    assert_eq!(body(&c, "loose.txt", BodyForm::Decoded), b"loose");
    let a = on_disk(&state, &mods.join("a.rpf"));
    assert!(a.nodes.iter().all(|n| n.name() != "moved.txt"));
    let b = on_disk(&state, &mods.join("b.rpf"));
    assert_eq!(body(&b, "moved.txt", BodyForm::Decoded), b"b's own");
}

/// ⚠ `n.rpf` is opened in the tree before the copy, so its rows are there to
/// be copied one by one; it still goes as one archive file.
#[test]
fn a_nested_archive_pastes_whole() {
    const INSIDE: &[u8] = b"inside the nested archive";
    let n = archive(&[("inside.txt", INSIDE), ("data/deeper.txt", b"deeper")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(
        dir.path(),
        &[
            ("y.rpf", archive(&[("n.rpf", &n)])),
            ("x.rpf", archive(&[("kept.txt", b"kept")])),
        ],
    );
    node_at(&mut state, tree, "y.rpf/n.rpf/inside.txt");
    let n_row = node_at(&mut state, tree, "y.rpf/n.rpf");
    let x_row = node_at(&mut state, tree, "x.rpf");

    state.copy_rows(tree, &[n_row], false).expect("the copy");
    let offered = fake::offered();
    assert_eq!(
        offered
            .iter()
            .map(|p| std::fs::read(p).ok())
            .collect::<Vec<_>>(),
        [Some(n.clone())],
        "Explorer gets the nested archive as one file: {offered:?}"
    );
    state.paste_rows(tree, x_row).expect("the paste");
    save(&mut state, tree);

    let x = on_disk(&state, &dir.path().join("mods").join("x.rpf"));
    let pasted = nested(&x, "n.rpf");
    assert_eq!(
        body(&pasted, "inside.txt", BodyForm::Decoded),
        INSIDE,
        "{:?}",
        errors(&state)
    );
    assert_eq!(body(&pasted, "deeper.txt", BodyForm::Decoded), b"deeper");
    forget_offered(&offered);
}

/// ⚠ Two rows copied, and one cut, so each paste is several of the session's
/// edits that one Undo takes back together.
#[test]
fn a_paste_into_an_archive_undoes_as_one_step() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(
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
    let a_row = node_at(&mut state, tree, "a.rpf");
    let b_row = node_at(&mut state, tree, "b.rpf");
    let a_had = names_under(&state, tree, a_row);
    state.set_active_document(tree);

    state.copy_rows(tree, &[one, two], false).expect("the copy");
    let offered = fake::offered();
    state.paste_rows(tree, b_row).expect("the paste");
    assert_eq!(
        names_under(&state, tree, b_row),
        ["kept.txt", "one.txt", "two.txt"]
    );
    assert_eq!(state.undo().as_deref(), Some("Paste 2 item(s)"));
    assert_eq!(
        names_under(&state, tree, b_row),
        ["kept.txt"],
        "one Undo took both out"
    );

    state.copy_rows(tree, &[one], true).expect("the cut");
    state.paste_rows(tree, b_row).expect("the paste");
    assert_eq!(names_under(&state, tree, b_row), ["kept.txt", "one.txt"]);
    assert_eq!(state.undo().as_deref(), Some("Paste 1 item(s)"));
    assert_eq!(names_under(&state, tree, b_row), ["kept.txt"]);
    assert_eq!(
        names_under(&state, tree, a_row),
        a_had,
        "the cut row is back where it was cut from"
    );
    forget_offered(&offered);
}

/// ⚠ An install, so the gate refuses in Read only and asks in Everything, and
/// only the answer pastes.
#[test]
fn a_paste_asks_the_edit_gate_and_refuses_a_name_already_taken() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let (mut state, tree) = install(
        dir.path(),
        &[
            ("a.rpf", archive(&[("copied.txt", b"copied")])),
            ("b.rpf", archive(&[("copied.txt", b"b's own")])),
            ("c.rpf", archive(&[("other.txt", b"other")])),
        ],
    );
    let copied = node_at(&mut state, tree, "a.rpf/copied.txt");
    let b_copied = node_at(&mut state, tree, "b.rpf/copied.txt");
    let b_row = node_at(&mut state, tree, "b.rpf");
    let c_row = node_at(&mut state, tree, "c.rpf");
    state.copy_rows(tree, &[copied], false).expect("the copy");
    let offered = fake::offered();

    state.prefs.edit_mode = EditMode::ReadOnly;
    assert!(state.paste_rows(tree, c_row).is_err(), "Read only refuses");
    assert!(!names_under(&state, tree, c_row).contains(&"copied.txt".to_string()));
    assert!(
        state.pending_retail_edit.is_none(),
        "a refusal asks nothing"
    );

    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    assert!(
        state.paste_rows(tree, c_row).is_err(),
        "the gate asks first"
    );
    assert!(!names_under(&state, tree, c_row).contains(&"copied.txt".to_string()));
    let asked = state
        .pending_retail_edit
        .as_ref()
        .map(|p| (p.target_path.clone(), p.verb));
    assert_eq!(asked, Some((dir.path().join("c.rpf"), "paste into")));
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    assert!(
        names_under(&state, tree, c_row).contains(&"copied.txt".to_string()),
        "the answer pastes: {:?}",
        errors(&state)
    );

    state.paste_rows(tree, b_row).expect("the paste runs");
    assert!(
        errors(&state).iter().any(|e| e.contains("already exists")),
        "{:?}",
        errors(&state)
    );
    assert_eq!(
        bytes_of(&state, tree, b_copied).as_deref(),
        Some(&b"b's own"[..]),
        "the file b.rpf holds is as it was"
    );
    forget_offered(&offered);
}

#[test]
fn a_cut_pasted_into_the_folder_its_rows_sit_in_stays_held_and_says_nothing_moved() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, tree) = folder_of(
        dir.path(),
        &[(
            "a.rpf",
            archive(&[("moved.txt", b"moved"), ("stays.txt", b"stays")]),
        )],
    );
    let moved = node_at(&mut state, tree, "a.rpf/moved.txt");
    let a_row = node_at(&mut state, tree, "a.rpf");

    state.copy_rows(tree, &[moved], true).expect("the cut");
    state.paste_rows(tree, a_row).expect("the paste runs");
    assert!(
        state.rows_to_paste(),
        "a cut that moved nothing left the clipboard"
    );
    let said: Vec<_> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(
        !said.iter().any(|m| m.contains("Pasted")),
        "a paste that moved nothing says Pasted: {said:?}"
    );
    assert!(
        said.iter().any(|m| m.contains("Already in this folder")),
        "nothing says why: {said:?}"
    );
    assert!(
        !state.document(tree).expect("open").history.can_undo(),
        "a paste that moved nothing made a step"
    );
}

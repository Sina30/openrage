//! An archive row with unsaved edits shows it, a rename on disk leaves nothing
//! to save, a delete waits for the extraction reading it, an extraction writes
//! the whole of what it names, and a folder is never imported into itself.

use std::path::Path;

use or_io::container::rpf::BodyForm;

use super::a_moved_or_deleted_archive_row_leaves_nothing_behind::folder_of;
use super::every_add_lands_where_the_tree_shows_it::opened;
use super::extract_job::{ExtractStart, TypeFilter};
use super::nested_archives_survive_a_save::{archive, body, errors, on_disk, save, EDITED};
use super::nothing_extracts_into_an_install::{files_under, run_to_completion};
use super::opening_a_ydd::node_at;
use super::os_drop::DropTarget;
use super::prefs::EditMode;
use super::two_readers_of_one_archive_across_a_save::install;
use super::{AppState, DocPayload};
use crate::platform::recycle_bin;

/// The badge Files draws beside a row holding unsaved edits.
const UNSAVED: char = '●';

/// Whether Files, drawn as the editor draws it, marks the archive row `name` unsaved.
fn marked(state: &mut AppState, name: &str) -> bool {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(600.0, 900.0),
        )),
        ..Default::default()
    };
    let output =
        egui::Context::default().run_ui(input, |ui| crate::ui::panels::hierarchy::draw(ui, state));
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut texts);
    }
    let row = texts
        .iter()
        .find(|t| t.starts_with(egui_phosphor::regular::ARCHIVE) && t.ends_with(name))
        .unwrap_or_else(|| panic!("Files drew no {name} row: {texts:?}"));
    row.contains(UNSAVED)
}

/// Every folder under `dir`, ending in `/`, and every file.
fn everything_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(dir).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                out.push(format!("{rel}/"));
                stack.push(path);
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_archive_row_with_unsaved_edits_shows_it_until_a_save_writes_them() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[
            ("x.rpf", archive(&[("a.txt", b"a")])),
            ("y.rpf", archive(&[("b.txt", b"b")])),
        ],
    );
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    node_at(&mut state, doc, "y.rpf/b.txt");
    assert!(!marked(&mut state, "x.rpf"), "opened, and marked unedited");

    state
        .update_rpf_entry_bytes(doc, a, EDITED.to_vec())
        .expect("the edit");
    assert!(
        marked(&mut state, "x.rpf"),
        "the edited archive is unmarked"
    );
    assert!(
        !marked(&mut state, "y.rpf"),
        "an archive nothing edited is marked"
    );

    save(&mut state, doc);
    assert!(
        !marked(&mut state, "x.rpf"),
        "the save left the row marked: {:?}",
        errors(&state)
    );
}

/// ⚠ In an install, so a drop the refusal misses stops at the edit gate's
/// question instead of copying the folder into itself until its path is too
/// long.
#[test]
fn a_folder_dropped_onto_itself_or_a_folder_inside_it_is_refused_with_nothing_written() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join("gta5.exe"), b"an install marker").expect("marker");
    let dropped = dir.path().join("a");
    std::fs::create_dir_all(dropped.join("b")).expect("the folders");
    std::fs::write(dropped.join("f.txt"), b"f").expect("a file");
    let (mut state, tree) = install(dir.path(), &[]);
    state.prefs.edit_mode = EditMode::Everything;
    state.retail_edit_session_allow = false;
    let before = everything_under(dir.path());

    for onto in ["a/b", "a"] {
        let folder = node_at(&mut state, tree, onto);
        let said = errors(&state).len();
        state.handle_os_drop(
            vec![dropped.clone()],
            DropTarget::Folder { doc: tree, folder },
        );
        assert!(
            state.pending_retail_edit.is_none(),
            "the drop onto {onto} asked to write rather than refusing"
        );
        let refusals = errors(&state).split_off(said);
        assert!(
            refusals.iter().any(|e| e.contains("into itself")),
            "the drop onto {onto}: {refusals:?}"
        );
        assert_eq!(
            everything_under(dir.path()),
            before,
            "the drop onto {onto} wrote"
        );
    }
}

/// ⚠ `n.rpf` carries a block OpenRage's builder does not write, so a save
/// that rebuilt it for its new name rather than storing its bytes loses it.
#[test]
fn renaming_an_opened_archive_on_disk_leaves_nothing_to_save() {
    let n = [archive(&[("inside.txt", b"inside")]), vec![0x5A; 512]].concat();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(
        dir.path(),
        &[("x.rpf", archive(&[("a.txt", b"a"), ("n.rpf", &n)]))],
    );
    node_at(&mut state, doc, "x.rpf/n.rpf/inside.txt");
    let row = node_at(&mut state, doc, "x.rpf");

    state
        .rpf_rename_node(doc, row, "y.rpf")
        .expect("the rename");
    let y = dir.path().join("mods").join("y.rpf");
    assert!(y.is_file(), "the rename did not reach the disk");
    assert!(
        state.unsaved_documents(&[doc]).is_empty(),
        "a rename on disk left something to save"
    );
    assert!(
        !marked(&mut state, "y.rpf"),
        "the renamed archive is marked"
    );

    let nested = node_at(&mut state, doc, "y.rpf/n.rpf");
    state
        .rpf_rename_node(doc, nested, "m.rpf")
        .expect("the nested archive's rename");
    assert!(
        marked(&mut state, "y.rpf"),
        "a rename inside y.rpf is unmarked"
    );
    save(&mut state, doc);
    assert_eq!(
        body(&on_disk(&state, &y), "m.rpf", BodyForm::AsStored),
        n,
        "{:?}",
        errors(&state)
    );
    assert!(state.unsaved_documents(&[doc]).is_empty());
}

/// ⚠ The extraction counts as reading until the per-frame tick takes its
/// report, so a fast one still holds its archive here.
#[test]
fn deleting_an_archive_during_its_extraction_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).expect("the bin");
    recycle_bin::fake::use_bin(&bin);
    let sub = dir.path().join("mods").join("sub");
    std::fs::create_dir_all(&sub).expect("the folders");
    let x = archive(&[("a.txt", b"a"), ("data/b.txt", b"b")]);
    std::fs::write(sub.join("x.rpf"), x).expect("x.rpf");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.path().join("mods"));
    let row = node_at(&mut state, doc, "sub/x.rpf");
    let folder = node_at(&mut state, doc, "sub");
    let out = dir.path().join("out");

    let started = state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
    assert_eq!(started, ExtractStart::Planning);
    for gone in [row, folder] {
        state.rpf_delete_node(doc, gone).expect("the delete asks");
        state.answer_delete(true);
    }
    let said = errors(&state);
    run_to_completion(&mut state);
    assert!(
        sub.join("x.rpf").is_file(),
        "x.rpf went to the bin during its extraction: {said:?}"
    );
    let named = said.iter().filter(|e| e.contains("extraction of x.rpf"));
    assert_eq!(named.count(), 2, "{said:?}");
    assert_eq!(files_under(&out), ["a.txt", "data/b.txt"]);

    state.rpf_delete_node(doc, row).expect("the delete asks");
    state.answer_delete(true);
    assert!(
        bin.join("x.rpf").is_file(),
        "refused once the extraction was done: {:?}",
        errors(&state)
    );
}

/// ⚠ `sub` is a row the tree has never scanned, and `y.rpf` is opened while
/// the archives nested in it are not.
#[test]
fn an_extraction_writes_an_unscanned_folder_and_every_archive_nested_in_what_it_names() {
    let m = archive(&[("deep.txt", b"deep")]);
    let n = archive(&[("inside.txt", b"inside"), ("m.rpf", &m)]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mods = dir.path().join("mods");
    let sub = mods.join("sub");
    std::fs::create_dir_all(sub.join("deeper")).expect("the folders");
    std::fs::write(sub.join("one.txt"), b"one").expect("a file");
    std::fs::write(sub.join("deeper").join("two.txt"), b"two").expect("a file");
    let x = archive(&[("a.txt", b"a"), ("data/n.rpf", &n)]);
    std::fs::write(sub.join("x.rpf"), x).expect("x.rpf");
    std::fs::write(
        mods.join("y.rpf"),
        archive(&[("b.txt", b"b"), ("n.rpf", &n)]),
    )
    .expect("y.rpf");
    let mut state = AppState::new();
    let doc = opened(&mut state, &mods);
    let sub_row = node_at(&mut state, doc, "sub");
    node_at(&mut state, doc, "y.rpf/b.txt");
    let y_row = node_at(&mut state, doc, "y.rpf");

    let mut written = Vec::new();
    for (row, into) in [(sub_row, "from_sub"), (y_row, "from_y")] {
        let out = dir.path().join(into);
        let started =
            state.extract_rpf_node(doc, row, out.clone(), BodyForm::Decoded, TypeFilter::All);
        run_to_completion(&mut state);
        assert_eq!(
            started,
            ExtractStart::Planning,
            "{into}: {:?}",
            errors(&state)
        );
        written.push(files_under(&out));
    }
    assert_eq!(
        written,
        [
            vec![
                "deeper/two.txt",
                "one.txt",
                "x.rpf/a.txt",
                "x.rpf/data/n.rpf/inside.txt",
                "x.rpf/data/n.rpf/m.rpf/deep.txt",
            ],
            vec!["b.txt", "n.rpf/inside.txt", "n.rpf/m.rpf/deep.txt"],
        ]
    );
    let deep = dir.path().join("from_y").join("n.rpf").join("m.rpf");
    assert_eq!(
        std::fs::read(deep.join("deep.txt")).expect("deep.txt"),
        b"deep"
    );
}

#[test]
fn unloading_an_archive_forgets_every_archive_opened_in_it() {
    let n = archive(&[("inside.txt", b"inside")]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = folder_of(dir.path(), &[("x.rpf", archive(&[("n.rpf", &n)]))]);
    node_at(&mut state, doc, "x.rpf/n.rpf/inside.txt");
    let row = node_at(&mut state, doc, "x.rpf");
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("the folder's tree");
    };
    assert_eq!(rpf.session.archives.len(), 2, "x.rpf and n.rpf are open");

    rpf.session.unload_archive(row);
    assert!(rpf.session.archives.is_empty(), "a slot outlived its rows");
}

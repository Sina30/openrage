//! A second save in one session writes every entry from the right bytes, what
//! is added to an archive the tree has not opened reaches it, an entry that
//! cannot be read refuses the save, and an opened folder saves its archives.

use std::sync::Arc;

use or_io::container::rpf::{BodyForm, RpfArchive};

use super::nested_archives_survive_a_save::{
    archive, body, errors, nested, on_disk, open, save, Opened, EDITED,
};
use super::opening_a_ydd::node_at;
use super::{AppState, DocumentKind};

/// Every file of `archive` and of each archive nested in it, decoded, by path.
fn contents(archive: &Arc<RpfArchive>) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let bytes = body(archive, node.name(), BodyForm::Decoded);
        if node.name().ends_with(".rpf") {
            for (path, inner) in contents(&nested(archive, node.name())) {
                out.push((format!("{}/{path}", node.path()), inner));
            }
        }
        out.push((node.path().to_string(), bytes));
    }
    out.sort();
    out
}

/// ⚠ More files than the session's read cache holds, a nested archive the
/// first save rebuilt and one never opened, none of which the cache keeps, so
/// a second save that read the replaced file's offsets writes some of them wrong.
#[test]
fn a_second_save_in_one_session_writes_every_entry_from_the_archive_the_first_wrote() {
    let files: Vec<(String, Vec<u8>)> = (0..80)
        .map(|n| {
            (
                format!("files/f{n:02}.txt"),
                format!("file {n} ").repeat(40).into_bytes(),
            )
        })
        .collect();
    let browsed = archive(&[("inside.txt", b"inside the browsed archive")]);
    let unopened = [archive(&[("inside.txt", b"never opened")]), vec![0x5A; 512]].concat();
    let mut entries: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    let first = vec![0x11; 64 * 1024];
    entries.extend([
        ("a.awc", first.as_slice()),
        ("browsed.rpf", browsed.as_slice()),
        ("unopened.rpf", unopened.as_slice()),
    ]);
    let outer = archive(&entries);

    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let (tree, path, prefix) = open(&mut state, dir.path(), &outer, Opened::InAnInstallTree);
    let inside = node_at(&mut state, tree, &format!("{prefix}browsed.rpf/inside.txt"));
    let a = node_at(&mut state, tree, &format!("{prefix}a.awc"));
    state
        .update_rpf_entry_bytes(tree, inside, EDITED.to_vec())
        .expect("an edit inside the browsed archive");
    state
        .update_rpf_entry_bytes(tree, a, vec![0x22; 128 * 1024])
        .expect("the first edit");
    save(&mut state, tree);
    let first = contents(&on_disk(&state, &path));
    assert!(
        first
            .iter()
            .any(|(path, bytes)| path == "browsed.rpf/inside.txt" && bytes == EDITED),
        "the first save did not rebuild the browsed archive: {:?}",
        errors(&state)
    );

    state
        .update_rpf_entry_bytes(tree, a, vec![0x33; 3 * 1024])
        .expect("the second edit");
    save(&mut state, tree);
    let saved = contents(&on_disk(&state, &path));
    let expected: Vec<(String, Vec<u8>)> = first
        .into_iter()
        .map(|(path, bytes)| match path.as_str() {
            "a.awc" => (path, vec![0x33; 3 * 1024]),
            _ => (path, bytes),
        })
        .collect();
    let wrong: Vec<&String> = expected
        .iter()
        .zip(&saved)
        .filter(|(e, s)| e != s)
        .map(|(e, _)| &e.0)
        .collect();
    assert!(
        wrong.is_empty() && expected.len() == saved.len(),
        "the second save wrote {} of {} entries wrong, among them {:?}: {:?}",
        wrong.len(),
        expected.len(),
        wrong.iter().take(5).collect::<Vec<_>>(),
        errors(&state)
    );
}

/// ⚠ `junk.rpf` is named like an archive and is not one, so a move into it
/// must fail and leave the file where it was.
#[test]
fn a_file_added_to_or_moved_into_a_nested_archive_not_yet_opened_reaches_it() {
    let unopened = archive(&[("inside.txt", b"inside")]);
    let outer = archive(&[
        ("a.txt", b"moved"),
        ("b.txt", b"stays"),
        ("data/unopened.rpf", &unopened),
        ("junk.rpf", b"not an archive"),
    ]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let loose = dir.path().join("added.txt");
    std::fs::write(&loose, b"added").expect("a loose file");
    let mut state = AppState::new();
    let (tree, path, prefix) = open(&mut state, dir.path(), &outer, Opened::InAnInstallTree);
    let into = node_at(&mut state, tree, &format!("{prefix}data/unopened.rpf"));
    let a = node_at(&mut state, tree, &format!("{prefix}a.txt"));
    let b = node_at(&mut state, tree, &format!("{prefix}b.txt"));
    let junk = node_at(&mut state, tree, &format!("{prefix}junk.rpf"));

    state
        .rpf_import_file_to_folder(tree, into, loose)
        .expect("a file imported into the nested archive");
    state
        .rpf_move_node(tree, a, into)
        .expect("a file moved into the nested archive");
    let into_junk = state.rpf_move_node(tree, b, junk);
    save(&mut state, tree);

    let saved = on_disk(&state, &path);
    let inner = nested(&saved, "unopened.rpf");
    for (name, bytes) in [
        ("inside.txt", b"inside".as_slice()),
        ("added.txt", b"added"),
        ("a.txt", b"moved"),
    ] {
        assert_eq!(
            body(&inner, name, BodyForm::Decoded),
            bytes,
            "{name} in the nested archive: {:?}",
            errors(&state)
        );
    }
    assert!(
        saved.nodes.iter().all(|n| n.name() != "a.txt"),
        "the moved file is still in the outer archive"
    );
    assert!(
        into_junk.is_err(),
        "a move into an archive that cannot be opened went ahead"
    );
    assert_eq!(body(&saved, "b.txt", BodyForm::Decoded), b"stays");
}

#[test]
fn a_save_that_cannot_read_an_entry_is_refused_and_names_it() {
    let outer = archive(&[("a.txt", b"shipped"), ("z.awc", &[0x7A; 64 * 1024])]);
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    let (tree, path, prefix) = open(&mut state, dir.path(), &outer, Opened::InAnInstallTree);
    let a = node_at(&mut state, tree, &format!("{prefix}a.txt"));
    state
        .update_rpf_entry_bytes(tree, a, EDITED.to_vec())
        .expect("the edit");
    let cut = &outer[..8 * 1024];
    std::fs::write(&path, cut).expect("the archive loses its last entry's body");

    save(&mut state, tree);
    assert_eq!(
        std::fs::read(&path).expect("the archive"),
        cut,
        "a save that could not read z.awc wrote the archive"
    );
    assert!(
        errors(&state).iter().any(|e| e.contains("outer.rpf/z.awc")),
        "the refusal does not name the entry: {:?}",
        errors(&state)
    );
}

#[test]
fn ctrl_s_in_an_opened_folder_saves_the_archives_edited_in_it() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let folder = dir.path().join("mods");
    std::fs::create_dir_all(&folder).expect("the folder");
    let path = folder.join("x.rpf");
    std::fs::write(&path, archive(&[("a.txt", b"shipped")])).expect("the archive");
    let mut state = AppState::new();
    state.open_folder(folder.clone());
    let doc = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::Folder)
        .expect("the folder's tree")
        .id;
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    state
        .update_rpf_entry_bytes(doc, a, EDITED.to_vec())
        .expect("the edit");

    save(&mut state, doc);
    assert_eq!(
        body(&on_disk(&state, &path), "a.txt", BodyForm::Decoded),
        EDITED,
        "Ctrl+S did not save the archive: {:?}",
        errors(&state)
    );
    assert!(folder.is_dir());
    let beside: Vec<String> = std::fs::read_dir(dir.path())
        .expect("the scratch directory")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(beside, ["mods"], "the save wrote beside the folder");
}

/// ⚠ Renamed, moved into a folder and that folder renamed, with edits unsaved
/// in it and in an archive nested in it, so a save that kept the path either
/// was opened from reads entries from a file that is gone, or writes the old
/// name back.
#[test]
fn an_edited_archive_renamed_and_moved_on_disk_saves_where_it_now_is() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let folder = dir.path().join("mods");
    std::fs::create_dir_all(folder.join("sub")).expect("the folders");
    let inner = archive(&[("deep.txt", b"deep"), ("other.txt", b"other")]);
    std::fs::write(
        folder.join("x.rpf"),
        archive(&[("a.txt", b"shipped"), ("b.txt", b"kept"), ("n.rpf", &inner)]),
    )
    .expect("the archive");
    let mut state = AppState::new();
    state.open_folder(folder.clone());
    let doc = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::Folder)
        .expect("the folder's tree")
        .id;
    let deep = node_at(&mut state, doc, "x.rpf/n.rpf/deep.txt");
    let a = node_at(&mut state, doc, "x.rpf/a.txt");
    let row = node_at(&mut state, doc, "x.rpf");
    let sub = node_at(&mut state, doc, "sub");
    for edited in [a, deep] {
        state
            .update_rpf_entry_bytes(doc, edited, EDITED.to_vec())
            .expect("the edit");
    }
    state
        .rpf_rename_node(doc, row, "y.rpf")
        .expect("the rename");
    state.rpf_move_node(doc, row, sub).expect("the move");
    state
        .rpf_rename_node(doc, sub, "moved")
        .expect("the folder's rename");

    save(&mut state, doc);
    let saved = on_disk(&state, &folder.join("moved").join("y.rpf"));
    assert_eq!(
        body(&saved, "a.txt", BodyForm::Decoded),
        EDITED,
        "the edit is not where the archive now is: {:?}",
        errors(&state)
    );
    assert_eq!(body(&saved, "b.txt", BodyForm::Decoded), b"kept");
    let n = nested(&saved, "n.rpf");
    assert_eq!(body(&n, "deep.txt", BodyForm::Decoded), EDITED);
    assert_eq!(body(&n, "other.txt", BodyForm::Decoded), b"other");
    assert!(
        ["x.rpf", "y.rpf", "sub"]
            .iter()
            .all(|old| !folder.join(old).exists()),
        "the save wrote an old name back"
    );
}

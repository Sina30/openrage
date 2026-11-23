//! New file, New folder and Copy into archive add to the folder or archive the row
//! shows: into a folder on disk at once, into an archive once a save writes it.

use std::path::Path;

use or_io::container::rpf::{BodyForm, RpfArchive};

use super::document_io::empty_file_bytes_for_kind;
use super::nested_archives_survive_a_save::{archive, body, errors, nested, on_disk, save};
use super::opening_a_ydd::node_at;
use super::{AppState, DocumentId, DocumentKind};

const KIND: DocumentKind = DocumentKind::Gxt2;

/// `folder` opened with Open Folder, and its document.
pub(super) fn opened(state: &mut AppState, folder: &Path) -> DocumentId {
    state.open_folder(folder.to_path_buf());
    state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::Folder)
        .expect("the folder's tree")
        .id
}

/// What `dir` holds, each folder as its name and a `/`, each file as its name and bytes.
fn on_disk_listing(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("the folder")
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                (format!("{name}/"), Vec::new())
            } else {
                (name, std::fs::read(e.path()).expect("the file"))
            }
        })
        .collect();
    out.sort();
    out
}

/// What `archive` holds, the same way, by path.
fn in_archive(archive: &RpfArchive) -> Vec<(String, Vec<u8>)> {
    archive
        .nodes
        .iter()
        .filter(|n| !n.path().is_empty())
        .map(|n| {
            if n.is_dir() {
                (format!("{}/", n.path()), Vec::new())
            } else {
                let bytes = body(archive, n.name(), BodyForm::Decoded);
                (n.path().to_string(), bytes)
            }
        })
        .collect()
}

/// ⚠ Every case runs before any is judged, so one run names each that fails.
/// `x.rpf` is not opened before its adds and `n.rpf` is listed and not opened.
#[test]
fn new_file_new_folder_and_import_file_land_in_the_folder_or_archive_each_row_shows() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let folder = dir.path().join("mods");
    std::fs::create_dir_all(folder.join("sub")).expect("the folders");
    let x = archive(&[("a.txt", b"in x")]);
    let n = archive(&[("b.txt", b"in n")]);
    std::fs::write(folder.join("x.rpf"), &x).expect("x.rpf");
    std::fs::write(folder.join("y.rpf"), archive(&[("n.rpf", &n)])).expect("y.rpf");
    let import = dir.path().join("imported.txt");
    std::fs::write(&import, b"imported").expect("a file to import");
    let mut state = AppState::new();
    let doc = opened(&mut state, &folder);
    let rows = [
        ("the folder sub on disk", node_at(&mut state, doc, "sub")),
        (
            "the archive x.rpf on disk",
            node_at(&mut state, doc, "x.rpf"),
        ),
        (
            "the archive n.rpf in y.rpf",
            node_at(&mut state, doc, "y.rpf/n.rpf"),
        ),
    ];

    let mut failed = Vec::new();
    for (row, id) in rows {
        for (command, result) in [
            ("New folder", state.rpf_add_folder(doc, id, "new_folder")),
            ("New file", state.rpf_add_empty_file(doc, id, KIND)),
            (
                "Copy into archive",
                state.rpf_import_file_to_folder(doc, id, import.clone()),
            ),
        ] {
            if let Err(e) = result {
                failed.push(format!("{command} on {row}: {e}"));
            }
        }
    }
    let added = [
        ("new_folder/".to_string(), Vec::new()),
        (
            format!("untitled.{}", KIND.extension()),
            empty_file_bytes_for_kind(KIND).expect("a blank file"),
        ),
        ("imported.txt".to_string(), b"imported".to_vec()),
    ];
    let lacks = |row: &str, found: &[(String, Vec<u8>)]| -> Vec<String> {
        added
            .iter()
            .filter(|a| !found.contains(a))
            .map(|(name, _)| format!("{row} lacks {name}"))
            .collect()
    };
    failed.extend(lacks(
        "sub on disk before any save",
        &on_disk_listing(&folder.join("sub")),
    ));
    if std::fs::read(folder.join("x.rpf")).expect("x.rpf") != x {
        failed.push("x.rpf was written before a save".into());
    }
    if state.unsaved_documents(&[doc]) != [doc] {
        failed.push("the archives added to are not counted as unsaved".into());
    }

    save(&mut state, doc);
    failed.extend(lacks(
        "x.rpf once saved",
        &in_archive(&on_disk(&state, &folder.join("x.rpf"))),
    ));
    let y = on_disk(&state, &folder.join("y.rpf"));
    failed.extend(lacks("n.rpf once saved", &in_archive(&nested(&y, "n.rpf"))));
    if !state.unsaved_documents(&[doc]).is_empty() {
        failed.push("the save left the folder unsaved".into());
    }
    assert!(
        failed.is_empty(),
        "{}\nerrors: {:?}",
        failed.join("\n"),
        errors(&state)
    );
}

/// ⚠ `sub` is not scanned, so only the disk knows what it holds.
#[test]
fn an_add_to_a_folder_on_disk_takes_no_name_the_disk_already_holds() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let sub = dir.path().join("mods").join("sub");
    std::fs::create_dir_all(sub.join("new_folder")).expect("the folders");
    std::fs::write(sub.join("new_folder").join("keep.txt"), b"kept").expect("a file");
    let taken = format!("untitled.{}", KIND.extension());
    std::fs::write(sub.join(&taken), b"mine").expect("a file in the way");
    let mut state = AppState::new();
    let doc = opened(&mut state, &dir.path().join("mods"));
    let row = node_at(&mut state, doc, "sub");

    assert!(
        state.rpf_add_folder(doc, row, "new_folder").is_err(),
        "New folder took a name the disk holds"
    );
    state
        .rpf_add_empty_file(doc, row, KIND)
        .expect("New file picks a name the disk does not hold");
    let mut expected = vec![
        ("new_folder/".to_string(), Vec::new()),
        (taken, b"mine".to_vec()),
        (
            format!("untitled1.{}", KIND.extension()),
            empty_file_bytes_for_kind(KIND).expect("a blank file"),
        ),
    ];
    expected.sort();
    assert_eq!(on_disk_listing(&sub), expected, "{:?}", errors(&state));
    assert_eq!(
        std::fs::read(sub.join("new_folder").join("keep.txt")).expect("keep.txt"),
        b"kept"
    );
}

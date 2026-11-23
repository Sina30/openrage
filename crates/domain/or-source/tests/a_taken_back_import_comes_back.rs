//! What an import made can be kept aside before it is taken back and put in its place again exactly, so Redo makes it again.

use std::fs;

use or_source::game_import::{tree_digest, Created};

fn made(path: &std::path::Path) -> Created {
    Created {
        path: path.to_path_buf(),
        digest: tree_digest(path).unwrap(),
    }
}

#[test]
fn a_folder_kept_aside_is_taken_back_and_put_in_its_place_exactly() {
    let project = tempfile::tempdir().unwrap();
    let folder = project.path().join("sources/road.textures.src");
    fs::create_dir_all(folder.join("textures")).unwrap();
    fs::write(folder.join("asset.json"), b"{}").unwrap();
    fs::write(folder.join("textures/road.dds"), [1u8, 2, 3, 4]).unwrap();
    let created = made(&folder);
    let aside = project.path().join("aside/0");

    created.copy_to(&aside).expect("kept aside");
    assert!(created.undo().unwrap(), "untouched, so it is removed");
    assert!(!folder.exists());

    created.restore_from(&aside).expect("put back");
    assert!(created.untouched(), "the same bytes in the same names");
    assert!(!aside.exists(), "what was put back is no longer kept aside");
}

#[test]
fn a_file_made_by_an_import_comes_back_too_and_a_place_taken_since_is_not_overwritten() {
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("files/config.meta");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"<meta/>").unwrap();
    let created = made(&file);
    let aside = project.path().join("aside/0");
    created.copy_to(&aside).unwrap();
    assert!(created.undo().unwrap());

    created.restore_from(&aside).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"<meta/>");

    // Somebody wrote the place in the meantime: it is theirs, and what was kept aside stays.
    created.copy_to(&project.path().join("aside/1")).unwrap();
    created.undo().unwrap();
    fs::write(&file, b"mine").unwrap();
    let refused = created
        .restore_from(&project.path().join("aside/1"))
        .unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&file).unwrap(), b"mine");
}

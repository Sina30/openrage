//! A pack of thousands of files is read once: what a session read is kept in the pack's private folder, and the next one reads only what changed.

mod common;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use common::built::{ytd, Tex};
use common::{garment, JBIB};
use or_io::codec::texture::GrcTextureFormat as F;
use or_project::content::facts::{files_read, remembered_told};
use or_project::{Container, Module};
use or_wardrobe::{cache, lock, planned_with_model, Depth, Ped, Wardrobe};

const GARMENTS: usize = 120;

/// The counters of `facts` belong to the process, so a test that reads a pack while another reads one counts the other's files; every test takes this first.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|held| held.into_inner())
}

/// A pack of `GARMENTS` garments whose files are all dated long ago, as the files of a pack that was cloned are.
fn pack(folder: &Path) -> Wardrobe {
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let mut w = Wardrobe::new("pack");
    for i in 0..GARMENTS {
        let (model, texture) = (format!("m{i}.ytd"), format!("t{i}.ytd"));
        for (name, side) in [(&model, 32u16), (&texture, 64)] {
            let path = folder.join(name);
            std::fs::write(&path, ytd(&[Tex::new("x", side, side, F::DXT1)])).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(long_ago).unwrap();
        }
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            Some(i as u32),
            &model,
            &[&texture],
        ));
    }
    w.write(folder).unwrap();
    w
}

fn read(folder: &Path) -> (Wardrobe, or_wardrobe::Planned) {
    let module = Module::clothing("wardrobe", ".");
    let _ = Container::default();
    planned_with_model(folder, &module, Depth::Measure).expect("a pack reads")
}

#[test]
fn a_second_session_reads_only_what_changed_since_the_first_kept() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());

    let before = files_read();
    let (_, first) = read(dir.path());
    assert!(
        files_read() - before >= 2 * GARMENTS,
        "the first session read every file"
    );
    cache::save(dir.path());
    let kept = dir.path().join(".openrage").join("files.json");
    assert!(kept.is_file(), "what the session read is kept");
    assert!(
        dir.path().join(".openrage").join(".gitignore").is_file(),
        "in a folder that ignores itself, so it never reaches a repository"
    );

    // A new session knows nothing but what the folder kept.
    cache::forget(dir.path());
    let before = files_read();
    let (_, second) = read(dir.path());
    assert_eq!(files_read() - before, 0, "the second session read no file");
    assert_eq!(
        second.contribution.figures, first.contribution.figures,
        "and says what the first said"
    );
    assert_eq!(second.findings, first.findings);

    // A file that changed is read again, and only that one.
    let changed = dir.path().join("t3.ytd");
    std::fs::write(&changed, ytd(&[Tex::new("x", 128, 128, F::DXT1)])).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&changed)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(1800))
        .unwrap();
    let before = files_read();
    read(dir.path());
    assert_eq!(files_read() - before, 1);
}

#[test]
fn a_cache_that_does_not_read_is_no_cache_and_costs_one_reading() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    let private = dir.path().join(".openrage");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("files.json"), b"{ not json").unwrap();
    cache::forget(dir.path());
    let before = files_read();
    read(dir.path());
    assert!(files_read() - before >= 2 * GARMENTS);
}

#[test]
fn a_pack_that_is_read_in_a_moment_leaves_nothing_in_its_folder() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let mut w = Wardrobe::new("pack");
    for i in 0..3 {
        let (model, texture) = (format!("m{i}.ytd"), format!("t{i}.ytd"));
        for name in [&model, &texture] {
            let path = dir.path().join(name);
            std::fs::write(&path, ytd(&[Tex::new("x", 32, 32, F::DXT1)])).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        w.garments
            .push(garment(Ped::Male, JBIB, Some(i), &model, &[&texture]));
    }
    w.write(dir.path()).unwrap();
    read(dir.path());
    cache::save(dir.path());
    assert!(
        !dir.path().join(".openrage").join("files.json").exists(),
        "six files are not worth a file of their own"
    );
}

#[test]
fn a_scan_that_read_no_file_keeps_nothing_again_and_one_that_read_a_file_keeps_it() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    read(dir.path());
    cache::save(dir.path());
    assert!(dir.path().join(".openrage").join("files.json").is_file());

    // ⚠ A scan that runs again on a timer finds every file as it was and has nothing to keep: writing the whole memory out as text to find that costs more than the scan.
    let before = remembered_told();
    for _ in 0..3 {
        read(dir.path());
        cache::save(dir.path());
    }
    assert_eq!(
        remembered_told() - before,
        0,
        "a scan that read nothing wrote nothing out"
    );

    let changed = dir.path().join("t3.ytd");
    std::fs::write(&changed, ytd(&[Tex::new("x", 128, 128, F::DXT1)])).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&changed)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(1800))
        .unwrap();
    read(dir.path());
    cache::save(dir.path());
    assert_eq!(
        remembered_told() - before,
        1,
        "a file that was read is kept"
    );

    // The next session reads none of it again, the changed file included, and has nothing to keep: what it took is what the folder holds.
    cache::forget(dir.path());
    let (before, told) = (files_read(), remembered_told());
    read(dir.path());
    cache::save(dir.path());
    assert_eq!(files_read() - before, 0);
    assert_eq!(
        remembered_told() - told,
        0,
        "a session that read no file wrote nothing out"
    );
}

#[test]
fn a_window_another_holds_the_pack_of_keeps_nothing_in_it_and_keeps_what_it_read_once_it_is_free() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    pack(dir.path());
    read(dir.path());
    let kept = dir.path().join(".openrage").join("files.json");

    // ⚠ A second window reads a pack the first edits and writes nothing in it, a cache of what it read included.
    let first = lock::acquire(dir.path()).expect("nobody holds it");
    cache::save(dir.path());
    assert!(
        !kept.exists(),
        "a window locked out of a pack made a file in it"
    );

    drop(first);
    cache::save(dir.path());
    assert!(
        kept.is_file(),
        "once the pack is free it keeps what it read"
    );
}

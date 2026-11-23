//! A mounted install counts the tables of contents of its root archives, so the Statistics panel
//! can show what a read of the install keeps.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfBuilder, RpfEntry};
use or_title::{build_mounted_game, DlcCutoff, GtaInstall, MountedGame};

fn archive_bytes(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut builder = RpfBuilder::default();
    for (inner, body) in entries {
        builder.insert_with(inner, body.clone(), false).unwrap();
    }
    builder.to_bytes().unwrap()
}

/// `n` small files, each in a folder of its own.
fn files(n: usize) -> Vec<(String, Vec<u8>)> {
    (0..n)
        .map(|i| (format!("props/set_{i:04}/prop.ydr"), b"x".to_vec()))
        .collect()
}

/// An install whose `x64a.rpf` holds `n` files.
fn install(name: &str, n: usize) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("an_install_mount_counts_its_tables")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("GTA5.exe"), b"").unwrap();
    std::fs::write(
        root.join("common.rpf"),
        archive_bytes(&[("data/version.txt".into(), b"base".to_vec())]),
    )
    .unwrap();
    std::fs::write(root.join("x64a.rpf"), archive_bytes(&files(n))).unwrap();
    root
}

fn mount(root: &Path) -> MountedGame {
    let crypto: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
    let install = GtaInstall {
        root: root.to_path_buf(),
    };
    build_mounted_game(&install, crypto, DlcCutoff::BaseGame).expect("the install mounts")
}

#[test]
fn an_archives_tables_grow_with_its_entries_names_and_tree() {
    let small = RpfArchive::from_bytes(archive_bytes(&files(2))).unwrap();
    let large = RpfArchive::from_bytes(archive_bytes(&files(400))).unwrap();
    let (few, many) = (small.table_bytes(), large.table_bytes());
    assert!(
        many >= few + 398 * std::mem::size_of::<RpfEntry>(),
        "398 more entries: {few} -> {many} bytes"
    );
    assert!(
        many >= large.entries.len() * std::mem::size_of::<RpfEntry>()
            + large.header.name_heap_size as usize,
        "the entries and the names they point at, at least"
    );
}

#[test]
fn a_mount_holds_the_tables_of_its_root_archives() {
    let small = mount(&install("small", 2)).held_bytes();
    let large = mount(&install("large", 400)).held_bytes();
    assert!(small > 0, "the roots an install opens");
    assert!(
        large >= small + 398 * std::mem::size_of::<RpfEntry>(),
        "398 more entries in one root: {small} -> {large} bytes"
    );
}

//! Two mods that write the same archive entry stack in a load order, and the later one wins.
//!
//! What is asserted is that a reorder changes which mod the stack resolves a
//! shared path to, and that a saved order rebuilds the same stack from a fresh
//! scan, so the order is a fact about the install and not about one scan.

use std::path::Path;
use std::sync::Arc;

use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::RpfBuilder;
use or_title::mods::{
    arrange, nudge, order_of, read_dlclist, resolve, scan_install, ClaimKind, InstalledMod,
    ModKind, ModPlace, ModStatus,
};
use or_title::{DlcList, MountOptions};

fn crypto() -> Arc<dyn RpfCryptoProvider> {
    Arc::new(NoOpCryptoProvider::new())
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// A `dlc.rpf` holding its two documents and `files`.
fn dlc_pack(files: &[&str]) -> Vec<u8> {
    let mut builder = RpfBuilder::default();
    for document in ["setup2.xml", "content.xml"] {
        builder
            .insert_with(document, b"<x/>", false)
            .expect("a document");
    }
    for file in files {
        builder
            .insert_with(file, file.as_bytes(), false)
            .expect("an entry");
    }
    builder.to_bytes().expect("save")
}

fn names(mods: &[InstalledMod], place: ModPlace) -> Vec<&str> {
    mods.iter()
        .filter(|m| m.place == place)
        .map(|m| m.name.as_str())
        .collect()
}

const SHARED: &str = "x64a.rpf/levels/gta5/props.ydr";

/// Two mods that each carry `SHARED` as a folder-form archive entry, and one entry of their own.
fn two_mods_on_one_entry(root: &Path) {
    write(root, &format!("mods/BigProps/{SHARED}"), b"big");
    write(
        root,
        "mods/BigProps/x64a.rpf/levels/gta5/only_big.ydr",
        b"b",
    );
    write(root, &format!("mods/SmallProps/{SHARED}"), b"small");
    write(
        root,
        "mods/SmallProps/x64a.rpf/levels/gta5/only_small.ydr",
        b"s",
    );
}

#[test]
fn reordering_two_mods_that_write_the_same_archive_entry_flips_which_one_wins() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    two_mods_on_one_entry(root);

    let mut mods = scan_install(root, &crypto(), None);
    // ⚠ Nothing saved: a mod the order has not met stacks last, by name.
    arrange(&mut mods, &[]);
    assert_eq!(names(&mods, ModPlace::Overlay), ["BigProps", "SmallProps"]);

    let before = resolve(&mods);
    assert_eq!(before.conflicts.len(), 1, "only the shared entry conflicts");
    let conflict = &before.conflicts[0];
    assert_eq!(conflict.kind, ClaimKind::ArchiveEntry);
    assert_eq!(conflict.targets, [SHARED]);
    assert_eq!(conflict.mods, ["BigProps", "SmallProps"]);
    assert_eq!(conflict.winner, "SmallProps");

    assert!(nudge(&mut mods, "SmallProps", true), "it can move up");
    let after = resolve(&mods);
    assert_eq!(after.conflicts.len(), 1);
    assert_eq!(after.conflicts[0].mods, ["SmallProps", "BigProps"]);
    assert_eq!(after.conflicts[0].winner, "BigProps");

    // The saved order rebuilds the same stack from a scan that knows nothing else.
    let saved = order_of(&mods);
    let mut again = scan_install(root, &crypto(), None);
    arrange(&mut again, &saved);
    assert_eq!(resolve(&again).conflicts[0].winner, "BigProps");

    // Moving it back flips it again, and a mod at the end cannot move down.
    assert!(nudge(&mut again, "SmallProps", false));
    assert_eq!(resolve(&again).conflicts[0].winner, "SmallProps");
    assert!(!nudge(&mut again, "SmallProps", false));
}

#[test]
fn a_parked_mod_is_disabled_and_conflicts_with_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, &format!("mods/Live/{SHARED}"), b"live");
    write(root, &format!("mods_/Parked/{SHARED}"), b"parked");

    let mut mods = scan_install(root, &crypto(), None);
    arrange(&mut mods, &[]);

    let parked = mods.iter().find(|m| m.name == "Parked").expect("listed");
    assert_eq!(parked.place, ModPlace::Parked);
    assert_eq!(parked.place.status(), ModStatus::Disabled);
    let live = mods.iter().find(|m| m.name == "Live").expect("listed");
    assert_eq!(live.place.status(), ModStatus::Enabled);

    assert!(
        resolve(&mods).conflicts.is_empty(),
        "a mod the game is not given takes no part in the stack"
    );
}

#[test]
fn two_dlc_packs_that_ship_the_same_file_conflict_and_their_documents_do_not() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    for pack in ["alpha", "beta"] {
        write(
            root,
            &format!("mods/{pack}/update/x64/dlcpacks/{pack}/dlc.rpf"),
            &dlc_pack(&["x64/anim/shared.ycd", &format!("x64/anim/{pack}_only.ycd")]),
        );
    }
    let list = DlcList {
        paths: vec!["dlcpacks:/alpha/".to_string().into()],
    };

    let mut mods = scan_install(root, &crypto(), Some(&list));
    arrange(&mut mods, &[]);

    let kinds: Vec<_> = mods.iter().map(|m| m.kind).collect();
    assert_eq!(kinds, [ModKind::DlcPack, ModKind::DlcPack]);
    let listed: Vec<_> = mods
        .iter()
        .map(|m| (m.packs[0].name.as_str(), m.packs[0].listed))
        .collect();
    assert_eq!(listed, [("alpha", Some(true)), ("beta", Some(false))]);

    let resolution = resolve(&mods);
    let dlc: Vec<_> = resolution
        .conflicts
        .iter()
        .filter(|c| c.kind == ClaimKind::DlcContent)
        .collect();
    assert_eq!(dlc.len(), 1);
    assert_eq!(
        dlc[0].targets,
        ["x64/anim/shared.ycd"],
        "setup2.xml and content.xml are each pack's own, and a pack's own file is no clash"
    );
    assert_eq!(dlc[0].winner, "beta");
}

#[test]
fn an_asi_a_loader_and_a_script_are_classified_and_take_no_part_in_the_stack() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "NativeTrainer.asi", b"MZ");
    write(root, "dinput8.dll", b"MZ");
    write(root, "scripts/Speedometer.cs", b"class S {}");
    write(root, "scripts/Hud/Hud.dll", b"MZ");
    write(root, "d3dcompiler_46.dll", b"MZ");

    let mods = scan_install(root, &crypto(), None);
    let found: Vec<_> = mods
        .iter()
        .map(|m| (m.name.as_str(), m.kind, m.place))
        .collect();
    assert_eq!(
        found,
        [
            ("dinput8.dll", ModKind::AsiLoader, ModPlace::GameFolder),
            (
                "NativeTrainer.asi",
                ModKind::AsiPlugin,
                ModPlace::GameFolder
            ),
            ("Hud", ModKind::ScriptMod, ModPlace::GameFolder),
            ("Speedometer.cs", ModKind::ScriptMod, ModPlace::GameFolder),
        ],
        "a shipped dll is not a mod"
    );
    assert!(resolve(&mods).conflicts.is_empty());
}

#[test]
fn a_pack_that_will_not_open_is_reported_and_not_looked_into() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "mods/Broken/update/x64/dlcpacks/broken/dlc.rpf",
        b"not an archive",
    );
    write(
        root,
        "mods/Fine/update/x64/dlcpacks/fine/dlc.rpf",
        &dlc_pack(&["x64/a.ycd"]),
    );

    let mut mods = scan_install(root, &crypto(), None);
    arrange(&mut mods, &[]);
    let resolution = resolve(&mods);

    assert!(
        !resolution.is_complete(),
        "an empty conflict list must not claim there are none"
    );
    assert_eq!(resolution.unread.len(), 1);
    assert_eq!(resolution.unread[0].mod_name, "Broken");
    assert!(resolution.conflicts.is_empty());
}

#[test]
fn the_dlclist_the_install_holds_is_read_and_an_install_with_none_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    assert!(
        read_dlclist(root, crypto(), MountOptions::default()).is_none(),
        "no `common.rpf` is no list"
    );

    let list = DlcList {
        paths: vec!["dlcpacks:/alpha/".to_string().into()],
    };
    let bytes = RpfBuilder::default()
        .insert_with("data/dlclist.xml", list.to_xml().into_bytes(), false)
        .expect("the list")
        .to_bytes()
        .expect("save");
    write(root, "common.rpf", &bytes);

    let read = read_dlclist(root, crypto(), MountOptions::default()).expect("the list is read");
    assert_eq!(read, list);
}

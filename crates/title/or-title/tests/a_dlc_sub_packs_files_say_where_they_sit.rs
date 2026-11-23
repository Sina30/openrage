//! A file a DLC's `dlc1.rpf` holds says which archive holds it, as a file of its `dlc.rpf` does,
//! and opens by either spelling of the pack's device.
//!
//! ⚠ The sub-pack mounts on the pack's device beside `dlc.rpf`, so a lookup
//! that finds the file in it must answer with the sub-pack's own path, or a
//! caller joining the answer onto the install root walks to a file that is not
//! there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::RpfBuilder;
use or_title::vfs::{build_mounted_game, DlcCutoff};
use or_title::{DlcList, GtaInstall, MountedGame, SetupData};

const MODEL: &str = "levels/dlc/prop_dlc.ydr";
const DATA: &str = "data/dlc_only.dat";
const DEVICE: &str = "dlc_testpack";

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = RpfBuilder::default();
    for (path, body) in entries {
        builder
            .insert_with(path, *body, false)
            .expect("add an entry");
    }
    builder.to_bytes().expect("save")
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// `dlc.rpf`, holding the pack's own file, and `dlc1.rpf`, holding the model.
fn pack() -> (Vec<u8>, Vec<u8>) {
    let setup = SetupData {
        name_hash: "testpack".into(),
        device_name: DEVICE.into(),
        dat_file: "content.xml".into(),
        sub_pack_count: 1,
        ..Default::default()
    };
    let dlc = archive(&[
        ("setup2.xml", setup.to_xml().as_bytes()),
        (
            "content.xml",
            or_title::ContentXml::default().to_xml().as_bytes(),
        ),
        (DATA, b"pack"),
    ]);
    (dlc, archive(&[(MODEL, b"sub-pack")]))
}

/// An install whose `dlclist.xml` names `testpack`, laid out as `holder` says, and the folder's guard.
fn install(name: &str, holder: Holder) -> (tempfile::TempDir, PathBuf) {
    let scratch = tempfile::Builder::new()
        .prefix(&format!("or-title-sub-pack-{name}-"))
        .tempdir()
        .expect("scratch");
    let root = scratch.path().to_path_buf();
    let list = DlcList {
        paths: vec!["dlcpacks:/testpack/".to_string().into()],
    };
    write(
        &root.join("common.rpf"),
        &archive(&[("data/dlclist.xml", list.to_xml().as_bytes())]),
    );
    let (dlc, sub) = pack();
    match holder {
        Holder::Disk => {
            write(&root.join("x64/dlcpacks/testpack/dlc.rpf"), &dlc);
            write(&root.join("x64/dlcpacks/testpack/dlc1.rpf"), &sub);
        }
        Holder::TitleUpdate => write(
            &root.join("update/update.rpf"),
            &archive(&[
                ("dlcpacks/testpack/dlc.rpf", &dlc),
                ("dlcpacks/testpack/dlc1.rpf", &sub),
            ]),
        ),
    }
    (scratch, root)
}

#[derive(Clone, Copy)]
enum Holder {
    /// The pack's archives on disk under `x64/dlcpacks`.
    Disk,
    /// The pack's archives inside `update/update.rpf`.
    TitleUpdate,
}

fn mount(root: &Path) -> MountedGame {
    let crypto: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
    let game = build_mounted_game(
        &GtaInstall {
            root: root.to_path_buf(),
        },
        crypto,
        DlcCutoff::All,
    )
    .expect("the synthetic install mounts");
    assert_eq!(
        (game.vfs.open(&format!("{DEVICE}:/{MODEL}"))).as_deref(),
        Some(&b"sub-pack"[..]),
        "the sub-pack mounted: {:?}",
        game.diagnostics.messages()
    );
    game
}

fn container_of(game: &MountedGame, logical: &str) -> String {
    let mut found = None;
    game.for_each_entry(|facts| {
        if facts.logical == logical {
            found = Some(facts.container.clone());
        }
    });
    found.unwrap_or_else(|| panic!("the walk reaches {logical}"))
}

/// ⚠ A content file writes a pack's path against `{device}CRC:` as often as
/// against `{device}:`, and `dlc.rpf` answers to both, so a sub-pack that
/// answered to one would hide its files from half the references.
#[test]
fn a_file_in_a_sub_pack_opens_by_a_path_written_against_the_crc_device() {
    for (name, holder) in [
        ("crc-disk", Holder::Disk),
        ("crc-title-update", Holder::TitleUpdate),
    ] {
        let (_scratch, root) = install(name, holder);
        let game = mount(&root);
        let (plain, crc) = (
            format!("{DEVICE}:/{MODEL}"),
            format!("{DEVICE}CRC:/{MODEL}"),
        );

        assert_eq!(
            game.vfs.open(&format!("{DEVICE}CRC:/{DATA}")).as_deref(),
            Some(&b"pack"[..]),
            "{name}: dlc.rpf answers to the CRC device"
        );
        assert_eq!(
            game.vfs.open(&crc).as_deref(),
            Some(&b"sub-pack"[..]),
            "{name}: so must dlc1.rpf: {:?}",
            game.diagnostics.messages()
        );
        assert!(game.vfs.exists(&crc), "{name}");
        assert_eq!(
            game.vfs.locate(&crc),
            game.vfs.locate(&plain),
            "{name}: and it names the same archive for both"
        );
    }
}

#[test]
fn a_file_in_a_sub_pack_on_disk_locates_inside_the_sub_pack() {
    let (_scratch, root) = install("disk", Holder::Disk);
    let game = mount(&root);

    assert_eq!(
        game.vfs.locate(&format!("{DEVICE}:/{DATA}")).as_deref(),
        Some("x64/dlcpacks/testpack/dlc.rpf/data/dlc_only.dat"),
        "a file of dlc.rpf names its archive"
    );
    assert_eq!(
        game.vfs.locate(&format!("{DEVICE}:/{MODEL}")).as_deref(),
        Some("x64/dlcpacks/testpack/dlc1.rpf/levels/dlc/prop_dlc.ydr"),
        "so does a file of dlc1.rpf"
    );
    assert_eq!(
        container_of(&game, &format!("{DEVICE}:/{MODEL}")),
        "x64/dlcpacks/testpack/dlc1.rpf"
    );
}

#[test]
fn a_file_in_a_sub_pack_inside_the_title_update_locates_inside_the_sub_pack() {
    let (_scratch, root) = install("title-update", Holder::TitleUpdate);
    let game = mount(&root);

    assert_eq!(
        game.vfs.locate(&format!("{DEVICE}:/{DATA}")).as_deref(),
        Some("update/update.rpf/dlcpacks/testpack/dlc.rpf/data/dlc_only.dat"),
        "a file of dlc.rpf names its archive"
    );
    assert_eq!(
        game.vfs.locate(&format!("{DEVICE}:/{MODEL}")).as_deref(),
        Some("update/update.rpf/dlcpacks/testpack/dlc1.rpf/levels/dlc/prop_dlc.ydr"),
        "so does a file of dlc1.rpf, the title update's archive first"
    );
    assert_eq!(
        container_of(&game, &format!("{DEVICE}:/{MODEL}")),
        "update/update.rpf/dlcpacks/testpack/dlc1.rpf",
        "the walk's container is the chain of archives holding it"
    );
}

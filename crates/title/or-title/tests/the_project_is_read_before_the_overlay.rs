//! A project's copy of a file is what the mount reads, ahead of the overlay and the install.
//!
//! The precedence is three layers — install, then `mods`, then the project —
//! and each is asserted with the one above it removed, so a chain that skips a
//! layer fails rather than passing on the layer it did not skip.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::RpfBuilder;
use or_title::vfs::{build_mounted_game_with, build_vfs_for_install_with, DlcCutoff};
use or_title::{load_clip_regions, mount_shadow, GtaInstall, MountOptions, CLIP_REGIONS_VFS_PATH};

fn provider() -> Arc<dyn RpfCryptoProvider> {
    Arc::new(NoOpCryptoProvider::new())
}

fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut builder = RpfBuilder::default();
    for (inner, body) in entries {
        builder
            .insert_with(inner, *body, false)
            .expect("add an entry");
    }
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    builder.save(path).expect("save");
}

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn clip_regions(texture: &str) -> Vec<u8> {
    format!("1 1\n{texture} 1 1 0.0 1.0 0.0 1.0\n").into_bytes()
}

struct Layout {
    _dir: tempfile::TempDir,
    install: PathBuf,
    shadow: PathBuf,
}

fn layout() -> Layout {
    let dir = tempfile::tempdir().expect("tempdir");
    let install = dir.path().join("GTAV");
    let shadow = dir.path().join("my-project").join("vfs");
    std::fs::create_dir_all(&shadow).unwrap();
    write_archive(
        &install.join("common.rpf"),
        &[
            ("data/where.txt", b"install"),
            ("data/untouched.txt", b"install"),
            (
                "data/effects/ptxclipregions.dat",
                &clip_regions("install_tex"),
            ),
        ],
    );
    write_archive(
        &install.join("x64a.rpf"),
        &[("levels/where.txt", b"install")],
    );
    Layout {
        _dir: dir,
        install,
        shadow,
    }
}

fn read(layout: &Layout, path: &str, with_project: bool) -> Option<Vec<u8>> {
    let mut game = build_mounted_game_with(
        &GtaInstall {
            root: layout.install.clone(),
        },
        provider(),
        DlcCutoff::BaseGame,
        MountOptions::default(),
    )
    .expect("the synthetic install mounts");
    if with_project {
        mount_shadow(&mut game.vfs, &layout.shadow);
    }
    game.vfs.open(path)
}

#[test]
fn the_project_outranks_the_overlay_which_outranks_the_install() {
    let layout = layout();
    write_archive(
        &layout.install.join("mods/common.rpf"),
        &[
            ("data/where.txt", b"overlay"),
            ("data/untouched.txt", b"overlay"),
        ],
    );
    write(&layout.shadow.join("common/data/where.txt"), b"project");
    write(&layout.shadow.join("platform/levels/where.txt"), b"project");

    assert_eq!(
        read(&layout, "common:/data/where.txt", true).as_deref(),
        Some(&b"project"[..]),
        "the project is mounted last, so it shadows the overlay's archive"
    );
    assert_eq!(
        read(&layout, "platform:/levels/where.txt", true).as_deref(),
        Some(&b"project"[..]),
        "and an archive with no overlay copy at all"
    );
    assert_eq!(
        read(&layout, "common:/data/untouched.txt", true).as_deref(),
        Some(&b"overlay"[..]),
        "a file the project does not hold falls through to the overlay"
    );
    assert_eq!(
        read(&layout, "common:/data/where.txt", false).as_deref(),
        Some(&b"overlay"[..]),
        "with no project open, the overlay is what is read"
    );

    std::fs::remove_dir_all(layout.install.join("mods")).unwrap();
    assert_eq!(
        read(&layout, "common:/data/untouched.txt", true).as_deref(),
        Some(&b"install"[..]),
        "and with no overlay either, the install"
    );
}

/// The render input an install VFS supplies, read through a project.
#[test]
fn the_clip_region_table_comes_from_the_project_when_it_holds_one() {
    let layout = layout();
    write(
        &layout.shadow.join("common/data/effects/ptxclipregions.dat"),
        &clip_regions("project_tex"),
    );
    let install = GtaInstall {
        root: layout.install.clone(),
    };

    let mut vfs =
        build_vfs_for_install_with(&install, provider(), MountOptions::default()).expect("mounts");
    let installed = load_clip_regions(&vfs).expect("the install's table parses");
    assert!(installed.get_by_name("install_tex").is_some());

    let report = mount_shadow(&mut vfs, &layout.shadow);
    assert!(
        report.shadowed.iter().any(|(mount, _)| mount == "common"),
        "{report:?}"
    );
    assert!(vfs.exists(CLIP_REGIONS_VFS_PATH));
    let table = load_clip_regions(&vfs).expect("the project's table parses");
    assert!(table.get_by_name("project_tex").is_some());
    assert!(
        table.get_by_name("install_tex").is_none(),
        "the whole file is shadowed, not merged"
    );
}

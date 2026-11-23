//! An archive the user copied into `mods` is the one the mount reads.
//!
//! What is asserted is that a path **inside** the overlay's archive is what
//! `common:/…` resolves to, which is only true if the overlay copy was opened
//! as an archive rather than served as bytes by a folder device over `mods`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{NoOpCryptoProvider, RpfCryptoProvider};
use or_io::container::rpf::RpfBuilder;
use or_title::vfs::{build_mounted_game, build_mounted_game_with, DlcCutoff};
use or_title::{ArchiveOrigin, GtaInstall, MountOptions};

fn provider() -> Arc<dyn RpfCryptoProvider> {
    Arc::new(NoOpCryptoProvider::new())
}

/// Write an RPF holding one file, at `path`.
fn write_archive(path: &Path, inner: &str, body: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
    RpfBuilder::default()
        .insert_with(inner, body, false)
        .expect("add an entry")
        .save(path)
        .expect("save");
}

/// An install with `common.rpf`, one platform pack and a title update, each
/// holding one file that says where it came from, and the folder's guard.
fn install_with_overlay(name: &str, overlaid: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let scratch = tempfile::Builder::new()
        .prefix(&format!("or-title-overlay-mount-{name}-"))
        .tempdir()
        .expect("mkdir");
    let root = scratch.path().to_path_buf();

    let shipped: [(&str, &str); 3] = [
        ("common.rpf", "data/where.txt"),
        ("x64a.rpf", "levels/where.txt"),
        ("update/update.rpf", "common/data/patched.txt"),
    ];
    for (archive, inner) in shipped {
        write_archive(&root.join(archive), inner, b"install");
        if overlaid.contains(&archive) {
            write_archive(&root.join("mods").join(archive), inner, b"overlay");
        }
    }
    (scratch, root)
}

fn mount(root: &Path, options: MountOptions) -> or_title::MountedGame {
    build_mounted_game_with(
        &GtaInstall {
            root: root.to_path_buf(),
        },
        provider(),
        DlcCutoff::BaseGame,
        options,
    )
    .expect("the synthetic install mounts")
}

fn origin_of(game: &or_title::MountedGame, container: &str) -> ArchiveOrigin {
    game.roots
        .iter()
        .find(|r| r.container == container)
        .unwrap_or_else(|| {
            panic!(
                "no mounted archive labelled `{container}`; the mount holds {:?}",
                game.roots.iter().map(|r| &r.container).collect::<Vec<_>>()
            )
        })
        .origin
}

#[test]
fn an_overlay_copy_of_an_archive_is_what_the_mount_reads() {
    let (_scratch, root) = install_with_overlay("reads", &["common.rpf", "update/update.rpf"]);
    let game = mount(&root, MountOptions::default());

    assert_eq!(
        game.vfs.open("common:/data/where.txt").as_deref(),
        Some(&b"overlay"[..]),
        "the overlay's `common.rpf` is what `common:` reads. Bytes from inside \
         it are also the proof that it was opened as an archive rather than \
         served as a file."
    );
    assert_eq!(
        game.vfs.open("common:/data/patched.txt").as_deref(),
        Some(&b"overlay"[..]),
        "and the overlay's `update.rpf` still mounts on `common:` under its \
         `common` sub-root, which is where the install's copy mounts"
    );

    assert_eq!(origin_of(&game, "mods/common.rpf"), ArchiveOrigin::Overlay);
    assert_eq!(
        origin_of(&game, "mods/update/update.rpf"),
        ArchiveOrigin::Overlay
    );
}

/// ⚠ A partial overlay is the normal state, and a whole-tree resolution fails it.
#[test]
fn an_archive_with_no_overlay_copy_still_comes_from_the_install() {
    let (_scratch, root) = install_with_overlay("partial", &["common.rpf"]);
    let game = mount(&root, MountOptions::default());

    assert_eq!(
        game.vfs.open("common:/data/where.txt").as_deref(),
        Some(&b"overlay"[..]),
        "the one archive that was copied"
    );
    assert_eq!(
        game.vfs.open("platform:/levels/where.txt").as_deref(),
        Some(&b"install"[..]),
        "and the one that was not. Resolving the whole tree to one root or the \
         other is the version of this that looks simpler and reads an install \
         nobody has."
    );

    assert_eq!(origin_of(&game, "mods/common.rpf"), ArchiveOrigin::Overlay);
    assert_eq!(origin_of(&game, "x64a.rpf"), ArchiveOrigin::Install);
}

#[test]
fn the_toggle_reads_the_install_with_the_overlay_in_place() {
    let (_scratch, root) = install_with_overlay("toggle", &["common.rpf", "update/update.rpf"]);
    let game = mount(&root, MountOptions::install_only());

    for (path, expected) in [
        ("common:/data/where.txt", &b"install"[..]),
        ("common:/data/patched.txt", &b"install"[..]),
    ] {
        assert_eq!(
            game.vfs.open(path).as_deref(),
            Some(expected),
            "`{path}` must come from the install when overlay resolution is off"
        );
    }
    assert!(
        game.roots
            .iter()
            .all(|r| r.origin == ArchiveOrigin::Install),
        "and nothing reports an overlay origin: {:?}",
        game.roots
            .iter()
            .map(|r| (&r.container, r.origin))
            .collect::<Vec<_>>()
    );
}

/// The three-argument entry point defaults to overlay-first, so its callers
/// read what the game reads.
#[test]
fn the_three_argument_entry_point_reads_the_overlay() {
    let (_scratch, root) = install_with_overlay("default", &["common.rpf"]);
    let game = build_mounted_game(
        &GtaInstall { root: root.clone() },
        provider(),
        DlcCutoff::BaseGame,
    )
    .expect("mounts");

    assert_eq!(
        game.vfs.open("common:/data/where.txt").as_deref(),
        Some(&b"overlay"[..])
    );
}

/// ⚠ **Zero is the number that has to be reportable.** An overlay that matched
/// nothing is what a user gets for putting one in the wrong place, and it is
/// indistinguishable from having no overlay unless the report says so.
#[test]
fn the_mount_report_names_where_each_archive_came_from() {
    let (_scratch, root) = install_with_overlay("report", &["common.rpf"]);

    let game = mount(&root, MountOptions::default());
    let findings: Vec<String> = game
        .diagnostics
        .messages()
        .iter()
        .map(|d| d.to_string())
        .collect();
    let joined = findings.join("\n");
    assert!(
        joined.contains("mods/common.rpf"),
        "the report must name the archive that came from the overlay: {joined}"
    );
    assert!(
        joined.contains("1 of 3 mounted archive(s) came from it"),
        "and summarise how much of the install the overlay actually replaced: {joined}"
    );

    // The same install with nothing copied in: the overlay directory is there
    // and changed nothing.
    let (_empty_scratch, empty) = install_with_overlay("report-empty", &[]);
    std::fs::create_dir_all(empty.join("mods")).expect("an empty overlay");
    let game = mount(&empty, MountOptions::default());
    let joined = game
        .diagnostics
        .messages()
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("0 of 3 mounted archive(s) came from it"),
        "an overlay that matched nothing has to say so: {joined}"
    );

    let game = mount(&root, MountOptions::install_only());
    let joined = game
        .diagnostics
        .messages()
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("overlay-first resolution is off"),
        "and an overlay that exists while the toggle is off is worth saying \
         once, rather than leaving the user to wonder why their files are \
         ignored: {joined}"
    );
}

/// An install with no overlay says nothing about one.
#[test]
fn an_install_with_no_overlay_reports_nothing_about_one() {
    let (_scratch, root) = install_with_overlay("none", &[]);
    let game = mount(&root, MountOptions::default());

    assert!(
        game.diagnostics
            .messages()
            .iter()
            .all(|d| !d.to_string().contains("overlay")),
        "no overlay, no report: {:?}",
        game.diagnostics
            .messages()
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
    assert!(game
        .roots
        .iter()
        .all(|r| r.origin == ArchiveOrigin::Install));
}

/// ⚠ An overlay is not an install.
///
/// `mods/common.rpf` beside nothing else is a mods folder, and mounting one as
/// a game would let a directory the user made look like a second install.
#[test]
fn an_overlay_alone_does_not_make_a_directory_an_install() {
    let scratch = tempfile::tempdir().expect("mkdir");
    let root = scratch.path().to_path_buf();
    write_archive(&root.join("mods/common.rpf"), "data/where.txt", b"overlay");

    let err = build_mounted_game_with(
        &GtaInstall { root: root.clone() },
        provider(),
        DlcCutoff::BaseGame,
        MountOptions::default(),
    );
    assert!(
        matches!(err, Err(or_title::VfsBuildError::MissingCommon { .. })),
        "membership is the install's own `common.rpf`"
    );
}

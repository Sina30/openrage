//! Which side of the VFS each archive job is on, asserted from the source.
//!
//! | job | side | because |
//! |---|---|---|
//! | `openrage-cli rpf extract` | **beside** | it is handed one archive file and asked about entry indices and body forms, which are archive concepts. Routing it through a mount would mean mounting an install to read a loose `.rpf`. |
//! | `openrage-cli bulk` | **above** | "every texture in the install" is a question about DLC override order and a logical namespace, and answering it by walking archives would be a second implementation of overlay precedence. |
//!
//! ⚠ Both drifts are one convenient import away and neither fails a test on its
//! own. `bulk` reaching for an `RpfArchive` to get at "every copy rather than
//! the winning one" would silently duplicate DLC ordering; `rpf extract`
//! reaching for a `MountedGame` to resolve a keys cache would make extracting
//! from a loose archive require an install.

use std::path::{Path, PathBuf};

fn or_cli_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read(name: &str) -> String {
    let path = or_cli_src().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must exist and be readable: {e}", path.display()))
}

/// The same text with comment lines removed.
///
/// ⚠ `bulk.rs` explains in comments why it does *not* hold an `RpfArchive`,
/// and a scan over raw text reads that explanation as the violation.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `bulk` asks the mount, never an archive.
#[test]
fn the_bulk_job_is_above_the_vfs_and_names_no_archive() {
    let source = code_only(&read("bulk.rs"));

    for wanted in ["mount_all", "game.vfs", "for_each_file"] {
        assert!(
            source.contains(wanted),
            "bulk.rs no longer uses `{wanted}`, so it has stopped asking the \
             mount what exists. Whatever it asks instead is a second \
             implementation of DLC override order."
        );
    }

    // ⚠ `bulk` reaching for an archive is how it would grow an "every copy,
    // not just the winning one" mode, and that mode cannot be built without
    // re-deciding precedence.
    for forbidden in ["RpfArchive", "RpfBuilder", "read_entry"] {
        assert!(
            !source.contains(forbidden),
            "bulk.rs names `{forbidden}`. It is above the VFS and must ask the \
             mount for bytes; holding an archive means it is deciding for itself \
             which copy of a path wins, which `or-title`'s DLC tests no longer \
             cover."
        );
    }
}

/// `rpf extract` is handed an archive, and stays that way.
#[test]
fn the_per_archive_extract_is_beside_the_vfs() {
    let source = code_only(&read("rpf.rs"));

    assert!(
        source.contains("read_form"),
        "rpf.rs must go through the named body forms, or the three extraction \
         actions R21 asks for are not what the CLI offers"
    );
    assert!(
        !source.contains("build_mounted_game"),
        "rpf.rs has started mounting an install. Extracting from a loose `.rpf` \
         must not require one — that is the inversion R19 removed from the asset \
         layer, arriving in the CLI."
    );
}

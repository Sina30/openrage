//! The staged scope is the index, and both kinds of damage are caught.
//!
//! The direction worth checking is the one that would pass silently: a BOM the
//! commit does not contain must *not* fail the staged run, or `--staged` is
//! scanning the tree and the scope is decoration.

use crate::common::Scratch;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

#[test]
fn the_staged_scope_is_the_index_and_both_kinds_of_damage_are_caught() {
    let repo = Scratch::repository("encoding");
    repo.write("Cargo.toml", b"[workspace]\r\nmembers = []");

    repo.write("clean.md", b"no damage here");
    repo.git(&["add", "clean.md"]);
    assert!(
        repo.xtask(&["lint-encoding", "--staged"]).status.success(),
        "a clean staged file was reported as damaged"
    );

    // A BOM on disk that the commit does not contain must pass.
    let mut bommed = BOM.to_vec();
    bommed.extend_from_slice(b"bommed but not staged");
    repo.write("unstaged.md", &bommed);
    assert!(
        repo.xtask(&["lint-encoding", "--staged"]).status.success(),
        "a BOM in an unstaged file failed the staged check, so the scope is the tree"
    );
    // And the full sweep does see it, or the scope is the only thing working.
    assert!(
        !repo.xtask(&["lint-encoding"]).status.success(),
        "the full sweep missed a BOM on disk"
    );

    repo.git(&["add", "unstaged.md"]);
    assert!(
        !repo.xtask(&["lint-encoding", "--staged"]).status.success(),
        "a staged BOM was not caught"
    );

    // The BOM is removable, and removing it leaves the sweep clean.
    assert!(repo.xtask(&["lint-encoding", "--fix-bom"]).status.success());
    assert!(repo.xtask(&["lint-encoding"]).status.success());
    repo.git(&["rm", "--cached", "-f", "-q", "unstaged.md"]);
    std::fs::remove_file(repo.0.join("unstaged.md")).expect("the file exists");

    // Mojibake is the other half, and is invisible in a diff viewer: U+00E2
    // U+0080 U+0094 is an em dash that went through cp1252.
    repo.write("mojibake.md", "\u{e2}\u{80}\u{94}".as_bytes());
    repo.git(&["add", "mojibake.md"]);
    assert!(
        !repo.xtask(&["lint-encoding", "--staged"]).status.success(),
        "staged mojibake was not caught"
    );
}

#[test]
fn a_nested_target_directory_is_not_read() {
    let repo = Scratch::repository("encoding-prune");
    repo.write("Cargo.toml", b"[workspace]\nmembers = []\n");
    let mut bommed = BOM.to_vec();
    bommed.extend_from_slice(b"x");
    // Pruned at any depth and on every OS: the script this replaces matched
    // backslashes only, so a nested one was read everywhere but Windows.
    repo.write("crates/a/target/debug/build.md", &bommed);
    repo.write("crates/a/src/lib.rs", b"pub fn f() {}\n");
    assert!(repo.xtask(&["lint-encoding"]).status.success());
}

//! The provenance lint's three checks, against repositories it can be wrong in.
//!
//! The patterns are built at run time, so this file is not the thing it lints.

use crate::common::{text, Scratch};

fn leak() -> String {
    ["dev", "_ng"].concat()
}

#[test]
fn a_clean_tree_passes() {
    let repo = Scratch::repository("prov-clean");
    repo.write("src/lib.rs", b"pub fn f() {}\n");
    repo.git(&["add", "-A"]);
    let out = repo.xtask(&["lint-provenance"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("clean"));
}

#[test]
fn a_tracked_line_citing_the_leaked_tree_is_fatal_and_named() {
    let repo = Scratch::repository("prov-citation");
    repo.write("docs/a.md", format!("see {} for it\n", leak()).as_bytes());
    repo.git(&["add", "-A"]);
    let out = repo.xtask(&["lint-provenance"]);
    assert!(!out.status.success(), "a leak pattern passed");
    assert!(text(&out).contains("docs/a.md:1"), "{}", text(&out));
}

#[test]
fn tracked_game_content_is_fatal_by_extension() {
    let repo = Scratch::repository("prov-content");
    repo.write("crates/x/tests/a.ytd", b"not really a texture");
    repo.git(&["add", "-A"]);
    let out = repo.xtask(&["lint-provenance"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("look like game content"));
}

#[test]
fn an_untracked_container_with_no_known_extension_warns_and_never_fails() {
    let repo = Scratch::repository("prov-stray");
    repo.write("src/lib.rs", b"pub fn f() {}\n");
    repo.git(&["add", "-A"]);
    // The four bytes a paged resource opens with, under an extension nothing lists.
    repo.write("corpus/thing.zzz", b"RSC7 and the rest");
    let out = repo.xtask(&["lint-provenance"]);
    assert!(out.status.success(), "the advisory half failed the run");
    assert!(text(&out).contains("untracked file(s) look like game content"));
    assert!(!text(&out).contains("provenance: clean"), "{}", text(&out));
}

#[test]
fn history_names_the_commit_and_the_line() {
    let repo = Scratch::repository("prov-history");
    repo.write("a.md", format!("fine\n{} here\n", leak()).as_bytes());
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "first"]);
    repo.write("a.md", b"scrubbed\n");
    repo.git(&["commit", "-q", "-am", "scrub"]);

    // The working tree is clean now; the history is not.
    assert!(repo.xtask(&["lint-provenance"]).status.success());
    let out = repo.xtask(&[
        "lint-provenance",
        "--history",
        repo.0.to_str().expect("utf-8"),
    ]);
    assert!(!out.status.success(), "history missed a reachable leak");
    assert!(text(&out).contains("a.md:2"), "{}", text(&out));
}

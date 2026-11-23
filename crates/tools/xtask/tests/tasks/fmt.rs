//! The chunked formatter reaches the files `cargo fmt --all` reaches.
//!
//! Unformatted on purpose, so rustfmt names every file it reaches and a run
//! that reached fewer than it should cannot look like one that did.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use crate::common::{text, Scratch};

const UNFORMATTED: &[u8] = b"pub fn f( ) ->u32{let  x=1;x}\n";

fn workspace(label: &str, tests: usize) -> Scratch {
    let dir = Scratch::new(label);
    dir.write(
        "Cargo.toml",
        b"[package]\nname = \"selftest\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    dir.write("src/lib.rs", UNFORMATTED);
    for i in 0..tests {
        dir.write(
            &format!("tests/a_synthetic_integration_target_{i:04}.rs"),
            UNFORMATTED,
        );
    }
    dir
}

/// rustfmt heads each file it would change with `Diff in <path>:<line>:`.
fn files_named(output: &str) -> BTreeSet<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("Diff in "))
        .filter_map(|rest| {
            rest.rsplit_once(':')
                .and_then(|(head, _)| head.rsplit_once(':'))
        })
        .map(|(path, _)| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)))
        .collect()
}

#[test]
fn the_chunked_run_reaches_the_files_cargo_fmt_all_reaches() {
    let dir = workspace("fmt-same", 40);
    let manifest = dir.0.join("Cargo.toml");

    let cargo = Command::new("cargo")
        .current_dir(&dir.0)
        .args(["fmt", "--all", "--check"])
        .output()
        .expect("cargo fmt runs");
    let from_cargo = files_named(&text(&cargo));
    assert_eq!(
        from_cargo.len(),
        41,
        "the oracle named {} files of 41, so the comparison below is not over what it claims",
        from_cargo.len()
    );

    // A tiny budget, so this is compared against a run that really chunked.
    let chunked = dir.xtask(&[
        "fmt",
        "--check",
        "--manifest-path",
        manifest.to_str().expect("utf-8"),
        "--budget",
        "2000",
    ]);
    assert!(
        !chunked.status.success(),
        "unformatted files passed the check"
    );
    assert_eq!(files_named(&text(&chunked)), from_cargo);
}

#[test]
fn a_formatted_tree_passes_and_fmt_fix_makes_one() {
    let dir = workspace("fmt-fix", 3);
    let manifest = dir.0.join("Cargo.toml");
    let manifest = manifest.to_str().expect("utf-8");

    assert!(!dir
        .xtask(&["fmt", "--check", "--manifest-path", manifest])
        .status
        .success());
    assert!(dir
        .xtask(&["fmt", "--manifest-path", manifest])
        .status
        .success());
    assert!(dir
        .xtask(&["fmt", "--check", "--manifest-path", manifest])
        .status
        .success());
}

#[test]
fn no_targets_is_an_error_not_a_green_run() {
    let dir = Scratch::new("fmt-none");
    dir.write("Cargo.toml", b"[workspace]\nmembers = []\n");
    let out = dir.xtask(&[
        "fmt",
        "--check",
        "--manifest-path",
        dir.0.join("Cargo.toml").to_str().expect("utf-8"),
    ]);
    assert!(!out.status.success(), "{}", text(&out));
}

/// The size that breaks `cargo fmt --all` does not break this. The error is
/// Windows': its command-line cap is 32,766 characters.
#[cfg(windows)]
#[test]
fn at_the_size_that_breaks_cargo_fmt_all_the_chunked_run_survives() {
    let dir = workspace("fmt-big", 220);
    let deep = Scratch::new(&"p".repeat(110));
    drop(deep);
    let cargo = Command::new("cargo")
        .current_dir(&dir.0)
        .args(["fmt", "--all", "--check"])
        .output()
        .expect("cargo fmt runs");
    let manifest = dir.0.join("Cargo.toml");
    let chunked = dir.xtask(&[
        "fmt",
        "--check",
        "--manifest-path",
        manifest.to_str().expect("utf-8"),
    ]);
    assert_eq!(files_named(&text(&chunked)).len(), 221);
    drop(cargo);
}

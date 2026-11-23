//! Record R66e's behaviour baselines into `openrage-corpus/baselines`
//! (OD-BASELINES), the only writer there, and scaffold a format.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};

use crate::git;

/// The suites recorded when none is named.
const SUITES: [&str; 6] = [
    "every_corpus_drawable_exports_as_recorded",
    "every_corpus_clip_poses_as_recorded",
    "every_corpus_effect_simulates_as_recorded",
    "every_documented_command_reports_as_recorded",
    "every_corpus_legacy_texture_decodes_as_recorded",
    "every_corpus_drawable_uploads_as_recorded",
];

/// The one suite that lives in the app and not in a crate's `tests/`.
const APP_SUITE: &str = "every_corpus_drawable_uploads_as_recorded";

/// ⚠ It refuses while the code under test differs from the merge base with
/// `origin/main` (sources, build scripts, toolchain, dependency versions), and
/// fetches nothing. `OPENRAGE_REQUIRE_FIXTURES` makes a missing corpus fail,
/// so nothing is recorded over nothing. Naming suites records only their kinds,
/// so a new kind does not retake the records the others hold.
pub fn record(named: &[String]) -> Result<ExitCode> {
    let root = git::here()?;
    let base = git::output(&root, &["merge-base", "HEAD", "origin/main"])?;
    let base = String::from_utf8_lossy(&base.stdout).trim().to_string();
    if base.is_empty() {
        bail!("no merge base with origin/main");
    }

    let unchanged = git::output(
        &root,
        &[
            "diff",
            "--quiet",
            &base,
            "--",
            "src",
            "rust-toolchain.toml",
            ":(glob)crates/**/src/**",
            ":(glob)crates/**/build.rs",
        ],
    )?
    .status
    .success();
    let lock = git::run(&root, &["diff", &base, "--", "Cargo.lock"])?;
    let lock_moved = lock.lines().any(|line| {
        [
            "-version",
            "+version",
            "-source",
            "+source",
            "-checksum",
            "+checksum",
        ]
        .iter()
        .any(|prefix| line.starts_with(&format!("{prefix} ")))
    });
    if !unchanged || lock_moved {
        bail!("the code under test differs from {base} on main; record from main's code");
    }

    let store = find_corpus(&root).context("no openrage-corpus beside or above this worktree")?;

    let names: Vec<String> = if named.is_empty() {
        SUITES.iter().map(|s| s.to_string()).collect()
    } else {
        named.to_vec()
    };
    let environment = |command: &mut Command| {
        command
            .current_dir(&root)
            .env("OPENRAGE_BASELINES", store.join("baselines"))
            .env("OPENRAGE_BASELINES_RECORD", &base)
            .env("OPENRAGE_REQUIRE_FIXTURES", "1");
    };

    let crates: Vec<&String> = names.iter().filter(|n| n.as_str() != APP_SUITE).collect();
    if !crates.is_empty() {
        let mut command = Command::new("cargo");
        command.args([
            "test",
            "-p",
            "or-interchange",
            "-p",
            "or-anim",
            "-p",
            "or-simulation",
            "-p",
            "or-cli",
            "-p",
            "or-io-verify",
        ]);
        for suite in &crates {
            command.args(["--test", suite]);
        }
        command.args(["--", "as_recorded", "--nocapture"]);
        environment(&mut command);
        let status = command.status().context("could not run cargo")?;
        if !status.success() {
            return Ok(ExitCode::FAILURE);
        }
    }
    if names.iter().any(|n| n == APP_SUITE) {
        let mut command = Command::new("cargo");
        command.args([
            "test",
            "-p",
            "openrage",
            "--bin",
            "openrage",
            "--",
            APP_SUITE,
            "--nocapture",
        ]);
        environment(&mut command);
        let status = command.status().context("could not run cargo")?;
        return Ok(if status.success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    Ok(ExitCode::SUCCESS)
}

/// The nearest `openrage-corpus` in `from` or an ancestor of it.
fn find_corpus(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .map(|dir| dir.join("openrage-corpus"))
        .find(|candidate| candidate.exists())
}

/// ⚠ It refuses before writing anything if a file it would create exists or a
/// line it would add is already there.
pub fn scaffold(args: &[String]) -> Result<ExitCode> {
    let [module, ext, root_type, version] = args else {
        bail!("scaffold-format takes: module ext root version");
    };
    let root = git::here()?;
    let out = Command::new("cargo")
        .args(["run", "-q", "-p", "or-io-verify", "--bin", "scaffold", "--"])
        .arg(&root)
        .args([module, ext, root_type, version])
        .current_dir(&root)
        .output()
        .context("could not run cargo")?;
    if !out.status.success() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        return Ok(ExitCode::FAILURE);
    }
    let written = git::lines(&String::from_utf8_lossy(&out.stdout));
    for line in &written {
        println!("{line}");
    }
    let status = Command::new("rustfmt")
        .args(["--edition", "2021"])
        .args(written.iter().map(|path| root.join(path)))
        .status()
        .context("rustfmt was not found. Install it with: rustup component add rustfmt")?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

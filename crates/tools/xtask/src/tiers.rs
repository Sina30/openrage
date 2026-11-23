//! The test tiers: what a recipe sets up around `cargo test` or nextest, and
//! the skip ledger a tier ends with, because a skip reads as a pass.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};

/// Refuse a run that needs cargo-nextest when it is not installed, and say so.
///
/// Without it cargo fails with `no such command: nextest`, which reads as "my
/// change broke the build" rather than "a tool is missing".
pub fn require_nextest() -> Result<ExitCode> {
    let present = Command::new("cargo-nextest")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if present {
        return Ok(ExitCode::SUCCESS);
    }
    println!();
    println!("cargo-nextest is not installed, and this recipe needs it.");
    println!();
    println!("    cargo install cargo-nextest --locked");
    println!();
    println!("It is a local dependency only. CI runs cargo test directly and never");
    println!("installs it, so a green CI says nothing about whether you have it.");
    println!();
    println!("To run the workspace tests without it:");
    println!();
    println!("    just test-cargo");
    println!();
    println!("There is deliberately no such fallback for test-lite. It ends by");
    println!("reporting what it did not run, and that listing comes from nextest");
    println!("list -- so a cargo-only tier would check less and report the same green.");
    println!();
    Ok(ExitCode::FAILURE)
}

pub(crate) fn target_directory() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("could not run `cargo metadata`")?;
    let metadata: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .context("cargo metadata named no target directory")
}

/// Where a tier keeps what it writes: inside the build directory, which goes
/// with the lane, and not in the owner's temporary folder.
fn scratch() -> Result<PathBuf> {
    let dir = target_directory()?.join("xtask");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Drain the skip ledger, report it with `--keep`, or with `--lite` account for
/// what `test-lite` left out.
///
/// ⚠ Built with `--workspace`, not run with `cargo run -p or-test-support`:
/// `-p` resolves features for that package alone, so after a workspace run it
/// rebuilt `or-io` and its dependencies (114 s against 14 s).
pub fn skip_ledger(args: &[String]) -> Result<ExitCode> {
    if args == ["--lite"] {
        return lite_ledger();
    }
    let built = Command::new("cargo")
        .args(["build", "-q", "--workspace", "--bin", "skip-ledger"])
        .status()
        .context("could not run cargo")?;
    if !built.success() {
        return Ok(exit_code(built.code()));
    }
    let exe = target_directory()?
        .join("debug")
        .join(format!("skip-ledger{}", std::env::consts::EXE_SUFFIX));
    let ran = Command::new(&exe)
        .args(args)
        .status()
        .with_context(|| format!("could not run {}", exe.display()))?;
    Ok(exit_code(ran.code()))
}

pub(crate) fn exit_code(code: Option<i32>) -> ExitCode {
    match code {
        Some(0) => ExitCode::SUCCESS,
        Some(n) => ExitCode::from(u8::try_from(n).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

/// Run `command`, then drain the ledger whatever it returned, and exit with
/// the command's own code: the ledger must not turn a red test green.
fn then_ledger(mut command: Command) -> Result<ExitCode> {
    let status = command.status().context("could not run cargo")?;
    let ledger = skip_ledger(&[])?;
    if !status.success() {
        return Ok(exit_code(status.code()));
    }
    Ok(ledger)
}

/// The workspace suites, with a missing corpus failing instead of skipping.
pub fn strict_tests() -> Result<ExitCode> {
    let mut command = Command::new("cargo");
    command
        .args(["test", "--workspace"])
        .env("OPENRAGE_REQUIRE_FIXTURES", "1");
    then_ledger(command)
}

/// The workspace suites as a clean checkout sees them: no schema snapshot, no
/// corpus.
///
/// ⚠ Both variables are needed. Discovery walks ancestors, so unsetting a
/// variable changes nothing, and PowerShell deletes one set to an empty
/// string: the schema opt-out is the word `none`.
/// `the_clean_checkout_condition.rs` holds this to the `layouts` job's step.
pub fn no_pack_tests() -> Result<ExitCode> {
    println!(
        "test-no-pack: OPENRAGE_SCHEMA=none and an empty OPENRAGE_FIXTURES - no snapshot, no corpus, which is what a clean checkout sees."
    );
    let empty = scratch()?.join("no-pack-no-corpus");
    std::fs::create_dir_all(&empty)?;
    let mut command = Command::new("cargo");
    command
        .args(["nextest", "run", "--workspace", "--profile", "all"])
        .env("OPENRAGE_SCHEMA", "none")
        .env("OPENRAGE_FIXTURES", &empty);
    then_ledger(command)
}

/// The tests `test-lite` left out, listed by nextest and handed to the ledger,
/// which would otherwise report nothing skipped.
fn lite_ledger() -> Result<ExitCode> {
    let listed = Command::new("cargo")
        .args([
            "nextest",
            "list",
            "--workspace",
            "--ignore-default-filter",
            "-E",
            "not (kind(lib) + kind(bin))",
        ])
        .output()
        .context("could not run `cargo nextest list`")?;
    if !listed.status.success() {
        eprint!("{}", String::from_utf8_lossy(&listed.stderr));
        return Ok(exit_code(listed.status.code()));
    }
    let file = scratch()?.join("tier-not-run.txt");
    std::fs::write(&file, &listed.stdout)?;
    skip_ledger(&["--not-run".to_string(), file.to_string_lossy().into_owned()])
}

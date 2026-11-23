//! Thin wrappers over the `git` command line, which every task shares.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{bail, Context, Result};

/// `git args` in `dir`, whatever it exits with.
pub fn output(dir: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("could not run `git {}`; is git on PATH?", args.join(" ")))
}

/// `git args` in `dir`, standard output as text, an error when git fails.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = output(dir, args)?;
    if !out.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The lines of `text`, trimmed of a trailing carriage return, blanks dropped.
pub fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The root of the working tree that holds `from`.
pub fn toplevel(from: &Path) -> Result<PathBuf> {
    let text =
        run(from, &["rev-parse", "--show-toplevel"]).context("not inside a git repository")?;
    Ok(PathBuf::from(text.trim()))
}

/// The root of the working tree the current directory is in.
pub fn here() -> Result<PathBuf> {
    toplevel(&std::env::current_dir()?)
}

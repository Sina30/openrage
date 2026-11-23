//! The pre-commit hook: formatting, encoding over the staged files, and
//! provenance, in under ten seconds.
//!
//! The installed hook is a short shell shim that runs a *built* binary kept
//! beside it, so a commit never waits on the workspace compiling. The binary is
//! this one, copied by `install-hooks`, and the shim refuses, naming the
//! recipe, when the binary is missing or its source has changed since: a stale
//! hook is how a check goes on passing after the rule moved.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{bail, Context, Result};

use crate::{encoding, fmt, git, provenance};

/// Where this crate's source lives, which the stamp covers.
const SOURCE: &str = "crates/tools/xtask";

const BINARY: &str = "openrage-xtask";
const STAMP: &str = "openrage-xtask.stamp";

pub fn install() -> Result<ExitCode> {
    let root = git::here()?;

    // ⚠ Not `<root>/.git/hooks`. In a `git worktree`, which is every agent
    // lane, `.git` is a file pointing at the real directory. Ask git, which
    // resolves it to the common directory: one hook for the repository,
    // shared by every worktree of it.
    let hooks = PathBuf::from(
        git::run(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
        )?
        .trim(),
    );
    let shim = root.join(".githooks/pre-commit");
    if !shim.is_file() {
        bail!("hook template not found: {}", shim.display());
    }
    std::fs::create_dir_all(&hooks)?;

    copy_executable(&shim, &hooks.join("pre-commit"))?;
    let binary = hooks.join(format!("{BINARY}{}", std::env::consts::EXE_SUFFIX));
    copy_executable(&std::env::current_exe()?, &binary)?;
    std::fs::write(hooks.join(STAMP), stamp(&root)?)?;

    println!("Installed the pre-commit hook to {}", hooks.display());
    println!("Commits will now run the chunked formatter, the encoding lint over the staged");
    println!("files, and the provenance lint, under ten seconds, nothing compiled.");
    println!("'just ready' adds clippy and the tests before a merge. Re-run this recipe");
    println!("whenever {SOURCE} changes: the hook refuses to run a stale copy.");
    Ok(ExitCode::SUCCESS)
}

/// A copy that replaces its target and keeps (or sets) the execute bit.
fn copy_executable(from: &Path, to: &Path) -> Result<()> {
    // Written beside and renamed, so a hook running right now is not truncated.
    let partial = to.with_extension("partial");
    std::fs::copy(from, &partial)
        .with_context(|| format!("copying {} to {}", from.display(), partial.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&partial, to)?;
    Ok(())
}

/// The hash of this crate's tracked files as they are on disk, laid out the way
/// `git ls-files -s` lists the index, so the shim can compute the same from the
/// index with plain git. They agree whenever nothing is left unstaged, which
/// the hook insists on before it checks anything.
fn stamp(root: &Path) -> Result<String> {
    let listing = git::run(root, &["ls-files", "-s", "--", SOURCE])?;
    let mut rebuilt = String::new();
    for line in listing.lines() {
        let (meta, path) = line
            .split_once('\t')
            .context("an unreadable `git ls-files -s` line")?;
        let mut fields = meta.split(' ');
        let (mode, _, stage) = (fields.next(), fields.next(), fields.next());
        let blob = git::run(root, &["hash-object", "--", path])?;
        rebuilt.push_str(&format!(
            "{} {} {}\t{path}\n",
            mode.unwrap_or("100644"),
            blob.trim(),
            stage.unwrap_or("0")
        ));
    }
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("git hash-object has no stdin")?
        .write_all(rebuilt.as_bytes())?;
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn pre_commit() -> Result<ExitCode> {
    let root = git::here()?;

    // The encoding lint reads the index. `cargo fmt` reads the working tree,
    // and those are the same content only when nothing is left unstaged: when
    // they differ, a green fmt run describes the tree rather than the commit.
    // Deletions count: a file dropped from disk but still in the index is one
    // the formatter will not read and the commit will still carry.
    let unstaged = git::lines(&git::run(&root, &["diff", "--name-only"])?);
    if !unstaged.is_empty() {
        eprintln!("These files differ between the working tree and the commit you are");
        eprintln!("about to make:");
        eprintln!();
        for file in &unstaged {
            eprintln!("  {file}");
        }
        eprintln!();
        eprintln!("cargo fmt reads the working tree, so running it now would report on");
        eprintln!("something other than this commit - and report it green.");
        eprintln!();
        eprintln!("Stage them, or commit them separately, then try again. To commit");
        eprintln!("without the checks having seen it:");
        eprintln!();
        eprintln!("  git commit --no-verify");
        return Ok(ExitCode::FAILURE);
    }

    // Untracked files are the milder half of the same gap: the formatter walks
    // the disk, so it reads files no commit will carry. That usually fails a
    // good commit rather than passing a bad one, so a note and not a refusal.
    let untracked = git::lines(&git::run(
        &root,
        &["ls-files", "--others", "--exclude-standard", "--", "*.rs"],
    )?);
    if !untracked.is_empty() {
        eprintln!("Note: cargo fmt will also read these untracked files, which this");
        eprintln!("commit does not contain:");
        for file in untracked.iter().take(5) {
            eprintln!("  {file}");
        }
        eprintln!();
    }

    println!("pre-commit: formatting, over every workspace target");
    if !is_success(fmt::run(&["--check".to_string()])?) {
        return Ok(ExitCode::FAILURE);
    }
    println!("pre-commit: encoding, over the staged files");
    if !is_success(encoding::run(&["--staged".to_string()])?) {
        return Ok(ExitCode::FAILURE);
    }
    println!("pre-commit: provenance");
    if !is_success(provenance::run(&[])?) {
        return Ok(ExitCode::FAILURE);
    }
    println!("pre-commit: ok");
    Ok(ExitCode::SUCCESS)
}

/// `ExitCode` has no accessor, and what the three checks return is only ever
/// success or not.
fn is_success(code: ExitCode) -> bool {
    code == ExitCode::SUCCESS
}

//! Format the workspace without putting every target on one command line.
//!
//! `cargo fmt --all` hands every target root to one `rustfmt` process, and
//! Windows stops a command line at 32,766 characters (error 206). This runs the
//! same rustfmt over the same roots in as many processes as it takes to stay
//! under a budget, on every OS, so there is one code path and the margin
//! never closes as targets are added.
//!
//! ⚠ The set of roots has to be the set `cargo fmt --all` uses, or this lands
//! unformatted code under a green check. It is derived the way cargo-fmt
//! derives it: `cargo metadata --no-deps` over the workspace, every target of
//! every member, plus the same recursion into path dependencies that are not
//! members, deduplicated by path. `--list-targets` prints it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};

use crate::git;

/// Characters a command line may spend on file arguments, under Windows' cap.
const DEFAULT_BUDGET: usize = 30_000;

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut check = false;
    let mut list_targets = false;
    let mut budget = DEFAULT_BUDGET;
    let mut lanes = 0usize;
    let mut manifest: Option<PathBuf> = None;
    let mut extra: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--list-targets" => list_targets = true,
            "--budget" => budget = number(it.next(), "--budget")?,
            "--concurrency" => lanes = number(it.next(), "--concurrency")?,
            "--manifest-path" => {
                manifest = Some(PathBuf::from(
                    it.next().context("--manifest-path needs a value")?,
                ))
            }
            "--" => {
                extra.extend(it.by_ref().cloned());
            }
            other => bail!("fmt: unknown argument `{other}`"),
        }
    }

    let manifest = match manifest {
        Some(path) => std::path::absolute(path)?,
        None => git::here()?.join("Cargo.toml"),
    };

    let found = targets(&manifest)?;
    if list_targets {
        for path in found.keys() {
            println!("{path}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    // ⚠ Only the read-only half runs concurrently. Two roots can reach one
    // module (a `mod` two integration tests share), and `cargo fmt --all`
    // formats such a file twice, serially, inside one process. Twice at once
    // is a truncate racing a write.
    if lanes == 0 {
        lanes = if check { cpus().min(8) } else { 1 };
    }
    format(&found, budget, lanes, check, &extra)
}

fn number(value: Option<&String>, flag: &str) -> Result<usize> {
    value
        .with_context(|| format!("{flag} needs a value"))?
        .parse()
        .with_context(|| format!("{flag} takes a number"))
}

fn cpus() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Every target source file of the workspace at `manifest`, with its edition,
/// keyed by path in byte order (a `BTreeMap<String, _>` is what cargo-fmt's
/// `BTreeSet<PathBuf>` order is, to the extent a path is text).
fn targets(manifest: &Path) -> Result<BTreeMap<String, String>> {
    let mut by_path: BTreeMap<String, String> = BTreeMap::new();
    let mut seen_manifest: BTreeSet<PathBuf> = BTreeSet::new();
    let mut seen_dependency: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::from([manifest.to_path_buf()]);

    while let Some(current) = queue.pop_front() {
        if !seen_manifest.insert(current.clone()) {
            continue;
        }
        let out = Command::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&current)
            .output()
            .context("could not run `cargo metadata`")?;
        if !out.status.success() {
            bail!(
                "cargo metadata failed for {}:\n{}",
                current.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&out.stdout).context("cargo metadata did not print JSON")?;
        let packages = metadata["packages"].as_array().cloned().unwrap_or_default();

        let members: BTreeSet<PathBuf> = packages
            .iter()
            .filter_map(|p| p["manifest_path"].as_str())
            .map(PathBuf::from)
            .collect();

        for package in &packages {
            for target in package["targets"].as_array().into_iter().flatten() {
                let (Some(path), Some(edition)) =
                    (target["src_path"].as_str(), target["edition"].as_str())
                else {
                    continue;
                };
                by_path
                    .entry(path.to_string())
                    .or_insert_with(|| edition.to_string());
            }

            // cargo-fmt follows a path dependency that is not itself a member,
            // and skips a dependency name it has already followed.
            for dependency in package["dependencies"].as_array().into_iter().flatten() {
                let (Some(path), Some(name)) =
                    (dependency["path"].as_str(), dependency["name"].as_str())
                else {
                    continue;
                };
                if seen_dependency.contains(name) {
                    continue;
                }
                let manifest = Path::new(path).join("Cargo.toml");
                if manifest.is_file() && !members.contains(&manifest) {
                    seen_dependency.insert(name.to_string());
                    queue.push_back(manifest);
                }
            }
        }
    }
    Ok(by_path)
}

/// How many characters `value` costs on a command line, quoted as Windows
/// would: the budget is only meaningful if it counts what the OS will see.
fn argument_len(value: &str) -> usize {
    if value.contains(char::is_whitespace) || value.contains('"') {
        value.len() + 2 + value.matches('"').count()
    } else {
        value.len()
    }
}

/// `files` in order, grouped so no group's command line passes `limit`.
///
/// A chunk always takes at least one file, so a path longer than the budget is
/// passed rather than dropped: rustfmt refusing it is a better failure than
/// quietly formatting less.
fn split_into_chunks(
    files: &[String],
    head: usize,
    limit: usize,
    max_per_chunk: usize,
) -> Vec<Vec<String>> {
    let mut chunks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut length = head;

    for file in files {
        let cost = 1 + argument_len(file);
        let full = !current.is_empty()
            && (length + cost > limit || (max_per_chunk > 0 && current.len() >= max_per_chunk));
        if full {
            chunks.push(std::mem::take(&mut current));
            length = head;
        }
        current.push(file.clone());
        length += cost;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn format(
    targets: &BTreeMap<String, String>,
    budget: usize,
    lanes: usize,
    check: bool,
    extra: &[String],
) -> Result<ExitCode> {
    // ⚠ Formatting nothing and reporting success is the one outcome that looks
    // exactly like a clean tree.
    if targets.is_empty() {
        bail!("no targets found, so nothing would have been formatted");
    }

    let rustfmt = std::env::var("RUSTFMT").unwrap_or_else(|_| "rustfmt".to_string());
    let mut flags: Vec<String> = Vec::new();
    if check {
        flags.push("--check".into());
    }
    // Output is captured, so rustfmt sees a pipe and drops its colours unless
    // told otherwise. Ask for them back only when this run's own is a terminal.
    if std::io::stdout().is_terminal() {
        flags.extend(["--color".into(), "always".into()]);
    }
    flags.extend(extra.iter().filter(|a| !a.is_empty()).cloned());

    // `"<exe>" --edition NNNN` plus the flags: what every chunk pays before a
    // single path is added. Every edition is four digits.
    let head = rustfmt.len()
        + 2
        + " --edition".len()
        + " 2015".len()
        + flags.iter().map(|f| 1 + argument_len(f)).sum::<usize>();

    let per_chunk = if lanes > 1 {
        targets.len().div_ceil(lanes).max(1)
    } else {
        0
    };

    let editions: BTreeSet<&String> = targets.values().collect();
    let mut work: Vec<Vec<String>> = Vec::new();
    for edition in editions {
        let files: Vec<String> = targets
            .iter()
            .filter(|(_, e)| *e == edition)
            .map(|(path, _)| path.clone())
            .collect();
        for chunk in split_into_chunks(&files, head, budget, per_chunk) {
            let mut arguments = vec!["--edition".to_string(), edition.clone()];
            arguments.extend(flags.iter().cloned());
            arguments.extend(chunk);
            work.push(arguments);
        }
    }

    let mut failure = 0i32;
    for batch in work.chunks(lanes.max(1)) {
        let running: Vec<_> = batch
            .iter()
            .map(|arguments| {
                let rustfmt = rustfmt.clone();
                let arguments = arguments.clone();
                std::thread::spawn(move || Command::new(&rustfmt).args(&arguments).output())
            })
            .collect();
        for handle in running {
            let out = handle
                .join()
                .map_err(|_| anyhow::anyhow!("a rustfmt worker panicked"))?
                .with_context(|| {
                    format!(
                        "{rustfmt} was not found. Install it with: rustup component add rustfmt"
                    )
                })?;
            print!("{}", String::from_utf8_lossy(&out.stdout));
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                failure = out.status.code().unwrap_or(1);
            }
        }
    }
    Ok(if failure == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(u8::try_from(failure).unwrap_or(1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize, len: usize) -> Vec<String> {
        (0..n).map(|i| format!("{i:0>len$}")).collect()
    }

    #[test]
    fn a_path_with_a_space_costs_its_quotes() {
        assert_eq!(argument_len("a/b.rs"), 6);
        assert_eq!(argument_len("a b.rs"), 8);
        assert_eq!(argument_len("a\"b.rs"), 6 + 2 + 1);
    }

    #[test]
    fn no_chunk_passes_the_budget_and_every_file_lands_in_one() {
        let files = paths(500, 60);
        let head = 40;
        let limit = 1_000;
        let chunks = split_into_chunks(&files, head, limit, 0);

        assert!(chunks.len() > 1, "a budget this small has to split");
        for chunk in &chunks {
            let spent: usize = head + chunk.iter().map(|f| 1 + argument_len(f)).sum::<usize>();
            assert!(spent <= limit, "a chunk spends {spent} of {limit}");
        }
        let flat: Vec<&String> = chunks.iter().flatten().collect();
        assert_eq!(flat, files.iter().collect::<Vec<_>>(), "order and content");
    }

    #[test]
    fn a_path_longer_than_the_budget_is_passed_not_dropped() {
        let files = vec!["x".repeat(2_000)];
        let chunks = split_into_chunks(&files, 10, 100, 0);
        assert_eq!(chunks, vec![files]);
    }

    #[test]
    fn a_per_chunk_cap_spreads_work_across_lanes() {
        let files = paths(10, 4);
        let chunks = split_into_chunks(&files, 0, usize::MAX, 3);
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            [3, 3, 3, 1]
        );
    }
}

//! Report files whose bytes decode as UTF-8 but read as mojibake, or open with a BOM.
//!
//! The signature is a U+00E2 (a-circumflex) immediately followed by another
//! Latin-1 supplement character: what a three-byte UTF-8 sequence looks like
//! after a round trip through cp1252. Box-drawing characters and em-dashes are
//! the usual casualties, since both are three-byte sequences.
//!
//! `--staged` checks what is about to be committed and walks nothing. That is
//! what the pre-commit hook uses, and the scope is the point as much as the
//! speed: a hook asks whether *this commit* introduces damage, which is a
//! question about the index rather than about the disk.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::git;

/// Extensions worth reading. One list, so the walk and the staged set agree.
const WANTED: [&str; 5] = ["md", "rs", "toml", "ps1", "yml"];

/// Directories nothing is read under, at any depth. `target/` holds hundreds
/// of thousands of files, so it is pruned rather than enumerated and filtered.
const PRUNED: [&str; 5] = ["target", "external", ".git", ".claude", "node_modules"];

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut path: Option<PathBuf> = None;
    let mut staged = false;
    let mut fix_bom = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--staged" => staged = true,
            "--fix-bom" => fix_bom = true,
            "--path" => path = Some(PathBuf::from(it.next().context("--path needs a value")?)),
            other => bail!("lint-encoding: unknown argument `{other}`"),
        }
    }

    let root = match path {
        Some(path) => std::fs::canonicalize(&path)
            .with_context(|| format!("{} does not exist", path.display()))?,
        None => git::here()?,
    };

    let files = if staged {
        staged_files(&root)?
    } else {
        let mut files = Vec::new();
        walk(&root, &mut files)?;
        files
    };
    scan(&root, &files, fix_bom)
}

/// What the index adds or changes, still on disk and worth reading.
fn staged_files(root: &Path) -> Result<Vec<PathBuf>> {
    let listed = git::run(
        root,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
    )?;
    Ok(git::lines(&listed)
        .into_iter()
        .map(|name| root.join(name))
        // A path git lists can still be gone from disk: a rename records both
        // ends, and the hook runs before the commit exists.
        .filter(|path| path.is_file() && wanted(path))
        .collect())
}

fn wanted(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "justfile" {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| WANTED.contains(&e.to_ascii_lowercase().as_str()))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // `file_type` does not follow a symlink, so a link to a directory is
        // not walked into and cannot loop.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !PRUNED.contains(&name.as_ref()) {
                walk(&path, out)?;
            }
        } else if kind.is_file() && wanted(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// The characters mojibake leaves: the Latin-1 lead bytes, then a continuation.
fn mojibake() -> Regex {
    Regex::new(
        "[\u{00c2}\u{00c3}\u{00e2}][\u{0080}-\u{00bf}\u{2013}\u{2014}\u{2018}\u{2019}\u{201c}\u{201d}\u{20ac}\u{0153}\u{017e}\u{0161}]",
    )
    .expect("the mojibake pattern is a valid expression")
}

struct Finding {
    path: String,
    mojibake: usize,
    bom: bool,
}

fn scan(root: &Path, files: &[PathBuf], fix_bom: bool) -> Result<ExitCode> {
    let pattern = mojibake();
    let mut bad: Vec<Finding> = Vec::new();

    for file in files {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        if bytes.is_empty() {
            continue;
        }
        let bom = bytes.starts_with(&BOM);
        let hits = pattern.find_iter(&String::from_utf8_lossy(&bytes)).count();
        if hits == 0 && !bom {
            continue;
        }
        if fix_bom && bom {
            std::fs::write(file, &bytes[BOM.len()..])?;
            println!("  {}  -- BOM stripped", relative(root, file));
            if hits == 0 {
                continue;
            }
        }
        bad.push(Finding {
            path: relative(root, file),
            mojibake: hits,
            bom: bom && !fix_bom,
        });
    }
    bad.retain(|f| f.mojibake > 0 || f.bom);

    if bad.is_empty() {
        println!("clean: {} files, no mojibake and no BOMs", files.len());
        return Ok(ExitCode::SUCCESS);
    }

    bad.sort_by(|a, b| {
        b.mojibake
            .cmp(&a.mojibake)
            .then_with(|| a.path.cmp(&b.path))
    });
    println!("{:>8}  {:<5}  Path", "Mojibake", "Bom");
    for finding in &bad {
        println!(
            "{:>8}  {:<5}  {}",
            finding.mojibake, finding.bom, finding.path
        );
    }
    println!("{} file(s) affected out of {}", bad.len(), files.len());
    // A BOM is three bytes and the fix is mechanical, so say it.
    if bad.iter().any(|f| f.bom) {
        println!("A BOM is removable with: cargo xtask lint-encoding --fix-bom");
    }
    Ok(ExitCode::FAILURE)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_em_dash_that_went_through_cp1252_is_mojibake_and_a_real_one_is_not() {
        let pattern = mojibake();
        assert!(pattern.is_match("\u{00e2}\u{0080}\u{0094}"));
        assert!(!pattern.is_match("a real em dash \u{2014} and an a-circumflex \u{00e2}"));
    }

    #[test]
    fn the_extensions_and_the_justfile_are_what_is_read() {
        assert!(wanted(Path::new("a/b.rs")));
        assert!(wanted(Path::new("a/B.MD")));
        assert!(wanted(Path::new("justfile")));
        assert!(!wanted(Path::new("a/b.png")));
    }
}

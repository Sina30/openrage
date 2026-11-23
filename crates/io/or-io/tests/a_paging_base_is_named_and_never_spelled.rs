//! Nothing writes `0x50000000` out longhand, anywhere in the workspace.
//!
//! A second container's address model is a `PagingModel` parameter, and the
//! model reaches every site that names the constant and no site that spells it.
//! `or-cli`'s `inspect` once spelled `0x50000000` twenty-one times, one
//! `saturating_sub` per pointer field it prints.
//!
//! It does not insist on `is_virtual_address`: several sites compare `>=` and
//! accept a physical address as a virtual one, and tightening them changes
//! what they print.

use std::path::{Path, PathBuf};

/// The two bases as somebody would write them by hand: `0x50000000` and
/// `0x50000000u64` both occur, and a walk that only knew one spelling would
/// report clean over the other.
const SPELLED: &[&str] = &["0x50000000", "0x60000000", "0x50000000u64", "0x60000000u64"];

/// ⚠ The only files a literal belongs in: `constants.rs` is the definition and
/// `image.rs` is `PagingModel::RAGE`, the model written down. Anything else
/// wanting to be here is the seam leaking. The vendored Kraken decoder is not ours: its tables
/// hold these numbers as data.
const WHERE_A_LITERAL_IS_THE_ANSWER: &[&str] = &[
    "or-io/src/constants.rs",
    "or-io/src/desc/image.rs",
    "or-kraken/oozextract/",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("or-container sits three directories below the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn a_paging_base_is_named_and_never_spelled() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    rust_files(&root.join("src"), &mut files);
    assert!(
        files.len() > 100,
        "the walk found {} file(s), which is not this workspace",
        files.len()
    );

    let mut spelled: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for path in &files {
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        // Tests may spell a base: a fixture's bytes are the thing under test,
        // and naming the constant there would make the test agree with the
        // code by construction.
        if shown.contains("/tests/")
            || WHERE_A_LITERAL_IS_THE_ANSWER
                .iter()
                .any(|w| shown.contains(w))
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        scanned += 1;

        let mut in_tests = false;
        let mut depth: i32 = 0;
        let mut armed = false;
        for (number, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if !in_tests {
                if trimmed.starts_with("#[cfg(test)]") {
                    armed = true;
                } else if armed && trimmed.starts_with("mod ") {
                    in_tests = true;
                    armed = false;
                    depth = braces(line);
                    continue;
                } else if armed && !trimmed.is_empty() && !trimmed.starts_with('#') {
                    armed = false;
                }
            }
            if in_tests {
                depth += braces(line);
                if depth <= 0 {
                    in_tests = false;
                }
                continue;
            }

            let code = match trimmed.find("//") {
                Some(at) => &trimmed[..at],
                None => trimmed,
            };
            if SPELLED.iter().any(|s| code.contains(s)) {
                spelled.push(format!("{shown}:{}: {}", number + 1, trimmed));
            }
        }
    }

    eprintln!("\n=== {scanned} production file(s) scanned for a spelled paging base ===");
    assert!(
        spelled.is_empty(),
        "a paging base is spelled rather than named, so `PagingModel` cannot reach \
         it and no survey of the seam can see it:\n  {}\n\n\
         Use `or_io::constants::FIXED_VIRTUAL_BASE` / `FIXED_PHYSICAL_BASE`, \
         or better, resolve through `Image` or `ResourceHeap::virtual_offset_of`, \
         which take the model.",
        spelled.join("\n  ")
    );
}

fn braces(line: &str) -> i32 {
    line.chars().filter(|c| *c == '{').count() as i32
        - line.chars().filter(|c| *c == '}').count() as i32
}

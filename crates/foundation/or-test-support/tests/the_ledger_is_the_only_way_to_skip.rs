use std::path::{Path, PathBuf};

const ELSEWHERE: &[(&str, &str)] = &[];

fn test_regions(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed != "#[cfg(test)]" && trimmed != "#[test]" {
            continue;
        }
        let Some(open) = (i + 1..lines.len().min(i + 8)).find(|&j| lines[j].contains('{')) else {
            continue;
        };
        let mut depth = 0i32;
        let mut end = open;
        for (j, body) in lines.iter().enumerate().skip(open) {
            depth += body.matches('{').count() as i32;
            depth -= body.matches('}').count() as i32;
            end = j;
            if depth <= 0 {
                break;
            }
        }
        regions.push((i, end));
    }
    regions
}

fn looks_like_a_skip_notice(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return false;
    }
    if !(trimmed.contains("eprintln!") || trimmed.contains("println!")) {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("skipped:")
        || lower.contains("skipping:")
        || lower.contains("; skipping")
        || lower.contains(", skipping")
}

fn workspace_root() -> PathBuf {
    or_test_support::workspace_root(env!("CARGO_MANIFEST_DIR"))
        .expect("the test runs from inside the workspace")
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&root.join("crates"), &mut out);
    collect(&root.join("src"), &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn skip_notices(rel: &str, text: &str) -> Vec<(usize, String)> {
    let whole_file_is_tests = rel.contains("/tests/");
    let regions = if whole_file_is_tests {
        Vec::new()
    } else {
        test_regions(text)
    };

    text.lines()
        .enumerate()
        .filter(|(index, line)| {
            looks_like_a_skip_notice(line)
                && (whole_file_is_tests || regions.iter().any(|(a, b)| index >= a && index <= b))
        })
        .map(|(index, line)| (index + 1, line.trim().to_string()))
        .collect()
}

#[test]
fn no_suite_prints_a_skip_notice_the_ledger_never_sees() {
    let root = workspace_root();
    let self_path = "crates/foundation/or-test-support/";

    let mut unrouted: Vec<String> = Vec::new();
    for path in rust_sources(&root) {
        let rel = relative(&root, &path);
        if rel.starts_with(self_path) {
            continue;
        }
        if ELSEWHERE.iter().any(|(file, _)| *file == rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in skip_notices(&rel, &text) {
            unrouted.push(format!("{rel}:{number}: {line}"));
        }
    }

    assert!(
        unrouted.is_empty(),
        "{} skip notice(s) print instead of recording, so the run's report \
         undercounts by at least that much:\n  {}\n\nUse \
         `or_test_support::skip_corpus` / `skip_install`, which \
         print the same sentence *and* count it. If one of these genuinely is \
         not a skip, add it to ELSEWHERE with the reason.",
        unrouted.len(),
        unrouted.join("\n  ")
    );
}

#[test]
fn every_elsewhere_entry_still_names_something_that_needs_one() {
    let root = workspace_root();
    let mut stale = Vec::new();

    for (file, reason) in ELSEWHERE {
        assert!(
            !reason.trim().is_empty(),
            "{file} is exempted without saying why"
        );

        let path = root.join(file);
        if !path.exists() {
            stale.push(format!("{file}: the file is gone"));
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable");
        if skip_notices(file, &text).is_empty() {
            stale.push(format!(
                "{file}: no longer prints a skip notice, so the exemption is spent"
            ));
        }
    }

    assert!(
        stale.is_empty(),
        "ELSEWHERE carries {} entry(s) that no longer apply:\n  {}\n\nDelete \
         them. An exemption nobody has to remove is one that only grows.",
        stale.len(),
        stale.join("\n  ")
    );
}

#[test]
fn the_lint_recognises_the_shapes_it_is_looking_for() {
    assert!(looks_like_a_skip_notice(
        r#"        eprintln!("SKIPPED: no .ydr corpus staged");"#
    ));
    assert!(looks_like_a_skip_notice(
        r#"    eprintln!("no gta5/gen8 pack staged; skipping");"#
    ));
    assert!(looks_like_a_skip_notice(
        r#"    println!("skipping: no fixtures");"#
    ));

    assert!(!looks_like_a_skip_notice(
        r#"    // eprintln!("SKIPPED: ...") used to live here"#
    ));
    assert!(!looks_like_a_skip_notice(
        r#"    /// Prints `SKIPPED:` — no longer true."#
    ));
    assert!(!looks_like_a_skip_notice(
        r#"    or_test_support::skip_corpus("a_test", "no .ydr corpus staged");"#
    ));
}

//! No doc comment in this tree carries the scaffolding the comment rule forbids.
//!
//! The five forbidden headings are conventional rustdoc sections whose body, in
//! this tree, restates the signature directly above it. A bespoke heading is
//! prose and stays: forbidding one for being long would need a blocklist.
//!
//! ⚠ **`# Safety` must never be added to the set.** Clippy's `missing_safety_doc`
//! *requires* that heading on a public `unsafe fn`, so forbidding it would put
//! this test in direct conflict with `-D warnings`. `# Errors` stays out because
//! *which* variants and *when* is the one thing a `Result` signature does not
//! carry. [`the_allowed_headings_stay_allowed`] stops either being tidied into
//! the forbidden set.
//!
//! They come back attached to something worth keeping: a doc test lives inside
//! a doc comment, so restoring an example out of history restores the
//! furniture wrapped around it.
//!
//! ```text
//! cargo test --bin openrage doc_comments
//! ```

use std::path::{Path, PathBuf};

/// A rustdoc section whose body says what the signature above it already says.
const FORBIDDEN: &[(&str, &str)] = &[
    ("Features", "a product page for a module"),
    ("Usage", "the call is the example, or it is the signature"),
    ("Arguments", "the signature names and types each one"),
    ("Panics", "named by the rule, and the tree has none"),
    ("Returns", "the signature carries the return type"),
];

/// A rustdoc section that stays, and the reason it is not furniture.
const ALLOWED: &[(&str, &str)] = &[
    (
        "Example",
        "the rule's one exception, where it is a real doc test that fails when \
         the API moves",
    ),
    (
        "Safety",
        "clippy's `missing_safety_doc` requires it on a public `unsafe fn`, so \
         forbidding it would contradict `-D warnings`",
    ),
    (
        "Errors",
        "not named by the rule, and which variants and when is the one thing a \
         `Result` signature does not carry",
    ),
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file on disk under `dir`, outside the directories git ignores.
///
/// ⚠ An ignored file that is not under an ignored directory is still read. The
/// pre-commit hook's staged-versus-working-tree refusal closes the side that
/// passes bad commits.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(
            name.as_str(),
            ".git" | "node_modules" | "external" | ".claude" | ".cursor"
        ) || name.starts_with("target")
            || name.starts_with('_')
        {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if crate::doc_citations::is_ignored_dir(&path) {
                continue;
            }
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The text after `///` or `//!`, or `None` for a line that is not a doc comment.
fn doc_body(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix("///")
        .or_else(|| line.strip_prefix("//!"))
}

/// The title of a `# Heading`, or `None` for doc text that is not one.
fn heading(body: &str) -> Option<&str> {
    let body = body.trim_start();
    let rest = body.trim_start_matches('#');
    if rest.len() == body.len() || !rest.starts_with(' ') {
        return None;
    }
    let title = rest.trim();
    (!title.is_empty()).then_some(title)
}

/// Every `(path, line, heading)` in the tree, fenced blocks excluded.
fn headings() -> (usize, Vec<(String, usize, String)>) {
    let mut paths = Vec::new();
    walk(repo_root(), &mut paths);

    let mut found = Vec::new();
    let mut scanned = 0;
    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        scanned += 1;
        let rel = path
            .strip_prefix(repo_root())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        // A `#` inside a fenced block is rustdoc's hidden-line marker or the
        // sample's own text, never a section of the comment.
        let mut fenced = false;
        for (index, line) in text.lines().enumerate() {
            let Some(body) = doc_body(line) else {
                fenced = false;
                continue;
            };
            if body.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            if let Some(title) = heading(body) {
                found.push((rel.clone(), index + 1, title.to_string()));
            }
        }
    }
    (scanned, found)
}

#[test]
fn no_doc_comment_carries_a_scaffolding_heading() {
    let (scanned, found) = headings();

    // A test for the absence of something passes just as well when it is
    // looking in the wrong place, so the walk and the parse are asserted
    // before the thing they are for. Both floors are far below what the tree
    // holds and would only trip on a walk or a parse that had stopped working.
    assert!(
        scanned > 400,
        "only {scanned} Rust file(s) were read — the walk is wrong, and this \
         check cannot fail until it is fixed"
    );
    assert!(
        found.len() > 15,
        "only {} doc-comment heading(s) were parsed — the parse is wrong, and \
         a check that finds none can never fire",
        found.len()
    );

    let mut carried: Vec<String> = found
        .iter()
        .filter_map(|(path, line, title)| {
            FORBIDDEN
                .iter()
                .find(|(name, _)| name == title)
                .map(|(name, why)| format!("  {path}:{line}  `# {name}` — {why}"))
        })
        .collect();
    carried.sort();

    assert!(
        carried.is_empty(),
        "these doc comments carry a heading the comment rule forbids:\n{}\n\n\
         Delete the heading. If its body only restates the signature, that goes \
         with it; if the body says something the signature genuinely cannot — a \
         flag mapping, a decode rule — keep the sentences and delete the heading \
         alone. A `# Example` that is a real doc test stays, and is the reason \
         most of these are here: restoring a doc test out of history restores \
         the doc comment wrapped around it, furniture included.\n\n\
         The rule is AGENTS.md § Comments.",
        carried.join("\n")
    );
}

#[test]
fn the_allowed_headings_stay_allowed() {
    for (title, why) in ALLOWED {
        assert!(
            !FORBIDDEN.iter().any(|(name, _)| name == title),
            "`# {title}` has been added to the forbidden set and must not be: \
             {why}"
        );
    }
}

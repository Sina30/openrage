//! A suppression that hides unused surface, or covers a whole module, names
//! its reason.
//!
//! [The shell work](../docs/reference/Crate_Reference.md#suppressions-name-their-reason) removed twenty-one module-level
//! `#![allow(dead_code)]`s and left a rule behind it: item-level ones survive
//! where they document a deliberate reservation, **and each says why**. That
//! half was prose, and prose is the half that does not hold — measured when
//! this was written, **fifteen of the sixteen `dead_code` suppressions in
//! `src/` said nothing at all**, including all four registration seams the
//! section names as the most valuable thing a blanket allow had been hiding.
//!
//! # Why a reason, and not a count
//!
//! The section's own finding is that **a blanket allow with a plausible
//! reason is worse than one with none, because the reason survives review
//! while the thing it covers drifts.** That argues against trusting a
//! sentence, not against requiring one — a suppression with no sentence is
//! not reviewable at all, and the four specimens it lists were each found by
//! reading what the suppression *covered*. What this asks for is the pair:
//! the item is right there, and the reason is beside it.
//!
//! So the bar is deliberately low and the gate is deliberately dumb. It
//! cannot tell a true reason from a plausible one, and saying so is better
//! than implying it can.
//!
//! # Why not every suppression
//!
//! ⚠ **A rule that asked for a sentence beside every `#[allow]` would be a
//! rule that manufactures furniture**, and the comment rule in `AGENTS.md`
//! forbids exactly that: `// this draw function takes eight arguments` above
//! `#[allow(clippy::too_many_arguments)]` says what the line below already
//! says. Nineteen of those exist and not one of them hides anything — the
//! function is in front of you.
//!
//! The family that has to argue is the one that **hides surface**: a lint
//! reporting something *unused*, silenced, is a claim that the unused thing
//! should stay, and nothing but a sentence carries that claim. A
//! module-level suppression is in for a different reason — it covers a
//! subtree, so there is no "what is under it" to read.
//!
//! The one thing asked of the rest is that they cover what they claim to,
//! which is [`a_too_many_arguments_suppression_sits_on_a_function`].
//!
//! # What `expect` buys that `allow` does not
//!
//! ⚠ `#[expect]` **fails when the lint would not have fired**, which is the
//! *condition that retires it* enforced by the compiler rather than written
//! down. Six `#![expect(deprecated)]`s in `src/ui` were there for that: they
//! covered egui 0.34's renamed panel constructors until the migration landed,
//! and they were errors under `-D warnings` the day it did rather than
//! blanket allows nobody looks at again.
//!
//! ⚠ **What held that set was a list, and the list is the part that did not
//! survive its own success.** It named the six files exactly, in both
//! directions, so a seventh could not join quietly — but once the set is
//! empty a list comparison notices nothing.
//! [`no_file_in_src_suppresses_a_deprecation`] is the successor and refuses
//! the first one back.
//!
//! It is not always available. A seam like `capabilities::register` is dead
//! in the bin build and live in the test build, so an `expect` would be
//! unfulfilled in one of the two. Those stay `allow`, with the sentence.
//!
//! ```text
//! cargo test --bin openrage suppressions
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A lint whose whole subject is surface nobody uses, so silencing one is a
/// claim that the surface should stay.
const SILENCING_UNUSED: &[(&str, &str)] = &[
    ("dead_code", "an item nothing calls"),
    ("unused_imports", "a name nothing in this build resolves"),
    ("unused_variables", "a binding nothing reads"),
    ("unused_mut", "a mutability nothing needs"),
    ("unused_must_use", "a result nobody looked at"),
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn src_dir() -> PathBuf {
    repo_root().join("src")
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// One `allow` or `expect` in the tree, with the line it opens on.
struct Suppression {
    file: String,
    line: usize,
    text: String,
    /// The non-blank lines above it, nearest first, up to the first that is
    /// neither a comment nor another attribute.
    preamble: Vec<String>,
}

impl Suppression {
    fn is_expect(&self) -> bool {
        self.text.contains("expect(")
    }

    /// A reason is a comment beside it, or `expect`'s own `reason =`.
    fn states_a_reason(&self) -> bool {
        if self.is_expect() && self.text.contains("reason =") {
            return true;
        }
        self.preamble.iter().any(|line| line.starts_with("//"))
    }

    fn mentions(&self, lint: &str) -> bool {
        self.text.contains(lint)
    }
}

/// Every `allow` and `expect` under `dir`, or why the scan looked nowhere.
///
/// ⚠ A misspelt root, an empty folder and a walk that read nothing all find no
/// suppression, and every check below passes on none. What the scan is held to
/// is that it read a `.rs` file, never that it found some: a count floor is
/// crossed by the next deletion a check asks for. The parser is proved on
/// fixtures instead.
///
/// An attribute spans lines, so it is gathered until its brackets balance.
fn scan(dir: &Path) -> Result<Vec<Suppression>, String> {
    let files = rust_files(dir);
    if files.is_empty() {
        return Err(format!(
            "{} holds no `.rs` file, so the scan looked nowhere, which is not \
             the same as a tree with nothing to suppress",
            relative(dir)
        ));
    }

    let mut out = Vec::new();

    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file = relative(&path);
        let lines: Vec<&str> = text.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let opens = trimmed.starts_with("#[") || trimmed.starts_with("#![");
            if !opens || !(trimmed.contains("allow(") || trimmed.contains("expect(")) {
                continue;
            }

            let mut whole = String::new();
            let mut depth = 0i32;
            for line in &lines[index..] {
                whole.push_str(line.trim());
                depth += line.matches('[').count() as i32;
                depth -= line.matches(']').count() as i32;
                if depth <= 0 {
                    break;
                }
                whole.push(' ');
            }

            let mut preamble = Vec::new();
            for above in lines[..index].iter().rev() {
                let above = above.trim();
                if above.is_empty() {
                    break;
                }
                if above.starts_with("//") || above.starts_with("#[") {
                    preamble.push(above.to_string());
                    continue;
                }
                break;
            }

            out.push(Suppression {
                file: file.clone(),
                line: index + 1,
                text: whole,
                preamble,
            });
        }
    }

    Ok(out)
}

fn suppressions() -> Vec<Suppression> {
    scan(&src_dir()).unwrap_or_else(|why| panic!("{why}"))
}

/// What the check refuses: a hiding suppression no comment or `reason =` backs.
fn hiding_without_a_reason(found: &[Suppression]) -> Vec<String> {
    let mut silent: Vec<String> = found
        .iter()
        .filter(|s| SILENCING_UNUSED.iter().any(|(lint, _)| s.mentions(lint)))
        .filter(|s| !s.states_a_reason())
        .map(|s| format!("  {}:{}  {}", s.file, s.line, s.text))
        .collect();
    silent.sort();
    silent
}

#[test]
fn a_suppression_that_hides_unused_surface_says_why() {
    let silent = hiding_without_a_reason(&suppressions());

    assert!(
        silent.is_empty(),
        "silencing one of these is a claim that the unused thing should stay, \
         and these carry no such claim:\n{}\n\n\
         Write the sentence, or delete the suppression and the item under it. \
         A comment on the line above is enough, and `#[expect(lint, reason = \
         \"…\")]` is better where the lint fires unconditionally, because the \
         compiler then retires it. docs/reference/Crate_Reference.md, \
         \"Suppressions name their reason\".",
        silent.join("\n")
    );
}

/// The family is what the module header says it is.
///
/// A list like this is one somebody widens to make a run green, and widening
/// it is how a lint leaves the set that has to argue. This does not stop
/// that; it makes it a diff on a named constant rather than an edit inside a
/// filter expression.
#[test]
fn the_family_that_has_to_argue_is_about_unused_surface() {
    for (lint, subject) in SILENCING_UNUSED {
        assert!(
            lint.starts_with("unused") || *lint == "dead_code",
            "`{lint}` is in the set that has to name a reason, whose whole \
             argument is that its subject is unused surface — this one's \
             subject is stated as {subject}, and its name says otherwise"
        );
    }
    assert!(
        !SILENCING_UNUSED
            .iter()
            .any(|(lint, _)| lint.starts_with("clippy::")),
        "a clippy style lint fires on code that is right in front of the \
         reader, so a sentence beside it is the furniture AGENTS.md's comment \
         rule forbids"
    );
}

/// The purest specimen the shell work recorded, as a rule.
///
/// ⚠ `render/particles.rs` carried a `#[allow(clippy::too_many_arguments)]`
/// **on an enum**, wedged between a function's doc comment and the type a
/// later insertion had stranded it above. Not merely stale: detached, and
/// still reading as considered. The lint only applies to a function, so a
/// suppression of it that is not on one is covering nothing.
#[test]
fn a_too_many_arguments_suppression_sits_on_a_function() {
    let found = suppressions();
    let lint = "too_many_arguments";

    let candidates: Vec<&Suppression> = found.iter().filter(|s| s.mentions(lint)).collect();

    let sources: Vec<(String, Vec<String>)> = rust_files(&src_dir())
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            Some((
                relative(path),
                text.lines().map(str::to_string).collect::<Vec<_>>(),
            ))
        })
        .collect();

    let mut detached = Vec::new();
    for suppression in candidates {
        let Some((_, lines)) = sources.iter().find(|(file, _)| *file == suppression.file) else {
            continue;
        };
        // The item is the first line below that is neither another attribute
        // nor a doc comment nor blank.
        let covered = lines[suppression.line..].iter().find(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("//")
        });
        let covered = covered.map(|l| l.trim()).unwrap_or("");
        if !covered.contains("fn ") {
            detached.push(format!(
                "  {}:{}  covers `{covered}`",
                suppression.file, suppression.line
            ));
        }
    }

    assert!(
        detached.is_empty(),
        "`clippy::{lint}` only fires on a function, so these suppress \
         nothing and read as considered:\n{}",
        detached.join("\n")
    );
}

/// ⚠ **This replaces the pair that sized the egui 0.34 panel migration**, and
/// it is the stronger check rather than the same one: the migration has
/// landed, so the set it held is empty, and an empty set is exactly what a
/// `BTreeSet` comparison stops noticing. Silencing a deprecation is deferring
/// a dependency bump, which is the thing that arrives as somebody else's red
/// build.
#[test]
fn no_file_in_src_suppresses_a_deprecation() {
    let silencing: Vec<String> = suppressions()
        .iter()
        .filter(|s| s.mentions("deprecated"))
        .map(|s| format!("  {}:{}  {}", s.file, s.line, s.text))
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();

    assert!(
        silencing.is_empty(),
        "a deprecated call silenced in the shell is a dependency bump's \
         critical path, deferred where nothing measures it:\n{}\n\n\
         Make the substitution, or — if it is genuinely a change to who owns \
         the screen, as `show(ctx, …)` → `show_inside(ui, …)` was — file it \
         and record the set here rather than leaving the suppression to \
         argue for itself.",
        silencing.join("\n")
    );
}

/// No module-level suppression arrives without a reason, which is how the
/// twenty-one came back last time.
#[test]
fn a_module_level_suppression_is_the_one_that_has_to_argue_for_itself() {
    let found = suppressions();
    let blanket: Vec<&Suppression> = found.iter().filter(|s| s.text.starts_with("#![")).collect();

    let silent: Vec<String> = blanket
        .iter()
        .filter(|s| !s.states_a_reason())
        .map(|s| format!("  {}:{}  {}", s.file, s.line, s.text))
        .collect();

    assert!(
        silent.is_empty(),
        "a module-level suppression covers a whole subtree, so it is the one \
         a reader cannot check by looking at what is under it:\n{}",
        silent.join("\n")
    );

    let dead_code: Vec<String> = blanket
        .iter()
        .filter(|s| s.mentions("dead_code"))
        .map(|s| format!("  {}:{}", s.file, s.line))
        .collect();
    assert!(
        dead_code.is_empty(),
        "docs/reference/Crate_Reference.md's rule is no module-level \
         `#![allow(dead_code)]` in src/, whatever reason it carries — a \
         blanket over a subtree is how the plugin API came to be reported \
         dead and nearly deleted:\n{}",
        dead_code.join("\n")
    );
}

/// A folder of `.rs` files, removed when it drops.
fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a scratch folder");
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a fixture folder");
        std::fs::write(&path, text).expect("a fixture file");
    }
    dir
}

#[test]
fn a_scan_of_a_folder_with_no_rust_file_is_refused_rather_than_judged_clean() {
    let empty = fixture(&[]);
    assert!(scan(empty.path()).is_err(), "an empty folder was judged");

    let misspelt = empty.path().join("scr");
    assert!(
        scan(&misspelt).is_err(),
        "a folder that is not there was judged"
    );

    let no_rust = fixture(&[("notes.txt", "#[allow(dead_code)]\n")]);
    assert!(
        scan(no_rust.path()).is_err(),
        "a folder of no `.rs` was judged"
    );
}

#[test]
fn a_suppression_with_a_sentence_beside_it_is_found_in_a_subfolder_and_passes() {
    let dir = fixture(&[(
        "nested/seam.rs",
        "// Called by the plugin loader, which this build does not compile.\n#[allow(dead_code)]\nfn seam() {}\n",
    )]);

    let found = scan(dir.path()).expect("a folder with a `.rs` file");
    assert_eq!(found.len(), 1, "the nested file was not walked");
    assert_eq!(found[0].line, 2);
    assert!(found[0].mentions("dead_code"));
    assert!(hiding_without_a_reason(&found).is_empty());
}

#[test]
fn a_suppression_with_no_sentence_is_named_by_the_check() {
    let dir = fixture(&[("seam.rs", "#[allow(dead_code)]\nfn seam() {}\n")]);

    let silent = hiding_without_a_reason(&scan(dir.path()).expect("a folder with a `.rs` file"));
    assert_eq!(silent.len(), 1, "{silent:?}");
    assert!(silent[0].contains(":1  #[allow(dead_code)]"), "{silent:?}");
}

#[test]
fn a_tree_with_no_suppression_at_all_passes_the_scan_and_the_check() {
    let dir = fixture(&[("main.rs", "fn main() {}\n")]);

    let found = scan(dir.path()).expect("a folder with a `.rs` file");
    assert!(found.is_empty());
    assert!(hiding_without_a_reason(&found).is_empty());
}

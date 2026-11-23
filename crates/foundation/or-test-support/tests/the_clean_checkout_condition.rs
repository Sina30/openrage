//! `just test-no-pack` and the `layouts` gate ask one question, in two places.
//!
//! ⚠ The clean-checkout condition is a set of environment variables stated in
//! the `xtask` task `just test-no-pack` runs and in
//! `.github/workflows/local-gate.yml`, with nothing else holding the two equal.
//!
//! Only the condition is compared, never the commands: the recipe runs
//! `cargo nextest` and the workflow runs `cargo test`, for reasons
//! [AGENTS.md](../../../../AGENTS.md) states.

use std::path::{Path, PathBuf};

/// The two variables that, together, are "a clean checkout".
///
/// ⚠ Schema discovery walks the workspace root's ancestors for a shared
/// schema snapshot, so on a machine that has one every worktree finds it.
/// A run without the
/// `SCHEMA` sentinel has a schema library and still calls itself a clean checkout —
/// and that is the difference between XML re-import succeeding for 6,056 of
/// 6,071 corpus documents and failing at the root type for all 6,071.
const FIXTURES: &str = "OPENRAGE_FIXTURES";
const SCHEMA: &str = "OPENRAGE_SCHEMA";

/// What `OPENRAGE_SCHEMA` has to be set to, spelled the way both files spell it.
///
/// ⚠ A word rather than an empty string, because PowerShell deletes a variable
/// assigned `""` and the obvious convention would silently mean "unset" on the
/// primary platform — which is the bug the sentinel exists for.
const SENTINEL: &str = "none";

fn root() -> PathBuf {
    or_test_support::workspace_root(env!("CARGO_MANIFEST_DIR")).expect("a workspace above us")
}

fn read(path: &Path) -> String {
    // ⚠ A Windows checkout with autocrlf holds CRLF, and `tier` looks for a closing brace after a lone LF.
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// The body of one function in `xtask`'s test tiers: its lines, up to the
/// closing brace in the first column.
fn tier(name: &str) -> String {
    let source = read(&root().join("crates/tools/xtask/src/tiers.rs"));
    let marker = format!("pub fn {name}(");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("xtask's tiers have no `{name}`"));
    let body = &source[start..];
    let end = body.find("\n}\n").expect("the function is closed");
    body[..end].to_string()
}

/// The `run:` block and `env:` block of one workflow step, by its `name:`.
fn step(workflow: &str, name: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut indent = 0usize;
    for line in workflow.lines() {
        let trimmed = line.trim_start();
        if !inside {
            if trimmed.starts_with("- name:") && trimmed.contains(name) {
                inside = true;
                indent = line.len() - trimmed.len();
            }
            continue;
        }
        let here = line.len() - line.trim_start().len();
        if !trimmed.is_empty() && here <= indent && trimmed.starts_with("- ") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    assert!(inside, "the workflow has no step named `{name}`");
    out
}

/// Which of the two variables a block sets, and to what.
///
/// Deliberately shallow. It answers *is this variable assigned here*, which is
/// the whole of what the two places have to agree about; the shell each uses
/// to do it is their own business.
#[derive(Debug, Default, PartialEq, Eq)]
struct Condition {
    schema: Option<String>,
    fixtures: bool,
}

impl Condition {
    /// Which variables are set, ignoring what they are set to.
    fn shape(&self) -> (bool, bool) {
        (self.schema.is_some(), self.fixtures)
    }
}

fn condition(block: &str) -> Condition {
    let mut found = Condition::default();

    for line in block.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        for name in [SCHEMA, FIXTURES] {
            if !line.contains(name) {
                continue;
            }
            // `.env("NAME", value)` (xtask), `$env:NAME = value;` (the workflow's
            // PowerShell step) or `NAME: value` (a workflow `env:` block). The
            // value runs to the statement separator, because a PowerShell step
            // is a line and everything after the semicolon is the next statement.
            let value = if let Some((_, rest)) = line.split_once(&format!(".env(\"{name}\",")) {
                // `.env("NAME", value)`, in xtask.
                Some(rest.trim().trim_end_matches(';').trim_end_matches(')'))
            } else if let Some((_, rest)) = line.split_once(&format!("$env:{name}")) {
                // `$env:NAME = value;`, in the workflow's PowerShell step.
                rest.trim_start().strip_prefix('=').map(|v| v.trim_start())
            } else {
                // `NAME: value`, in a workflow `env:` block.
                line.strip_prefix(name)
                    .and_then(|r| r.trim_start().strip_prefix(':'))
                    .map(|v| v.trim_start())
            };
            let Some(value) = value else { continue };
            let value = value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .trim_matches('"')
                .to_string();

            match name {
                SCHEMA => found.schema = Some(value),
                _ => found.fixtures = true,
            }
        }
    }
    found
}

#[test]
fn the_recipe_and_the_gate_ask_the_same_question() {
    let root = root();
    let workflow = read(&root.join(".github/workflows/local-gate.yml"));

    let from_recipe = condition(&tier("no_pack_tests"));
    let from_gate = condition(&step(&workflow, "No pack, no corpus"));

    eprintln!(
        "\n=== the clean-checkout condition, as each place states it ===\n  \
         xtask no-pack-tests          : {SCHEMA}={:?} {FIXTURES} set = {}\n  \
         local-gate No pack, no corpus: {SCHEMA}={:?} {FIXTURES} set = {}",
        from_recipe.schema, from_recipe.fixtures, from_gate.schema, from_gate.fixtures
    );

    assert_eq!(
        from_gate.shape(),
        (true, true),
        "the `layouts` gate's clean-checkout step no longer sets both \
         variables. It is the definition the recipe is held to, so if the \
         condition has genuinely changed, change it here first and deliberately."
    );

    assert_eq!(
        from_recipe.shape(),
        from_gate.shape(),
        "`just test-no-pack` and the `layouts` gate's clean-checkout step set \
         different variables, so they are two conditions wearing one name.\n\n\
         ⚠ `{SCHEMA}` is a trap: a machine with a shared snapshot has one in \
         every worktree.\n\n\
         If a second condition is genuinely wanted, give it a second recipe \
         name — do not let one name mean two things in two files."
    );

    for (place, found) in [("recipe", &from_recipe), ("gate", &from_gate)] {
        assert_eq!(
            found.schema.as_deref(),
            Some(SENTINEL),
            "the {place} sets {SCHEMA} to something other than the `{SENTINEL}` \
             sentinel. An empty value is not the opt-out and cannot be — \
             PowerShell deletes a variable assigned `\"\"`, so the obvious \
             convention silently means \"unset\" on the primary platform."
        );
    }
}

/// The recipe says which condition it ran, before it runs it.
#[test]
fn the_recipe_names_the_condition_it_runs() {
    let body = tier("no_pack_tests");

    // The banner is the string literal `println!` is given.
    let banner: Vec<&str> = body
        .lines()
        .filter(|l| l.trim_start().starts_with('"'))
        .collect();

    assert!(
        !banner.is_empty(),
        "`test-no-pack` prints nothing about the condition it is running. It \
         must say so: the recipe's whole subject is a state that is invisible \
         unless it is announced, and it was stated wrongly for as long as \
         nothing printed it."
    );
    let said = banner.join(" ");
    for wanted in [SCHEMA, FIXTURES, SENTINEL] {
        assert!(
            said.contains(wanted),
            "the banner does not mention `{wanted}`, so it names the condition \
             only partly:\n  {said}"
        );
    }
}

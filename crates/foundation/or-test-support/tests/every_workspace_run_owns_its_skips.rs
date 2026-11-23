//! A run's skips belong to that run.
//!
//! The ledger is a directory every test process appends to and `skip-ledger`
//! drains. ⚠ A recipe that runs the tests and never drains it does not lose
//! the records — it leaves them for whichever run drains next, which reports
//! them as its own. Measured: `test-no-pack` left roughly 660 clean-checkout
//! skips behind, and the next run read 659, then 654, then 657 and 668 on
//! deliberate repeats, against a true isolated figure of 64 checks across 31
//! suites.

use std::path::{Path, PathBuf};

/// Recipes that run the whole workspace's tests, and so must account for what
/// those tests skipped.
const RUNS_THE_WORKSPACE: &[&str] = &["test", "test-cargo", "test-lite"];

/// The same rule for the two tiers whose recipes call `xtask` and so name no
/// `--workspace` themselves: `test-no-pack` and `test-strict` run it there.
const XTASK_TIERS_THAT_RUN_THE_WORKSPACE: &[&str] = &["no_pack_tests", "strict_tests"];

/// ⚠ One crate's tests deliberately report no ledger: "what this did not run"
/// is "almost everything". The rule keys on `--workspace`, so `test-crate`'s
/// `-p {{ crate }}` is outside it by what it runs rather than by being named.
const SCOPED_AND_SO_EXEMPT: &str = "test-crate";

fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while !dir.join("justfile").is_file() {
        assert!(
            dir.pop(),
            "no justfile above {}",
            env!("CARGO_MANIFEST_DIR")
        );
    }
    dir
}

struct Recipe {
    name: String,
    body: String,
}

/// just's grammar, to the depth this needs: a recipe header starts in column
/// one and ends in `:`, and its body is the indented lines under it.
fn recipes(text: &str) -> Vec<Recipe> {
    let mut out: Vec<Recipe> = Vec::new();
    for line in text.lines() {
        let is_header = !line.starts_with(char::is_whitespace)
            && !line.starts_with('#')
            && !line.starts_with('@')
            && line.contains(':')
            && !line.trim_start().starts_with("//");
        if is_header {
            let head = line.split(':').next().unwrap_or_default().trim();
            // `name arg="default"` and `name: deps` both name the recipe first.
            let name = head.split_whitespace().next().unwrap_or_default();
            // An assignment (`x := y`) is not a recipe.
            if name.is_empty() || line.contains(":=") {
                continue;
            }
            out.push(Recipe {
                name: name.to_string(),
                body: String::new(),
            });
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            if let Some(last) = out.last_mut() {
                last.body.push_str(line);
                last.body.push('\n');
            }
        }
    }
    out
}

fn runs_the_workspace(body: &str) -> bool {
    body.lines().any(|line| {
        (line.contains("cargo nextest run") || line.contains("cargo test"))
            && line.contains("--workspace")
    })
}

fn drains_the_ledger(body: &str) -> bool {
    body.contains("skip-ledger") && !body.contains("--keep")
}

#[test]
fn every_recipe_that_runs_the_workspace_drains_the_ledger() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("justfile")).expect("read justfile");
    let recipes = recipes(&text);

    // ⚠ The parse is the thing most likely to break silently: a grammar change
    // that matched nothing would leave every assertion below vacuously true.
    assert!(
        recipes.len() > 20,
        "only {} recipe(s) parsed out of the justfile — the parse is wrong, \
         and a parse that finds nothing agrees with every rule below",
        recipes.len()
    );

    // Sorted: where a recipe sits in the file is not what this is about.
    let mut found: Vec<&str> = recipes
        .iter()
        .filter(|r| runs_the_workspace(&r.body))
        .map(|r| r.name.as_str())
        .collect();
    found.sort_unstable();

    assert_eq!(
        found, RUNS_THE_WORKSPACE,
        "the set of recipes running the whole workspace's tests has changed.\n\
         Add it to RUNS_THE_WORKSPACE and give it a `skip-ledger` call, or \
         scope it to one crate. A recipe that runs every test and accounts \
         for none of their skips hands them to the next run."
    );

    let leaking: Vec<&str> = recipes
        .iter()
        .filter(|r| runs_the_workspace(&r.body) && !drains_the_ledger(&r.body))
        .map(|r| r.name.as_str())
        .collect();

    assert!(
        leaking.is_empty(),
        "these recipes run the workspace's tests and never drain the ledger, \
         so their skips are attributed to whichever run drains next: {leaking:?}"
    );
}

/// The functions of `xtask`'s test tiers, each with its body.
fn xtask_tiers() -> Vec<(String, String)> {
    // ⚠ A Windows checkout with autocrlf holds CRLF, and the split on `"\n}\n"` below then never ends a body.
    let source = std::fs::read_to_string(workspace_root().join("crates/tools/xtask/src/tiers.rs"))
        .expect("read xtask's tiers")
        .replace("\r\n", "\n");
    source
        .split("\nfn ")
        .chain(source.split("\npub fn ").skip(1))
        .filter_map(|chunk| {
            let name = chunk.split('(').next()?.trim().to_string();
            let body = chunk.split("\n}\n").next()?.to_string();
            (!name.is_empty() && !name.contains(' ')).then_some((name, body))
        })
        .collect()
}

/// A tier runs the workspace's tests when it hands cargo `--workspace` and a
/// verb that runs them; building the ledger or listing tests does neither.
fn tier_runs_the_workspace(body: &str) -> bool {
    body.contains("\"--workspace\"") && (body.contains("\"test\"") || body.contains("\"run\""))
}

#[test]
fn every_xtask_tier_that_runs_the_workspace_drains_the_ledger() {
    let tiers = xtask_tiers();
    assert!(
        tiers.len() > 5,
        "only {} function(s) parsed out of xtask's tiers, so the parse is wrong",
        tiers.len()
    );

    let mut found: Vec<&str> = tiers
        .iter()
        .filter(|(_, body)| tier_runs_the_workspace(body))
        .map(|(name, _)| name.as_str())
        .collect();
    found.sort_unstable();
    assert_eq!(
        found, XTASK_TIERS_THAT_RUN_THE_WORKSPACE,
        "the set of xtask tiers running the whole workspace's tests has changed.\n\
         Add it to XTASK_TIERS_THAT_RUN_THE_WORKSPACE and drain the ledger \
         through `then_ledger`, or scope it to one crate."
    );

    for (name, body) in &tiers {
        if tier_runs_the_workspace(body) {
            assert!(
                body.contains("then_ledger("),
                "`{name}` runs the workspace's tests and never drains the \
                 ledger, so their skips are attributed to whichever run drains next"
            );
        }
    }
}

#[test]
fn a_recipe_scoped_to_one_crate_is_outside_the_rule_by_what_it_runs() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("justfile")).expect("read justfile");
    let recipes = recipes(&text);

    let scoped = recipes
        .iter()
        .find(|r| r.name == SCOPED_AND_SO_EXEMPT)
        .unwrap_or_else(|| panic!("{SCOPED_AND_SO_EXEMPT} is gone; re-rule the exemption"));

    assert!(
        !runs_the_workspace(&scoped.body),
        "{SCOPED_AND_SO_EXEMPT} now runs the whole workspace, so it owes a ledger call"
    );
    assert!(
        scoped.body.contains("-p "),
        "the exemption is that it names one crate; if it no longer does, the \
         reason it is exempt has gone with it"
    );
}

#[test]
fn the_reporter_that_only_reports_does_not_clear() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("justfile")).expect("read justfile");
    let skips = recipes(&text)
        .into_iter()
        .find(|r| r.name == "skips")
        .expect("the `skips` recipe");

    // Reading the ledger without running the tests must not consume it, or
    // asking what the last run skipped destroys the answer for the next asker.
    assert!(
        skips.body.contains("--keep"),
        "`just skips` reports without re-running, so it must not drain"
    );
}

fn write_records(dir: &Path, lines: &[&str]) {
    std::fs::create_dir_all(dir).expect("create ledger dir");
    std::fs::write(dir.join("suite.tsv"), lines.join("\n") + "\n").expect("write records");
}

fn run_ledger(dir: &Path, extra: &[&str]) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_skip-ledger"))
        .arg("--dir")
        .arg(dir)
        .args(extra)
        .output()
        .expect("run skip-ledger");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// One record, in the ledger's own on-disk shape.
fn record(what: &str) -> String {
    format!("corpus\tydd_roundtrip\ttests/ydd_roundtrip.rs:1\t{what}\tno corpus staged")
}

#[test]
fn a_ledger_left_behind_is_reported_again_by_the_next_reader() {
    // ⚠ This is the defect itself, not a model of it: records nobody drained
    // are read by whoever drains next and counted as theirs.
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("ledger");

    let first = record("a_check_the_first_run_skipped");
    write_records(&dir, &[&first]);

    let kept = run_ledger(&dir, &["--keep"]);
    assert!(
        kept.contains("1 check(s)"),
        "the first reader should see one record, got:\n{kept}"
    );
    assert!(dir.exists(), "--keep must leave the ledger where it was");

    // A second run records one of its own on top of the residue.
    let second = record("a_check_the_second_run_skipped");
    write_records(&dir, &[&first, &second]);

    let polluted = run_ledger(&dir, &[]);
    assert!(
        polluted.contains("2 check(s)"),
        "the second reader inherits the first run's record — this is the \
         attribution defect, and it must stay demonstrated:\n{polluted}"
    );
    assert!(
        !dir.exists(),
        "draining must clear the ledger, or the next run inherits it too"
    );
}

#[test]
fn a_drain_empties_a_directory_of_many_files_rather_than_most_of_them() {
    // A whole-workspace run writes one file per test process — 73 for
    // one crate's lib suite alone — and a drain that clears most of them
    // leaves a residue the next run is told is its own.
    //
    // ⚠ This holds "the directory is empty afterwards" and nothing narrower.
    // The two real failures were both partial — `remove_dir_all` stopping at
    // the first entry it could not delete, and Windows skipping entries when
    // files are removed while the same directory handle is enumerated, 143
    // files to 126 and then 126 to 66 — but neither reproduces here, because
    // both needed the file locks a real run produces and a temp directory has
    // none: deleting while enumerating still passes this. Many files rather
    // than one because one file cannot tell "emptied" from "mostly emptied".
    const FILES: usize = 200;

    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("ledger");
    std::fs::create_dir_all(&dir).expect("create ledger dir");
    for i in 0..FILES {
        std::fs::write(
            dir.join(format!("or_io-{i}.tsv")),
            record(&format!("check{i}")) + "\n",
        )
        .expect("write record file");
    }
    assert_eq!(
        std::fs::read_dir(&dir).expect("read back").count(),
        FILES,
        "the fixture must really hold {FILES} files, or this proves nothing"
    );

    let reported = run_ledger(&dir, &[]);
    assert!(
        reported.contains(&format!("{FILES} check(s)")),
        "every record should be reported:\n{reported}"
    );
    let left = std::fs::read_dir(&dir).map(Iterator::count).unwrap_or(0);
    assert_eq!(
        left, 0,
        "a drain left {left} of {FILES} files behind; the next run inherits them"
    );
}

#[test]
fn draining_reports_a_genuine_skip_rather_than_swallowing_it() {
    // ⚠ A drain that clears the ledger silently costs the whole point of
    // having one, so it must say what it drained.
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("ledger");

    write_records(
        &dir,
        &[&record("a_real_skip"), &record("another_real_skip")],
    );

    let reported = run_ledger(&dir, &["--all"]);
    assert!(
        reported.contains("2 check(s)"),
        "a drain must report what it drained:\n{reported}"
    );
    assert!(
        reported.contains("a_real_skip") && reported.contains("ydd_roundtrip"),
        "a drain must name the checks, not only count them:\n{reported}"
    );
    assert!(!dir.exists(), "a drain clears what it reported");

    // And an empty ledger says so rather than saying nothing at all.
    std::fs::create_dir_all(&dir).expect("recreate");
    let empty = run_ledger(&dir, &[]);
    assert!(
        empty.contains("nothing skipped"),
        "an empty ledger must say it is empty:\n{empty}"
    );
}

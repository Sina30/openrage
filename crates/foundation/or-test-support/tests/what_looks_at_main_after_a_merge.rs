//! Something checks `main` after every merge, and it reports in minutes.
//!
//! ⚠ Two pull requests can each merge green and leave `main` unable to
//! compile when neither branch's run contains both halves; `cargo check`
//! failed on `main` for about ninety minutes that way.
//!
//! A missing `push` trigger is the wrong diagnosis for a broken `main` nobody
//! saw: `local-gate.yml` always ran on a push to `main`, but five such runs in a
//! row were cancelled by the next merge, because its concurrency group was the
//! ref and merges arrive seconds apart, and its compile check sat about forty
//! minutes in, behind pack staging and the corpus suites. Detection needs a
//! check that is **cheap**, **first**, and **not cancelled by its own pull
//! request**. Each is one line of YAML and each would be invisible to remove,
//! so each is asserted here.
//!
//! ⚠ A newer tip does cancel an older one: every landing is a squash onto the
//! tip, so the newer tree contains the older one's changes. What is lost is
//! attribution, not coverage: every tip not superseded before its run finishes
//! gets the whole suite, so no break survives past the next completed run.
//!
//! ⚠ This is deliberately **not** a merge gate: the runner serialises, so a
//! required check on one machine stalls a queue of branches.

use std::path::{Path, PathBuf};

fn workflows() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-test-support")
        .join(".github/workflows")
}

fn read(name: &str) -> String {
    let path = workflows().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must exist and be readable: {e}", path.display()))
}

/// The lines of a top-level YAML block, by its key, with comments dropped.
///
/// ⚠ Comments are dropped because every property below is also *described* in
/// these files, and a scan over raw text would pass on the prose rather than on
/// the setting.
fn block(source: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if inside {
            // A top-level key ends the block.
            if !line.starts_with(' ') && !line.trim().is_empty() {
                break;
            }
            out.push(line.trim().to_string());
        } else if line.starts_with(&format!("{key}:")) {
            inside = true;
        }
    }
    assert!(inside, "no top-level `{key}:` block");
    out
}

/// Every non-comment line of a workflow, in order.
fn code(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// ⚠ **Superseded by a newer tip, and by nothing else.**
#[test]
fn only_a_newer_tip_cancels_the_tip_check() {
    let tip = read("main-tip.yml");
    let group = block(&tip, "concurrency").join(" ");

    assert!(
        group.contains("github.ref") && group.contains("cancel-in-progress: true"),
        "`main-tip.yml` runs the full Windows suite on the owner's desktop, so a \
         newer tip of `main` cancels an older one: history is linear and the \
         newer tree contains the older one's changes. Got {group:?}"
    );
    assert!(
        !group.contains("github.sha"),
        "keyed on the commit as well, nothing would ever supersede anything and a \
         batch of merges would queue one full suite each on the owner's machine. \
         Got {group:?}"
    );

    // And it must actually fire on a merge.
    let on = block(&tip, "on").join(" ");
    assert!(
        on.contains("push") && on.contains("main"),
        "it has to trigger on a push to `main`, which is what a merge is. Got {on:?}"
    );
}

/// ⚠ **Cheap and first.** The compile check must come before anything slow, and
/// must see test targets.
#[test]
fn the_compile_check_comes_before_anything_that_needs_a_pack_or_a_corpus() {
    let tip = read("main-tip.yml");
    let lines = code(&tip);

    let check = lines
        .iter()
        .position(|l| l.contains("cargo check --workspace --all-targets"))
        .expect(
            "`main-tip.yml` must run `cargo check --workspace --all-targets`. \
             ⚠ `--all-targets` is what makes it see a test file, and a test file \
             is where the break that caused this workflow lived — a library that \
             compiles beside a test target that does not is still a red `main`.",
        );

    let tests = lines
        .iter()
        .position(|l| l.contains("cargo test --workspace"))
        .expect("it must also run the tests");
    assert!(
        check < tests,
        "the compile check must come first. It is the cheapest thing that can \
         fail and the one a cancelled run still has time to report: 2m41s on a \
         cold target directory against the ~40 minutes it used to sit behind."
    );

    // Nothing here may reach for the slow half; the corpus is local-gate's.
    assert!(
        !tip.contains("openrage-corpus\\or-asset"),
        "`main-tip.yml` points at the corpus. A compile break does not need \
         one, and reading it here removes the only reason this workflow exists."
    );
}

/// ⚠ **Loud.** A failure has to say what broke, not that something did.
#[test]
fn a_failure_names_the_error_rather_than_only_failing() {
    let tip = read("main-tip.yml");
    assert!(
        tip.contains("GITHUB_STEP_SUMMARY"),
        "a red run must write a summary. A notification saying `Main tip failed` \
         sends somebody to open a job; one naming the error tells them whether it \
         is theirs — which is the whole difference between this and the state \
         where an agent found the break at their first build."
    );
    let mentions = tip.matches("GITHUB_STEP_SUMMARY").count();
    assert!(
        mentions >= 3,
        "both failing paths and at least one success must report, or a green run \
         is indistinguishable from one that did not look. Found {mentions}."
    );
}

/// ⚠ **Not a gate.** It runs after the merge and must stay that way.
#[test]
fn the_tip_check_cannot_block_a_merge() {
    let tip = read("main-tip.yml");
    let on = block(&tip, "on").join(" ");
    assert!(
        !on.contains("pull_request"),
        "`main-tip.yml` must not trigger on a pull request. The runner is one \
         machine that serialises, so a check on every branch stalls a queue of \
         five — and the deep gate already runs per pull request. This exists to \
         detect, loudly and quickly, not to block. Got {on:?}"
    );
}

/// ⚠ The two workflows must not cancel each other.
///
/// With the group keyed on the ref alone, a merge to `main` and the pull request
/// that produced it shared one, so merging killed the PR run that was about to
/// report.
#[test]
fn a_merge_and_its_pull_request_no_longer_cancel_each_other() {
    let gate = read("local-gate.yml");
    let group = block(&gate, "concurrency").join(" ");

    assert!(
        group.contains("github.event_name"),
        "`local-gate.yml`'s concurrency group must distinguish a push from a \
         pull request. Sharing one group means a merge cancels the run for the \
         branch it merged. Got {group:?}"
    );

    let tip_group = block(&read("main-tip.yml"), "concurrency").join(" ");
    assert_ne!(
        group, tip_group,
        "the two workflows must not share a concurrency group, or the cheap check \
         is cancelled by the expensive one"
    );
}

/// The deep gate still looks at `main`.
///
/// ⚠ Its corpus half is the only automatic check on a moved offset.
#[test]
fn the_deep_gate_still_covers_what_only_it_can() {
    let gate = read("local-gate.yml");
    let on = block(&gate, "on").join(" ");
    assert!(
        on.contains("pull_request"),
        "the deep gate is the per-branch check and must stay one"
    );
    assert!(
        on.contains("push") && on.contains("main"),
        "and it must still look at `main`, because the corpus suites run nowhere \
         else"
    );
    assert!(
        gate.contains("cargo test -p or-io-verify"),
        "`or-io-verify` on the corpus, the recorded testimony among it, is what \
         only the deep gate can run. If it has moved, say where — `main-tip.yml` \
         cannot host it without becoming the slow job it was written to get \
         ahead of."
    );
}

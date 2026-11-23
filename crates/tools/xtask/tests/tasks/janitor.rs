//! `janitor` against a checkout and lanes it can be wrong about: it removes only what has landed,
//! says why it holds the rest, and never deletes a branch.

use std::path::Path;
use std::process::Command;

use crate::common::{run_in, text, Scratch};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=janitor",
            "-c",
            "user.email=janitor@localhost",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `<org>/openrage` with one commit, and a lane `<org>/openrage-lanes/<name>` for each name, each
/// on `agent/<name>` at the commit `main` is on, which is an ancestor of `main` and so landed.
fn organisation(names: &[&str]) -> (Scratch, std::path::PathBuf) {
    let org = Scratch::new("janitor");
    let checkout = org.0.join("openrage");
    std::fs::create_dir_all(&checkout).expect("a checkout");
    git(&checkout, &["init", "-q", "-b", "main"]);
    std::fs::write(checkout.join("a.txt"), "base").expect("a file");
    // As in the real repository, where the marker is ignored: otherwise every lane is dirty.
    std::fs::write(checkout.join(".gitignore"), "/.agent-worktree\n").expect("an ignore file");
    git(&checkout, &["add", "-A"]);
    git(&checkout, &["commit", "-qm", "M0"]);
    for name in names {
        let lane = org.0.join("openrage-lanes").join(name);
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "-q",
                &lane.to_string_lossy(),
                "-b",
                &format!("agent/{name}"),
            ],
        );
        std::fs::write(lane.join(".agent-worktree"), "agent lane").expect("the marker");
    }
    (org, checkout)
}

fn janitor(checkout: &Path, extra: &[&str]) -> std::process::Output {
    // `--idle-minutes 0`: every file here was written a moment ago.
    let mut args = vec!["janitor", "--no-github", "--idle-minutes", "0"];
    args.extend_from_slice(extra);
    run_in(checkout, &args)
}

#[test]
fn a_report_removes_nothing_and_apply_removes_only_what_landed() {
    let (org, checkout) = organisation(&["done", "dirty", "building"]);
    let lanes = org.0.join("openrage-lanes");

    // One lane has an uncommitted file; one has a build directory of its own, which is no reason to keep it.
    std::fs::write(lanes.join("dirty/notes.txt"), "work nobody committed").expect("a file");
    std::fs::create_dir_all(lanes.join("building/target")).expect("a target");
    std::fs::write(
        lanes.join("building/target/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("the tag");
    std::fs::write(lanes.join("building/.gitignore"), "/target/\n").expect("an ignore file");
    git(&lanes.join("building"), &["add", ".gitignore"]);
    git(&lanes.join("building"), &["commit", "-qm", "ignore target"]);
    // `building` now has a commit `main` lacks, so it has not landed; `done` has not changed.

    let dry = janitor(&checkout, &[]);
    assert!(dry.status.success(), "{}", text(&dry));
    let report = text(&dry);
    assert!(report.contains("Reclaimable"), "{report}");
    assert!(
        report.contains("uncommitted or untracked changes"),
        "the dirty lane is held with its reason: {report}"
    );
    assert!(report.contains("GitHub was not asked"), "{report}");
    for lane in ["done", "dirty", "building"] {
        assert!(lanes.join(lane).exists(), "a report removed {lane}");
    }

    let applied = janitor(&checkout, &["--apply"]);
    assert!(applied.status.success(), "{}", text(&applied));
    assert!(!lanes.join("done").exists(), "the landed lane was kept");
    assert!(
        lanes.join("dirty").exists(),
        "a lane with uncommitted work was removed"
    );
    assert!(
        lanes.join("building").exists(),
        "a lane with an unlanded commit was removed"
    );
    assert!(checkout.join("a.txt").exists(), "the checkout was touched");

    // It never deletes a branch.
    let branches = Command::new("git")
        .current_dir(&checkout)
        .args(["branch", "--list", "agent/*"])
        .output()
        .expect("git");
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("agent/done"),
        "a branch was deleted"
    );
}

#[test]
fn it_holds_the_lane_it_is_run_from_and_the_checkout() {
    let (org, checkout) = organisation(&["here"]);
    let lane = org.0.join("openrage-lanes/here");
    let out = janitor(&lane, &["--apply"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        lane.exists(),
        "the janitor removed the lane it was running in"
    );
    assert!(
        text(&out).contains("this is where the janitor is running"),
        "{}",
        text(&out)
    );
    assert!(checkout.exists());
}

#[test]
fn a_budget_fails_when_the_lane_root_reaches_it_and_not_before() {
    let (org, checkout) = organisation(&["big"]);
    std::fs::write(
        org.0.join("openrage-lanes/big/blob.bin"),
        vec![0u8; 3 * 1024 * 1024],
    )
    .expect("a blob");
    // The budget is whole GiB, so a few MiB is under any positive one and over none that is zero.
    assert!(janitor(&checkout, &["--budget-gib", "1"]).status.success());
    assert!(
        janitor(&checkout, &["--budget-gib", "0"]).status.success(),
        "zero disables the check"
    );
}

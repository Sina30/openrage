//! The landing detection falsified in both directions against a repository built here, with only
//! GitHub's payload injected. The two recognitions are the defects this exists for; every other
//! case is a way it could delete work that never landed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn g(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str, message: &str) {
    std::fs::write(dir.join(file), text).expect("a file");
    g(dir, &["add", "-A"]);
    g(dir, &["commit", "-qm", message]);
}

fn pr(number: u64, state: &str, oid: &str) -> Pr {
    Pr {
        number,
        state: state.into(),
        head_ref_oid: oid.into(),
    }
}

/// A lookup that runs the real classifier over injected pull requests. ⚠ Not a stub that hands
/// back a status: that has already decided what the sha comparison decides, so a table of stubs
/// can pass while the comparison is broken.
fn lookup(records: Vec<Pr>) -> impl FnMut(&str, &str) -> PrStatus {
    move |_, sha| select_pr_state(&records, sha)
}

struct Heads {
    squashed: String,
    squashed_ahead: String,
    wip: String,
    ffwd: String,
    conflicted: String,
    conflicted_ahead: String,
    abandoned: String,
    in_review: String,
    main: String,
}

fn history(dir: &Path) -> Heads {
    g(dir, &["init", "-q", "-b", "main"]);
    commit(dir, "a.txt", "base", "M0");

    // A branch squash-merged exactly as GitHub does it.
    g(dir, &["checkout", "-qb", "squashed"]);
    commit(dir, "b.txt", "landed", "F1");
    let squashed = g(dir, &["rev-parse", "squashed"]);
    g(dir, &["checkout", "-q", "main"]);
    g(dir, &["merge", "--squash", "squashed"]);
    g(dir, &["commit", "-qm", "squash merge (#1)"]);

    // The same branch, carrying one commit that never reached the pull request.
    g(dir, &["checkout", "-q", "squashed"]);
    commit(dir, "c.txt", "never pushed", "F2");
    let squashed_ahead = g(dir, &["rev-parse", "squashed"]);

    // Genuine unlanded work and no pull request at all.
    g(dir, &["checkout", "-q", "-b", "wip", "main"]);
    commit(dir, "d.txt", "in progress", "W1");
    let wip = g(dir, &["rev-parse", "wip"]);

    // Landed as an ordinary merge commit, so it is an ancestor.
    g(dir, &["checkout", "-q", "-b", "ffwd", "main"]);
    commit(dir, "e.txt", "merged", "E1");
    let ffwd = g(dir, &["rev-parse", "ffwd"]);
    g(dir, &["checkout", "-q", "main"]);
    g(dir, &["merge", "--no-ff", "-qm", "merge ffwd", "ffwd"]);

    // A branch GitHub could not merge: landed by local squash, its pull request closed, and the
    // landing recorded in the subject the squash carries.
    g(dir, &["checkout", "-q", "-b", "conflicted", "main"]);
    commit(dir, "f.txt", "landed by local squash", "C1");
    let conflicted = g(dir, &["rev-parse", "conflicted"]);
    g(dir, &["checkout", "-q", "main"]);
    g(dir, &["merge", "--squash", "conflicted"]);
    g(
        dir,
        &[
            "commit",
            "-qm",
            "landed locally because GitHub would not merge it (#7)",
        ],
    );
    g(dir, &["checkout", "-q", "conflicted"]);
    commit(dir, "g.txt", "after the landing", "C2");
    let conflicted_ahead = g(dir, &["rev-parse", "conflicted"]);

    g(dir, &["checkout", "-q", "-b", "abandoned", "main"]);
    commit(dir, "h.txt", "never landed", "A1");
    let abandoned = g(dir, &["rev-parse", "abandoned"]);

    g(dir, &["checkout", "-q", "-b", "in-review", "main"]);
    commit(dir, "i.txt", "still in review", "O1");
    let in_review = g(dir, &["rev-parse", "in-review"]);

    Heads {
        squashed,
        squashed_ahead,
        wip,
        ffwd,
        conflicted,
        conflicted_ahead,
        abandoned,
        in_review,
        main: g(dir, &["rev-parse", "main"]),
    }
}

fn state(dir: &Path, h: &Heads, head: &str, branch: &str, lookup: Lookup<'_>) -> MergeState {
    let mut cache = HashMap::new();
    resolve_merge_state(dir, head, branch, &h.main, "main", lookup, &mut cache)
}

#[test]
fn landing_is_recognised_where_it_happened_and_refused_everywhere_else() {
    let repo = tempfile::tempdir().expect("a repository");
    let dir = repo.path();
    let h = history(dir);

    let mut none = lookup(vec![]);
    let mut merged = lookup(vec![pr(1, "MERGED", &h.squashed)]);
    let mut closed_landed = lookup(vec![pr(7, "CLOSED", &h.conflicted)]);
    let mut closed_never = lookup(vec![pr(8, "CLOSED", &h.abandoned)]);
    let mut open = lookup(vec![pr(9, "OPEN", &h.in_review)]);
    let mut offline = |_: &str, _: &str| PrStatus::Unavailable("offline".into());

    // Landed: the ancestor test needs no GitHub at all, and every other way needs both halves.
    let s = state(dir, &h, &h.ffwd, "ffwd", Some(&mut none));
    assert!(
        s.landed && s.how == "ancestor",
        "a merge-commit branch is an ancestor: {}",
        s.reason
    );
    let s = state(dir, &h, &h.ffwd, "ffwd", None);
    assert!(
        s.landed && s.how == "ancestor",
        "an ancestor needs no GitHub"
    );
    let s = state(dir, &h, &h.main, "main", Some(&mut none));
    assert!(
        s.landed && s.how == "ancestor",
        "the reference itself has landed"
    );
    let s = state(dir, &h, &h.squashed, "squashed", Some(&mut merged));
    assert!(
        s.landed && s.how == "PR #1",
        "a squash-merged branch is recognised: {}",
        s.reason
    );
    let s = state(
        dir,
        &h,
        &h.conflicted,
        "conflicted",
        Some(&mut closed_landed),
    );
    assert!(
        s.landed && s.how == "PR #7 (closed)",
        "squashed and closed is recognised: {}",
        s.reason
    );

    // Refused: each is a way this could delete work that never went anywhere.
    for (name, head, branch, which) in [
        (
            "a closed PR the reference never records",
            &h.abandoned,
            "abandoned",
            0,
        ),
        (
            "a commit past a closed landing",
            &h.conflicted_ahead,
            "conflicted",
            1,
        ),
        (
            "a commit past the merged head",
            &h.squashed_ahead,
            "squashed",
            2,
        ),
        ("an open PR with this HEAD", &h.in_review, "in-review", 3),
        ("unlanded work with no PR", &h.wip, "wip", 4),
        ("an unanswerable question", &h.wip, "wip", 5),
        ("a reused name over a landing", &h.wip, "conflicted", 6),
    ] {
        let lookup: Lookup<'_> = match which {
            0 => Some(&mut closed_never),
            1 => Some(&mut closed_landed),
            2 => Some(&mut merged),
            3 => Some(&mut open),
            4 => Some(&mut none),
            5 => Some(&mut offline),
            _ => Some(&mut closed_landed),
        };
        let s = state(dir, &h, head, branch, lookup);
        assert!(!s.landed, "{name} was called landed ({})", s.how);
        assert!(!s.reason.is_empty(), "{name} was held with no reason");
    }
    let s = state(dir, &h, &h.wip, "wip", None);
    assert!(
        !s.landed && s.reason.contains("GitHub was not asked"),
        "unlanded work with no GitHub"
    );
}

#[test]
fn github_is_asked_about_this_branch_and_this_head() {
    let repo = tempfile::tempdir().expect("a repository");
    let h = history(repo.path());
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut recording = |branch: &str, sha: &str| {
        seen.push((branch.to_string(), sha.to_string()));
        PrStatus::Merged(1)
    };
    state(
        repo.path(),
        &h,
        &h.squashed_ahead,
        "squashed",
        Some(&mut recording),
    );
    assert_eq!(
        seen,
        vec![("squashed".to_string(), h.squashed_ahead.clone())]
    );
}

#[test]
fn a_payload_with_no_pull_request_is_none_and_two_pick_one_number() {
    let prs = parse_prs("[]").expect("an empty list");
    assert_eq!(
        select_pr_state(&prs, "abc"),
        PrStatus::NoPr,
        "not a numberless moved"
    );

    let prs = parse_prs(
        r#"[{"number":134,"state":"MERGED","headRefOid":"aaa"},{"number":111,"state":"MERGED","headRefOid":"bbb"}]"#,
    )
    .expect("two pull requests");
    assert_eq!(prs.len(), 2);
    assert_eq!(select_pr_state(&prs, "aaa"), PrStatus::Merged(134));
    assert_eq!(
        select_pr_state(&prs, "zzz"),
        PrStatus::Moved(134),
        "a different commit moved"
    );
}

/// A worktree with an ignored build directory and docs, and a checkout holding a staged copy.
struct Workspace {
    _keep: tempfile::TempDir,
    ws: PathBuf,
    co: PathBuf,
}

fn workspace() -> Workspace {
    let keep = tempfile::tempdir().expect("a folder");
    let (ws, co) = (keep.path().join("ws"), keep.path().join("co"));
    for dir in [&ws, &co] {
        std::fs::create_dir_all(dir).expect("a folder");
    }
    g(&ws, &["init", "-q", "-b", "main"]);
    commit(
        &ws,
        ".gitignore",
        "/target/\n/docs/extra/\n/.agent-worktree\n",
        "M0",
    );

    std::fs::write(ws.join(".agent-worktree"), "agent lane").expect("the marker");
    std::fs::create_dir_all(ws.join("target")).expect("target");
    std::fs::write(ws.join("target/CACHEDIR.TAG"), CARGO_TAG).expect("the tag");
    std::fs::write(ws.join("target/big.rlib"), "x".repeat(4096)).expect("build output");
    std::fs::create_dir_all(ws.join("docs/extra")).expect("docs");
    std::fs::create_dir_all(co.join("docs/extra")).expect("docs");
    std::fs::write(co.join("docs/extra/Shared.md"), "staged from the checkout").expect("staged");
    Workspace {
        _keep: keep,
        ws,
        co,
    }
}

#[test]
fn what_no_branch_carries_is_held_and_what_it_does_not_is_not() {
    let w = workspace();
    let reason = |max: usize| ignored_work_reason(&w.ws, &w.co, max);
    let authored = w.ws.join("docs/extra/YND_File_Structure_Specification.md");
    let staged = w.ws.join("docs/extra/Shared.md");

    assert_eq!(reason(200), "", "build output and the marker hold nothing");

    std::fs::write(&authored, "a day of work, in no commit").expect("authored");
    assert!(
        reason(200).contains("YND_File_Structure_Specification.md"),
        "an authored spec holds"
    );

    std::fs::remove_file(&authored).expect("remove");
    std::fs::copy(w.co.join("docs/extra/Shared.md"), &staged).expect("a staged copy");
    assert_eq!(
        reason(200),
        "",
        "a byte-identical staged copy holds nothing"
    );

    std::fs::write(&staged, "staged from the checkout and a local edit").expect("edited");
    assert!(
        reason(200).contains("Shared.md"),
        "a staged copy edited here holds"
    );

    std::fs::remove_file(&staged).expect("remove");
    std::fs::remove_file(w.ws.join("target/CACHEDIR.TAG")).expect("remove the tag");
    assert!(
        reason(200).contains("target/big.rlib"),
        "a target without CACHEDIR.TAG is not build output"
    );

    std::fs::write(w.ws.join("target/.rustc_info.json"), "{}").expect("a maintained marker");
    assert_eq!(
        reason(200),
        "",
        "a target cargo did not stamp is still build output"
    );

    for n in 1..=4 {
        std::fs::write(w.ws.join(format!("docs/extra/part{n}.md")), "x").expect("a part");
    }
    assert!(
        reason(2).contains("at least"),
        "a walk that stopped at its cap says so"
    );
}

#[test]
fn a_plain_file_in_the_lane_root_is_reported_and_a_live_lane_is_not_residue() {
    let root = tempfile::tempdir().expect("a lane root");
    let lane = root.path().join("a-lane");
    std::fs::create_dir_all(&lane).expect("a lane");
    let stray = root.path().join("stray-run.log");
    std::fs::write(&stray, "written beside the lanes").expect("a stray file");

    let (dirs, files) = lane_residue(root.path(), std::slice::from_ref(&lane), 60);
    assert!(dirs.is_empty(), "a live lane is not residue: {dirs:?}");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, stray);
}

#[test]
fn a_reference_records_the_pull_requests_it_landed() {
    let repo = tempfile::tempdir().expect("a repository");
    let h = history(repo.path());
    let mut cache: HashMap<String, HashSet<u64>> = HashMap::new();
    assert!(references_pr(repo.path(), &h.main, 1, &mut cache));
    assert!(references_pr(repo.path(), &h.main, 7, &mut cache));
    assert!(
        !references_pr(repo.path(), &h.main, 8, &mut cache),
        "closed for being superseded"
    );
}

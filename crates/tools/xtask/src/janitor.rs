//! Remove finished agent worktrees and their build output, and say how much that freed.
//!
//! Reports by default and deletes only with `--apply`, so the list can be read before it is acted
//! on. A worktree is removed only when its branch has landed on the reference, it holds nothing
//! no branch carries, nothing has written to it recently and git has not locked it. Anything else
//! is held and listed with the reason. It never deletes a branch.
//!
//! ⚠ Landed is not "is an ancestor". Every pull request here is squash-merged, so the commit that
//! lands has a different sha from anything on the branch and `git merge-base --is-ancestor` is
//! false forever; measured 2026-09-22, it recognised none of thirteen lanes of which twelve had a
//! merged pull request. Landing is judged twice: the ancestor test, then GitHub, asked whether a
//! pull request recorded this worktree's exact HEAD as its head commit, and what became of it.
//!
//! ⚠ The sha has to match exactly, and that is the safety property and not a detail: matching on
//! the branch name alone would call a reused name landed and delete a lane whose work never went
//! anywhere. A closed pull request whose head matches is read beside the reference's own history,
//! which records a local squash as a `(#N)` suffix, because GitHub will not merge a conflicting
//! pull request and AGENTS.md lands one by local squash and closes it.
//!
//! Every question this cannot settle is answered "no": holding a lane costs a directory nobody
//! reclaimed, and removing one that still holds unlanded work costs the work.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::git;

/// What cargo writes into `CACHEDIR.TAG`, which is how its build output is told from a folder
/// somebody merely named `target`.
const CARGO_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55";

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub struct Options {
    apply: bool,
    budget_gib: u64,
    idle_minutes: u64,
    reference: Option<String>,
    lane_root: Option<PathBuf>,
    no_github: bool,
}

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut options = Options {
        apply: false,
        budget_gib: 0,
        idle_minutes: 60,
        reference: None,
        lane_root: None,
        no_github: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| -> Result<String> {
            Ok(it
                .next()
                .with_context(|| format!("{flag} needs a value"))?
                .clone())
        };
        match arg.as_str() {
            "--apply" => options.apply = true,
            "--no-github" => options.no_github = true,
            "--budget-gib" => options.budget_gib = value("--budget-gib")?.parse()?,
            "--idle-minutes" => options.idle_minutes = value("--idle-minutes")?.parse()?,
            "--reference" => options.reference = Some(value("--reference")?),
            "--lane-root" => options.lane_root = Some(PathBuf::from(value("--lane-root")?)),
            other => bail!("janitor: unknown argument `{other}`"),
        }
    }
    sweep(&options)
}

// ---------------------------------------------------------------- worktrees

struct Worktree {
    path: PathBuf,
    head: String,
    branch: String,
    locked: bool,
    prunable: String,
}

fn worktrees(root: &Path) -> Result<Vec<Worktree>> {
    let listing = git::run(root, &["worktree", "list", "--porcelain"])?;
    let mut out: Vec<Worktree> = Vec::new();
    for line in listing.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            out.push(Worktree {
                path: PathBuf::from(path),
                head: String::new(),
                branch: String::new(),
                locked: false,
                prunable: String::new(),
            });
        } else if let Some(last) = out.last_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                last.head = head.to_string();
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                last.branch = branch.to_string();
            } else if line.starts_with("locked") {
                last.locked = true;
            } else if let Some(why) = line.strip_prefix("prunable ") {
                last.prunable = why.to_string();
            }
        }
    }
    Ok(out)
}

/// Two paths name the same place: compared resolved where both exist, and without regard to
/// case on Windows, where the tools disagree about it.
fn same_path(a: &Path, b: &Path) -> bool {
    let resolve = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (resolve(a), resolve(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn under(path: &Path, root: &Path) -> bool {
    let resolve = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    resolve(path).starts_with(resolve(root))
}

// ------------------------------------------------------------------ sizing

fn tree_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| tree_bytes(&e.path())).sum()
}

fn gib(bytes: u64) -> f64 {
    (bytes as f64 / GIB * 100.0).round() / 100.0
}

/// The newest modification time under `path`, to the depth the idle test looks.
fn newest(path: &Path, depth: usize) -> Option<SystemTime> {
    let mut best: Option<SystemTime> = None;
    let Ok(entries) = std::fs::read_dir(path) else {
        return None;
    };
    for entry in entries.flatten() {
        if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
            best = best.max(Some(modified));
        }
        if depth > 0 && entry.file_type().is_ok_and(|t| t.is_dir()) {
            best = best.max(newest(&entry.path(), depth - 1));
        }
    }
    best
}

/// Nothing under `path` has been written within `minutes`.
fn is_idle(path: &Path, minutes: u64) -> bool {
    let cut = SystemTime::now()
        .checked_sub(Duration::from_secs(minutes * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    newest(path, 3).is_none_or(|time| time < cut)
}

// ------------------------------------------------------------ build output

fn is_cargo_target(path: &Path) -> bool {
    std::fs::read_to_string(path.join("CACHEDIR.TAG"))
        .ok()
        .and_then(|text| text.lines().next().map(str::to_string))
        .is_some_and(|first| first.trim_end() == CARGO_TAG)
}

/// Is this directory cargo's build output?
///
/// ⚠ `CACHEDIR.TAG` alone cannot answer it: cargo stamps that file once, when it creates the
/// target directory, and skips it when one already exists, so a `target/` something else made
/// first never gets one. `.rustc_info.json` and `<profile>/.fingerprint` are kept by every build
/// and are answerable on a directory somebody else created. Not folded into
/// [`is_cargo_target`]: the orphan scan says it identifies a directory by cargo's signature, and
/// that claim is about a directory this will never delete.
fn is_build_output(path: &Path) -> bool {
    is_cargo_target(path)
        || path.join(".rustc_info.json").exists()
        || ["debug", "release"]
            .iter()
            .any(|profile| path.join(profile).join(".fingerprint").exists())
}

// ------------------------------------------------------------ ignored work

/// Why a worktree is dirty, if `git status` says so; "cannot read" is dirty, not clean.
fn dirty_reason(path: &Path) -> String {
    match git::output(path, &["status", "--porcelain"]) {
        Ok(out) if out.status.success() => {
            if out.stdout.is_empty() {
                String::new()
            } else {
                "uncommitted or untracked changes".into()
            }
        }
        _ => "git cannot read a status here".into(),
    }
}

fn walk_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_file() {
        out.push(path.to_path_buf());
    } else if meta.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                walk_files(&entry.path(), out);
            }
        }
    }
}

/// The same file in the checkout, byte for byte: a staged copy and not something authored here.
fn has_identical_twin(file: &Path, twin: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(file), std::fs::metadata(twin)) else {
        return false;
    };
    a.len() == b.len()
        && matches!((std::fs::read(file), std::fs::read(twin)), (Ok(x), Ok(y)) if x == y)
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / (1 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

/// What does this worktree hold that no branch carries?
///
/// ⚠ Removal is safe because a removed worktree's work is already on the reference, which is true
/// of tracked content and nothing else. An ignored file is in no commit and on no remote, so
/// removing the worktree destroys it, and `git status --porcelain` omits ignored files by design.
/// Measured 2026-09-22: two live lanes each held a format specification under `docs/extra/` in
/// zero commits, both reported a clean tree, and an hour of inactivity was the whole protection.
fn ignored_work_reason(path: &Path, checkout: &Path, max_files: usize) -> String {
    let out = match git::output(
        path,
        &[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain",
            "--ignored",
            "-z",
        ],
    ) {
        Ok(out) if out.status.success() => out,
        _ => return "git cannot list ignored files here".into(),
    };

    let mut files: Vec<(String, u64)> = Vec::new();
    let mut capped = false;
    'entries: for record in out.stdout.split(|b| *b == 0) {
        let Some(rest) = record.strip_prefix(b"!! ") else {
            continue;
        };
        let rel = String::from_utf8_lossy(rest)
            .trim_end_matches('/')
            .to_string();
        if rel == ".agent-worktree" {
            continue;
        }
        let full = path.join(&rel);
        if !full.exists() {
            continue;
        }
        if full.is_dir() && is_build_output(&full) {
            continue;
        }
        let mut found = Vec::new();
        walk_files(&full, &mut found);
        for file in found {
            if files.len() >= max_files {
                capped = true;
                break 'entries;
            }
            let under = file.strip_prefix(path).unwrap_or(&file);
            if has_identical_twin(&file, &checkout.join(under)) {
                continue;
            }
            let bytes = std::fs::metadata(&file).map_or(0, |m| m.len());
            files.push((under.to_string_lossy().replace('\\', "/"), bytes));
        }
    }
    if files.is_empty() {
        return String::new();
    }

    let total: u64 = files.iter().map(|(_, bytes)| bytes).sum();
    // ⚠ Both figures are floors once the walk stopped at the cap, and a floor printed as a total
    // is a measurement that overstates what it looked at.
    let least = if capped { "at least " } else { "" };
    let noun = if files.len() == 1 { "file" } else { "files" };
    let named: Vec<&str> = files
        .iter()
        .take(2)
        .map(|(name, _)| name.as_str())
        .collect();
    let rest = files.len() - named.len();
    let tail = if rest > 0 {
        format!(" (+{least}{rest} more)")
    } else {
        String::new()
    };
    format!(
        "holds {least}{} ignored {noun} no branch carries, {least}{}: {}{tail}",
        files.len(),
        size_text(total),
        named.join(", ")
    )
}

// ---------------------------------------------------------------- landing

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    Merged(u64),
    Closed(u64),
    Open(u64),
    /// The branch name belongs to a pull request and this commit does not.
    Moved(u64),
    NoPr,
    Unavailable(String),
}

pub struct Pr {
    pub number: u64,
    pub state: String,
    pub head_ref_oid: String,
}

/// gh's JSON as a flat list of pull requests.
fn parse_prs(json: &str) -> Result<Vec<Pr>> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(json)?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Pr {
                number: p["number"].as_u64()?,
                state: p["state"].as_str()?.to_string(),
                head_ref_oid: p["headRefOid"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// What do this branch's pull requests say about this exact commit?
///
/// ⚠ Separate from the query that fetches them so the tests exercise this and not a stub of it:
/// the sha comparison is the safety property, and a stub that hands back a status has already
/// made the decision this makes.
fn select_pr_state(prs: &[Pr], sha: &str) -> PrStatus {
    let mine: Vec<&Pr> = prs.iter().filter(|p| p.head_ref_oid == sha).collect();
    for state in ["MERGED", "CLOSED", "OPEN"] {
        if let Some(p) = mine.iter().find(|p| p.state == state) {
            return match state {
                "MERGED" => PrStatus::Merged(p.number),
                "CLOSED" => PrStatus::Closed(p.number),
                _ => PrStatus::Open(p.number),
            };
        }
    }
    match prs.first() {
        Some(p) => PrStatus::Moved(p.number),
        None => PrStatus::NoPr,
    }
}

pub struct MergeState {
    landed: bool,
    how: String,
    reason: String,
}

fn held(reason: String) -> MergeState {
    MergeState {
        landed: false,
        how: String::new(),
        reason,
    }
}

fn landed(how: String) -> MergeState {
    MergeState {
        landed: true,
        how,
        reason: String::new(),
    }
}

/// The pull requests the reference's own history says it landed: a squash writes `(#N)` at the end
/// of the subject, GitHub's and a local one alike. That suffix is the merging process's own marker,
/// sitting on the reference where nothing but another commit can put it, unlike a pull-request
/// comment, which anyone can write about work that never landed.
fn references_pr(
    repo: &Path,
    ref_sha: &str,
    number: u64,
    cache: &mut HashMap<String, HashSet<u64>>,
) -> bool {
    let seen = cache.entry(ref_sha.to_string()).or_insert_with(|| {
        let subjects = git::run(repo, &["log", ref_sha, "--format=%s"]).unwrap_or_default();
        let pattern = Regex::new(r"\(#(\d+)\)\s*$").expect("a valid expression");
        subjects
            .lines()
            .filter_map(|s| pattern.captures(s).and_then(|c| c[1].parse().ok()))
            .collect()
    });
    seen.contains(&number)
}

type Lookup<'a> = Option<&'a mut dyn FnMut(&str, &str) -> PrStatus>;

/// Has this branch landed on the reference? Answers "no" for every question it cannot settle.
fn resolve_merge_state(
    repo: &Path,
    head: &str,
    branch: &str,
    ref_sha: &str,
    reference: &str,
    lookup: Lookup<'_>,
    cache: &mut HashMap<String, HashSet<u64>>,
) -> MergeState {
    let ancestor = git::output(repo, &["merge-base", "--is-ancestor", head, ref_sha])
        .is_ok_and(|out| out.status.success());
    if ancestor {
        return landed("ancestor".into());
    }
    let Some(lookup) = lookup else {
        return held(format!("not on {reference}, and GitHub was not asked"));
    };
    match lookup(branch, head) {
        PrStatus::Merged(n) => landed(format!("PR #{n}")),
        // Landed by local squash and closed. ⚠ Closed proves nothing alone (a superseded pull
        // request is closed too), so the reference has to record the landing as well.
        PrStatus::Closed(n) => {
            if references_pr(repo, ref_sha, n, cache) {
                landed(format!("PR #{n} (closed)"))
            } else {
                held(format!("PR #{n} was closed without merging, and no commit on {reference} records it landing"))
            }
        }
        PrStatus::Open(n) => held(format!("PR #{n} has this HEAD and is still open")),
        PrStatus::Moved(n) => held(format!(
            "PR #{n} records a different commit -- this HEAD did not land"
        )),
        PrStatus::NoPr => held(format!("not on {reference}, and no PR has this HEAD")),
        PrStatus::Unavailable(_) => {
            held(format!("not on {reference}, and GitHub could not be asked"))
        }
    }
}

/// Asks `gh`, once it has been found ready.
struct GitHub {
    state: String,
    asked: usize,
}

impl GitHub {
    fn new(disabled: bool) -> Self {
        let state = if disabled {
            "disabled with --no-github".to_string()
        } else if Command::new("gh").arg("--version").output().is_err() {
            "the gh CLI is not on PATH".to_string()
        } else if !Command::new("gh")
            .args(["auth", "status"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            "gh is not authenticated -- run `gh auth login`".to_string()
        } else {
            "ready".to_string()
        };
        GitHub { state, asked: 0 }
    }

    fn ready(&self) -> bool {
        self.state == "ready"
    }

    /// Asked per branch and not by listing every pull request: a list takes a limit, and a limit
    /// that elides an old one reports the same as one that never existed. Every state, because a
    /// pull request GitHub could not merge is landed locally and closed.
    fn pr_for(&mut self, repo: &Path, branch: &str, sha: &str) -> PrStatus {
        if !self.ready() {
            return PrStatus::Unavailable(self.state.clone());
        }
        self.asked += 1;
        let out = Command::new("gh")
            .current_dir(repo)
            .args([
                "pr", "list", "--state", "all", "--head", branch, "--limit", "20",
            ])
            .args(["--json", "number,state,headRefOid"])
            .output();
        match out {
            Ok(out) if out.status.success() => {
                match parse_prs(&String::from_utf8_lossy(&out.stdout)) {
                    Ok(prs) => select_pr_state(&prs, sha),
                    Err(_) => PrStatus::Unavailable("gh printed something unreadable".into()),
                }
            }
            _ => {
                self.state = "the gh query failed -- offline?".into();
                PrStatus::Unavailable(self.state.clone())
            }
        }
    }
}

// ------------------------------------------------------------------ report

struct Row {
    path: PathBuf,
    branch: String,
    gib: f64,
    note: String,
    misplaced: bool,
    inside: bool,
}

/// What sits directly under the lane root that is not a live worktree.
fn lane_residue(
    root: &Path,
    live: &[PathBuf],
    minutes: u64,
) -> (Vec<PathBuf>, Vec<(PathBuf, u64)>) {
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(root) else {
        return (dirs, files);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if live.iter().any(|l| same_path(l, &path)) || !is_idle(&path, minutes) {
                continue;
            }
            dirs.push(path);
        } else if kind.is_file() {
            // ⚠ A plain file is never a lane, so the rule that makes everything here deletable
            // does not reach it: reported whatever its age, never removed.
            files.push((path.clone(), entry.metadata().map_or(0, |m| m.len())));
        }
    }
    (dirs, files)
}

fn remove_tree(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn sweep(options: &Options) -> Result<ExitCode> {
    let here = git::here()?;
    let all = worktrees(&here)?;
    // `rev-parse --show-toplevel` in a worktree names that worktree, not the checkout the janitor
    // must never remove: the first entry of the list is the checkout.
    let checkout = all.first().context("git lists no worktree")?.path.clone();
    let org_folder = checkout
        .parent()
        .context("the checkout has no parent folder")?
        .to_path_buf();
    let lane_root = options
        .lane_root
        .clone()
        .unwrap_or_else(|| org_folder.join("openrage-lanes"));

    let reference = options.reference.clone().unwrap_or_else(|| {
        if git::output(&here, &["rev-parse", "--verify", "--quiet", "origin/main"])
            .is_ok_and(|o| o.status.success())
        {
            "origin/main".into()
        } else {
            "main".into()
        }
    });
    let ref_sha = git::run(&here, &["rev-parse", &reference])
        .with_context(|| format!("cannot resolve {reference}"))?
        .trim()
        .to_string();

    let mut github = GitHub::new(options.no_github);
    let mut cache: HashMap<String, HashSet<u64>> = HashMap::new();

    let mut removed: Vec<Row> = Vec::new();
    let mut held_rows: Vec<Row> = Vec::new();
    let mut claimed: BTreeSet<PathBuf> = BTreeSet::new();

    for wt in &all {
        let mut reason: String;
        let mut landed_by = String::new();

        if same_path(&wt.path, &checkout) {
            reason = "the checkout itself".into();
        } else if same_path(&wt.path, &here) {
            reason = "this is where the janitor is running".into();
        } else if !wt.path.exists() {
            reason = "gone from disk (prune will drop it)".into();
        } else if wt.locked {
            reason = "git worktree lock".into();
        } else if !wt.prunable.is_empty() {
            reason = format!("prunable -- {}", wt.prunable);
        } else if wt.branch.is_empty() {
            reason = "detached HEAD -- no branch would outlive it".into();
        } else {
            reason = dirty_reason(&wt.path);
            if reason.is_empty() {
                reason = ignored_work_reason(&wt.path, &checkout, 200);
            }
            if reason.is_empty() && !is_idle(&wt.path, options.idle_minutes) {
                reason = format!("written to within {} min", options.idle_minutes);
            }
            if reason.is_empty() {
                let mut ask = |branch: &str, sha: &str| github.pr_for(&here, branch, sha);
                let state = resolve_merge_state(
                    &here,
                    &wt.head,
                    &wt.branch,
                    &ref_sha,
                    &reference,
                    Some(&mut ask),
                    &mut cache,
                );
                if state.landed {
                    landed_by = state.how;
                } else {
                    reason = state.reason;
                }
            }
        }

        // A build directory that used to sit beside the lane, which a lane's own target replaced.
        let mut siblings: Vec<PathBuf> = Vec::new();
        if reason.is_empty() {
            let parent = wt.path.parent().unwrap_or(Path::new("."));
            let leaf = wt.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            for shape in [
                format!("{leaf}-target"),
                format!("target-{leaf}"),
                format!(".target-{leaf}"),
            ] {
                let candidate = parent.join(shape);
                if candidate.exists()
                    && is_cargo_target(&candidate)
                    && is_idle(&candidate, options.idle_minutes)
                {
                    claimed.insert(
                        std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone()),
                    );
                    siblings.push(candidate);
                }
            }
        }

        let bytes = tree_bytes(&wt.path) + siblings.iter().map(|s| tree_bytes(s)).sum::<u64>();
        let misplaced =
            !same_path(&wt.path, &checkout) && !under(&wt.path, &lane_root) && wt.path.exists();
        let inside = misplaced && under(&wt.path, &checkout);

        let row = Row {
            path: wt.path.clone(),
            branch: wt.branch.clone(),
            gib: gib(bytes),
            note: if reason.is_empty() {
                landed_by
            } else {
                reason.clone()
            },
            misplaced,
            inside,
        };
        if reason.is_empty() {
            if options.apply {
                remove_tree(&wt.path);
                siblings.iter().for_each(|s| remove_tree(s));
            }
            removed.push(row);
        } else {
            held_rows.push(row);
        }
    }

    if options.apply {
        let _ = git::run(&here, &["worktree", "prune"]);
    }

    let live: Vec<PathBuf> = all.iter().map(|w| w.path.clone()).collect();
    let (residue_dirs, stray_files) = lane_residue(&lane_root, &live, options.idle_minutes);
    let mut residue_gib = 0.0;
    for dir in &residue_dirs {
        residue_gib += gib(tree_bytes(dir));
        if options.apply {
            remove_tree(dir);
        }
    }

    // Build output beside the lanes that no worktree claims: reported, never deleted, and only when
    // it carries cargo's own signature, because the organisation's folder holds unrelated projects.
    let name = Regex::new(r"(^\.?target|-target$)").expect("a valid expression");
    let mut orphans: Vec<(PathBuf, f64, bool)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&org_folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_dir() || !name.is_match(&file_name) || !is_cargo_target(&path) {
                continue;
            }
            if live.iter().any(|l| same_path(l, &path)) {
                continue;
            }
            if claimed.contains(&std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone())) {
                continue;
            }
            orphans.push((
                path.clone(),
                gib(tree_bytes(&path)),
                is_idle(&path, options.idle_minutes),
            ));
        }
    }

    print_report(
        options,
        &reference,
        &lane_root,
        &removed,
        &held_rows,
        residue_gib,
        &residue_dirs,
        &stray_files,
        &orphans,
        &github,
    );

    let lane_gib = gib(tree_bytes(&lane_root));
    println!("Lane root holds {lane_gib:.2} GiB.");
    if options.budget_gib > 0 && lane_gib >= options.budget_gib as f64 {
        println!();
        eprintln!(
            "error: the lane root holds {lane_gib:.2} GiB, at or over the {} GiB budget.",
            options.budget_gib
        );
        println!(
            "Run `just janitor` to reclaim what is finished, or raise the budget deliberately."
        );
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn print_report(
    options: &Options,
    reference: &str,
    lane_root: &Path,
    removed: &[Row],
    held_rows: &[Row],
    residue_gib: f64,
    residue_dirs: &[PathBuf],
    stray_files: &[(PathBuf, u64)],
    orphans: &[(PathBuf, f64, bool)],
    github: &GitHub,
) {
    let table = |rows: &[Row]| {
        let mut sorted: Vec<&Row> = rows.iter().collect();
        sorted.sort_by(|a, b| b.gib.total_cmp(&a.gib));
        for row in sorted {
            let place = if row.misplaced {
                "  [outside the lane root]"
            } else {
                ""
            };
            println!(
                "  {:>7.2} GiB  {:<28} {}{place}",
                row.gib,
                row.branch,
                row.path.display()
            );
            if !row.note.is_empty() {
                println!("              {}", row.note);
            }
        }
    };

    println!();
    println!(
        "=== worktree janitor ===  reference {reference}, idle threshold {} min",
        options.idle_minutes
    );
    println!("lane root: {}", lane_root.display());

    let verb = if options.apply {
        "Removed"
    } else {
        "Reclaimable"
    };
    if removed.is_empty() {
        println!("\nNothing to reclaim.");
    } else {
        println!("\n{verb} ({}):", removed.len());
        table(removed);
    }
    if !residue_dirs.is_empty() {
        println!(
            "\nLane residue under the lane root ({}):",
            residue_dirs.len()
        );
        for dir in residue_dirs {
            println!("  {}", dir.display());
        }
    }
    if !stray_files.is_empty() {
        println!(
            "\nStray files in the lane root -- reported, not deleted ({}):",
            stray_files.len()
        );
        for (path, bytes) in stray_files {
            println!("  {:>8.1} KiB  {}", *bytes as f64 / 1024.0, path.display());
        }
        println!("  The lane root holds lanes; a file belongs inside a lane or in the temporary folder. Check and remove by hand.");
    }
    if !held_rows.is_empty() {
        println!("\nHeld ({}):", held_rows.len());
        table(held_rows);
    }

    if github.ready() {
        println!(
            "\nLanding judged against {reference} and GitHub; {} branch(es) asked.",
            github.asked
        );
    } else {
        println!("\nGitHub was not asked: {}.", github.state);
        println!("  Landing fell back to the ancestor test, which a squash merge never satisfies,");
        println!("  so any lane above held for not being on {reference} may in fact be finished.");
    }

    let outside: Vec<&Row> = removed
        .iter()
        .chain(held_rows)
        .filter(|r| r.misplaced)
        .collect();
    if !outside.is_empty() {
        println!("\nWARNING -- outside the lane root ({}):", outside.len());
        for row in &outside {
            let note = if row.inside {
                "  [inside the checkout]"
            } else {
                ""
            };
            println!("    {}{note}", row.path.display());
        }
        println!(
            "  A lane belongs under {}, created by `just agent-worktree`.",
            lane_root.display()
        );
        println!("  Anywhere else is in the organisation's folder beside unrelated projects,");
        println!("  where nothing enumerates it and only a name someone recognises finds it.");
    }
    if !orphans.is_empty() {
        println!(
            "\nOrphaned target directories -- reported, not deleted ({}):",
            orphans.len()
        );
        for (path, size, idle) in orphans {
            println!("  {size:>7.2} GiB  idle={idle}  {}", path.display());
        }
        println!("  No worktree claims these. Each carries cargo's CACHEDIR.TAG, so each is build");
        println!("  output -- but the janitor will not delete what it cannot tie to a worktree of");
        println!("  this repository. Check and remove by hand.");
    }

    let reclaim: f64 = removed.iter().map(|r| r.gib).sum::<f64>() + residue_gib;
    if options.apply {
        println!("\nReclaimed {reclaim:.2} GiB.");
        let further: f64 = orphans.iter().map(|o| o.1).sum();
        if further > 0.0 {
            println!(
                "A further {further:.2} GiB sits in orphaned target directories listed above."
            );
        }
    } else {
        let all: f64 = reclaim + orphans.iter().map(|o| o.1).sum::<f64>();
        println!("\nReclaimable {all:.2} GiB. Run `just janitor` to reclaim it.");
    }
}

#[cfg(test)]
mod tests;

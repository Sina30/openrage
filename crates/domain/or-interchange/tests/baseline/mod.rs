//! Behaviour baselines: a digest of what a consumer makes of each corpus
//! file, recorded from one `main` revision by `just baselines-record` into
//! `openrage-corpus/baselines` and compared on every later run.
//!
//! ⚠ Shared by path with the other consumer crates' suites, so every item
//! here has a caller in each of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the records are; otherwise `openrage-corpus/baselines` beside or
/// above the workspace. Named explicitly, a missing record is a failure.
pub const DIR_ENV: &str = "OPENRAGE_BASELINES";

/// Set to the `main` revision the lane is based on, it writes the records
/// instead of comparing against them. Only `just baselines-record` sets it.
pub const RECORD_ENV: &str = "OPENRAGE_BASELINES_RECORD";

/// Messages `or-io` refuses an owned open with, which a consumer moved onto
/// owned values may now give where the record holds a digest.
pub const NAMED_REFUSALS: &[(&str, &str)] = &[
    (
        "a null pointer the game never leaves null",
        "the game never leaves",
    ),
    (
        "a live pointer beside a stated length of zero",
        "the length its owner states is zero",
    ),
    (
        "a live pointer beside a zero-byte payload",
        "zero-byte payload beside a live pointer",
    ),
    (
        "two pointers at one allocation",
        "a pointer at a shared target",
    ),
    ("a codec's blob", "a codec's blob"),
];

/// A recorded entry whose record holds wrong output, and why it was wrong.
pub struct Correction {
    pub kind: &'static str,
    pub entry: &'static str,
    pub target: &'static str,
    /// The line the recorded output printed.
    pub holds: &'static str,
    /// The line the output prints in its place.
    pub right: &'static str,
    pub because: &'static str,
}

/// Entries recorded from a reader since found wrong (OD-P60-CORRECTIONS). A
/// listed entry the output does not correct fails the run.
pub const CORRECTED: &[Correction] = &[];

/// What one entry of a record says.
const REFUSED: &str = "refused";

/// A corpus path relative to the corpus root (with `#` and a sub-key where a
/// file holds several entries), and its target.
pub type Key = (String, String);

/// What the code under test made of one entry.
#[derive(Debug, Clone)]
pub enum Now {
    Digest(String),
    Refused(String),
}

/// FNV-1a over 128 bits. It needs no dependency, and a regression digest has
/// no adversary.
pub fn digest(bytes: &[u8]) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")
}

/// The key a corpus file is recorded under.
pub fn key(root: &Path, path: &Path, sub: Option<&str>, target: &str) -> Key {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut entry = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if let Some(sub) = sub {
        entry.push('#');
        entry.push_str(sub);
    }
    (entry, target.to_string())
}

pub fn recording() -> Option<String> {
    std::env::var(RECORD_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

fn directory(manifest_dir: &str) -> (Option<PathBuf>, bool) {
    if let Some(raw) = std::env::var_os(DIR_ENV).filter(|v| !v.is_empty()) {
        return (Some(PathBuf::from(raw)), true);
    }
    let found = or_test_support::workspace_root(manifest_dir).and_then(|root| {
        root.ancestors()
            .skip(1)
            .map(|dir| dir.join("openrage-corpus").join("baselines"))
            .find(|dir| dir.is_dir())
    });
    (found, false)
}

fn render(kind: &str, revision: &str, entries: &BTreeMap<Key, Now>) -> String {
    let mut out = format!("# openrage behaviour baseline\n# kind {kind}\n# revision {revision}\n");
    for ((entry, target), now) in entries {
        let value = match now {
            Now::Digest(d) => d.as_str(),
            Now::Refused(_) => REFUSED,
        };
        out.push_str(&format!("{entry}\t{target}\t{value}\n"));
    }
    out
}

/// A record's revision and entries, or why it is not one.
pub fn parse(text: &str) -> Result<(String, BTreeMap<Key, String>), String> {
    let mut revision = None;
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# revision ") {
            revision = Some(rest.trim().to_string());
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        match (fields.next(), fields.next(), fields.next(), fields.next()) {
            (Some(entry), Some(target), Some(value), None) => {
                entries.insert((entry.to_string(), target.to_string()), value.to_string());
            }
            _ => return Err(format!("not a record line: {line:?}")),
        }
    }
    let revision = revision.ok_or("the record names no revision")?;
    Ok((revision, entries))
}

/// Every difference between a record and what the code made now, each naming
/// its file; and the differences `or-io`'s named refusals and `corrected`
/// account for.
pub fn compare(
    recorded: &BTreeMap<Key, String>,
    now: &BTreeMap<Key, Now>,
    corrected: &[&Correction],
) -> (Vec<String>, Vec<String>) {
    // A file refused whole answers for every entry recorded inside it.
    let file_refused = |entry: &str, target: &str| {
        let (file, _) = entry.split_once('#')?;
        match now.get(&(file.to_string(), target.to_string())) {
            Some(refused @ Now::Refused(_)) => Some(refused),
            _ => None,
        }
    };
    let mut failures = Vec::new();
    let mut expected = Vec::new();
    for c in corrected {
        if !recorded.contains_key(&(c.entry.to_string(), c.target.to_string())) {
            failures.push(format!(
                "{} ({}): listed as corrected, and not in the record",
                c.entry, c.target
            ));
        }
    }
    for ((entry, target), was) in recorded {
        let name = format!("{entry} ({target})");
        let found = now
            .get(&(entry.clone(), target.clone()))
            .or_else(|| file_refused(entry, target));
        if let Some(c) = corrected
            .iter()
            .find(|c| c.entry == entry && c.target == target)
        {
            match found {
                Some(Now::Digest(d)) if d != was => expected.push(format!(
                    "{name}: corrected, the record holds {:?} where the output prints {:?}, \
                     because {}",
                    c.holds, c.right, c.because
                )),
                Some(Now::Digest(_)) => failures.push(format!(
                    "{name}: listed as corrected, and the output is as recorded"
                )),
                Some(Now::Refused(why)) => failures.push(format!(
                    "{name}: listed as corrected, and not corrected now ({why})"
                )),
                None => failures.push(format!("{name}: listed as corrected, and not produced now")),
            }
            continue;
        }
        match found {
            None => failures.push(format!("{name}: in the record, and not produced now")),
            Some(Now::Digest(d)) if d == was => {}
            Some(Now::Digest(d)) if was == REFUSED => {
                failures.push(format!("{name}: refused when recorded, digest {d} now"))
            }
            Some(Now::Digest(d)) => failures.push(format!("{name}: digest {d}, recorded {was}")),
            Some(Now::Refused(_)) if was == REFUSED => {}
            Some(Now::Refused(why)) => {
                match NAMED_REFUSALS.iter().find(|(_, text)| why.contains(text)) {
                    Some((refusal, _)) => expected.push(format!("{name}: {refusal}")),
                    None => failures.push(format!("{name}: refused now ({why}), recorded {was}")),
                }
            }
        }
    }
    for ((entry, target), what) in now {
        let inside = format!("{entry}#");
        let answers_for_recorded = matches!(what, Now::Refused(_))
            && recorded
                .keys()
                .any(|(e, t)| t == target && e.starts_with(&inside));
        if !answers_for_recorded && !recorded.contains_key(&(entry.clone(), target.clone())) {
            failures.push(format!(
                "{entry} ({target}): produced now, and not in the record"
            ));
        }
    }
    (failures, expected)
}

/// Write the record when recording, and otherwise hold `now` to it.
pub fn settle(manifest_dir: &str, kind: &str, now: &BTreeMap<Key, Now>) {
    let what = format!("the {kind} baseline");
    let (dir, named) = directory(manifest_dir);
    if let Some(revision) = recording() {
        let dir = dir.unwrap_or_else(|| panic!("{what}: set {DIR_ENV} to record into"));
        assert!(
            now.values().any(|n| matches!(n, Now::Digest(_))),
            "{what}: nothing was produced, so there is nothing to record"
        );
        std::fs::create_dir_all(&dir).expect("the baselines directory is creatable");
        let path = dir.join(format!("{kind}.tsv"));
        std::fs::write(&path, render(kind, &revision, now)).expect("the record writes");
        eprintln!(
            "{what}: recorded {} entries at {revision} into {}",
            now.len(),
            path.display()
        );
        return;
    }
    let Some(path) = dir.map(|d| d.join(format!("{kind}.tsv"))) else {
        return or_test_support::skip_corpus(&what, "no openrage-corpus/baselines is staged");
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if named => panic!("{what}: {DIR_ENV} names {}: {e}", path.display()),
        Err(_) => {
            return or_test_support::skip_corpus(
                &what,
                format_args!("no record at {}; run just baselines-record", path.display()),
            )
        }
    };
    let (revision, recorded) =
        parse(&text).unwrap_or_else(|e| panic!("{what}: {}: {e}", path.display()));
    let corrected: Vec<&Correction> = CORRECTED.iter().filter(|c| c.kind == kind).collect();
    let (failures, expected) = compare(&recorded, now, &corrected);
    for line in &expected {
        eprintln!("{what}: expected difference, {line}");
    }
    assert!(
        failures.is_empty(),
        "{what}: {} of {} entries differ from the record taken at {revision} ({}):\n  {}",
        failures.len(),
        recorded.len(),
        path.display(),
        failures.join("\n  ")
    );
    eprintln!(
        "{what}: {} entries match the record taken at {revision}, {} expected differences",
        recorded.len() - expected.len(),
        expected.len()
    );
}

/// Hold the comparer to a record of `stream` against `stream` with its
/// `at`th byte's lowest bit flipped: it must fail, naming the file.
pub fn a_perturbed_stream_is_named(key: Key, stream: &[u8], at: usize) {
    let mut perturbed = stream.to_vec();
    perturbed[at] ^= 1;
    let recorded = BTreeMap::from([(key.clone(), digest(stream))]);
    let now = BTreeMap::from([(key.clone(), Now::Digest(digest(&perturbed)))]);
    let (failures, _) = compare(&recorded, &now, &[]);
    assert!(
        failures.iter().any(|f| f.starts_with(&key.0)),
        "the comparer passed {} with byte {at} perturbed: {failures:?}",
        key.0
    );
}

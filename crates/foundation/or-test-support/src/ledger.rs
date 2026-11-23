use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const REQUIRE_FIXTURES_ENV: &str = "OPENRAGE_REQUIRE_FIXTURES";

pub const SKIP_LEDGER_ENV: &str = "OPENRAGE_SKIP_LEDGER";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipReason {
    Corpus,
    Build,
    Device,
    /// No game install is reachable.
    ///
    /// ⚠ Not `Corpus`: it gates questions a corpus is the wrong denominator
    /// for — what a shipped executable carries, what a whole install
    /// contains — and a machine can have every fixture and no install.
    Install,
    /// The tier that was run does not select it.
    ///
    /// ⚠ **No test can record this one**: a tier excludes whole binaries, so
    /// the processes that would record never start. Without it a fast tier
    /// reports "nothing skipped" and means "I did not look". Written by
    /// `skip-ledger --not-run`, from the tier's filter against the workspace.
    Tier,
}

impl SkipReason {
    pub const fn tag(self) -> &'static str {
        match self {
            SkipReason::Corpus => "corpus",
            SkipReason::Build => "build",
            SkipReason::Device => "device",
            SkipReason::Install => "install",
            SkipReason::Tier => "tier",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "corpus" => Some(SkipReason::Corpus),
            "build" => Some(SkipReason::Build),
            "device" => Some(SkipReason::Device),
            "install" => Some(SkipReason::Install),
            "tier" => Some(SkipReason::Tier),
            _ => None,
        }
    }

    pub const fn remedy(self) -> &'static str {
        match self {
            SkipReason::Corpus => "stage a corpus",
            SkipReason::Build => "build without --release",
            SkipReason::Device => "run where a GPU adapter is available",
            SkipReason::Install => concat!("point ", "OPENRAGE_GAME_ROOT", " at a game install"),
            SkipReason::Tier => "run `just test`",
        }
    }

    /// ⚠ An install is **not** staged content. `--require` is about a
    /// staging fault in this repository, and a licensed game installed
    /// somewhere on the machine is neither staged nor this repository's to
    /// provide.
    pub const fn is_about_staged_content(self) -> bool {
        matches!(self, SkipReason::Corpus)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkipRecord {
    pub reason: SkipReason,
    pub suite: String,
    pub what: String,
    pub detail: String,
    pub site: String,
}

#[track_caller]
pub fn record(reason: SkipReason, what: &str, detail: impl Display) {
    let detail = one_line(&detail.to_string());
    let what = one_line(what);
    let caller = std::panic::Location::caller();

    if reason.is_about_staged_content() && require_fixtures() {
        panic!(
            "{REQUIRE_FIXTURES_ENV} is set, so this skip is a failure.\n\
             \n\
             did not run: {what}\n\
             {:>12}: {detail}\n\
             \n\
             This machine is configured as one that has staged content, so a \
             suite that cannot find any is a staging fault rather than an \
             absence. Either {}, or unset {REQUIRE_FIXTURES_ENV}.",
            reason.tag(),
            reason.remedy(),
        );
    }

    let record = SkipRecord {
        reason,
        suite: suite_name(),
        what,
        detail,
        site: format!("{}:{}", caller.file().replace('\\', "/"), caller.line()),
    };

    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    if !state.counted.insert(record.clone()) {
        return;
    }
    append_to_ledger(&record);

    let line = format!(
        "SKIP [{}] {}: {}",
        record.reason.tag(),
        record.suite,
        record.detail
    );
    if !state.printed.insert(line.clone()) {
        return;
    }

    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{line}");
    let _ = err.flush();
}

pub fn read_dir(dir: &Path) -> Vec<SkipRecord> {
    let mut out = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            if let Some(record) = parse_line(line) {
                out.insert(record);
            }
        }
    }
    out.into_iter().collect()
}

/// The directory every process in one test run writes its skips into.
///
/// ⚠ Found by `<profile>/.fingerprint`, which every build writes, and not by
/// `CACHEDIR.TAG`: cargo writes that only when it creates the target
/// directory, so a `target/` anything else made first never gets one.
/// Measured: eleven of twelve lanes had one, and the owner's checkout, where
/// `just ci` runs, did not. The profile directory is also the right scope: it
/// holds exactly the binaries built together, which `<target>/` stops doing
/// once `--target` or a second profile is in play.
pub fn ledger_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("this process cannot name its own executable: {err}"))?;
    ledger_dir_from(&exe, std::env::var_os(SKIP_LEDGER_ENV).as_deref())
}

/// What a cargo profile directory has in it that a directory above it does not.
const PROFILE_MARKERS: [&str; 2] = [".fingerprint", ".cargo-lock"];

pub fn ledger_dir_from(exe: &Path, configured: Option<&OsStr>) -> Result<PathBuf, String> {
    if let Some(dir) = configured {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    let mut searched = Vec::new();
    for dir in exe.ancestors().skip(1) {
        if PROFILE_MARKERS.iter().any(|m| dir.join(m).exists()) {
            return Ok(dir.join(LEDGER_DIR_NAME));
        }
        // Still correct where present: cargo created this target directory.
        if dir.join("CACHEDIR.TAG").is_file() {
            return Ok(dir.join(LEDGER_DIR_NAME));
        }
        searched.push(dir.display().to_string());
    }

    Err(format!(
        "no cargo build directory above {}\n  \
         looked for {} or CACHEDIR.TAG in:\n{}",
        exe.display(),
        PROFILE_MARKERS.join(" / "),
        searched
            .iter()
            .map(|d| format!("    {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

const LEDGER_DIR_NAME: &str = "openrage-skip-ledger";

fn require_fixtures() -> bool {
    matches!(
        std::env::var("OPENRAGE_REQUIRE_FIXTURES").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[derive(Default)]
struct State {
    counted: BTreeSet<SkipRecord>,
    printed: BTreeSet<String>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn suite_name() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return "<unknown>".to_string();
    };
    let Some(name) = exe.file_stem().and_then(|s| s.to_str()) else {
        return "<unknown>".to_string();
    };
    match name.rsplit_once('-') {
        Some((stem, hash)) if hash.len() >= 8 && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            stem.to_string()
        }
        _ => name.to_string(),
    }
}

fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(ch);
    }
    out
}

/// ⚠ A record that cannot be filed is said, never dropped: dropped, the
/// report would read "nothing skipped". Once per process, because the reason
/// is the same for every record it will write.
fn warn_unfiled(reason: &str) {
    static SAID: OnceLock<()> = OnceLock::new();
    if SAID.set(()).is_err() {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "\nskip ledger: this suite's skips are going to stderr only and will \
         not reach the report.\n{reason}\n"
    );
    let _ = err.flush();
}

fn append_to_ledger(record: &SkipRecord) {
    let dir = match ledger_dir() {
        Ok(dir) => dir,
        Err(reason) => {
            warn_unfiled(&reason);
            return;
        }
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn_unfiled(&format!("  cannot create {}: {err}", dir.display()));
        return;
    }
    let path = dir.join(format!("{}-{}.tsv", record.suite, std::process::id()));
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(
        file,
        "{}\t{}\t{}\t{}\t{}",
        record.reason.tag(),
        record.suite,
        record.site,
        record.what,
        record.detail
    );
}

fn parse_line(line: &str) -> Option<SkipRecord> {
    let mut fields = line.splitn(5, '\t');
    let reason = SkipReason::from_tag(fields.next()?)?;
    let suite = fields.next()?.to_string();
    let site = fields.next()?.to_string();
    let what = fields.next()?.to_string();
    let detail = fields.next().unwrap_or_default().to_string();
    Some(SkipRecord {
        reason,
        suite,
        what,
        detail,
        site,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ Hosted CI always has a `CACHEDIR.TAG`, so only a checkout whose
    /// `target/` cargo did not create can turn this red.
    #[test]
    fn a_running_test_binary_can_find_the_ledger() {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        match ledger_dir_from(&exe, None) {
            Ok(dir) => assert!(
                dir.ends_with(LEDGER_DIR_NAME),
                "{} is not a ledger directory",
                dir.display()
            ),
            Err(reason) => panic!(
                "a test binary cannot locate the ledger, so every skip it records is \
                 dropped and the report reads as though nothing was skipped:\n{reason}"
            ),
        }
    }

    /// A folder removed when the first of the pair drops, so hold it as long as the second is used.
    fn scratch(name: &str) -> (tempfile::TempDir, PathBuf) {
        let held = tempfile::Builder::new()
            .prefix(&format!("ledger-test-{name}-"))
            .tempdir()
            .expect("a scratch folder");
        let dir = held.path().to_path_buf();
        (held, dir)
    }

    #[test]
    fn the_profile_directory_is_found_by_a_marker_cargo_maintains() {
        // What cargo lays out: the suite runs from `<profile>/deps/`, and the
        // marker is one level up.
        for marker in PROFILE_MARKERS {
            let (_held, root) = scratch(marker.trim_start_matches('.'));
            let profile = root.join("target/debug");
            std::fs::create_dir_all(profile.join("deps")).expect("scratch");
            std::fs::write(profile.join(marker), "").expect("marker");

            let exe = profile.join("deps/ydd_roundtrip-3f81a2c9e4d10b77.exe");
            assert_eq!(
                ledger_dir_from(&exe, None).expect("locatable"),
                profile.join(LEDGER_DIR_NAME),
                "{marker} did not identify the profile directory"
            );
        }
    }

    /// A tree carrying only the tag, as cargo leaves one it created.
    #[test]
    fn a_cache_tag_still_locates_a_tree_that_has_one() {
        let (_held, root) = scratch("tagged");
        let target = root.join("target");
        std::fs::create_dir_all(target.join("debug")).expect("scratch");
        std::fs::write(target.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172").expect("tag");

        let exe = target.join("debug/skip-ledger.exe");
        assert_eq!(
            ledger_dir_from(&exe, None).expect("locatable"),
            target.join(LEDGER_DIR_NAME)
        );
    }

    #[test]
    fn an_unlocatable_ledger_is_an_error_that_says_where_it_looked() {
        let (_held, root) = scratch("bare");
        let dir = root.join("nowhere/at/all");
        std::fs::create_dir_all(&dir).expect("scratch");

        let exe = dir.join("stray.exe");
        let reason = ledger_dir_from(&exe, None).expect_err("nothing here marks a cargo build");
        assert!(reason.contains("stray.exe"), "{reason}");
        assert!(reason.contains(".fingerprint"), "{reason}");
        assert!(reason.contains(&dir.display().to_string()), "{reason}");
    }

    #[test]
    fn an_explicit_directory_outranks_the_search() {
        let exe = PathBuf::from("/nowhere/stray.exe");
        assert_eq!(
            ledger_dir_from(&exe, Some(OsStr::new("/tmp/asked-for"))).expect("configured"),
            PathBuf::from("/tmp/asked-for")
        );
        // An empty value is PowerShell deleting the variable, not a request.
        assert!(ledger_dir_from(&exe, Some(OsStr::new(""))).is_err());
    }

    #[test]
    fn a_tag_round_trips() {
        for reason in [
            SkipReason::Corpus,
            SkipReason::Build,
            SkipReason::Device,
            SkipReason::Install,
            SkipReason::Tier,
        ] {
            assert_eq!(SkipReason::from_tag(reason.tag()), Some(reason));
        }
        assert_eq!(SkipReason::from_tag("nonsense"), None);
    }

    #[test]
    fn only_staged_content_answers_to_the_require_flag() {
        assert!(SkipReason::Corpus.is_about_staged_content());
        assert!(!SkipReason::Build.is_about_staged_content());
        assert!(!SkipReason::Device.is_about_staged_content());
        assert!(!SkipReason::Install.is_about_staged_content());
        // `--require` failing on a tier skip would make the fast tier
        // unusable on every machine that has a corpus.
        assert!(!SkipReason::Tier.is_about_staged_content());
    }

    #[test]
    fn a_record_survives_the_tsv() {
        let record = SkipRecord {
            reason: SkipReason::Corpus,
            suite: "texture_dict".to_string(),
            what: "gen8 texture reads".to_string(),
            detail: "no .ytd staged".to_string(),
            site: "crates/io/or-io-verify/tests/texture_dict.rs:93".to_string(),
        };
        let line = format!(
            "{}\t{}\t{}\t{}\t{}",
            record.reason.tag(),
            record.suite,
            record.site,
            record.what,
            record.detail
        );
        assert_eq!(parse_line(&line), Some(record));
    }

    #[test]
    fn a_malformed_line_is_dropped_rather_than_guessed_at() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("corpus"), None);
        assert_eq!(parse_line("not-a-reason\ta\tb\tc\td"), None);
    }

    #[test]
    fn a_shared_macro_counts_once_per_call_site_and_prints_once() {
        let wording = "corpus\tydd_roundtrip\t{site}\tYDD round-trip\tno .ydd fixtures";
        let sites: Vec<SkipRecord> = (1..=10)
            .map(|line| {
                parse_line(&wording.replace("{site}", &format!("tests/ydd_roundtrip.rs:{line}")))
                    .expect("well-formed")
            })
            .collect();

        let counted: BTreeSet<_> = sites.iter().cloned().collect();
        assert_eq!(counted.len(), 10, "each call site is its own missed check");

        let printed: BTreeSet<_> = sites
            .iter()
            .map(|r| format!("{}: {}", r.what, r.detail))
            .collect();
        assert_eq!(printed.len(), 1, "one reason is one line of output");
    }

    #[test]
    fn a_multi_line_detail_becomes_one_line() {
        assert_eq!(
            one_line("no fixtures staged\n         for any format"),
            "no fixtures staged for any format"
        );
        assert_eq!(
            one_line("  leading\tand trailing  "),
            "leading and trailing"
        );
    }

    #[test]
    fn a_cargo_hash_suffix_is_not_part_of_the_suite_name() {
        assert_eq!(
            "ydd_roundtrip-3f81a2c9e4d10b77"
                .rsplit_once('-')
                .map(|(stem, _)| stem),
            Some("ydd_roundtrip")
        );
        assert!(!"skip-ledger".rsplit_once('-').is_some_and(
            |(_, hash)| hash.len() >= 8 && hash.chars().all(|c| c.is_ascii_hexdigit())
        ));
    }
}

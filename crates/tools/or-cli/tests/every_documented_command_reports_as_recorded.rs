//! The documented `openrage-cli` commands print, over a fixed sample of the
//! Gen8 corpus, the stdout they printed when the baseline was recorded.
//!
//! ⚠ The commands print with `println!`, which a test cannot read back in
//! its own process, so each runs in a child of this test binary that calls
//! `or_cli::run` between two markers.

#[path = "../../../domain/or-interchange/tests/baseline/mod.rs"]
mod baseline;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use baseline::{Key, Now};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const KIND: &str = "cli";
/// How many files of each extension, first in path order, each command reads.
const SAMPLE: usize = 2;

/// A command's key, and the arguments it runs with.
type Invocation = (Key, Vec<String>);

const CHILD_ENV: &str = "OPENRAGE_CLI_BASELINE_ARGS";
const CHILD_TEST: &str = "run_openrage_cli_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";

/// Each command's words before the file, and the extensions it is run on.
const COMMANDS: &[(&[&str], &[&str])] = &[
    (
        &["convert", "info"],
        &[
            "ydr", "ydd", "yft", "ytd", "ycd", "ypt", "ybn", "yld", "ynd", "ynv", "yed", "yfd",
        ],
    ),
    (&["inspect", "textures"], &["ytd", "ypt"]),
    (&["inspect", "drawables"], &["ydr", "ydd", "yft"]),
    (&["inspect", "skeleton"], &["yft", "ydr"]),
    (&["inspect", "bounds"], &["ybn"]),
    (&["inspect", "cloth"], &["yld"]),
    (&["inspect", "clips"], &["ycd"]),
    (&["mesh", "channels"], &["ydr", "ydd", "yft"]),
    (&["texture", "info"], &["ytd"]),
    (&["deflate", "--info"], &["ydr", "ytd"]),
    (&["awc", "info"], &["awc"]),
];

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_openrage_cli_for_a_parent() {
    let Some(args) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let args: Vec<String> = args
        .to_string_lossy()
        .split(SEPARATOR)
        .map(str::to_string)
        .collect();
    let mut out = std::io::stdout();
    let _ = write!(out, "{BEGIN}");
    let _ = out.flush();
    let code = or_cli::run(&args);
    let _ = out.flush();
    let _ = write!(out, "{END}{}", u8::from(code == ExitCode::SUCCESS));
    let _ = out.flush();
}

/// What one command printed, with the corpus root written as `<corpus>`, as
/// the bytes the digest is taken over; or what it said when it failed.
fn stdout_of(root: &Path, args: &[String]) -> Result<Vec<u8>, String> {
    let output = Command::new(std::env::current_exe().expect("this test binary"))
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, args.join(&SEPARATOR.to_string()))
        .output()
        .expect("this test binary runs as a child");
    let text = String::from_utf8_lossy(&output.stdout);
    let (_, rest) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("{args:?}: the child printed no start marker: {text}"));
    // No end marker: the command panicked, which is a failure it reports.
    let (printed, tail) = rest.split_once(END).unwrap_or((rest, ""));
    if !tail.starts_with('1') {
        return Err(format!(
            "{printed}{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let shown = root.display().to_string();
    let mut printed = printed.to_string();
    for spelling in [
        shown.clone(),
        shown.replace('\\', "/"),
        shown.replace('/', "\\"),
    ] {
        printed = printed.replace(&spelling, "<corpus>");
    }
    Ok(printed.into_bytes())
}

/// Every command over the sample: its key, and the arguments it runs with.
fn invocations(root: &Path) -> Vec<Invocation> {
    let mut out = Vec::new();
    for (words, extensions) in COMMANDS {
        for ext in *extensions {
            let files = Corpus::with_root(root, ext)
                .target(TARGET_GEN8)
                .limit(SAMPLE)
                .find();
            for path in files {
                let mut args: Vec<String> = words.iter().map(|w| w.to_string()).collect();
                args.push(path.display().to_string());
                let key = baseline::key(root, &path, Some(&words.join(" ")), TARGET_GEN8);
                out.push((key, args));
            }
        }
    }
    out
}

fn corpus(what: &str) -> Option<(PathBuf, Vec<Invocation>)> {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let invocations = invocations(&root);
    if invocations.is_empty() {
        or_test_support::skip(what, &root, "ydr, .ytd, .ycd or any sampled");
        return None;
    }
    Some((root, invocations))
}

/// What a listed correction's output lacks: its right line, or it still
/// prints the line the record holds.
fn uncorrected(key: &Key, printed: &str) -> Option<String> {
    let c = baseline::CORRECTED
        .iter()
        .find(|c| c.kind == KIND && c.entry == key.0 && c.target == key.1)?;
    let prints = |line: &str| printed.lines().any(|l| l == line);
    (!prints(c.right) || prints(c.holds))
        .then(|| format!("prints no {:?} in place of {:?}", c.right, c.holds))
}

#[test]
fn every_documented_command_reports_as_recorded() {
    let what = "every_documented_command_reports_as_recorded";
    let Some((root, invocations)) = corpus(what) else {
        return;
    };
    let now: BTreeMap<Key, Now> = invocations
        .into_iter()
        .map(|(key, args)| match stdout_of(&root, &args) {
            Ok(stdout) => match uncorrected(&key, &String::from_utf8_lossy(&stdout)) {
                Some(said) => (key, Now::Refused(said)),
                None => (key, Now::Digest(baseline::digest(&stdout))),
            },
            Err(said) => (key, Now::Refused(said)),
        })
        .collect();
    baseline::settle(env!("CARGO_MANIFEST_DIR"), KIND, &now);
}

#[test]
fn a_perturbed_stdout_names_its_file() {
    let what = "a_perturbed_stdout_names_its_file";
    let Some((root, invocations)) = corpus(what) else {
        return;
    };
    let (key, stdout) = invocations
        .into_iter()
        .find_map(|(key, args)| Some((key, stdout_of(&root, &args).ok()?)))
        .expect("some command over the sample succeeds");
    baseline::a_perturbed_stream_is_named(key, &stdout, 0);
}

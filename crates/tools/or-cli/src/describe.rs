//! What a file is, without opening it.
//!
//! `openrage-cli describe --game <root> [--glob <pattern>] [--json]`.
//!
//! Every fact comes from `or_title::vfs::EntryFacts`, which the archive's entry
//! table already holds — the RSC7 version is the high nibble of `system_info`
//! and of `graphics_info` — so no body is decrypted or inflated. `versions
//! --game`, which reads the version from the body, took **370.9 s** for 289,631
//! resource headers over a retail Legacy install with a release build.
//! `the_cheap_version_is_the_version_in_the_body` in `or-title` holds that the
//! two answers agree.
//!
//! ⚠ **It therefore cannot say anything a body would have told it**: no geometry
//! counts, no texture dimensions, no shader names. A summary that grew one would
//! have to open the file and lose the speed that is the reason to have it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use or_io::container::rpf::RpfEntryKind;
use or_title::vfs::{EntryFacts, Pattern};

pub fn print_usage() {
    eprintln!("Usage: openrage-cli describe --game <path> [options]");
    eprintln!();
    eprintln!("Summarises files from archive entry tables alone — no body is read,");
    eprintln!("decrypted or inflated, so this answers for a whole install in about");
    eprintln!("a second.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --game <path>   The install root.");
    eprintln!("  --glob <pat>    Only files matching a VFS glob, e.g. `**/*.ydr`.");
    eprintln!("  --json          One JSON object per line.");
    eprintln!("  --limit <n>     Stop after n files.");
    eprintln!("  --summary       Totals per extension and version instead of rows.");
}

pub fn run(args: &[String]) -> ExitCode {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) if e.is_empty() => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn row(facts: &EntryFacts) -> String {
    let kind = match facts.kind {
        RpfEntryKind::Resource => "resource",
        RpfEntryKind::Binary => "binary  ",
        RpfEntryKind::Directory => "dir     ",
    };
    let version = match facts.resource_version() {
        Some(v) => format!("v{v:<4}"),
        // ⚠ Not `v0`. A binary entry has no RSC7 header, and a dash says that
        // where a zero would claim a version it does not have.
        None => "  -  ".to_string(),
    };
    let compressed = if facts.compressed {
        "deflated"
    } else {
        "stored  "
    };
    format!(
        "{kind}  {version}  {:>10} {:>10}  {compressed}  {}",
        facts.stored_size, facts.size, facts.logical
    )
}

fn json(facts: &EntryFacts) -> String {
    let kind = match facts.kind {
        RpfEntryKind::Resource => "resource",
        RpfEntryKind::Binary => "binary",
        RpfEntryKind::Directory => "directory",
    };
    let version = match facts.resource_version() {
        Some(v) => v.to_string(),
        None => "null".to_string(),
    };
    format!(
        r#"{{"path":{},"container":{},"leaf":{},"kind":"{kind}","version":{version},"stored_size":{},"size":{},"compressed":{}}}"#,
        quote(&facts.logical),
        quote(&facts.container),
        quote(&facts.leaf),
        facts.stored_size,
        facts.size,
        facts.compressed,
    )
}

/// A JSON string, escaping what a RAGE path can actually contain.
///
/// ⚠ Backslashes reach here — a container chain is built from archive paths —
/// and an unescaped one makes the line invalid JSON rather than merely ugly.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Default)]
struct Totals {
    files: u64,
    stored: u64,
    size: u64,
    /// extension → count
    extensions: BTreeMap<String, u64>,
    /// RSC7 version → count. `None` is the binary entries.
    versions: BTreeMap<Option<u32>, u64>,
    /// Logical paths seen more than once, and how many times over.
    ///
    /// This walks **entries**, where `openrage-cli versions` counts **distinct
    /// logical paths**; DLC overlay puts one path in more than one pack, so the
    /// two differ by exactly the overlap. Measured on retail Legacy for
    /// `**/*.ydr`: 83,934 entries against 83,746 distinct paths — **188**
    /// overridden copies — with the version tallies agreeing wherever the
    /// denominators do (v164: 13 and 13).
    seen: BTreeMap<String, u32>,
}

impl Totals {
    fn record(&mut self, facts: &EntryFacts) {
        self.files += 1;
        self.stored += u64::from(facts.stored_size);
        self.size += u64::from(facts.size);
        let ext = facts
            .leaf
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_else(|| "(none)".to_string());
        *self.extensions.entry(ext).or_default() += 1;
        *self.versions.entry(facts.resource_version()).or_default() += 1;
        *self.seen.entry(facts.logical.clone()).or_default() += 1;
    }

    /// Distinct logical paths, and how many entries were extra copies.
    fn distinct(&self) -> (u64, u64) {
        let distinct = self.seen.len() as u64;
        (distinct, self.files - distinct)
    }
}

fn run_inner(args: &[String]) -> Result<ExitCode, String> {
    let mut root: Option<PathBuf> = None;
    let mut keys: Option<PathBuf> = None;
    let mut glob: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut as_json = false;
    let mut summary = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                root = Some(PathBuf::from(args.get(i + 1).ok_or("--game needs a path")?));
                i += 2;
            }
            "--keys" => {
                keys = Some(PathBuf::from(args.get(i + 1).ok_or("--keys needs a path")?));
                i += 2;
            }
            "--glob" => {
                glob = Some(args.get(i + 1).ok_or("--glob needs a pattern")?.clone());
                i += 2;
            }
            "--limit" => {
                limit = Some(
                    args.get(i + 1)
                        .ok_or("--limit needs a number")?
                        .parse()
                        .map_err(|e| format!("--limit: {e}"))?,
                );
                i += 2;
            }
            "--json" => {
                as_json = true;
                i += 1;
            }
            "--summary" => {
                summary = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_usage();
                return Err(String::new());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let root = root.ok_or_else(|| {
        print_usage();
        "missing --game".to_string()
    })?;
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }

    let game = super::keys::mount_all(&root, keys.as_deref())?;
    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }

    let pattern = glob.as_deref().map(Pattern::new);
    let mut totals = Totals::default();
    let mut printed = 0usize;
    let started = std::time::Instant::now();

    game.for_each_entry(|facts| {
        if let Some(pattern) = &pattern {
            if !pattern.matches_path(&facts.logical) {
                return;
            }
        }
        if limit.is_some_and(|n| totals.files as usize >= n) {
            return;
        }
        totals.record(facts);
        if !summary {
            println!("{}", if as_json { json(facts) } else { row(facts) });
            printed += 1;
        }
    });

    if summary {
        let (distinct, overridden) = totals.distinct();
        println!("{} entr(ies)", totals.files);
        println!(
            "  {distinct} distinct logical path(s), {overridden} overridden by a \
             later DLC pack"
        );
        println!(
            "  {} byte(s) stored, {} byte(s) inflated",
            totals.stored, totals.size
        );
        println!();
        println!("  by extension:");
        let mut by_count: Vec<(&String, &u64)> = totals.extensions.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (ext, count) in by_count.iter().take(25) {
            println!("    {count:>8}  .{ext}");
        }
        println!();
        println!("  by RSC7 version:");
        for (version, count) in &totals.versions {
            match version {
                Some(v) => println!("    {count:>8}  v{v} ({v:#04x})"),
                None => println!("    {count:>8}  not a resource"),
            }
        }
    }

    eprintln!(
        "described {} file(s) in {:.2}s, reading no bodies",
        totals.files,
        started.elapsed().as_secs_f64()
    );
    Ok(ExitCode::SUCCESS)
}

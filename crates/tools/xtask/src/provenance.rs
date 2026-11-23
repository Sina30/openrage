//! Report private-tier content that has reached the committed tree.
//!
//! Three checks. Two ask what is in a clone that should not be; the third asks
//! what is one `git add -A` away from becoming one.
//!
//! **Patterns.** Leaked-source path shapes and source-file-and-line citations,
//! fatal in every tracked file, because the whole repository is published with
//! the crate (D8) and the history rewrite (D3) scrubs such references before it
//! is. Engine names are not a pattern here: by D15, or-io declares GTA V under
//! the names the engine's own source uses.
//!
//! **Game content.** Fixture corpora are copyrighted game files and are never
//! committed. This keys on file extension rather than path, so it keeps working
//! across a crate move, which is what the ignore rules did not.
//!
//! **Game content nobody is ignoring.** Untracked files no ignore rule covers,
//! the set a `git add -A` would stage, recognised by extension *or* by the four
//! bytes a RAGE container opens with. Advisory: it never fails, because the
//! working tree is not a clone and a lint that failed for data on disk would
//! trip the hook on every unrelated commit.
//!
//! **History.** `--history <repository>` runs none of the above. It scans every
//! commit message and every blob reachable from any ref of that repository, a
//! mirror included, with the pattern check's expressions and those of
//! `--expressions`, a `git filter-repo --replace-text` file.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::git;

/// Files that cannot avoid naming the patterns they look for, this one first.
///
/// ⚠ Not `AGENTS.md`: it is tracked, so exempting it would keep the references
/// out of the report and not out of a clone.
const EXEMPT: [&str; 4] = [
    "scripts/lint_provenance.ps1",
    "crates/tools/xtask/src/provenance.rs",
    // The glue to the vendored C++ translator: it has to name its own `.cpp` and its includes.
    "crates/tools/or-shader-front/build.rs",
    "crates/tools/or-shader-front/src/shim.cpp",
];

/// Substrings that identify a reference to the leaked tree, matched literally:
/// nothing here is an escape. Both backslash spellings stay, because neither is
/// a substring of the other, and the doubled one is what a path looks like
/// inside a Rust string literal or escaped JSON.
const PATTERNS: [&str; 4] = [
    "dev_ng",
    "TheftAuto/gta5/src",
    "TheftAuto\\gta5\\src",
    "TheftAuto\\\\gta5\\\\src",
];

/// Extensions that only ever belong to game content: nothing in this
/// repository authors one, so a tracked file carrying any came out of an install.
///
/// ⚠ It grows by measurement, never by transcribing the engine's extension
/// table: a list wider than the evidence buries the hits that matter.
const GAME_CONTENT_EXTENSIONS: [&str; 19] = [
    "ypt", "ytd", "ydr", "ydd", "yft", "ybn", "yld", "ycd", "yed", "ypdb", "yfd", "ymt", "ymf",
    "ymap", "ytyp", "pso", "rpf", "gxt2", "fxc",
];

/// Files that are not game content by extension but must not be committed:
/// a machine-extracted derivative of a source tree that is not ours. By
/// basename, because this one has moved three times.
const FORBIDDEN_NAMES: [&str; 1] = ["schema_snapshot.bin"];

/// The four bytes a RAGE container opens with: a paged resource, a PSO
/// metadata container, an archive.
const CONTAINER_MAGICS: [&[u8; 4]; 3] = [b"RSC7", b"PSIN", b"RPF7"];

/// Third-party code carries its own headers and its own citations of them.
const FOREIGN_PREFIXES: [&str; 2] = ["external/", "crates/tools/or-shader-front/vendor/"];

/// Binary files have no citations to fix and produce noise when matched.
const BINARY_EXTENSIONS: [&str; 14] = [
    "bin", "dat", "png", "ico", "zip", "lib", "pdb", "exe", "dll", "ttf", "otf", "glb", "dds",
    "wav",
];

/// Leaked-source path shapes and source-file-and-line citations.
///
/// ⚠ A `.h` is only a citation with a directory, a line number or backticks
/// around it: `bounds.h` is a field access.
const CITATION_REGEXES: [&str; 10] = [
    r"(?i)TheftAuto[\\/]+gta5[\\/]+src",
    r"(?i)\brage[\\/]+(base|suite|framework|script|scaleform)[\\/]+src\b",
    r"(?i)\bshader_source[\\/]",
    r"(?i)\bgame[\\/]+[A-Za-z_]+[\\/]+[\w\\/]*\.(cpp|h|fx)\b",
    r"\b\w+\.(cpp|hpp|inl|fxh)\b",
    r"\b\w+\.fx(:\d+|::\w+)",
    r"[\w]+[\\/][\w\\/]*\w+\.h\b",
    r"\b\w+\.h:\d+",
    r"`\w+\.h`",
    r"DeclareStruct",
];

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut history: Option<PathBuf> = None;
    let mut expressions: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--history" => history = Some(value(it.next(), "--history")?),
            "--expressions" => expressions = Some(value(it.next(), "--expressions")?),
            other => bail!("lint-provenance: unknown argument `{other}`"),
        }
    }
    if expressions.is_some() && history.is_none() {
        bail!("--expressions is read only with --history");
    }

    let root = git::here().context("not inside a git repository")?;
    match history {
        Some(repository) => scan_history(&std::fs::canonicalize(repository)?, expressions),
        None => scan_tree(&root),
    }
}

fn value(arg: Option<&String>, flag: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        arg.with_context(|| format!("{flag} needs a value"))?,
    ))
}

fn compiled(sources: &[&str]) -> Result<Vec<Regex>> {
    sources
        .iter()
        .map(|source| Regex::new(source).with_context(|| format!("bad expression {source}")))
        .collect()
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_game_content(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    GAME_CONTENT_EXTENSIONS.contains(&extension(path).as_str()) || FORBIDDEN_NAMES.contains(&name)
}

/// Whether the file opens with a RAGE container's four bytes.
fn is_rage_container(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    file.read_exact(&mut head).is_ok() && CONTAINER_MAGICS.iter().any(|magic| **magic == head)
}

fn skipped(path: &str) -> bool {
    EXEMPT.contains(&path)
        || FOREIGN_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
        || BINARY_EXTENSIONS.contains(&extension(path).as_str())
}

/// The first reason a line cites the leaked source, if it does.
fn cites(line: &str, regexes: &[Regex]) -> Option<&'static str> {
    (regexes.iter().any(|r| r.is_match(line)) || PATTERNS.iter().any(|p| line.contains(p)))
        .then_some("cites the leaked source")
}

fn list_paths(root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let mut command = vec!["-c", "core.quotePath=false"];
    command.extend_from_slice(args);
    Ok(git::lines(&git::run(root, &command)?))
}

fn scan_tree(root: &Path) -> Result<ExitCode> {
    let tracked = list_paths(root, &["ls-files"])?;
    let regexes = compiled(&CITATION_REGEXES)?;

    // --- Game content ------------------------------------------------------
    let leaked: Vec<&String> = tracked.iter().filter(|p| is_game_content(p)).collect();
    if !leaked.is_empty() {
        println!();
        println!(
            "provenance: {} tracked file(s) look like game content.",
            leaked.len()
        );
        println!();
        for path in leaked.iter().take(25) {
            println!("       {path}");
        }
        if leaked.len() > 25 {
            println!("       ... and {} more", leaked.len() - 25);
        }
        println!();
        println!("These came out of a game install or a source tree that is not ours, and must");
        println!("not be committed. Check that .gitignore still names the directory they are in");
        println!("- the last time this fired, a crate had moved and the ignore rules had not.");
        println!();
        println!("To untrack without deleting:  git rm --cached <path>");
        println!("If they are already pushed, removing them needs a history rewrite. That has");
        println!("been done once; docs/guides/Private_Tier.md records how and what it cost.");
        println!();
        println!("FAILED.");
        return Ok(ExitCode::FAILURE);
    }

    // --- Game content nobody is ignoring -----------------------------------
    // `--exclude-standard` is the whole design of this check: it reports what
    // a `git add -A` would stage and nothing else, so an ignored corpus is
    // silent.
    let stray: Vec<String> = list_paths(root, &["ls-files", "--others", "--exclude-standard"])?
        .into_iter()
        .filter(|p| is_game_content(p) || is_rage_container(&root.join(p)))
        .collect();
    if !stray.is_empty() {
        println!();
        println!(
            "provenance: {} untracked file(s) look like game content.",
            stray.len()
        );
        println!();
        for path in stray.iter().take(10) {
            println!("       {path}");
        }
        if stray.len() > 10 {
            println!("       ... and {} more", stray.len() - 10);
        }
        println!();
        println!("Nothing is committed and nothing here is wrong. But no rule is ignoring");
        println!("them either, so one stage-everything commits the lot - which is how 414 MB");
        println!("got in last time. If this is a corpus, .gitignore should name where it sits.");
        println!();
        println!("Advisory. This half never fails.");
    }

    // --- Patterns ----------------------------------------------------------
    let mut fatal: Vec<(String, usize, &str)> = Vec::new();
    for file in &tracked {
        if skipped(file) {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join(file)) else {
            continue; // listed by git and gone from disk
        };
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.split('\n').enumerate() {
            if let Some(why) = cites(line, &regexes) {
                fatal.push((file.clone(), index + 1, why));
            }
        }
    }
    if !fatal.is_empty() {
        println!();
        println!(
            "provenance: {} tracked line(s) carry a leak pattern.",
            fatal.len()
        );
        println!();
        for (file, line, why) in fatal.iter().take(40) {
            println!("       {file}:{line}  {why}");
        }
        if fatal.len() > 40 {
            println!("       ... and {} more", fatal.len() - 40);
        }
        println!();
        println!("Cite a public reference instead - CodeWalker, Materia's published API,");
        println!("GTAMods, or a measurement over the corpus - or drop it. File-and-line");
        println!("citations of the source tree belong in docs/extra/, which is gitignored;");
        println!("names derived from it are fine (D15).");
        println!();
        println!("FAILED.");
        return Ok(ExitCode::FAILURE);
    }

    if stray.is_empty() {
        println!(
            "provenance: clean - no game content and no references to the leaked source tree."
        );
    } else {
        // Not "clean": the warning above is still on screen, and a summary line
        // contradicting it is the line a reader will believe.
        println!("provenance: nothing tracked - no committed game content and no references to the leaked source tree.");
    }
    Ok(ExitCode::SUCCESS)
}

/// A `git filter-repo --replace-text` file as expressions: `regex:` is an
/// expression, `literal:` or no prefix a literal, and the last `==>` starts the
/// replacement. `glob:` is refused rather than guessed at.
fn expressions_file(path: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    for rule in text.lines() {
        let rule = match rule.rfind("==>") {
            Some(arrow) => &rule[..arrow],
            None => rule,
        };
        if rule.is_empty() {
            continue;
        }
        if let Some(expression) = rule.strip_prefix("regex:") {
            out.push(expression.to_string());
        } else if rule.starts_with("glob:") {
            bail!("{}: a glob: expression is not supported", path.display());
        } else {
            out.push(regex::escape(rule.strip_prefix("literal:").unwrap_or(rule)));
        }
    }
    Ok(out)
}

fn scan_history(repository: &Path, expressions: Option<PathBuf>) -> Result<ExitCode> {
    let mut sources: Vec<String> = CITATION_REGEXES.iter().map(|s| s.to_string()).collect();
    sources.extend(PATTERNS.iter().map(|p| regex::escape(p)));
    if let Some(path) = expressions {
        sources.extend(expressions_file(&path)?);
    }
    // Each is grouped so that one's inline `(?i)` stays its own.
    let combined = Regex::new(
        &sources
            .iter()
            .map(|s| format!("(?:{s})"))
            .collect::<Vec<_>>()
            .join("|"),
    )?;

    let mut hits: Vec<String> = Vec::new();

    // Commit messages.
    let log = git::run(repository, &["log", "--all", "--format=%x01%H%n%B"])?;
    let mut commit = String::new();
    let mut commits = 0usize;
    let mut line_no = 0usize;
    for line in log.lines() {
        if let Some(id) = line.strip_prefix('\u{1}') {
            commit = id.to_string();
            commits += 1;
            line_no = 0;
            continue;
        }
        line_no += 1;
        if combined.is_match(line) {
            hits.push(format!("{commit}  (message):{line_no}"));
        }
    }

    // Every reachable blob, keyed to the commit and path that introduced it:
    // `--root` and `-m` make every blob of every tree appear as added or
    // modified somewhere, and the walk runs newest first, so the last write
    // wins with the oldest.
    let raw = git::run(
        repository,
        &[
            "-c",
            "core.quotePath=false",
            "log",
            "--all",
            "-m",
            "--root",
            "--raw",
            "--no-abbrev",
            "--no-renames",
            "--format=%x01%H",
        ],
    )?;
    let mut blobs: BTreeMap<String, (String, String)> = BTreeMap::new();
    for line in raw.lines() {
        if let Some(id) = line.strip_prefix('\u{1}') {
            commit = id.to_string();
        } else if line.starts_with(':') {
            let Some((meta, path)) = line.split_once('\t') else {
                continue;
            };
            let fields: Vec<&str> = meta.split(' ').collect();
            if fields.len() < 4 || !["100644", "100755", "120000"].contains(&fields[1]) {
                continue;
            }
            blobs.insert(fields[3].to_string(), (commit.clone(), path.to_string()));
        }
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("could not run `git cat-file`")?;
    let mut stdin = child.stdin.take().context("cat-file has no stdin")?;
    let mut stdout = BufReader::with_capacity(1 << 20, child.stdout.take().context("no stdout")?);

    let mut scanned = 0usize;
    for (id, (introduced, path)) in &blobs {
        if skipped(path) {
            continue;
        }
        writeln!(stdin, "{id}")?;
        stdin.flush()?;
        let mut header = String::new();
        stdout.read_line(&mut header)?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[1] != "blob" {
            bail!("git cat-file: {}", header.trim());
        }
        let size: usize = fields[2].parse().context("a blob size")?;
        let mut bytes = vec![0u8; size + 1];
        stdout.read_exact(&mut bytes)?;
        bytes.truncate(size);
        scanned += 1;

        let text = String::from_utf8_lossy(&bytes);
        if !combined.is_match(&text) {
            continue;
        }
        for (index, line) in text.split('\n').enumerate() {
            if combined.is_match(line) {
                hits.push(format!("{introduced}  {path}:{}", index + 1));
            }
        }
    }
    drop(stdin);
    child.wait()?;

    if !hits.is_empty() {
        println!();
        println!(
            "provenance history: {} line(s) reachable in {} carry a leak pattern.",
            hits.len(),
            repository.display()
        );
        println!();
        for hit in &hits {
            println!("       {hit}");
        }
        println!();
        println!("FAILED.");
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "provenance history: clean - {commits} commit message(s) and {scanned} blob(s) reachable from every ref of {}.",
        repository.display()
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regexes() -> Vec<Regex> {
        compiled(&CITATION_REGEXES).expect("the citation expressions compile")
    }

    /// A pattern built at run time, so this file is not the thing it lints.
    fn leak(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn every_spelling_of_the_leaked_path_is_caught() {
        let regexes = regexes();
        for spelling in [
            leak(&["dev", "_ng"]),
            leak(&["TheftAuto", "/gta5/", "src"]),
            leak(&["TheftAuto", "\\gta5\\", "src"]),
            leak(&["TheftAuto", "\\\\gta5\\\\", "src"]),
        ] {
            assert!(
                cites(&format!("see {spelling} for it"), &regexes).is_some(),
                "missed {spelling}"
            );
        }
    }

    #[test]
    fn a_field_access_is_not_a_header_citation() {
        let regexes = regexes();
        assert!(cites("let x = bounds.h;", &regexes).is_none());
        assert!(cites("see `grc.h` for the layout", &regexes).is_some());
        assert!(cites("see foo.h:42", &regexes).is_some());
    }

    #[test]
    fn game_content_is_found_by_extension_and_by_name() {
        assert!(is_game_content("crates/x/tests/fixtures/a.YDR"));
        assert!(is_game_content("somewhere/schema_snapshot.bin"));
        assert!(!is_game_content("docs/readme.md"));
        assert!(!is_game_content("a.bin"));
    }

    #[test]
    fn the_exempt_and_foreign_and_binary_paths_are_skipped() {
        assert!(skipped("scripts/lint_provenance.ps1"));
        assert!(skipped("crates/tools/xtask/src/provenance.rs"));
        assert!(skipped("external/x/a.cpp"));
        assert!(skipped("assets/a.png"));
        assert!(!skipped("src/main.rs"));
    }

    #[test]
    fn the_shader_front_vendors_c_plus_plus_and_its_own_glue_is_the_only_other_file_that_names_it()
    {
        assert!(skipped(
            "crates/tools/or-shader-front/vendor/dxbc-spirv/dxbc/dxbc_api.cpp"
        ));
        assert!(skipped("crates/tools/or-shader-front/src/shim.cpp"));
        assert!(skipped("crates/tools/or-shader-front/build.rs"));
        assert!(!skipped("crates/tools/or-shader-front/src/lib.rs"));
        assert!(!skipped("crates/tools/or-shader-front/src/tests.rs"));
    }

    #[test]
    fn a_replace_text_file_becomes_expressions() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let file = dir.path().join("rules.txt");
        std::fs::write(&file, "regex:foo\\d+==>X\nliteral:a.b==>Y\nplain==>Z\n\n")
            .expect("a rules file");
        let rules = expressions_file(&file).expect("the rules parse");
        assert_eq!(rules, vec!["foo\\d+", "a\\.b", "plain"]);
        std::fs::write(&file, "glob:*.x==>Y\n").expect("a rules file");
        assert!(expressions_file(&file).is_err(), "a glob is refused");
    }
}

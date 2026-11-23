//! A count a committed document states is asserted by a test, anchored to where it was measured, or waived by name.
//!
//! A count is a numeral or a number word from eleven up that is a percentage, is followed by `of`,
//! or is followed within two words by a plural noun. It is anchored when its paragraph (its item,
//! for the Roadmap and the Project context) names a revision, a date, a pull request, a command or
//! a suite that exists in the tree.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use super::{read, repo_root, rust_sources, walk};

/// Counts a test holds: the document, the noun the figure counts, and the test that fails when the
/// document and the code disagree. A figure here is not copied into the test.
const ASSERTED: &[(&str, &str, &str)] = &[
    (
        "docs/features/Mesh_Interchange.md",
        "slots",
        "a_gen9_vertex_declaration_has_the_slots_the_mesh_document_states",
    ),
    (
        "docs/features/Particle_Preview.md",
        "units",
        "a_teleporting_emitter_jumps_the_distance_the_preview_document_states",
    ),
    (
        "docs/direction/Roadmap.md",
        "tiles",
        "the_thumbnail_cache_holds_the_tiles_the_roadmap_states",
    ),
];

/// Counts that name no measurement and no test, each with why that is right.
const WAIVED: &[(&str, &str, &str)] = &[
    (
        "docs/guides/Documentation_Standard.md",
        "lines",
        "a budget the Standard sets for a kind of document: a rule, not a measurement",
    ),
    (
        "docs/guides/Documentation_Standard.md",
        "words",
        "a budget the Standard sets for a kind of document: a rule, not a measurement",
    ),
    (
        "docs/reference/Plugin_ABI.md",
        "elements",
        "an illustration of an attack on a declared count, not a count of anything",
    ),
];

/// A Plan item and a Record entry are each one dated, cited unit; everywhere else a paragraph is.
const BY_ITEM: &[&str] = &[
    "docs/direction/Roadmap.md",
    "docs/direction/Project_Context.md",
];

const CODE: char = '\u{1}';
const DATE: char = '\u{2}';
const QUOTED: char = '\u{3}';

const TENS: &[&str] = &[
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const UNITS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];
const BIG_WORDS: &[&str] = &[
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "hundred",
    "thousand",
    "million",
    "billion",
    "dozen",
];

/// Words that end the search for the noun a figure counts: verbs, conjunctions and units.
const NOT_A_NOUN: &[&str] = &[
    "is", "as", "was", "has", "this", "its", "does", "less", "unless", "across", "plus", "bus",
    "status", "pass", "class", "process", "focus", "thus", "various", "previous", "analysis",
    "basis", "axis", "while", "when", "whereas", "that", "which", "who", "but", "then", "so", "if",
    "than", "because", "after", "before", "until", "since", "although", "though", "not", "no",
    "now", "still", "also", "only", "just", "even", "always", "never", "already", "again", "and",
    "or", "to", "on", "in", "at", "by", "for", "from", "with", "are", "of", "be", "were", "bytes",
    "byte", "bits", "bit", "kib", "mib", "gib", "kb", "mb", "gb", "ms", "ns", "seconds", "second",
    "minutes", "minute", "hours", "hour", "metres", "metre", "px", "hz", "khz", "fps", "bpp",
    "means", "stays", "shrinks", "accepts", "persists", "reopens", "remains", "reads", "takes",
    "holds", "gives", "makes", "needs", "uses", "keeps", "sits", "runs", "goes", "carries",
    "names", "states", "says", "shows", "stops", "starts", "ends", "moves", "writes", "opens",
    "refuses", "follows", "equals", "matches", "differs", "agrees", "fails", "passes", "counts",
    "covers", "returns", "stands", "lands", "lets", "plays",
];

/// A word before a figure that makes it a position or a name rather than a count: `bit 3`.
const LABELS: &[&str] = &[
    "bit",
    "bits",
    "byte",
    "bytes",
    "offset",
    "offsets",
    "index",
    "indices",
    "slot",
    "slots",
    "register",
    "registers",
    "bucket",
    "column",
    "columns",
    "line",
    "lines",
    "page",
    "pages",
    "step",
    "steps",
    "phase",
    "version",
    "versions",
    "level",
    "levels",
    "stage",
    "tier",
    "bar",
    "word",
    "words",
    "value",
    "values",
    "row",
    "rows",
    "part",
    "parts",
    "id",
    "ids",
    "number",
    "numbers",
    "code",
    "codes",
    "no",
    "ordinal",
    "kind",
    "type",
    "case",
    "cases",
    "b",
    "t",
    "u",
    "s",
    "r",
    "d",
    "w",
    "v",
    "gen",
    "sm",
    "ps",
    "wave",
    "item",
    "items",
    "entry",
    "entries",
    "chunk",
    "chunks",
    "block",
    "blocks",
    "schema",
    "abi",
    "rsc",
    "binding",
    "group",
    "set",
];

/// A capitalised word that opens a clause and so does not make the figure after it a name.
const CLAUSE_OPENERS: &[&str] = &[
    "Of", "The", "A", "An", "All", "Every", "Each", "Both", "No", "Only", "Over", "Under", "About",
    "Nearly", "Almost", "Some", "And", "Or", "But", "For", "In", "On", "At", "By", "If", "When",
    "Then", "With", "Since", "Zero", "Not", "Whether", "Up",
];

/// A figure a document states as a count, and the word that makes it one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Count {
    line: usize,
    figure: String,
    /// The counted noun, `of` for `N of M`, `%` for a percentage.
    noun: String,
    anchored: bool,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_number_word(lower: &str) -> bool {
    match lower.split('-').collect::<Vec<_>>().as_slice() {
        [word] => BIG_WORDS.contains(word) || TENS.contains(word),
        [tens, unit] => TENS.contains(tens) && UNITS.contains(unit),
        _ => false,
    }
}

/// A plural noun: it ends in `s`, and is neither a verb nor a possessive nor a word that merely ends so.
fn is_plural(word: &str) -> bool {
    let w = word.to_lowercase();
    w.len() > 3
        && w.ends_with('s')
        && !w.ends_with("ss")
        && !w.ends_with("us")
        && !w.ends_with("is")
        && !w.contains(['\'', '’'])
        && !NOT_A_NOUN.contains(&w.as_str())
}

fn is_iso_date(chars: &[char]) -> bool {
    chars.len() == 10
        && chars.iter().enumerate().all(|(k, &c)| match k {
            4 | 7 => c == '-',
            _ => c.is_ascii_digit(),
        })
}

/// The line with link targets gone: what a reader reads, and where it cites.
fn without_link_targets(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ']' && chars.get(i + 1) == Some(&'(') {
            if let Some(len) = chars[i + 2..].iter().position(|&c| c == ')') {
                out.push(']');
                i += len + 3;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The line with each code span, date and quoted message one marker, so no figure inside one is read.
fn masked(line: &str) -> String {
    let chars: Vec<char> = without_link_targets(line).chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' || c == '"' {
            if let Some(len) = chars[i + 1..].iter().position(|&x| x == c) {
                out.push(if c == '`' { CODE } else { QUOTED });
                i += len + 2;
                continue;
            }
        }
        if c.is_ascii_digit()
            && i + 10 <= chars.len()
            && is_iso_date(&chars[i..i + 10])
            && (i == 0 || !is_word_char(chars[i - 1]))
        {
            out.push(DATE);
            i += 10;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The numeral or range starting at `from`, and where it ends.
fn numeral(chars: &[char], from: usize) -> (String, usize) {
    let digits = |mut j: usize| {
        while j < chars.len()
            && (chars[j].is_ascii_digit()
                || (chars[j] == ',' && chars.get(j + 1).is_some_and(char::is_ascii_digit)))
        {
            j += 1;
        }
        j
    };
    let mut end = digits(from);
    // a decimal, or the far end of a range
    if matches!(chars.get(end), Some('.' | '–' | '-'))
        && chars.get(end + 1).is_some_and(char::is_ascii_digit)
    {
        end = digits(end + 1);
    }
    (chars[from..end].iter().collect(), end)
}

/// The word that ends at `end`, skipping the space before it.
fn word_before(chars: &[char], end: usize) -> String {
    let upto: String = chars[..end].iter().collect();
    upto.trim_end()
        .chars()
        .rev()
        .take_while(|&c| c.is_alphanumeric() || c == '\'' || c == '-')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Whether nothing but a clause opener's punctuation lies before the word that ends at `end`.
fn opens_a_clause(chars: &[char], word: &str, end: usize) -> bool {
    let upto: String = chars[..end].iter().collect();
    let trimmed = upto.trim_end();
    let head = trimmed[..trimmed.len().saturating_sub(word.len())].trim_end();
    head.is_empty() || head.ends_with(['.', '!', '?', ':', '|', '*'])
}

/// The noun a figure counts, from the words after it: `%`, `of`, or a plural within two words.
fn counted_noun(rest: &[char]) -> Option<String> {
    if rest.first() == Some(&'%') {
        return Some("%".to_string());
    }
    let mut words: Vec<Option<String>> = Vec::new();
    let mut k = 0;
    while words.len() < 4 {
        while rest.get(k) == Some(&' ') {
            k += 1;
        }
        match rest.get(k) {
            Some(&CODE) => {
                words.push(None);
                k += 1;
            }
            Some(c) if c.is_alphabetic() => {
                let start = k;
                while k < rest.len()
                    && (rest[k].is_alphabetic() || matches!(rest[k], '\'' | '’' | '-'))
                {
                    k += 1;
                }
                words.push(Some(rest[start..k].iter().collect()));
            }
            _ => break,
        }
    }
    if words
        .first()
        .is_some_and(|w| w.as_deref().is_some_and(|w| w.eq_ignore_ascii_case("of")))
    {
        return Some("of".to_string());
    }
    let mut seen = 0;
    for word in words.iter().flatten() {
        let lower = word.to_lowercase();
        if NOT_A_NOUN.contains(&lower.as_str()) {
            return None;
        }
        if is_plural(word) {
            return Some(lower);
        }
        seen += 1;
        if seen == 2 {
            return None;
        }
    }
    None
}

/// The counts one masked line states, as (figure, noun).
///
/// ⚠ A figure that names a place or a thing (`bit 3`, `version 12`, `Phase 7`, `R66`), a year, a
/// path, a dimension, a decimal, a quoted message, or a number word below eleven is not read: prose
/// says "two archives" for a pair as often as for a count, and a grammar that cannot tell them apart
/// would flag every pair.
fn counts_in(prose: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = prose.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let before = if i == 0 { ' ' } else { chars[i - 1] };
        let (figure, end, spelled) = if c.is_ascii_digit() {
            let (figure, end) = numeral(&chars, i);
            (figure, end, false)
        } else if c.is_alphabetic() && !is_word_char(before) {
            let mut end = i;
            while end < chars.len() && (chars[end].is_alphabetic() || chars[end] == '-') {
                end += 1;
            }
            let word: String = chars[i..end].iter().collect();
            if !is_number_word(&word.to_lowercase()) {
                i = end;
                continue;
            }
            (word, end, true)
        } else {
            i += 1;
            continue;
        };
        let start = i;
        i = end.max(start + 1);

        if !spelled {
            if before.is_alphabetic()
                || matches!(before, '#' | '.' | '/' | ':' | '@' | '-' | '×' | '_')
            {
                continue;
            }
            let after = chars.get(end).copied();
            let is_version_or_measure = figure.contains('.') && after != Some('%');
            if is_version_or_measure || after.is_some_and(|x| is_word_char(x) || x == '×') {
                continue;
            }
            if figure.len() == 4
                && figure
                    .parse::<u32>()
                    .is_ok_and(|y| (1900..=2099).contains(&y))
            {
                continue;
            }
            let label = word_before(&chars, start);
            if LABELS.contains(&label.to_lowercase().as_str()) {
                continue;
            }
            if label.chars().next().is_some_and(char::is_uppercase)
                && !CLAUSE_OPENERS.contains(&label.as_str())
                && !opens_a_clause(&chars, &label, start)
            {
                continue;
            }
        }
        if let Some(noun) = counted_noun(&chars[end..]) {
            found.push((figure, noun));
        }
    }
    found
}

/// Whether the text names a revision, a date, a pull request, a command or a suite in `suites`.
fn names_a_source(unit: &str, suites: &BTreeSet<String>) -> bool {
    let chars: Vec<char> = unit.chars().collect();
    let dated = (0..chars.len().saturating_sub(9))
        .any(|i| is_iso_date(&chars[i..i + 10]) && (i == 0 || !is_word_char(chars[i - 1])));
    let pulled = chars.iter().enumerate().any(|(i, &c)| {
        c == '#'
            && (i == 0 || !is_word_char(chars[i - 1]))
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
    });
    dated
        || pulled
        || unit.split('`').skip(1).step_by(2).any(|span| {
            (span.len() == 8
                && span
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)))
                || suites.contains(span.trim_end_matches("()"))
                || ["openrage-cli ", "just ", "cargo "]
                    .iter()
                    .any(|command| span.starts_with(command))
                || span.contains("/tests/")
        })
}

/// A document's lines in the units a figure is anchored by, each with its line number.
///
/// A unit is a paragraph, which a heading or a line ending in a colon joins to the paragraph after
/// it, or, for a Plan or a Record, everything under one heading. Fenced code is not prose.
fn units(text: &str, by_item: bool) -> Vec<Vec<(usize, String)>> {
    let mut chunks: Vec<Vec<(usize, String)>> = Vec::new();
    let mut current: Vec<(usize, String)> = Vec::new();
    let mut fenced = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let heading = line.starts_with('#');
        let blank = line.trim().is_empty();
        if (heading || blank && !by_item) && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if blank {
            continue;
        }
        current.push((index + 1, line.to_string()));
        if heading && !by_item {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if by_item {
        return chunks;
    }
    let mut merged: Vec<Vec<(usize, String)>> = Vec::new();
    let mut joins_next = false;
    for chunk in chunks {
        let leads = (chunk.len() == 1 && chunk[0].1.starts_with('#'))
            || chunk
                .last()
                .is_some_and(|(_, l)| l.trim_end().ends_with(':'));
        match merged.last_mut() {
            Some(last) if joins_next => last.extend(chunk),
            _ => merged.push(chunk),
        }
        joins_next = leads;
    }
    merged
}

fn counts(text: &str, by_item: bool, suites: &BTreeSet<String>) -> Vec<Count> {
    let mut out = Vec::new();
    for unit in units(text, by_item) {
        let whole = unit
            .iter()
            .map(|(_, line)| without_link_targets(line))
            .collect::<Vec<_>>()
            .join("\n");
        let anchored = names_a_source(&whole, suites);
        for (line, text) in &unit {
            for (figure, noun) in counts_in(&masked(text)) {
                out.push(Count {
                    line: *line,
                    figure,
                    noun,
                    anchored,
                });
            }
        }
    }
    out
}

/// Every test function and every integration-test file in the tree, by name.
fn suites() -> &'static BTreeSet<String> {
    static SUITES: OnceLock<BTreeSet<String>> = OnceLock::new();
    SUITES.get_or_init(|| {
        let mut names = BTreeSet::new();
        for path in rust_sources() {
            if path.components().any(|c| c.as_os_str() == "tests") {
                if let Some(stem) = path.file_stem() {
                    names.insert(stem.to_string_lossy().to_string());
                }
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut after_test_attribute = false;
            for line in text.lines().map(str::trim) {
                if line.starts_with("#[test]") || line.starts_with("#[tokio::test") {
                    after_test_attribute = true;
                } else if after_test_attribute && !line.starts_with("#[") {
                    let declared = line
                        .strip_prefix("pub fn ")
                        .or_else(|| line.strip_prefix("async fn "))
                        .or_else(|| line.strip_prefix("fn "));
                    if let Some(rest) = declared {
                        names.insert(rest.chars().take_while(|&c| is_word_char(c)).collect());
                    }
                    after_test_attribute = false;
                }
            }
        }
        names
    })
}

/// The committed documents the gate reads: `docs/`, `README.md`, `AGENTS.md` and every crate's.
fn documents() -> Vec<(String, String)> {
    let mut paths = vec!["README.md".to_string(), "AGENTS.md".to_string()];
    for top in ["docs", "crates"] {
        walk(&repo_root().join(top), &mut |path| {
            if path.extension().is_some_and(|e| e == "md") {
                if let Ok(rel) = path.strip_prefix(repo_root()) {
                    paths.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        });
    }
    paths
        .into_iter()
        .map(|rel| {
            let text = read(&rel);
            (rel, text)
        })
        .collect()
}

fn counts_of(document: &str, text: &str) -> Vec<Count> {
    counts(text, BY_ITEM.contains(&document), suites())
}

fn is_waived(document: &str, noun: &str) -> bool {
    ASSERTED
        .iter()
        .chain(WAIVED)
        .any(|(d, n, _)| *d == document && *n == noun)
}

/// The one figure `document` states as a count of `noun`.
fn stated(document: &str, noun: &str) -> u64 {
    let figures: BTreeSet<String> = counts_of(document, &read(document))
        .into_iter()
        .filter(|count| count.noun == noun)
        .map(|count| count.figure)
        .collect();
    assert_eq!(
        figures.len(),
        1,
        "{document} states {figures:?} as counts of `{noun}`, and a test can hold only one"
    );
    let figure = figures.iter().next().expect("one figure");
    figure
        .replace(',', "")
        .parse()
        .unwrap_or_else(|_| panic!("`{figure}` is not a whole number"))
}

/// The text of `path` after `marker`, up to the next `;` or `)`: where a constant is written.
fn source_after(path: &str, marker: &str) -> String {
    let text = read(path);
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("`{marker}` is not in {path} any more"));
    text[at + marker.len()..]
        .chars()
        .take_while(|c| !matches!(c, ';' | ')' | '{' | ','))
        .collect::<String>()
        .trim()
        .replace('_', "")
}

#[test]
fn every_count_a_document_states_is_asserted_or_a_named_measurement() {
    let documents = documents();
    let all: Vec<(String, Count)> = documents
        .iter()
        .flat_map(|(rel, text)| counts_of(rel, text).into_iter().map(|c| (rel.clone(), c)))
        .collect();
    assert!(
        documents.len() > 20 && all.len() > 300,
        "{} count(s) in {} document(s) is not this documentation: the parse is broken rather than \
         the prose",
        all.len(),
        documents.len()
    );

    let unheld: Vec<String> = all
        .iter()
        .filter(|(rel, count)| !count.anchored && !is_waived(rel, &count.noun))
        .map(|(rel, count)| format!("  {rel}:{}: {} {}", count.line, count.figure, count.noun))
        .collect();
    assert!(
        unheld.is_empty(),
        "these documents state a count that no test holds and that names no revision, date, pull \
         request, command or suite beside it:\n{}\n\n\
         docs/guides/Documentation_Standard.md#figures says what each kind of figure takes. Name the \
         suite and revision it was measured at, or the test that asserts it (and list it in \
         ASSERTED in src/doc_counts/figures.rs), or delete the figure.",
        unheld.join("\n")
    );
}

/// Each ledger entry still excuses a count and names a test that exists.
#[test]
fn the_ledgers_excuse_only_counts_a_document_states() {
    let mut listed = BTreeSet::new();
    let mut stale = Vec::new();
    for (document, noun, why) in ASSERTED.iter().chain(WAIVED) {
        if !listed.insert((*document, *noun)) {
            stale.push(format!("  {document}: `{noun}` is listed twice"));
        }
        if why.is_empty() {
            stale.push(format!("  {document}: `{noun}` has no reason"));
        }
        let unanchored = counts_of(document, &read(document))
            .iter()
            .any(|count| !count.anchored && count.noun == *noun);
        if !unanchored {
            stale.push(format!(
                "  {document}: no unanchored count of `{noun}` is left to excuse; delete the entry"
            ));
        }
    }
    let tests: BTreeSet<String> = rust_sources()
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| line.trim().strip_prefix("fn ").map(str::to_string))
                .map(|rest| {
                    rest.chars()
                        .take_while(|&c| is_word_char(c))
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (document, noun, test) in ASSERTED {
        if !tests.contains(*test) {
            stale.push(format!(
                "  {document}: `{noun}` names `{test}`, which is not a function in the tree"
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "the ledgers in src/doc_counts/figures.rs are out of date:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_count_check_can_tell_its_shapes_apart() {
    let read_as = |line: &str| counts_in(&masked(line));
    let pair = |figure: &str, noun: &str| (figure.to_string(), noun.to_string());

    assert_eq!(
        read_as("holds 9,801 of 10,142 archives"),
        vec![pair("9,801", "of"), pair("10,142", "archives")],
        "a ratio's numerator and its denominator are each a count"
    );
    assert_eq!(
        read_as("a retail install has 391 presets"),
        vec![pair("391", "presets")],
        "a plural noun"
    );
    assert_eq!(
        read_as("a 25 distinct `x` regions"),
        vec![pair("25", "regions")],
        "a code span between"
    );
    assert_eq!(
        read_as("names 99.5% of them"),
        vec![pair("99.5", "%")],
        "a percentage may be decimal"
    );
    assert_eq!(
        read_as("is 400–550 lines"),
        vec![pair("400–550", "lines")],
        "a range is one figure"
    );
    assert_eq!(
        read_as("ninety-two items"),
        vec![pair("ninety-two", "items")],
        "a number word from eleven up"
    );
    assert_eq!(
        read_as("Forty rows sit"),
        vec![pair("Forty", "rows")],
        "a capital opens a clause"
    );

    for (line, why) in [
        ("bit 3 of the flags", "a position"),
        ("RSC7 version 12 files", "a version"),
        ("see Phase 7 files", "a name with a number"),
        ("R66 rows and D15 items", "an identifier"),
        ("lands in 2026 builds", "a year"),
        ("on 2026-10-05 entries", "a date"),
        ("0×0 textures", "a dimension"),
        ("egui 0.34 deprecations", "a decimal"),
        ("two archives and three rows", "a number word below eleven"),
        ("the `10,000 elements` span", "a code span"),
        ("shows \"1 models\" in the report", "a quoted message"),
        ("a 16-byte row and 32 bytes", "a size"),
        ("goes 13 → 5 while it grows", "no noun"),
        ("a file holding 2 still reads", "a verb"),
    ] {
        assert_eq!(
            read_as(line),
            Vec::<(String, String)>::new(),
            "{why} is not a count: `{line}`"
        );
    }
}

#[test]
fn the_anchor_check_can_tell_its_shapes_apart() {
    let suites: BTreeSet<String> = ["a_real_suite".to_string()].into();
    let anchored = |unit: &str| names_a_source(&without_link_targets(unit), &suites);

    for (unit, why) in [
        ("measured at `67a47fd6`", "a revision"),
        ("measured on 2026-10-05", "a date"),
        ("measured in #44", "a pull request"),
        ("measured by `a_real_suite`", "a suite that exists"),
        ("measured by `a_real_suite()`", "a suite named as a call"),
        ("run `openrage-cli depends census`", "a command"),
    ] {
        assert!(anchored(unit), "{why} anchors a count: `{unit}`");
    }
    for (unit, why) in [
        (
            "measured by `a_made_up_suite`",
            "a suite that does not exist",
        ),
        (
            "measured at `67A47FD6` or `67a47fd`",
            "a hash of the wrong shape",
        ),
        ("see [the plan](#r66-the-or-io-crate)", "a link anchor"),
    ] {
        assert!(
            !names_a_source(&without_link_targets(unit), &suites),
            "{why} anchors nothing: `{unit}`"
        );
    }

    let text = "# Title\n\nThe tree has 12 crates.\n\nA second paragraph names `a_real_suite`.\n\n\
                Ten of 11 files, in `a_real_suite`:\n\n| file | count |\n|---|---|\n| x | 14 files |\n\n\
                ```\n99 lines in a fence\n```\n";
    let by_paragraph: Vec<(String, bool)> = counts(text, false, &suites)
        .into_iter()
        .map(|c| (c.figure, c.anchored))
        .collect();
    assert_eq!(
        by_paragraph,
        [
            ("12".to_string(), false),
            ("11".to_string(), true),
            ("14".to_string(), true)
        ],
        "a paragraph anchors only itself, a colon joins the table after it, a fence is not prose"
    );
    let by_item: Vec<bool> = counts(text, true, &suites)
        .into_iter()
        .map(|c| c.anchored)
        .collect();
    assert_eq!(
        by_item,
        [true, true, true],
        "an item anchors everything under its heading"
    );
}

#[test]
fn a_gen9_vertex_declaration_has_the_slots_the_mesh_document_states() {
    assert_eq!(
        stated("docs/features/Mesh_Interchange.md", "slots"),
        or_io::codec::mesh::G9_ELEMENT_SLOTS as u64,
        "docs/features/Mesh_Interchange.md states the size of a Gen9 vertex declaration; \
         `or_io::codec::mesh::G9_ELEMENT_SLOTS` is it"
    );
}

#[test]
fn a_teleporting_emitter_jumps_the_distance_the_preview_document_states() {
    let in_code = source_after(
        "crates/runtime/or-simulation/src/emitter_inst.rs",
        "dist_travelled > ",
    );
    let in_code: f64 = in_code
        .parse()
        .expect("the jump distance is a literal in the emitter");
    assert_eq!(
        stated("docs/features/Particle_Preview.md", "units") as f64,
        in_code,
        "docs/features/Particle_Preview.md states the distance after which an emitter with a rate over \
         distance spawns nothing; the emitter's own test is the value"
    );
}

#[test]
fn the_thumbnail_cache_holds_the_tiles_the_roadmap_states() {
    let in_code: u64 = source_after("src/render/thumbnails.rs", "const CAPACITY: usize = ")
        .parse()
        .expect("the cache capacity is a literal");
    assert_eq!(
        stated("docs/direction/Roadmap.md", "tiles"),
        in_code,
        "docs/direction/Roadmap.md states how many tiles the thumbnail cache keeps"
    );
}

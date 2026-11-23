//! What a person reads about a build says Content and Build: the words `module` and `output` are the
//! code's, so no sentence of the plan, the container, the merge, the build or the commands holds one.
//!
//! The scan reads the string literals of the source files that write those sentences, leaving out
//! tests, `{placeholders}`, a name in backticks (a key of the project file, which is spelt as it is
//! stored) and a `--flag`.

use std::path::{Path, PathBuf};

/// Files that write sentences a person reads, from the root of the workspace.
const FILES: &[&str] = &[
    "crates/domain/or-project/src/plan.rs",
    "crates/domain/or-project/src/merge.rs",
    "crates/domain/or-project/src/container.rs",
    "crates/domain/or-project/src/build.rs",
    "crates/domain/or-project/src/dlc.rs",
    "crates/domain/or-project/src/oiv.rs",
    "crates/domain/or-project/src/split.rs",
    "crates/domain/or-project/src/diff.rs",
    "crates/tools/or-cli/src/project.rs",
    "src/state/workbench/builds.rs",
];

/// The keys of the project file, which are not sentences.
const KEYS: &[&str] = &["modules", "outputs"];

const CODE_WORDS: &[&str] = &["module", "modules", "output", "outputs"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-project")
        .to_path_buf()
}

/// The string literals of `source` before its test module, with the line each begins on.
fn literals(source: &str) -> Vec<(usize, String)> {
    let code = match source.find("\n#[cfg(test)]") {
        Some(at) => &source[..at],
        None => source,
    };
    let chars: Vec<char> = code.chars().collect();
    let mut found = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
            }
            '\'' => {
                // A character literal such as '"' or '\'', else a lifetime.
                if chars.get(i + 1) == Some(&'\\') {
                    i += 2;
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                } else if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            'r' if matches!(chars.get(i + 1), Some('"' | '#'))
                && (i == 0 || !chars[i - 1].is_alphanumeric()) =>
            {
                let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
                if chars.get(i + 1 + hashes) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let start_line = line;
                let mut j = i + 2 + hashes;
                let mut text = String::new();
                while j < chars.len() {
                    let closes = chars[j] == '"'
                        && chars[j + 1..].iter().take(hashes).all(|c| *c == '#')
                        && chars.len() >= j + 1 + hashes;
                    if closes {
                        break;
                    }
                    if chars[j] == '\n' {
                        line += 1;
                    }
                    text.push(chars[j]);
                    j += 1;
                }
                found.push((start_line, text));
                i = j + 1 + hashes;
            }
            '"' => {
                let start_line = line;
                let mut j = i + 1;
                let mut text = String::new();
                while j < chars.len() && chars[j] != '"' {
                    if chars[j] == '\\' {
                        j += 1;
                        match chars.get(j) {
                            Some('\n') => {
                                line += 1;
                                while chars.get(j + 1).is_some_and(|c| c.is_whitespace()) {
                                    if chars[j + 1] == '\n' {
                                        line += 1;
                                    }
                                    j += 1;
                                }
                            }
                            Some('n') => text.push('\n'),
                            Some(other) => text.push(*other),
                            None => {}
                        }
                    } else {
                        if chars[j] == '\n' {
                            line += 1;
                        }
                        text.push(chars[j]);
                    }
                    j += 1;
                }
                found.push((start_line, text));
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    found
}

/// `text` as a person reads it: no `{placeholder}`, no name in backticks, no `--flag`.
fn spoken(text: &str) -> String {
    let (mut out, mut in_braces, mut in_ticks) = (String::new(), false, false);
    for c in text.chars() {
        match c {
            '{' => in_braces = true,
            '}' if in_braces => in_braces = false,
            '`' if !in_braces => in_ticks = !in_ticks,
            _ if in_braces || in_ticks => {}
            c => out.push(c),
        }
    }
    out.split_whitespace()
        .filter(|word| !word.starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn code_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|word| CODE_WORDS.contains(&word.to_ascii_lowercase().as_str()))
        .map(str::to_string)
        .collect()
}

#[test]
fn the_scan_reads_a_sentence_and_leaves_a_key_a_placeholder_and_a_flag_alone() {
    let source = concat!(
        r##"
        const KEY: &str = "outputs";
        fn f() {
            let a = format!("output `{id}` takes {module}, not a module id");
            let b = "name one with --output or `modules`";
            let c = r#"a "raw" output"#;
            let d = '"';
            let e = "a continued \
                     sentence about a module";
            // "a comment about a module"
        }"##,
        "\n#[cfg(test)]\nmod tests { fn t() { \"a test about an output\"; } }"
    );
    let found: Vec<(usize, String)> = literals(source)
        .into_iter()
        .filter(|(_, text)| !KEYS.contains(&text.as_str()))
        .map(|(line, text)| (line, spoken(&text)))
        .filter(|(_, text)| !code_words(text).is_empty())
        .collect();
    let lines: Vec<usize> = found.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, [4, 6, 8], "{found:?}");
    assert_eq!(found[0].1, "output takes , not a module id");
}

#[test]
fn no_sentence_of_a_plan_a_container_a_merge_or_a_build_holds_module_or_output() {
    let mut found = Vec::new();
    for file in FILES {
        let source = std::fs::read_to_string(root().join(file)).unwrap_or_else(|e| {
            panic!("{file}: {e}");
        });
        for (line, text) in literals(&source) {
            if KEYS.contains(&text.as_str()) {
                continue;
            }
            let said = spoken(&text);
            if !code_words(&said).is_empty() {
                found.push(format!("{file}:{line}: {said}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} sentence(s) say module or output where the screen says Content and Build:\n{}",
        found.len(),
        found.join("\n")
    );
}

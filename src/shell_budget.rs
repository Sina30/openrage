use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const GATE: usize = 900;

const DECIDED: &[(&str, &str)] = &[(
    "ui/panels/text_editor/mod.rs",
    "the paint-and-input hot path for a virtualized editor; folds, search and \
     highlighting already live beside it, and splitting the per-frame loop \
     across modules costs more than it documents",
)];

const NOT_YET_SPLIT: &[(&str, usize, &str)] = &[];

fn code_lines(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let line_count = text.lines().count().max(1);
    let mut has_code = vec![false; line_count + 1];
    let mut test_ranges: Vec<(usize, usize)> = Vec::new();

    let mut line = 0usize;
    let mut depth: i32 = 0;
    let mut pending: Option<(usize, i32, bool)> = None;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut block = 1usize;
            i += 2;
            while i < chars.len() && block > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    block += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    block -= 1;
                    i += 2;
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }

        has_code[line] = true;

        if c == '#' && matches!(chars.get(i + 1), Some(&'[') | Some(&'!')) {
            let open = if chars[i + 1] == '!' { i + 2 } else { i + 1 };
            if chars.get(open) == Some(&'[') {
                let mut bracket = 0i32;
                let mut j = open;
                let mut attr = String::new();
                while j < chars.len() {
                    match chars[j] {
                        '[' => bracket += 1,
                        ']' => bracket -= 1,
                        '\n' => line += 1,
                        _ => {}
                    }
                    if !chars[j].is_whitespace() {
                        attr.push(chars[j]);
                    }
                    has_code[line] = true;
                    j += 1;
                    if bracket == 0 {
                        break;
                    }
                }
                if attr == "[cfg(test)]" && pending.is_none() {
                    pending = Some((line, depth, false));
                }
                i = j;
                continue;
            }
        }

        if c == 'r' || c == 'b' {
            let mut j = i;
            if chars[j] == 'b' {
                j += 1;
            }
            if chars.get(j) == Some(&'r') {
                j += 1;
                let mut hashes = 0usize;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'"') {
                    j += 1;
                    loop {
                        if j >= chars.len() {
                            break;
                        }
                        if chars[j] == '\n' {
                            line += 1;
                            has_code[line] = true;
                        }
                        if chars[j] == '"' {
                            let closed = (1..=hashes).all(|k| chars.get(j + k) == Some(&'#'));
                            if closed {
                                j += hashes + 1;
                                break;
                            }
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            }
        }
        if c == '"' {
            i += 1;
            while i < chars.len() {
                match chars[i] {
                    '\\' => i += 1,
                    '"' => break,
                    '\n' => {
                        line += 1;
                        has_code[line] = true;
                    }
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            let is_char = chars.get(i + 1) == Some(&'\\')
                || (chars.get(i + 2) == Some(&'\'') && chars.get(i + 1).is_some());
            if is_char {
                i += 1;
                while i < chars.len() {
                    match chars[i] {
                        '\\' => i += 1,
                        '\'' => break,
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
            } else {
                i += 1;
            }
            continue;
        }

        if c == '{' {
            depth += 1;
            if let Some((_, _, opened)) = pending.as_mut() {
                *opened = true;
            }
        } else if c == '}' {
            depth -= 1;
            if let Some((start, base, true)) = pending {
                if depth <= base {
                    test_ranges.push((start, line));
                    pending = None;
                }
            }
        } else if c == ';' {
            if let Some((start, base, false)) = pending {
                if depth <= base {
                    test_ranges.push((start, line));
                    pending = None;
                }
            }
        }
        i += 1;
    }

    if let Some((start, _, _)) = pending {
        test_ranges.push((start, line));
    }

    (0..line_count)
        .filter(|l| has_code[*l])
        .filter(|l| !test_ranges.iter().any(|(a, b)| l >= a && l <= b))
        .count()
}

fn measure_src() -> Vec<(String, usize)> {
    let root = src_root();
    let mut out = Vec::new();
    walk(&root, &mut |path| {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, code_lines(&text)));
    });
    out.sort();
    out
}

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            f(&path);
        }
    }
}

#[test]
fn no_file_in_src_exceeds_the_code_line_gate() {
    let measured = measure_src();
    assert!(
        measured.len() > 50,
        "the gate walked {} file(s) under {}, which is too few to be the app \
         crate — it is measuring the wrong tree and would pass on anything",
        measured.len(),
        src_root().display()
    );

    let mut breaches = Vec::new();
    for (rel, count) in &measured {
        if DECIDED.iter().any(|(p, _)| p == rel) {
            continue;
        }
        if let Some((_, budget, owner)) = NOT_YET_SPLIT.iter().find(|(p, _, _)| p == rel) {
            if count > budget {
                breaches.push(format!(
                    "  {rel}: {count} code lines, up from {budget}. Already over \
                     the gate and held to its own count until it is split \
                     ({owner}). Adding to it is not available."
                ));
            }
            continue;
        }
        if *count > GATE {
            breaches.push(format!("  {rel}: {count} code lines, over the {GATE} gate"));
        }
    }

    assert!(
        breaches.is_empty(),
        "docs/reference/Crate_Reference.md records a {GATE}-code-line gate on src/:\n{}\n\n\
         Split the file. If it genuinely should not be split, add it to \
         DECIDED in src/shell_budget.rs with the reason — not to \
         NOT_YET_SPLIT, which is for files due to be split.",
        breaches.join("\n")
    );
}

#[test]
fn the_not_yet_split_list_shrinks_and_never_lies() {
    let measured = measure_src();
    let mut stale = Vec::new();

    for (rel, _, _) in NOT_YET_SPLIT {
        match measured.iter().find(|(p, _)| p == rel) {
            None => stale.push(format!("  {rel}: listed, but no such file under src/")),
            Some((_, count)) if *count <= GATE => stale.push(format!(
                "  {rel}: now {count} code lines, under the {GATE} gate. Delete the entry."
            )),
            Some(_) => {}
        }
    }
    for (rel, _) in DECIDED {
        match measured.iter().find(|(p, _)| p == rel) {
            None => stale.push(format!("  {rel}: listed in DECIDED, but no such file")),
            Some((_, count)) if *count <= GATE => stale.push(format!(
                "  {rel}: DECIDED, but now {count} code lines and under the {GATE} \
                 gate. The exception is no longer doing anything — delete it."
            )),
            Some(_) => {}
        }
    }

    assert!(
        stale.is_empty(),
        "the exception lists in src/shell_budget.rs no longer describe the \
         tree:\n{}\n\n\
         An entry that has stopped breaching has to leave the list. A \
         grandfather list nobody is required to shrink stops being a gate.",
        stale.join("\n")
    );
}

fn pin_for(rel: &str) -> Option<(usize, &'static str)> {
    if let Some((_, budget, _)) = NOT_YET_SPLIT.iter().find(|(p, _, _)| *p == rel) {
        return Some((*budget, "pinned"));
    }
    if DECIDED.iter().any(|(p, _)| *p == rel) {
        return None;
    }
    Some((GATE, "gate"))
}

#[test]
#[ignore = "reporting: prints the whole tree rather than asserting anything"]
fn report_the_tree_by_code_lines() {
    let root = src_root();
    let mut rows: Vec<(usize, usize, String)> = measure_src()
        .into_iter()
        .map(|(rel, code)| {
            let raw = std::fs::read_to_string(root.join(&rel))
                .map(|t| t.lines().count())
                .unwrap_or(0);
            (code, raw, rel)
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));

    println!("{:>6}  {:>6}  file", "code", "raw");
    for (code, raw, rel) in rows.iter().take(40) {
        let note = match pin_for(rel) {
            None => "  (decided exception)".to_string(),
            Some((budget, kind)) => match code.cmp(&budget) {
                std::cmp::Ordering::Less => format!("  ({kind} {budget}, -{})", budget - code),
                std::cmp::Ordering::Equal => format!("  ({kind} {budget})"),
                std::cmp::Ordering::Greater => format!("  ({kind} {budget}, +{})", code - budget),
            },
        };
        println!("{code:>6}  {raw:>6}  {rel}{note}");
    }

    let shrunk: Vec<String> = NOT_YET_SPLIT
        .iter()
        .filter_map(|(rel, budget, _)| {
            let (code, _, _) = rows.iter().find(|(_, _, r)| r == rel)?;
            (code < budget)
                .then(|| format!("  {rel}: {code} (pinned {budget}, -{})", budget - code))
        })
        .collect();
    if !shrunk.is_empty() {
        println!(
            "\n{} pinned file(s) below their pin. Lowering a pin is deliberate, in \
             the commit that earns it — the pins do not ratchet, so this is a \
             report and not a failure:\n{}",
            shrunk.len(),
            shrunk.join("\n")
        );
    }

    let total: usize = rows.iter().map(|r| r.0).sum();
    let raw_total: usize = rows.iter().map(|r| r.1).sum();
    println!(
        "\n{} files, {total} code lines of {raw_total} raw",
        rows.len()
    );
}

#[test]
fn code_lines_counts_what_the_gate_says_it_counts() {
    let sample = "\
//! A module doc.

/// An item doc.
fn a() {
    // An ordinary comment.
    let x = 1; // A trailing comment, on a line that is still code.

    /* a /* nested */ block comment */
    let y = 2;
}
";
    assert_eq!(code_lines(sample), 4, "sample:\n{sample}");

    let with_tests = "\
fn a() {}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let s = \"}\";
        assert_eq!(s, \"}\");
    }
}
";
    assert_eq!(code_lines(with_tests), 1, "sample:\n{with_tests}");

    let bare = "\
fn a() {}
#[cfg(test)]
use std::fmt::Debug;
#[cfg(test)]
fn helper() {
    let _ = 1;
}
fn b() {}
";
    assert_eq!(code_lines(bare), 2, "sample:\n{bare}");

    let tricky = "\
fn a<'life>(_: &'life str) {
    let _ = '{';
    let _ = \"} // not a comment\";
    let _ = r#\"a \"quoted\" } brace\"#;
}
#[cfg(test)]
mod tests {
    fn t() {}
}
fn b() {}
";
    assert_eq!(code_lines(tricky), 6, "sample:\n{tricky}");

    assert_eq!(code_lines("//! Only a doc.\n\n// And a comment.\n"), 0);
}

#[test]
fn the_gate_reproduces_the_arguments_shape() {
    let root = src_root();
    let read = |rel: &str| -> (usize, usize) {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel} is named by Crate_Reference.md's argument: {e}"));
        (text.lines().count(), code_lines(&text))
    };

    let (cap_raw, cap_code) = read("state/capabilities.rs");
    assert!(
        cap_code * 2 < cap_raw,
        "state/capabilities.rs is the plan's example of a file that is mostly \
         docs and tests, and it is now {cap_code} code of {cap_raw} raw. If \
         that has genuinely changed, the argument in Crate_Reference.md needs \
         rewriting, not this assertion relaxing."
    );

    for name in [
        "ui/panels/inspector/ypt.rs",
        "ui/panels/hierarchy/ypt/mod.rs",
    ] {
        let (raw, code) = read(name);
        assert!(
            code * 4 > raw * 3,
            "{name} is one of the plan's two examples of a file that is \
             almost all code, and it is now {code} code of {raw} raw."
        );
    }

    assert!(
        cap_raw > GATE && cap_code <= GATE,
        "state/capabilities.rs is the plan's case for counting code: a raw \
         gate at {GATE} flags it and a code gate should not. It is now \
         {cap_code} code of {cap_raw} raw."
    );
    let caught: Vec<String> = measure_src()
        .into_iter()
        .filter(|(_, code)| *code > GATE)
        .map(|(rel, code)| format!("{rel} ({code})"))
        .collect();
    assert!(
        !caught.is_empty(),
        "the argument needs a file the code gate catches and the raw gate's \
         false positive to sit beside. Nothing in src/ is over {GATE} code \
         lines any more, which means R13's split is finished and this test \
         should be deleted along with NOT_YET_SPLIT."
    );
}

/// The name a `mod name;` line declares; an inline `mod name {` declares no file.
fn declared_module(line: &str) -> Option<&str> {
    let code = line.split("//").next()?.trim_end().strip_suffix(';')?;
    let mut words = code.split_whitespace();
    let first = words.next()?;
    let keyword = if first.starts_with("pub") {
        words.next()?
    } else {
        first
    };
    let name = words.next()?;
    (keyword == "mod" && words.next().is_none()).then_some(name)
}

/// Every file the two binaries compile, found the way rustc finds them.
///
/// ⚠ Only a declaration at column 0 is read, which is where rustfmt leaves a
/// `mod name;` in a file; one indented inside an inline module would resolve
/// from another directory and is not followed.
fn declared_files() -> BTreeSet<PathBuf> {
    let root = src_root();
    let mut pending: Vec<PathBuf> = ["main.rs", "cli_main.rs"]
        .iter()
        .map(|name| root.join(name))
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(file) = pending.pop() {
        let Ok(file) = file.canonicalize() else {
            continue;
        };
        if !seen.insert(file.clone()) {
            continue;
        }
        let (Ok(text), Some(dir), Some(name)) = (
            std::fs::read_to_string(&file),
            file.parent(),
            file.file_stem().and_then(|s| s.to_str()),
        ) else {
            continue;
        };
        let children = if matches!(name, "mod" | "main" | "cli_main") {
            dir.to_path_buf()
        } else {
            dir.join(name)
        };
        let mut path_attribute: Option<&str> = None;
        for line in text.lines() {
            if let Some(path) = line
                .strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
            {
                path_attribute = Some(path);
                continue;
            }
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            let Some(module) = declared_module(line) else {
                continue;
            };
            pending.push(match path_attribute.take() {
                Some(path) => dir.join(path),
                None => {
                    let flat = children.join(format!("{module}.rs"));
                    if flat.is_file() {
                        flat
                    } else {
                        children.join(module).join("mod.rs")
                    }
                }
            });
        }
    }
    seen
}

#[test]
fn every_rs_file_in_src_is_declared_by_a_module() {
    let declared = declared_files();
    assert!(
        declared.len() > 50,
        "the walk from main.rs found {} file(s), which is too few to be the \
         app crate — it is following the wrong declarations and would pass on \
         anything",
        declared.len()
    );

    let root = src_root();
    let mut undeclared = Vec::new();
    walk(&root, &mut |path| {
        if path.canonicalize().is_ok_and(|p| !declared.contains(&p)) {
            let rel = path.strip_prefix(&root).unwrap_or(path);
            undeclared.push(format!("  {}", rel.to_string_lossy().replace('\\', "/")));
        }
    });

    assert!(
        undeclared.is_empty(),
        "no `mod` declares these files, so nothing compiles or tests them:\n{}\n\n\
         Declare each from its parent module, or delete it.",
        undeclared.join("\n")
    );
}

#[test]
fn a_mod_line_declares_a_file_only_when_it_ends_in_a_semicolon() {
    assert_eq!(declared_module("mod a;"), Some("a"));
    assert_eq!(declared_module("pub mod a;"), Some("a"));
    assert_eq!(declared_module("pub(crate) mod a; // why"), Some("a"));
    assert_eq!(declared_module("mod a {"), None);
    assert_eq!(declared_module("use a::mod_b;"), None);
    assert_eq!(declared_module("// mod a;"), None);
}

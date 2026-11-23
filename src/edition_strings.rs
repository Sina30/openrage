//! Text a user reads names an edition, Legacy or Enhanced, through `Generation::edition_label()`;
//! `Gen8` and `Gen9` are the names code gives a resource's layout.
//!
//! Every string literal outside test code in the app and in `openrage-cli` counts, log and error
//! text included, because a user reads those too.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The source roots whose literals reach a user: the app and `openrage-cli`.
const ROOTS: &[&str] = &["src", "crates/tools/or-cli/src"];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

pub(crate) fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// A source file with its comments and literals blanked, and each literal with the line it opens on.
pub(crate) struct Lexed {
    pub(crate) code: Vec<u8>,
    literals: Vec<(usize, usize, String)>,
}

pub(crate) fn lex(src: &str) -> Lexed {
    let b = src.as_bytes();
    let mut code = b.to_vec();
    let mut literals = Vec::new();
    let blank = |code: &mut Vec<u8>, from: usize, to: usize| {
        for c in &mut code[from..to] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |p| i + p);
                blank(&mut code, start, i);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 0;
                while i < b.len() {
                    if b[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                blank(&mut code, start, i);
            }
            b'\'' => {
                if b.get(i + 1) == Some(&b'\\') {
                    i += 3;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                    i += 1;
                } else {
                    let width = src[i + 1..].chars().next().map_or(0, char::len_utf8);
                    // A lifetime has no closing quote after its first character.
                    i += if b.get(i + 1 + width) == Some(&b'\'') {
                        width + 2
                    } else {
                        1
                    };
                }
                blank(&mut code, start, i);
            }
            b'r' if (i == 0
                || !is_ident(b[i - 1])
                || (b[i - 1] == b'b' && (i == 1 || !is_ident(b[i - 2]))))
                && matches!(b.get(i + 1), Some(b'"' | b'#')) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let open = i + 2 + hashes;
                let mut close = open;
                while close < b.len()
                    && !(b[close] == b'"'
                        && b[close + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|&&c| c == b'#')
                            .count()
                            == hashes)
                {
                    close += 1;
                }
                if i == 0 || b[i - 1] != b'b' {
                    literals.push((
                        start,
                        line_of(b, start),
                        String::from_utf8_lossy(&b[open..close]).into_owned(),
                    ));
                }
                i = (close + 1 + hashes).min(b.len());
                blank(&mut code, start, i);
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                // A byte string is a file's bytes, never text a user reads.
                let bytes =
                    start > 0 && b[start - 1] == b'b' && (start == 1 || !is_ident(b[start - 2]));
                if !bytes {
                    literals.push((
                        start,
                        line_of(b, start),
                        String::from_utf8_lossy(&b[start + 1..i.min(b.len())]).into_owned(),
                    ));
                }
                i += 1;
                blank(&mut code, start, i.min(b.len()));
            }
            _ => i += 1,
        }
    }
    Lexed { code, literals }
}

fn line_of(b: &[u8], at: usize) -> usize {
    b[..at].iter().filter(|&&c| c == b'\n').count() + 1
}

/// The byte ranges of the items `#[cfg(test)]` gates, and the names of the out-of-line modules among them.
pub(crate) fn test_items(code: &[u8]) -> (Vec<(usize, usize)>, Vec<String>) {
    const ATTR: &[u8] = b"#[cfg(test)]";
    let mut ranges = Vec::new();
    let mut modules = Vec::new();
    let mut at = 0;
    while let Some(p) = code[at..].windows(ATTR.len()).position(|w| w == ATTR) {
        let start = at + p;
        let mut i = start + ATTR.len();
        while i < code.len() && code[i] != b';' && code[i] != b'{' {
            i += 1;
        }
        if code.get(i) == Some(&b';') {
            let item = String::from_utf8_lossy(&code[start + ATTR.len()..i]).into_owned();
            let words: Vec<&str> = item.split_whitespace().collect();
            if let [.., "mod", name] = words.as_slice() {
                modules.push((*name).to_string());
            }
        } else {
            let mut depth = 0;
            while i < code.len() {
                match code[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        ranges.push((start, i));
        at = i.max(start + 1);
    }
    (ranges, modules)
}

/// Whether `text` says `Gen8` or `Gen9` as a word, in any case and with or without a space.
fn names_a_generation(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    let t = t.as_bytes();
    (0..t.len()).any(|i| {
        if !t[i..].starts_with(b"gen") || (i > 0 && is_ident(t[i - 1])) {
            return false;
        }
        let mut j = i + 3;
        if t.get(j) == Some(&b' ') {
            j += 1;
        }
        matches!(t.get(j), Some(b'8' | b'9')) && !t.get(j + 1).is_some_and(|&c| is_ident(c))
    })
}

/// The files read under `roots`, and every literal outside test code as `(file, line, text)`.
pub(crate) fn user_facing_literals(roots: &[&str]) -> (usize, Vec<(String, usize, String)>) {
    let mut files = Vec::new();
    for root in roots {
        rust_files(&repo_root().join(root), &mut files);
    }
    files.sort();

    let mut test_modules = BTreeSet::new();
    let mut lexed = Vec::new();
    for path in files {
        let src =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let file = lex(&src);
        let (ranges, modules) = test_items(&file.code);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let dir = match stem {
            "main" | "lib" | "mod" => path.parent().map(Path::to_path_buf),
            _ => path.parent().map(|p| p.join(stem)),
        }
        .unwrap_or_default();
        test_modules.extend(modules.into_iter().map(|m| dir.join(m)));
        lexed.push((path, file.literals, ranges));
    }

    let mut literals = Vec::new();
    let scanned = lexed.len();
    for (path, file_literals, ranges) in lexed {
        if test_modules
            .iter()
            .any(|m| path == m.with_extension("rs") || path.starts_with(m))
        {
            continue;
        }
        let rel = path.strip_prefix(repo_root()).unwrap_or(&path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (at, line, text) in file_literals {
            if !ranges.iter().any(|&(from, to)| (from..to).contains(&at)) {
                literals.push((rel.clone(), line, text));
            }
        }
    }
    (scanned, literals)
}

/// Each user-facing literal under `ROOTS` that names a generation, as `file:line: text`.
fn offenders() -> (usize, usize, Vec<String>) {
    let (scanned, literals) = user_facing_literals(ROOTS);
    let found = literals
        .iter()
        .filter(|(_, _, text)| names_a_generation(text))
        .map(|(file, line, text)| format!("{file}:{line}: {text}"))
        .collect();
    (scanned, literals.len(), found)
}

#[test]
fn no_user_facing_literal_names_gen8_or_gen9() {
    let (scanned, literals, found) = offenders();
    // A walk or a lexer that stopped working finds nothing and passes, so both are held to a floor.
    assert!(
        scanned > 200 && literals > 2000,
        "only {scanned} files and {literals} literals were read"
    );
    assert!(
        found.is_empty(),
        "a user reads Legacy and Enhanced, through Generation::edition_label(), never Gen8 or Gen9:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_lexer_finds_a_planted_literal_and_skips_test_code() {
    let src = r####"
        // "Gen8 in a comment"
        fn f<'a>(x: &'a str) -> char { let _ = r#"a "Gen9" raw"#; '"' }
        fn g() { log("the Gen 8 layout"); log("ParticlesGen9 and GrcTextureG9"); magic(b"Gen8"); magic(br"Gen9"); }
        #[cfg(test)]
        mod tests { fn h() { assert!("Gen8".is_empty()); } }
        #[cfg(test)]
        mod more;
        fn i() { let _ = "after"; }
    "####;
    let lexed = lex(src);
    let (ranges, modules) = test_items(&lexed.code);
    let live: Vec<&str> = lexed
        .literals
        .iter()
        .filter(|(at, ..)| !ranges.iter().any(|&(from, to)| (from..to).contains(at)))
        .map(|(_, _, text)| text.as_str())
        .collect();
    assert_eq!(
        live,
        [
            r#"a "Gen9" raw"#,
            "the Gen 8 layout",
            "ParticlesGen9 and GrcTextureG9",
            "after"
        ]
    );
    assert_eq!(modules, ["more"]);
    let named: Vec<bool> = live.iter().map(|t| names_a_generation(t)).collect();
    assert_eq!(named, [true, true, false, false]);
}

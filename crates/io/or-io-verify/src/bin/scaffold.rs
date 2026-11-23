//! Write a new format's skeleton into a tree: its `or_io::gta5` module, its
//! `FORMATS` row, its `Asset` variant, its census rows, a synthetic suite and a
//! corpus suite.
//!
//! ```text
//! cargo run -q -p or-io-verify --bin scaffold -- <tree> <module> <ext> <RootType> <rsc7 version>
//! ```
//!
//! ⚠ It refuses before writing anything when a file it would create exists or
//! a line it would add is already there, so it never overwrites.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [tree, module, ext, root, version] = args.as_slice() else {
        eprintln!("scaffold <tree> <module> <ext> <RootType> <rsc7 version>");
        return ExitCode::from(2);
    };
    let plan = match Plan::new(module, ext, root, version) {
        Ok(plan) => plan,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    match scaffold(Path::new(tree), &plan) {
        Ok(written) => {
            for path in written {
                println!("{}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("nothing written: {why}");
            ExitCode::FAILURE
        }
    }
}

/// One format to scaffold, its names checked.
struct Plan {
    module: String,
    ext: String,
    root: String,
    version: u32,
}

impl Plan {
    fn new(module: &str, ext: &str, root: &str, version: &str) -> Result<Self, String> {
        let snake = |s: &str| {
            s.starts_with(|c: char| c.is_ascii_lowercase())
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        if !snake(module) {
            return Err(format!("`{module}` is not a snake_case module name"));
        }
        if !snake(ext) || ext.contains('_') {
            return Err(format!("`{ext}` is not a lowercase extension"));
        }
        if !root.starts_with(|c: char| c.is_ascii_uppercase())
            || !root.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(format!("`{root}` is not an UpperCamelCase type name"));
        }
        let version = version
            .parse()
            .map_err(|_| format!("`{version}` is not an RSC7 version"))?;
        Ok(Self {
            module: module.to_string(),
            ext: ext.to_string(),
            root: root.to_string(),
            version,
        })
    }
}

const MODULES: &str = "crates/io/or-io/src/gta5/mod.rs";
const FORMATS: &str = "crates/io/or-io/src/gta5/formats.rs";
const ASSET: &str = "crates/io/or-io/src/asset.rs";
/// Read for the file type the extension names, never written.
const FILE_TYPES: &str = "crates/io/or-io/src/container/resource/header.rs";
/// Each census suite, its macro and its test's name with `{}` for the extension;
/// `every_declared_format_is_censused` holds each to `FORMATS`, in its order.
const CENSUSES: [(&str, &str, &str); 2] = [
    (
        "crates/io/or-io-verify/tests/what_the_padding_holds.rs",
        "padding_census",
        "what_a_{}_holds_where_it_declares_nothing",
    ),
    (
        "crates/io/or-io-verify/tests/which_variants_occur.rs",
        "variant_census",
        "every_{}_tag_selects_a_declared_variant",
    ),
];

/// Every file the scaffold reads or edits in place.
#[cfg(test)]
const EDITED: [&str; 6] = [
    MODULES,
    FORMATS,
    ASSET,
    FILE_TYPES,
    CENSUSES[0].0,
    CENSUSES[1].0,
];

/// Every file written or edited, relative to `tree`; nothing is written unless every one can be.
fn scaffold(tree: &Path, plan: &Plan) -> Result<Vec<PathBuf>, String> {
    let read = |rel: &str| {
        std::fs::read_to_string(tree.join(rel))
            .map(|text| text.replace("\r\n", "\n"))
            .map_err(|e| format!("{rel}: {e}"))
    };
    let crlf = std::fs::read(tree.join(MODULES))
        .map_err(|e| format!("{MODULES}: {e}"))?
        .windows(2)
        .any(|w| w == b"\r\n");
    let m = &plan.module;
    let mut writes = vec![
        (
            format!("crates/io/or-io/src/gta5/{m}.rs"),
            plan.fill(MODULE_TEMPLATE),
        ),
        (
            format!("crates/io/or-io/tests/gta5_{m}.rs"),
            plan.fill(SYNTHETIC_TEMPLATE),
        ),
        (
            format!("crates/io/or-io-verify/tests/{m}.rs"),
            plan.fill(GATES_TEMPLATE),
        ),
    ];
    for (rel, _) in &writes {
        if tree.join(rel).exists() {
            return Err(format!("{rel} already exists"));
        }
    }
    let edits = [
        (
            MODULES,
            with_sorted_line(&read(MODULES)?, "pub mod ", &format!("pub mod {m};"))?,
        ),
        (
            FORMATS,
            plan.with_format(&read(FORMATS)?, &read(FILE_TYPES)?)?,
        ),
        (ASSET, plan.with_asset(&read(ASSET)?)?),
    ];
    writes.extend(edits.map(|(rel, text)| (rel.to_string(), text)));
    for (rel, census, test) in CENSUSES {
        let text = with_census_row(
            &read(rel)?,
            census,
            &plan.ext,
            &test.replace("{}", &plan.ext),
        )
        .map_err(|why| format!("{rel}: {why}"))?;
        writes.push((rel.to_string(), text));
    }
    let mut written = Vec::new();
    for (rel, text) in writes {
        let text = if crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        };
        let path = tree.join(&rel);
        std::fs::write(&path, text).map_err(|e| format!("{rel}: {e}"))?;
        written.push(PathBuf::from(rel));
    }
    Ok(written)
}

/// `text` with `line` added to the run of lines starting `prefix`, in sorted order.
fn with_sorted_line(text: &str, prefix: &str, line: &str) -> Result<String, String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.contains(&line) {
        return Err(format!("`{line}` is already there"));
    }
    let run: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with(prefix) && lines[i].ends_with(';'))
        .collect();
    let last = *run
        .last()
        .ok_or_else(|| format!("no line starts `{prefix}`"))?;
    let at = run
        .iter()
        .copied()
        .find(|&i| lines[i] > line)
        .unwrap_or(last + 1);
    lines.insert(at, line);
    Ok(lines.join("\n") + "\n")
}

/// `text` with `"ext" => test,` as the last row of its `census! { .. }` invocation.
fn with_census_row(text: &str, census: &str, ext: &str, test: &str) -> Result<String, String> {
    let open = format!("\n{census}! {{\n");
    let start = text
        .find(&open)
        .ok_or_else(|| format!("no `{census}!` invocation"))?;
    let end = start
        + text[start..]
            .find("\n}\n")
            .ok_or_else(|| format!("`{census}!` does not close"))?;
    if text[start..end].contains(&format!("\"{ext}\" =>")) {
        return Err(format!("`{census}!` already censuses .{ext}"));
    }
    let mut text = text.to_string();
    text.insert_str(end, &format!("\n    \"{ext}\" => {test},"));
    Ok(text)
}

impl Plan {
    /// The `Asset` variant: the module's name in UpperCamelCase.
    fn variant(&self) -> String {
        self.module
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// How a user names one: "a path region".
    fn noun(&self) -> String {
        let words = self.module.replace('_', " ");
        let article = if words.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an"
        } else {
            "a"
        };
        format!("{article} {words}")
    }

    fn fill(&self, template: &str) -> String {
        template
            .replace("__MODULE__", &self.module)
            .replace("__EXT__", &self.ext)
            .replace("__ROOT__", &self.root)
            .replace("__NOUN__", &self.noun())
    }

    /// `formats.rs` with the module imported and a `FORMATS` row for its one Gen8 root.
    fn with_format(&self, text: &str, file_types: &str) -> Result<String, String> {
        if text.contains(&format!("ext: \"{}\",", self.ext)) {
            return Err(format!("FORMATS already lists .{}", self.ext));
        }
        let import = "use super::{";
        let at = text
            .find(import)
            .ok_or_else(|| format!("no `{import}` in {FORMATS}"))?
            + import.len();
        let mut text = format!("{}{}, {}", &text[..at], self.module, &text[at..]);
        let start = text
            .find("pub static FORMATS: &[Format] = &[")
            .ok_or_else(|| format!("no FORMATS in {FORMATS}"))?;
        let end = start
            + text[start..]
                .find("\n];")
                .ok_or_else(|| format!("FORMATS in {FORMATS} does not close"))?;
        let named = {
            let mut c = self.ext.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string())
                .unwrap_or_default()
                + c.as_str()
        };
        let file_type = if file_types.contains(&format!("\n    {named},\n")) {
            named
        } else {
            format!("Custom({})", self.version)
        };
        let row = self
            .fill(FORMAT_ROW_TEMPLATE)
            .replace("__FILE_TYPE__", &file_type)
            .replace("__VERSION__", &self.version.to_string());
        text.insert_str(end, &row);
        Ok(text)
    }

    /// `asset.rs` with the root imported and an `Asset` variant holding it.
    fn with_asset(&self, text: &str) -> Result<String, String> {
        let variant = self.variant();
        let entry = format!("\n    {variant}(");
        let start = text
            .find("\nassets! {\n")
            .ok_or_else(|| format!("no `assets!` in {ASSET}"))?;
        if text[start..].contains(&entry) {
            return Err(format!("Asset already has a {variant} variant"));
        }
        let end = start
            + text[start..]
                .find("\n}\n")
                .ok_or_else(|| format!("`assets!` in {ASSET} does not close"))?;
        let mut text = text.to_string();
        text.insert_str(
            end,
            &format!("\n    /// A `.{}`.{entry}{}),", self.ext, self.root),
        );
        let anchor = "use crate::Generation;\n";
        let at = text
            .find(anchor)
            .ok_or_else(|| format!("no `{anchor}` in {ASSET}"))?;
        text.insert_str(
            at,
            &format!("use crate::gta5::{}::{};\n", self.module, self.root),
        );
        Ok(text)
    }
}

const FORMAT_ROW_TEMPLATE: &str = r#"
    Format {
        ext: "__EXT__",
        noun: "__NOUN__",
        file_type: DatResourceFileType::__FILE_TYPE__,
        codecs: None,
        roots: &[root_for::<crate::decl::Raw<__MODULE__::__ROOT__>>(
            Generation::Gen8,
            __VERSION__,
        )],
        validate: |image, at, _| __MODULE__::validate(image, at).map_err(Into::into),
    },"#;

const MODULE_TEMPLATE: &str = r#"//! `.__EXT__`: __NOUN__. A scaffold: replace `entries` with the engine's
//! fields in its order, under its names (D15).

use super::shared::PgBase;
use crate::decl::*;
use crate::Generation;

impl Resource for __ROOT__ {
    const EXT: &'static str = "__EXT__";
    const TARGET: Generation = Generation::Gen8;
}

/// A `.__EXT__`'s root.
#[record(size = 0x20)]
pub struct __ROOT__ {
    pub base: PgBase,
    pub entries: Vec<u32>,
}

/// A `.__EXT__` the game would misread, and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("{0}")]
    View(#[from] ViewError),
}

/// Every invariant the engine assumes of a `.__EXT__` and never checks.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    View::<Raw<__ROOT__>>::at_offset(image, root)?;
    Ok(())
}
"#;

const SYNTHETIC_TEMPLATE: &str = r#"//! A `.__EXT__` authored as a plain Rust value, with invented content: saved
//! whole, reopened, and saved again to the same bytes. Needs no game file.

use or_io::decl::File;
use or_io::gta5::__MODULE__::__ROOT__;

fn payload(file: &[u8]) -> Vec<u8> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

#[test]
fn an_authored___EXT___saves_whole_reopens_and_reaches_a_fixed_point() {
    let authored = __ROOT__ {
        entries: vec![1, 2, 3],
        ..Default::default()
    };
    let first = authored.to_bytes().unwrap();
    let again = __ROOT__::from_bytes(&first).unwrap();
    assert_eq!(again.entries, [1, 2, 3]);
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&first));
}
"#;

const GATES_TEMPLATE: &str = r#"//! `.__EXT__` through its declarations, over the corpus: the gates
//! `declared_gates!` writes.

use or_io::decl::{Plain, Raw, View};
use or_io::desc::Image;
use or_io::gta5::__MODULE__::__ROOT__;
use or_io::Generation;
use or_io_verify::gates::{Filed, Gates};

/// What the typed API reads of a file, in the same shape as [`Filed`].
fn typed(image: Image<'_>) -> Result<Filed, String> {
    let root = View::<Raw<__ROOT__>>::at_offset(image, 0).map_err(|e| e.to_string())?;
    let mut out = Filed::default();
    out.structs.insert(
        (0, "__ROOT__".to_string()),
        Plain::as_bytes(View::get(&root)).to_vec(),
    );
    Ok(out)
}

const GATES: Gates = Gates::new("__EXT__", Generation::Gen8, ("__ROOT__", "entries"));

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every___EXT___reads_typed_as_the_tree_reads,
    owned: __ROOT__ => every___EXT___opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every___EXT___saves_to_a_fixed_point,
    census: every___EXT___instance_is_aligned_for_its_view,
    testimony: the___EXT___testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::process::Command;

    use or_test_support::ledger::{self, SkipReason};

    fn invented() -> Plan {
        Plan::new("invented_format", "yzz", "CInventedRoot", "1").expect("valid names")
    }

    fn this_tree() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    /// Every file git tracks or would track in `from`, copied to `to`.
    fn copy_tree(from: &Path, to: &Path) {
        let listed = Command::new("git")
            .arg("-C")
            .arg(from)
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .output()
            .expect("git runs");
        assert!(listed.status.success(), "git ls-files failed");
        for rel in listed.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            let rel = Path::new(std::str::from_utf8(rel).expect("a UTF-8 path"));
            let source = from.join(rel);
            if !source.is_file() {
                continue;
            }
            let dest = to.join(rel);
            std::fs::create_dir_all(dest.parent().expect("a parent")).expect("a directory");
            std::fs::copy(&source, &dest).expect("a copy");
        }
    }

    fn fresh(dir: &Path) {
        if dir.exists() {
            std::fs::remove_dir_all(dir).expect("a scratch directory removed");
        }
        std::fs::create_dir_all(dir).expect("a scratch directory");
    }

    /// `cargo test` in `tree` as a clean checkout sees it, its target and ledger under `scratch`.
    fn cargo_test(tree: &Path, scratch: &Path, args: &[&str]) -> usize {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = Command::new(cargo)
            .current_dir(tree)
            .arg("test")
            .args(args)
            .env("CARGO_TARGET_DIR", scratch.join("target"))
            .env("OPENRAGE_SCHEMA", "none")
            .env("OPENRAGE_FIXTURES", scratch.join("empty"))
            .env(ledger::SKIP_LEDGER_ENV, scratch.join("ledger"))
            .env_remove(ledger::REQUIRE_FIXTURES_ENV)
            .output()
            .expect("cargo runs");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "cargo test {args:?} failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
            .lines()
            .filter_map(|l| l.strip_prefix("test result: ok. "))
            .filter_map(|l| l.split(' ').next()?.parse::<usize>().ok())
            .sum()
    }

    #[test]
    fn a_second_scaffold_of_one_format_is_refused_and_writes_nothing() {
        let held = tempfile::tempdir().expect("a scratch directory");
        let scratch = held.path().to_path_buf();
        for rel in EDITED {
            let dest = scratch.join(rel);
            std::fs::create_dir_all(dest.parent().expect("a parent")).expect("a directory");
            std::fs::copy(this_tree().join(rel), &dest).expect("a copy");
        }
        // No edited file lives in or-io's `tests/`, where the synthetic suite is written.
        std::fs::create_dir_all(scratch.join("crates/io/or-io/tests")).expect("a directory");
        let written = scaffold(&scratch, &invented()).expect("the first scaffold writes");
        let snapshot = |paths: &[PathBuf]| -> Vec<Vec<u8>> {
            paths
                .iter()
                .map(|p| std::fs::read(scratch.join(p)).expect("written"))
                .collect()
        };
        let before = snapshot(&written);
        let why = scaffold(&scratch, &invented()).expect_err("a second scaffold is refused");
        println!("{why}");
        assert_eq!(snapshot(&written), before, "a refused scaffold wrote");
    }

    #[test]
    #[ignore = "builds a scratch copy of the workspace under $TEMP; run by name"]
    fn a_scaffolded_format_compiles_and_its_gates_skip_without_a_corpus() {
        let scratch = std::env::temp_dir().join("openrage-scaffold-check");
        let tree = scratch.join("tree");
        for dir in [&tree, &scratch.join("ledger"), &scratch.join("empty")] {
            fresh(dir);
        }
        copy_tree(&this_tree(), &tree);
        let plan = invented();
        let written = scaffold(&tree, &plan).expect("the scaffold writes");
        println!("wrote {written:?}");

        let synthetic = cargo_test(
            &tree,
            &scratch,
            &["-p", "or-io", "--test", "gta5_invented_format"],
        );
        assert_eq!(synthetic, 1, "the synthetic suite's one test");
        let gates = cargo_test(
            &tree,
            &scratch,
            &["-p", "or-io-verify", "--test", "invented_format"],
        ) + cargo_test(
            &tree,
            &scratch,
            &[
                "-p",
                "or-io-verify",
                "--test",
                "what_the_padding_holds",
                "--test",
                "which_variants_occur",
                "--",
                "yzz",
                "every_declared_format_is_censused",
            ],
        );
        assert_eq!(
            gates, 10,
            "declared_gates!'s six tests with an owned root, two censuses of .yzz and \
             the two checks that every format is censused"
        );

        let skipped: BTreeSet<String> = ledger::read_dir(&scratch.join("ledger"))
            .into_iter()
            .filter(|r| r.reason == SkipReason::Corpus && r.what.contains(".yzz"))
            .map(|r| r.what)
            .collect();
        assert_eq!(
            skipped.len(),
            8,
            "a corpus skip per gate, and for the padding and variant censuses: {skipped:?}"
        );
    }
}

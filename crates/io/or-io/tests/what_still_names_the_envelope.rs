//! Nothing outside `or-io/src/container/resource/` names RSC7's shape.

use std::path::{Path, PathBuf};

/// The RSC7 envelope's own vocabulary, declared no further out than `resource/`.
/// The declared header (`DatResourceFileHeader`, `DatResourceInfo`) is public
/// and not on it; neither is DEFLATE, which RPF7 shares for entries that are
/// not resources.
const RSC7_SHAPE: &[&str] = &[
    "DatResourceWriter",
    "DatResourceReader",
    "DatResourceContents",
    "DAT_RESOURCE_MAGIC",
    "DAT_RESOURCE_HEADER_SIZE",
];

const ENVELOPE: &str = "crates/io/or-io/src/container/resource/";

/// Lines naming one of `names`, comment lines aside.
fn hits(source: &str, names: &[&str]) -> usize {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && names.iter().any(|n| line.contains(n)))
        .count()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("or-io sits three directories below the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
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

/// Every `.rs` under `crates/` and `src/` apart from the envelope and this file,
/// `tests/` directories included: a suite that hand-builds a header is the
/// likeliest place for a name to return.
fn walked_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    rust_files(&root.join("src"), &mut files);
    files.retain(|p| {
        let s = p.to_string_lossy().replace('\\', "/");
        !s.contains(ENVELOPE) && !s.ends_with("what_still_names_the_envelope.rs")
    });
    files.sort();
    files
}

#[test]
fn nothing_outside_the_envelope_names_the_rsc7_shape() {
    let offenders: Vec<String> = walked_files()
        .into_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).expect("read source");
            let n = hits(&source, RSC7_SHAPE);
            (n > 0).then(|| format!("{n:>3} {}", path.display()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "RSC7's shape is named outside `{ENVELOPE}`. The envelope is \
         `ResourceEnvelope`: `Rsc7::peek`, `Rsc7::open`, `Rsc7::reseal`, \
         `Rsc7::author` and `Rsc7Seal` are what a caller asks, and a suite that \
         needs a header has `Rsc7::bare_header`.\n  {}",
        offenders.join("\n  ")
    );
}

/// A scan that looks nowhere reports zero just as convincingly, so the walk
/// and the matcher are asserted before the absence they report.
#[test]
fn the_walk_reaches_the_tree_it_claims_to() {
    let files = walked_files();
    assert!(
        files.len() > 400,
        "only {} file(s) walked, which is not this workspace",
        files.len()
    );
    assert!(
        files
            .iter()
            .any(|p| p.to_string_lossy().replace('\\', "/").contains("/tests/")),
        "the walk reaches no `tests/` directory"
    );
    assert_eq!(hits("let _ = DAT_RESOURCE_MAGIC;", RSC7_SHAPE), 1);
    assert_eq!(hits("// DAT_RESOURCE_MAGIC", RSC7_SHAPE), 0);
}

/// Each name on the list is still declared inside the envelope, and no wider
/// than `resource/`: a `pub` there would compile and put it back on the
/// crate's surface.
#[test]
fn the_rsc7_shape_is_declared_inside_the_envelope_and_no_wider() {
    let header = workspace_root().join(ENVELOPE).join("header.rs");
    let source = std::fs::read_to_string(&header).expect("read the envelope's header module");
    for declared in [
        "pub(super) struct DatResourceWriter",
        "pub(super) struct DatResourceReader",
        "pub(super) struct DatResourceContents",
        "pub(super) const DAT_RESOURCE_MAGIC",
        "pub(super) const DAT_RESOURCE_HEADER_SIZE",
    ] {
        assert!(
            source.contains(declared),
            "{}: `{declared}` is gone; the scan guards a name the tree no longer declares \
             as it was",
            header.display()
        );
    }
}

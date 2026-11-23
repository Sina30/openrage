//! A snapshot is a cache of what `build_library` said, and the file does not
//! record which `build_library` that was.

use std::path::{Path, PathBuf};

use or_io::meta::snapshot_discovery::{
    fingerprint_of, generator_fingerprint, generator_sources, stamp_path, GENERATOR_SOURCE_DIR,
};
use or_io::meta::SnapshotAge;

/// `or-io`'s manifest directory, which `builder_stamp` digests beneath.
fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../or-io")
}

/// ⚠ The stamp must be over the real `src/meta/schema`: a fingerprint of an empty
/// set is a constant, and a constant never goes stale.
#[test]
fn the_fingerprint_is_taken_over_the_sources_build_library_is() {
    let sources = generator_sources(&manifest());
    let names: Vec<String> = sources
        .iter()
        .map(|p| {
            p.strip_prefix(manifest())
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for wanted in [
        "src/meta/schema/mod.rs",
        "src/meta/schema/parser.rs",
        "src/meta/schema/normalize.rs",
        "src/meta/schema/types.rs",
        "src/meta/schema/library.rs",
    ] {
        assert!(names.contains(&wanted.to_string()), "{wanted} in {names:?}");
    }
    assert_eq!(
        names.len(),
        5,
        "a file added under {GENERATOR_SOURCE_DIR} belongs in this list: {names:?}"
    );
}

#[test]
fn a_changed_generator_source_changes_the_stamp() {
    let before = generator_fingerprint([
        ("src/meta/schema/parser.rs", b"fn parse() {}".as_slice()),
        ("src/meta/schema/types.rs", b"struct Field;".as_slice()),
    ]);
    let after = generator_fingerprint([
        (
            "src/meta/schema/parser.rs",
            b"fn parse() { fixed_size() }".as_slice(),
        ),
        ("src/meta/schema/types.rs", b"struct Field;".as_slice()),
    ]);
    assert_ne!(before, after);

    // A file appearing is a change too, which a per-file digest of a fixed
    // list would miss.
    let added = generator_fingerprint([
        ("src/meta/schema/parser.rs", b"fn parse() {}".as_slice()),
        ("src/meta/schema/types.rs", b"struct Field;".as_slice()),
        ("src/meta/schema/arrays.rs", b"struct Fixed;".as_slice()),
    ]);
    assert_ne!(before, added);
}

/// Order of discovery must not reach the digest, or two machines listing a
/// directory differently disagree about a tree they both have.
#[test]
fn the_stamp_does_not_depend_on_the_order_files_arrive_in() {
    let a = generator_fingerprint([("a.rs", b"one".as_slice()), ("b.rs", b"two".as_slice())]);
    let b = generator_fingerprint([("b.rs", b"two".as_slice()), ("a.rs", b"one".as_slice())]);
    assert_eq!(a, b);
}

/// ⚠ Either would mark a current snapshot stale.
#[test]
fn neither_the_worktree_nor_the_line_endings_reach_the_stamp() {
    let lf = generator_fingerprint([("src/meta/schema/mod.rs", b"a\nb\n".as_slice())]);
    let crlf = generator_fingerprint([("src/meta/schema/mod.rs", b"a\r\nb\r\n".as_slice())]);
    assert_eq!(lf, crlf, "a CRLF checkout is not a different generator");

    // `fingerprint_of` keys on the path relative to the crate, so the same
    // sources under two worktrees stamp alike. Demonstrated on the keys: a
    // test cannot have two worktrees.
    let absolute = generator_fingerprint([(
        "D:/lanes/mine/crates/io/or-io/src/meta/schema/mod.rs",
        b"a\n".as_slice(),
    )]);
    assert_ne!(
        absolute,
        generator_fingerprint([("src/meta/schema/mod.rs", b"a\n".as_slice())]),
        "if these agreed the relative key would not be load-bearing"
    );
    assert_eq!(
        or_io::meta::builder_stamp(),
        Some(fingerprint_of(&manifest()).as_str())
    );
}

#[test]
fn the_stamp_sits_beside_the_snapshot_it_describes() {
    let snapshot = Path::new("D:/org/openrage-corpus/or-meta/schema_snapshot.bin");
    assert_eq!(
        stamp_path(snapshot),
        Path::new("D:/org/openrage-corpus/or-meta/schema_snapshot.stamp")
    );
}

/// Reports which state the staged snapshot is in: a suite that skipped would
/// be as green as one that agreed.
#[test]
fn the_staged_snapshot_says_which_generator_it_came_from() {
    let Some(path) = or_test_support::schema_snapshot() else {
        or_test_support::skip_schema("the_staged_snapshot_says_which_generator_it_came_from");
        return;
    };
    let age = or_io::meta::snapshot_age(&path);
    match &age {
        SnapshotAge::Unjudged => panic!("a test runs from a source tree, so a stamp can be judged"),
        SnapshotAge::Current => {
            assert!(age.concern().is_none());
            eprintln!(
                "EXERCISED: {} was built by this tree's schema/ ({:?}).",
                path.display(),
                or_io::meta::builder_stamp()
            );
        }
        SnapshotAge::Unstamped | SnapshotAge::Stale { .. } => {
            let concern = age.concern().expect("a concern is the whole point");
            assert!(
                concern.contains("just snapshot"),
                "a report nobody can act on is a report nobody reads: {concern}"
            );
            eprintln!("EXERCISED: {concern}");
        }
    }
}

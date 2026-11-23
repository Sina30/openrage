use std::fs;
use std::path::{Path, PathBuf};

use or_test_support::{Corpus, DEFAULT_TARGET, TARGET_GEN8, TARGET_GEN9};

struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("or-test-support-{name}-"))
            .tempdir()
            .expect("scratch dir");
        Self(dir)
    }

    fn write(&self, relative: &str) -> PathBuf {
        let path = self.0.path().join(relative);
        fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
        fs::write(&path, b"not a real resource").expect("write");
        path
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_query_without_a_target_never_returns_another_targets_files() {
    let scratch = Scratch::new("default-target");
    scratch.write("gta5/gen8/legacy.ydr");
    scratch.write("gta5/gen9/enhanced.ydr");

    let found = Corpus::with_root(scratch.path(), "ydr").find();

    assert_eq!(
        names(&found),
        vec!["legacy.ydr"],
        "the default target is {DEFAULT_TARGET}; an Enhanced file must not leak in"
    );
}

#[test]
fn a_flat_corpus_is_treated_as_the_default_target() {
    let scratch = Scratch::new("flat");
    scratch.write("loose.ydr");
    scratch.write("gta5/gen9/enhanced.ydr");

    assert_eq!(
        names(&Corpus::with_root(scratch.path(), "ydr").find()),
        vec!["loose.ydr"]
    );
    assert_eq!(
        names(
            &Corpus::with_root(scratch.path(), "ydr")
                .target(TARGET_GEN9)
                .find()
        ),
        vec!["enhanced.ydr"]
    );
}

#[test]
fn an_explicit_target_selects_only_that_target() {
    let scratch = Scratch::new("explicit");
    scratch.write("gta5/gen8/a.ydr");
    scratch.write("gta5/gen9/a.ydr");
    scratch.write("loose.ydr");

    let gen9 = Corpus::with_root(scratch.path(), "ydr")
        .target(TARGET_GEN9)
        .find();
    assert_eq!(gen9.len(), 1);
    assert!(gen9[0].to_string_lossy().contains("gen9"));

    let gen8 = Corpus::with_root(scratch.path(), "ydr")
        .target(TARGET_GEN8)
        .find();
    assert_eq!(gen8.len(), 2, "the filed Gen8 file and the loose one");
}

#[test]
fn all_targets_has_to_be_asked_for() {
    let scratch = Scratch::new("all-targets");
    scratch.write("gta5/gen8/a.yft");
    scratch.write("gta5/gen9/a.yft");

    assert_eq!(Corpus::with_root(scratch.path(), "yft").find().len(), 1);
    assert_eq!(
        Corpus::with_root(scratch.path(), "yft")
            .all_targets()
            .find()
            .len(),
        2,
        "correct only where the thing under test is generation-agnostic, e.g. a skeleton"
    );
}

#[test]
fn results_are_sorted() {
    let scratch = Scratch::new("sorted");
    for name in ["c.ybn", "a.ybn", "b.ybn"] {
        scratch.write(name);
    }

    assert_eq!(
        names(&Corpus::with_root(scratch.path(), "ybn").find()),
        vec!["a.ybn", "b.ybn", "c.ybn"]
    );
}

#[test]
fn an_absent_corpus_is_empty_rather_than_an_error() {
    let scratch = Scratch::new("absent");
    let found = Corpus::with_root(scratch.path().join("definitely-not-here"), "ydr").find();
    assert!(found.is_empty());
}

#[test]
fn the_extension_match_is_case_insensitive() {
    let scratch = Scratch::new("case");
    scratch.write("shouty.YDR");

    assert_eq!(Corpus::with_root(scratch.path(), "ydr").find().len(), 1);
}

#[test]
fn matched_targets_pairs_files_present_on_both_sides() {
    let scratch = Scratch::new("matched");
    scratch.write("gta5/gen8/shared.ydr");
    scratch.write("gta5/gen9/shared.ydr");
    scratch.write("gta5/gen8/legacy_only.ydr");

    let pairs = or_test_support::matched_targets_in(
        scratch.path(),
        "gta5",
        &[TARGET_GEN8, TARGET_GEN9],
        "ydr",
    );

    assert_eq!(
        pairs.len(),
        1,
        "only the file present under both targets pairs up"
    );
    assert_eq!(pairs[0].0, "shared.ydr");
    assert_eq!(pairs[0].1.len(), 2);
}

#[test]
fn a_limit_truncates_deterministically() {
    let scratch = Scratch::new("limit");
    for name in ["a.ytd", "b.ytd", "c.ytd", "d.ytd"] {
        scratch.write(name);
    }

    assert_eq!(
        names(&Corpus::with_root(scratch.path(), "ytd").limit(2).find()),
        vec!["a.ytd", "b.ytd"],
        "truncation happens after sorting, so a capped run is a prefix of a full one"
    );
}

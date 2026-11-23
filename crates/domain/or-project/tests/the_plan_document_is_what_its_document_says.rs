//! The JSON a plan emits, which `openrage-cli wardrobe plan` prints and a script, a CI job and a plugin read, is the one
//! `docs/reference/Plan_Document.md` describes: the same keys at every level, and every word it uses documented.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use or_project::{
    plan_with, Container, Contribution, Figure, Item, Module, Origin, Output, OutputKind,
    PlannedFile,
};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-project")
        .to_path_buf()
}

fn the_document() -> String {
    std::fs::read_to_string(root().join("docs/reference/Plan_Document.md"))
        .expect("docs/reference/Plan_Document.md")
}

/// The first fenced JSON block of the document: the example of a plan.
fn example() -> Value {
    let doc = the_document();
    let block = doc
        .split("```json")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the document shows an example in a json block");
    serde_json::from_str(block).expect("the example is JSON")
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// A plan with one file of each kind a plan can hold, a figure, an item and a problem.
fn emitted(kind: OutputKind) -> Value {
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::of(kind, "out", "pack", &["files"])],
    };
    let mut contribution = Contribution::of("files");
    contribution.files = vec![
        PlannedFile {
            path: "client.lua".into(),
            origin: Origin::Document("print(1)".into()),
        },
        PlannedFile {
            path: "stream/a.ytd".into(),
            origin: Origin::Generated(vec![1, 2, 3]),
        },
        PlannedFile {
            path: "stream/b.ytd".into(),
            origin: Origin::Copy {
                from: "/projects/pack/b.ytd".into(),
                bytes: 7,
            },
        },
    ];
    contribution.figures = vec![Figure {
        name: "entries".into(),
        amount: 3,
    }];
    contribution.items = vec![Item {
        id: "g1".into(),
        name: "pack^jbib_000".into(),
        digest: "d1".into(),
    }];
    contribution.problems = vec!["something is wrong".into()];
    plan_with(
        &container,
        "out",
        Path::new("/projects/pack"),
        &[contribution],
    )
    .expect("the output is declared")
    .document()
}

#[test]
fn a_plan_has_the_keys_its_document_shows_at_every_level() {
    let shown = example();
    let made = emitted(OutputKind::FivemResource);

    assert_eq!(keys(&made), keys(&shown), "the keys of a plan");
    for (list, what) in [
        ("files", "a file"),
        ("budgets", "a budget"),
        ("items", "an item"),
    ] {
        let (have, want) = (&made[list][0], &shown[list][0]);
        assert_eq!(keys(have), keys(want), "the keys of {what}");
    }
    assert_eq!(keys(&made["totals"]), keys(&shown["totals"]));
    assert_eq!(made["format"], shown["format"]);
    assert_eq!(made["schema"], shown["schema"]);
    assert!(
        shown["problems"].is_array() && made["problems"][0].is_string(),
        "a problem is a sentence"
    );
}

#[test]
fn every_word_a_plan_uses_is_in_the_document() {
    let doc = the_document();
    let mut hows = BTreeSet::new();
    for kind in [
        OutputKind::FivemResource,
        OutputKind::SingleplayerDlc,
        OutputKind::OivPackage,
    ] {
        let made = emitted(kind.clone());
        let word = made["kind"].as_str().unwrap();
        assert!(doc.contains(&format!("`{word}`")), "the kind `{word}`");
        for file in made["files"].as_array().unwrap() {
            hows.insert(file["how"].as_str().unwrap().to_string());
        }
    }
    // A document, a generated file and a copy; a retarget and a cook are the other two, which a build of sources and of another target makes.
    for how in ["document", "generated", "copy", "retarget", "cook"] {
        assert!(
            doc.contains(&format!("`{how}`")),
            "the way a file is made, `{how}`"
        );
    }
    assert!(hows.is_superset(&["document", "generated", "copy"].map(String::from).into()));
    for target in ["legacy", "enhanced"] {
        assert!(doc.contains(&format!("`{target}`")), "{target}");
    }
}

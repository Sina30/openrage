//! A build says what changed since the last one, by stable id: added, changed, moved and removed,
//! with a redirect map of what moved.

use std::path::Path;

use or_project::{
    build, diff, plan_with, since_last_build, Container, Contribution, Diff, Item, Module,
    ModuleKind, Origin, Output, Plan, PlannedFile,
};
use serde_json::Value;

fn item(id: &str, name: &str, digest: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        digest: digest.into(),
    }
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.id.as_str()).collect()
}

/// A last build of four garments and the next of five.
fn before_and_after() -> (Vec<Item>, Vec<Item>) {
    let before = vec![
        item("g1", "jbib_000_u", "d1"),
        item("g2", "jbib_001_u", "d2"),
        item("g3", "jbib_002_u", "d3"),
        item("g4", "jbib_003_u", "d4"),
    ];
    let after = vec![
        item("g1", "jbib_000_u", "d1"),
        // Moved to another position, content as it was.
        item("g2", "jbib_005_u", "d2"),
        // Edited in place.
        item("g3", "jbib_002_u", "d3 edited"),
        // g4 is gone, and a new garment took the name it left, which is not the same thing.
        item("g5", "jbib_003_u", "d5"),
    ];
    (before, after)
}

#[test]
fn a_moved_id_and_a_removed_one_are_reported() {
    let (before, after) = before_and_after();

    let found = diff(&before, &after);

    assert_eq!(ids(&found.added), ["g5"]);
    assert_eq!(ids(&found.changed), ["g3"]);
    assert_eq!(ids(&found.removed), ["g4"]);
    assert_eq!(found.moved.len(), 1, "{:?}", found.moved);
    let moved = &found.moved[0];
    assert_eq!(
        (moved.id.as_str(), moved.from.as_str(), moved.to.as_str()),
        ("g2", "jbib_001_u", "jbib_005_u")
    );
    assert!(!found.is_empty());
}

#[test]
fn what_did_not_change_is_in_no_list_and_an_item_that_moved_and_changed_is_in_both() {
    let before = vec![item("a", "x", "1"), item("b", "y", "2")];
    let after = vec![item("a", "x", "1"), item("b", "z", "3")];

    let found = diff(&before, &after);

    assert_eq!(ids(&found.changed), ["b"]);
    assert_eq!(found.moved.len(), 1);
    assert!(found.added.is_empty() && found.removed.is_empty());
    assert!(diff(&before, &before).is_empty());
    assert_eq!(diff(&[], &[]), Diff::default());
}

#[test]
fn the_redirect_map_names_each_move_by_id_and_is_canonical() {
    let (before, mut after) = before_and_after();
    after.push(item("g0", "jbib_009_u", "d0"));
    after.iter_mut().find(|i| i.id == "g1").unwrap().name = "jbib_004_u".into();

    let text = diff(&before, &after).redirect_map();

    let map: Value = serde_json::from_str(&text).expect("a JSON document");
    assert_eq!(map["format"], "openrage-redirects");
    let redirects = map["redirects"].as_array().expect("a list");
    let shown: Vec<(&str, &str, &str)> = redirects
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap(),
                r["from"].as_str().unwrap(),
                r["to"].as_str().unwrap(),
            )
        })
        .collect();
    // Only what moved, ordered by id; what was added or removed has nowhere to be sent.
    assert_eq!(
        shown,
        [
            ("g1", "jbib_000_u", "jbib_004_u"),
            ("g2", "jbib_001_u", "jbib_005_u")
        ]
    );
    assert!(text.ends_with('\n') && !text.contains('\r'));
    assert_eq!(
        text,
        diff(&before, &after).redirect_map(),
        "two exports differ"
    );
}

fn container() -> Container {
    Container {
        modules: vec![Module {
            id: "uniforms".into(),
            kind: ModuleKind::Unknown("widgets".into()),
            extra: Default::default(),
        }],
        outputs: vec![Output::fivem_resource("fivem", "pack", &["uniforms"])],
    }
}

/// The plan of a build that ships `items`, and a document `body` to say it changed.
fn planned(items: Vec<Item>, body: &str) -> Plan {
    let mut contribution = Contribution::of("uniforms");
    contribution.items = items;
    contribution.files.push(PlannedFile {
        path: "data/peds.txt".into(),
        origin: Origin::Document(body.into()),
    });
    plan_with(
        &container(),
        "fivem",
        Path::new("/projects/pack"),
        &[contribution],
    )
    .expect("the output is declared")
}

#[test]
fn a_build_records_its_items_and_the_next_one_reports_the_difference() {
    let (before, after) = before_and_after();
    let root = tempfile::tempdir().unwrap();

    let first = planned(before.clone(), "one");
    assert_eq!(
        since_last_build(&first, root.path()),
        None,
        "nothing was built yet"
    );
    let built = build(&first, root.path()).expect("the first build");
    assert_eq!(
        built.since, None,
        "a first build has nothing to differ from"
    );

    let second = planned(after, "two");
    let preview =
        since_last_build(&second, root.path()).expect("the first build recorded its items");
    let rebuilt = build(&second, root.path()).expect("the second build");

    assert_eq!(
        rebuilt.since.as_ref(),
        Some(&preview),
        "the preview and the build disagree"
    );
    let found = preview;
    assert_eq!(ids(&found.added), ["g5"]);
    assert_eq!(ids(&found.removed), ["g4"]);
    assert_eq!(found.moved.len(), 1);
    assert_eq!(ids(&found.changed), ["g3"]);

    // The record now describes the build that is live, so the same plan differs from it by nothing.
    assert_eq!(
        since_last_build(&second, root.path()),
        Some(Diff::default())
    );
}

#[test]
fn a_build_that_fails_leaves_the_record_of_the_last_good_one() {
    let (before, after) = before_and_after();
    let root = tempfile::tempdir().unwrap();
    build(&planned(before, "one"), root.path()).expect("the first build");

    // A document named for a format whose reader refuses these bytes stops the build after
    // everything was written and before anything is swapped in.
    let mut broken = planned(after, "two");
    broken.files.push(PlannedFile {
        path: "stream/bad.ytd".into(),
        origin: Origin::Document("not a texture dictionary".into()),
    });
    build(&broken, root.path()).expect_err("it does not read back");

    let next = planned(vec![item("g1", "jbib_000_u", "d1")], "three");
    let found = since_last_build(&next, root.path()).expect("the first build's record stands");
    assert_eq!(ids(&found.removed), ["g2", "g3", "g4"], "{found:?}");
}

#[test]
fn a_record_that_cannot_be_read_is_no_record_and_the_next_build_writes_a_good_one() {
    let (before, _) = before_and_after();
    let root = tempfile::tempdir().unwrap();
    build(&planned(before.clone(), "one"), root.path()).expect("the first build");
    let record = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().contains("items"))
        .expect("a build leaves a record of its items beside it");
    std::fs::write(&record, b"{ not json").unwrap();

    let plan = planned(before.clone(), "two");
    assert_eq!(since_last_build(&plan, root.path()), None);
    let rebuilt = build(&plan, root.path()).expect("a bad record does not stop a build");
    assert_eq!(rebuilt.since, None);

    assert_eq!(since_last_build(&plan, root.path()), Some(Diff::default()));
}

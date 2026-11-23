//! The plan of a build is a JSON document that a script or a plugin reads, and a plugin that emits
//! adds text files to it, which the plan checks as it checks every other file.

use std::path::Path;

use or_project::{
    plan_with, Container, Contribution, Item, Module, Origin, Output, OutputKind, Plan, PlannedFile,
};
use serde_json::{json, Value};

const FOLDER: &str = "/projects/pack";

fn document_file(path: &str, text: &str) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Document(text.to_string()),
    }
}

/// A plan of a FiveM resource whose one content hands over three documents and one item.
fn planned_with(configure: impl FnOnce(&mut Output)) -> Plan {
    let mut output = Output::fivem_resource("fivem", "pack", &["files"]);
    configure(&mut output);
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![output],
    };
    let mut contribution = Contribution::of("files");
    contribution.files = vec![
        document_file("stream/a.ytd", "aaaa"),
        document_file("client.lua", "print(1)"),
        document_file("data/shop.meta", "<x/>"),
    ];
    contribution.items = vec![Item {
        id: "g1".into(),
        name: "pack^jbib_000".into(),
        digest: "d1".into(),
    }];
    plan_with(&container, "fivem", Path::new(FOLDER), &[contribution]).expect("the output exists")
}

fn paths(plan: &Plan) -> Vec<&str> {
    plan.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn a_plan_is_a_document_with_the_keys_a_script_reads() {
    let plan = planned_with(|_| {});
    let document = plan.document();

    assert_eq!(document["format"], "openrage-wardrobe-plan");
    assert_eq!(document["schema"], 1);
    assert_eq!(document["output"], "fivem");
    assert_eq!(document["resource"], "pack");
    assert_eq!(
        document["kind"], "fivem-resource",
        "the kind is the word the project file stores"
    );
    assert_eq!(document["target"], Value::Null);
    assert_eq!(
        document["totals"],
        json!({ "files": plan.totals.files, "bytes": plan.totals.bytes })
    );
    assert_eq!(
        document["items"],
        json!([{ "id": "g1", "name": "pack^jbib_000", "digest": "d1" }])
    );
    assert_eq!(document["problems"], json!([]));

    let files = document["files"].as_array().expect("a list of files");
    let listed: Vec<&str> = files
        .iter()
        .map(|f| f["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(listed, paths(&plan), "the files are in the plan's order");
    let manifest = files
        .iter()
        .find(|f| f["path"] == "fxmanifest.lua")
        .expect("the manifest is a planned file");
    assert_eq!(manifest["how"], "document");
    assert_eq!(manifest["from"], Value::Null);
}

#[test]
fn the_document_a_plugin_is_handed_names_no_folder_of_the_users_disk() {
    let mut plan = planned_with(|_| {});
    plan.files.push(PlannedFile {
        path: "stream/a.ydd".to_string(),
        origin: Origin::Copy {
            from: Path::new(FOLDER).join("modules").join("a.ydd"),
            bytes: 3,
        },
    });
    plan.files.push(PlannedFile {
        path: "stream/b.ydd".to_string(),
        origin: Origin::Retarget {
            from: Path::new(FOLDER).join("modules").join("b.ydd"),
            bytes: 3,
        },
    });

    let script = plan.document();
    let copies: Vec<&Value> = script["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["how"] == "copy" || f["how"] == "retarget")
        .collect();
    assert_eq!(copies.len(), 2);
    assert!(
        copies.iter().all(|f| f["from"]
            .as_str()
            .is_some_and(|from| from.contains("modules"))),
        "a script reads where a file comes from: {copies:?}"
    );

    let guest = plan.guest_document();
    let text = guest.to_string();
    assert!(
        !text.contains("modules") && !text.contains(FOLDER),
        "a guest has no path: {text}"
    );
    let mut without = script.clone();
    for file in without["files"].as_array_mut().unwrap() {
        file["from"] = Value::Null;
    }
    assert_eq!(
        guest, without,
        "and is the document with nothing else taken out"
    );
}

#[test]
fn an_emitted_file_joins_the_plan_in_path_order_and_the_totals_follow() {
    let plan = planned_with(|_| {});
    let before = (plan.totals.files, plan.totals.bytes);

    let extended = plan.with_emitted(
        "catalogue",
        &[
            ("config/shop.json".to_string(), "{\"items\":1}".to_string()),
            ("README.txt".to_string(), "hello".to_string()),
        ],
    );

    assert!(extended.problems.is_empty(), "{:?}", extended.problems);
    let listed = paths(&extended);
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(listed, sorted, "the plan lists its files by path");
    assert!(listed.contains(&"config/shop.json") && listed.contains(&"README.txt"));
    assert_eq!(extended.totals.files, before.0 + 2);
    assert_eq!(extended.totals.bytes, before.1 + 11 + 5);
    let added = extended
        .files
        .iter()
        .find(|f| f.path == "config/shop.json")
        .expect("it is in the plan");
    assert_eq!(
        added.origin,
        Origin::Document("{\"items\":1}".to_string()),
        "an emitted file is a document the build writes as given"
    );
    assert_eq!(
        plan.totals.files, before.0,
        "the plan it extended is as it was"
    );
}

#[test]
fn a_path_that_could_leave_the_resource_or_hide_a_file_is_a_problem_naming_the_plugin() {
    let plan = planned_with(|_| {});
    for (path, why) in [
        ("../outside.json", "leaves"),
        ("a/../b.json", "leaves"),
        ("/absolute.json", "leaves"),
        ("C:/drive.json", "leaves"),
        ("back\\slash.json", "leaves"),
        ("trailing.", "leaves"),
        ("", "leaves"),
        ("stream/note.txt", "stream"),
        ("fxmanifest.lua", "manifest"),
        ("FXMANIFEST.LUA", "manifest"),
        ("CLIENT.LUA", "one path"),
    ] {
        let extended = plan.with_emitted("catalogue", &[(path.to_string(), "x".to_string())]);
        assert!(
            extended
                .problems
                .iter()
                .any(|p| p.contains("catalogue") && p.contains(why)),
            "`{path}` is refused naming the plugin and the reason ({why}): {:?}",
            extended.problems
        );
        assert!(
            !extended.is_buildable(),
            "a plan with a problem is not built"
        );
        assert_eq!(
            extended.files.len(),
            plan.files.len(),
            "`{path}` was added to the plan"
        );
    }
}

#[test]
fn two_files_of_one_path_in_any_case_are_refused_and_the_first_stands() {
    let plan = planned_with(|_| {});
    let extended = plan.with_emitted(
        "catalogue",
        &[
            ("config/a.json".to_string(), "1".to_string()),
            ("Config/A.json".to_string(), "2".to_string()),
        ],
    );
    assert!(
        extended.problems.iter().any(|p| p.contains("one path")),
        "{:?}",
        extended.problems
    );
    assert_eq!(extended.files.len(), plan.files.len() + 1);
    let kept = extended
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("config/a.json"))
        .expect("the first stands");
    assert_eq!(kept.origin, Origin::Document("1".to_string()));
}

#[test]
fn a_ceiling_the_emitted_files_cross_is_a_problem_and_one_already_crossed_is_said_once() {
    // The plan lists four files; a ceiling of four is met exactly.
    let met = planned_with(|o| o.max_files = Some(4));
    assert!(met.problems.is_empty(), "{:?}", met.problems);
    let over = met.with_emitted("catalogue", &[("a.json".to_string(), "{}".to_string())]);
    let sayings: Vec<&String> = over
        .problems
        .iter()
        .filter(|p| p.contains("files, over the 4"))
        .collect();
    assert_eq!(sayings.len(), 1, "{:?}", over.problems);
    assert!(sayings[0].contains("writes 5 files"), "{}", sayings[0]);

    // A plan over a ceiling already says so; emitting more says the new figure, not both.
    let already = planned_with(|o| o.max_files = Some(2));
    assert_eq!(
        already
            .problems
            .iter()
            .filter(|p| p.contains("over the 2"))
            .count(),
        1,
        "{:?}",
        already.problems
    );
    let more = already.with_emitted("catalogue", &[("a.json".to_string(), "{}".to_string())]);
    let sayings: Vec<&String> = more
        .problems
        .iter()
        .filter(|p| p.contains("over the 2"))
        .collect();
    assert_eq!(sayings.len(), 1, "{:?}", more.problems);
    assert!(sayings[0].contains("writes 5 files"), "{}", sayings[0]);
}

#[test]
fn only_a_fivem_resource_takes_an_emitted_file() {
    for kind in [OutputKind::SingleplayerDlc, OutputKind::OivPackage] {
        let plan = planned_with(|o| o.kind = kind.clone());
        let before = plan.files.len();
        let extended = plan.with_emitted("catalogue", &[("a.json".to_string(), "{}".to_string())]);
        assert_eq!(extended.files.len(), before, "{kind:?} took a file");
        assert!(
            extended
                .problems
                .iter()
                .any(|p| p.contains("catalogue") && p.contains("FiveM resource")),
            "{kind:?}: {:?}",
            extended.problems
        );
    }
}

#[test]
fn nothing_emitted_changes_nothing() {
    let plan = planned_with(|_| {});
    assert_eq!(plan.with_emitted("catalogue", &[]), plan);
}

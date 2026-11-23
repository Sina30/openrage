//! Content of any kind contributes to a plan through one contract, and the plan merges what it is
//! handed: the names the game resolves once, paths, manifest lines, budgets and stable ids.

use std::path::Path;

use or_project::{
    plan_with, Budget, Claim, Container, Contribution, Figure, Item, ManifestEntry, Module,
    ModuleKind, Origin, Output, Plan, PlannedFile,
};

const FOLDER: &str = "/projects/pack";

/// Three contents of a kind this build has no code for, which is how a kind that a plugin or a
/// later wave builds reaches a plan: by handing over its contribution.
fn container(take: &[&str]) -> Container {
    Container {
        modules: ["uniforms", "station", "scripts"]
            .map(|id| Module {
                id: id.to_string(),
                kind: ModuleKind::Unknown("widgets".into()),
                extra: Default::default(),
            })
            .to_vec(),
        outputs: vec![Output::fivem_resource("fivem", "pack", take)],
    }
}

fn planned(container: &Container, contributions: &[Contribution]) -> Plan {
    plan_with(container, "fivem", Path::new(FOLDER), contributions).expect("the output is declared")
}

fn document(path: &str, bytes: usize) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Document("x".repeat(bytes)),
    }
}

fn claiming(module: &str, kind: &str, name: &str) -> Contribution {
    let mut contribution = Contribution::of(module);
    contribution.claims.push(Claim {
        kind: kind.into(),
        name: name.into(),
    });
    contribution
}

fn stating(module: &str, key: &str, values: &[&str]) -> Contribution {
    let mut contribution = Contribution::of(module);
    contribution.manifest.push(ManifestEntry {
        key: key.into(),
        values: values.iter().map(|v| v.to_string()).collect(),
    });
    contribution
}

fn figure(module: &str, name: &str, amount: u64) -> Contribution {
    let mut contribution = Contribution::of(module);
    contribution.figures.push(Figure {
        name: name.into(),
        amount,
    });
    contribution
}

fn item(id: &str, name: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        digest: format!("digest of {id}"),
    }
}

/// Whether some problem of `plan` holds every one of `words`.
fn says(plan: &Plan, words: &[&str]) -> bool {
    plan.problems
        .iter()
        .any(|problem| words.iter().all(|word| problem.contains(word)))
}

fn manifest(plan: &Plan) -> &str {
    match &plan
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .expect("a manifest")
        .origin
    {
        Origin::Document(text) => text,
        Origin::Copy { .. }
        | Origin::Retarget { .. }
        | Origin::Generated(_)
        | Origin::Cook { .. } => {
            panic!("the manifest is a document unless the project holds its own")
        }
    }
}

#[test]
fn two_contents_claiming_one_collection_name_are_refused_at_planning() {
    let take = container(&["uniforms", "station"]);
    let a = claiming("uniforms", "collection", "Police_Pack");
    let clash = claiming("station", "collection", "police_pack");

    let refused = planned(&take, &[a.clone(), clash]);

    assert!(!refused.is_buildable(), "a clash was planned");
    assert!(
        says(
            &refused,
            &["collection", "Police_Pack", "uniforms", "station"]
        ),
        "{:?}",
        refused.problems
    );

    let distinct = planned(
        &take,
        &[a, claiming("station", "collection", "station_pack")],
    );
    assert!(distinct.is_buildable(), "{:?}", distinct.problems);
}

#[test]
fn a_name_is_only_a_clash_within_its_kind() {
    let take = container(&["uniforms", "station"]);
    let plan = planned(
        &take,
        &[
            claiming("uniforms", "collection", "police"),
            claiming("station", "archetype", "police"),
        ],
    );
    assert!(plan.is_buildable(), "{:?}", plan.problems);
}

#[test]
fn a_content_that_claims_one_name_twice_is_refused() {
    let mut twice = claiming("uniforms", "collection", "police");
    twice.claims.extend(twice.claims.clone());

    let plan = planned(&container(&["uniforms"]), &[twice]);

    assert!(
        says(&plan, &["collection", "police", "uniforms"]),
        "{:?}",
        plan.problems
    );
}

#[test]
fn the_names_of_the_whole_project_count_even_for_a_content_the_output_leaves_out() {
    let contributions = [
        claiming("uniforms", "collection", "police"),
        claiming("station", "collection", "police"),
        Contribution::of("scripts"),
    ];

    // The resource that ships one of the two answers for the clash with the other.
    let one_of_them = planned(&container(&["uniforms"]), &contributions);
    assert!(
        says(&one_of_them, &["police", "uniforms", "station"]),
        "{:?}",
        one_of_them.problems
    );

    // A resource that ships neither is not the one that clashes.
    let neither = planned(&container(&["scripts"]), &contributions);
    assert!(neither.is_buildable(), "{:?}", neither.problems);
}

#[test]
fn two_contents_writing_one_path_are_refused_and_both_are_named() {
    let mut a = Contribution::of("uniforms");
    a.files.push(document("data/peds.meta", 1));
    let mut b = Contribution::of("station");
    b.files.push(document("data/PEDS.meta", 1));

    let plan = planned(&container(&["uniforms", "station"]), &[a, b]);

    assert!(
        says(&plan, &["peds.meta", "PEDS.meta", "uniforms", "station"]),
        "{:?}",
        plan.problems
    );
}

#[test]
fn a_content_nobody_contributes_for_is_refused_and_one_that_is_handed_over_is_planned() {
    let mut contribution = Contribution::of("uniforms");
    contribution.files.push(document("data/peds.meta", 3));

    let handed_over = planned(&container(&["uniforms"]), &[contribution]);
    assert!(handed_over.is_buildable(), "{:?}", handed_over.problems);
    assert!(handed_over.files.iter().any(|f| f.path == "data/peds.meta"));

    let unhandled = planned(&container(&["uniforms"]), &[]);
    assert!(
        says(&unhandled, &["uniforms", "widgets"]),
        "{:?}",
        unhandled.problems
    );
}

#[test]
fn a_content_of_a_kind_this_build_knows_is_refused_too_when_nobody_contributes_for_it() {
    // A caller that forgets one module's contribution would otherwise ship a resource without its content.
    let mut container = container(&["files", "uniforms"]);
    container.modules.push(Module::files("files"));
    let mut contribution = Contribution::of("uniforms");
    contribution.files.push(document("data/peds.meta", 3));

    let plan = planned(&container, &[contribution]);

    assert!(!plan.is_buildable(), "{:?}", plan.files);
    assert!(says(&plan, &["files", "contribut"]), "{:?}", plan.problems);
    assert_eq!(
        plan.problems.len(),
        1,
        "`uniforms` handed its part over: {:?}",
        plan.problems
    );
}

#[test]
fn a_contributions_own_problems_stop_the_plan() {
    let mut contribution = Contribution::of("uniforms");
    contribution.problems.push("garment 3 has no slot".into());

    let plan = planned(&container(&["uniforms"]), &[contribution]);

    assert!(
        says(&plan, &["garment 3 has no slot"]),
        "{:?}",
        plan.problems
    );
}

#[test]
fn the_keys_that_describe_the_whole_resource_must_agree() {
    let take = container(&["uniforms", "station"]);

    let disagree = planned(
        &take,
        &[
            stating("uniforms", "fx_version", &["bodacious"]),
            stating("station", "fx_version", &["cerulean"]),
        ],
    );
    assert!(
        says(&disagree, &["fx_version", "uniforms", "station"]),
        "{:?}",
        disagree.problems
    );

    let agree = planned(
        &take,
        &[
            stating("uniforms", "fx_version", &["bodacious"]),
            stating("station", "fx_version", &["bodacious"]),
        ],
    );
    assert!(agree.is_buildable(), "{:?}", agree.problems);
    assert!(
        manifest(&agree).starts_with("fx_version 'bodacious'\ngame 'gta5'\n"),
        "{}",
        manifest(&agree)
    );
    assert_eq!(manifest(&agree).matches("fx_version").count(), 1);
}

#[test]
fn the_keys_that_add_up_are_merged_in_one_order_whichever_content_came_first() {
    let data = |module, path| stating(module, "data_file", &["PED_VARIATION_FILE", path]);
    let contributions = [
        data("uniforms", "data/a.meta"),
        data("station", "data/b.meta"),
        // Both contents say it, and the resource needs it once.
        data("scripts", "data/a.meta"),
    ];

    let forward = planned(
        &container(&["uniforms", "station", "scripts"]),
        &contributions,
    );
    let backward = planned(
        &container(&["scripts", "station", "uniforms"]),
        &contributions,
    );

    assert!(forward.is_buildable(), "{:?}", forward.problems);
    assert_eq!(manifest(&forward), manifest(&backward));
    let text = manifest(&forward);
    assert_eq!(text.matches("data/a.meta").count(), 1, "{text}");
    assert!(
        text.contains("data_file 'PED_VARIATION_FILE' 'data/b.meta'"),
        "{text}"
    );
}

#[test]
fn a_manifest_line_that_is_not_a_line_is_refused() {
    let take = container(&["uniforms"]);
    for (key, values) in [
        ("two words", vec!["a"]),
        ("a'; os.execute('x') --", vec!["a"]),
        ("", vec!["a"]),
        ("data_file", vec![]),
        ("fx_version", vec!["a", "b"]),
    ] {
        let plan = planned(&take, &[stating("uniforms", key, &values)]);
        assert!(!plan.is_buildable(), "{key:?} {values:?} was planned");
    }
}

#[test]
fn a_resource_that_holds_its_own_manifest_is_not_edited_for_a_content() {
    let mut own = stating(
        "uniforms",
        "data_file",
        &["PED_VARIATION_FILE", "data/a.meta"],
    );
    own.files.push(document("fxmanifest.lua", 5));

    let plan = planned(&container(&["uniforms"]), &[own]);

    assert!(
        says(&plan, &["uniforms", "fxmanifest.lua"]),
        "{:?}",
        plan.problems
    );
}

#[test]
fn figures_add_up_across_contents_against_the_ceiling_the_output_declares() {
    let mut take = container(&["uniforms", "station"]);
    take.outputs[0].limits.insert("entries".into(), 40);
    take.outputs[0].limits.insert("slots".into(), 255);
    let contributions = [
        figure("uniforms", "entries", 30),
        figure("station", "entries", 20),
    ];

    let over = planned(&take, &contributions);

    assert!(says(&over, &["entries", "50", "40"]), "{:?}", over.problems);
    assert_eq!(
        over.budgets,
        [
            Budget {
                name: "entries".into(),
                used: 50,
                ceiling: Some(40)
            },
            // A ceiling nobody contributes to is still the output's, and shows empty.
            Budget {
                name: "slots".into(),
                used: 0,
                ceiling: Some(255)
            },
        ]
    );

    take.outputs[0].limits.insert("entries".into(), 50);
    let within = planned(&take, &contributions);
    assert!(within.is_buildable(), "{:?}", within.problems);
}

#[test]
fn a_figure_nobody_limits_is_listed_and_the_two_built_in_names_are_not_figures() {
    let take = container(&["uniforms"]);

    let free = planned(&take, &[figure("uniforms", "entries", 7)]);
    assert!(free.is_buildable(), "{:?}", free.problems);
    assert_eq!(
        free.budgets,
        [Budget {
            name: "entries".into(),
            used: 7,
            ceiling: None
        }]
    );

    // `files` and `bytes` are totals of the plan, with `max_files` and `max_bytes` for ceilings.
    for name in ["files", "bytes"] {
        let plan = planned(&take, &[figure("uniforms", name, 1)]);
        assert!(says(&plan, &[name, "uniforms"]), "{:?}", plan.problems);
    }
}

#[test]
fn an_id_belongs_to_one_item_and_the_plan_lists_the_items_it_ships_by_id() {
    let mut a = Contribution::of("uniforms");
    a.items = vec![item("g2", "jbib_001_u"), item("g1", "jbib_000_u")];
    let mut b = Contribution::of("station");
    b.items = vec![item("g3", "prop_000")];
    let mut left_out = Contribution::of("scripts");
    left_out.items = vec![item("g9", "never_shipped")];

    let plan = planned(
        &container(&["uniforms", "station"]),
        &[a.clone(), b.clone(), left_out],
    );

    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let ids: Vec<&str> = plan.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["g1", "g2", "g3"]);

    b.items.push(item("g1", "prop_001"));
    let reused = planned(&container(&["uniforms", "station"]), &[a, b]);
    assert!(
        says(&reused, &["g1", "uniforms", "station"]),
        "{:?}",
        reused.problems
    );
}

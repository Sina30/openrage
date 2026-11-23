//! An archive of the game holds at most 65,535 entries: the plan of a singleplayer DLC counts what
//! each archive of the pack would hold, refuses a pack over that, and a build that divides cuts it
//! between contents.

use std::path::Path;

use or_project::{
    plan_with, plans_with, Container, Contribution, Module, Origin, Output, OutputKind, Plan,
    PlannedFile,
};

const FOLDER: &str = "/projects/pack";
const MOST: u64 = 65_535;

/// A content that puts `count` files into the archive `archive` of the pack.
fn content(id: &str, archive: &str, count: usize) -> Contribution {
    let mut contribution = Contribution::of(id);
    contribution.files = (0..count)
        .map(|n| PlannedFile {
            path: format!("{archive}/{id}_{n}.ydd"),
            origin: Origin::Generated(Vec::new()),
        })
        .collect();
    contribution
}

fn project(split: bool) -> Container {
    let mut output = Output::of(
        OutputKind::SingleplayerDlc,
        "dlc",
        "pack",
        &["first", "second"],
    );
    output.split = split;
    Container {
        modules: vec![Module::files("first"), Module::files("second")],
        outputs: vec![output],
    }
}

fn entries(plan: &Plan) -> u64 {
    plan.budgets
        .iter()
        .find(|b| b.name == "archive_entries")
        .map_or(0, |b| b.used)
}

#[test]
fn the_plan_counts_the_files_of_the_biggest_archive_and_the_folders_it_holds() {
    let made = [
        content("first", "x64/models/cdimages/a.rpf/stem", 10),
        content("second", "x64/models/cdimages/a.rpf/stem", 0),
    ];
    let plan = plan_with(&project(false), "dlc", Path::new(FOLDER), &made).unwrap();
    // The ten files, the folder `stem` they sit in and the root of the archive.
    assert_eq!(entries(&plan), 12, "{:?}", plan.budgets);
    let budget = plan
        .budgets
        .iter()
        .find(|b| b.name == "archive_entries")
        .unwrap();
    assert_eq!(budget.ceiling, Some(MOST));
}

#[test]
fn seventy_thousand_entries_in_one_archive_are_refused_with_the_sentence_that_says_why() {
    let made = [
        content("first", "x64/models/cdimages/a.rpf/stem", 35_000),
        content("second", "x64/models/cdimages/a.rpf/stem", 35_000),
    ];
    let plan = plan_with(&project(false), "dlc", Path::new(FOLDER), &made).unwrap();
    assert!(!plan.is_buildable());
    assert!(
        plan.problems
            .iter()
            .any(|p| p.contains("archive_entries") || (p.contains("70002") && p.contains("65535"))),
        "{:?}",
        plan.problems
    );
}

#[test]
fn seventy_thousand_entries_divide_into_two_packs_at_the_boundary_between_contents() {
    let made = [
        content("first", "x64/models/cdimages/a.rpf/stem", 35_000),
        content("second", "x64/models/cdimages/a.rpf/stem", 35_000),
    ];
    let plans = plans_with(&project(true), "dlc", Path::new(FOLDER), &made).unwrap();
    assert_eq!(plans.len(), 2, "one pack per content");
    assert_eq!(plans[0].resource, "pack_1");
    assert_eq!(plans[1].resource, "pack_2");
    for plan in &plans {
        assert!(plan.is_buildable(), "{:?}", plan.problems);
        assert!(entries(plan) <= MOST, "{}", entries(plan));
    }
}

#[test]
fn a_content_over_the_most_an_archive_holds_by_itself_says_a_split_cannot_cut_it() {
    let made = [
        content("first", "x64/models/cdimages/a.rpf/stem", 70_000),
        content("second", "x64/models/cdimages/a.rpf/stem", 0),
    ];
    let plans = plans_with(&project(true), "dlc", Path::new(FOLDER), &made).unwrap();
    assert!(
        plans
            .iter()
            .any(|p| p.problems.iter().any(|p| p.contains("not inside one"))),
        "{:?}",
        plans.iter().map(|p| &p.problems).collect::<Vec<_>>()
    );
}

#[test]
fn a_ceiling_the_build_states_below_the_most_is_the_one_that_counts() {
    let mut container = project(false);
    container.outputs[0]
        .limits
        .insert("archive_entries".into(), 100);
    let made = [
        content("first", "x64/models/cdimages/a.rpf/stem", 150),
        content("second", "x64/models/cdimages/a.rpf/stem", 0),
    ];
    let plan = plan_with(&container, "dlc", Path::new(FOLDER), &made).unwrap();
    assert!(!plan.is_buildable(), "{:?}", plan.problems);
}

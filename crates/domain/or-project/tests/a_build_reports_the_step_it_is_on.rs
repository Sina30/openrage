//! A build says which of its steps it is on, so a screen can show the one running.

use std::cell::RefCell;

use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{
    build_all_observed, build_observed, plan, BuildStep, Container, Output, Plan, Project,
};

fn a_planned_pack(dir: &std::path::Path) -> (Plan, std::path::PathBuf) {
    let folder = dir.join("pack");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("client.lua"), b"print('a')").unwrap();
    let mut container = Container::default();
    container
        .outputs
        .push(Output::fivem_resource("fivem", "pack", &["files"]));
    let file = folder.join("pack.openrage-proj");
    Project::create(&file, &container, &[folder.join("client.lua")]).expect("a new project");
    let project = Project::load(&file).expect("it reads back");
    let planned = plan(
        &project.container,
        "fivem",
        &project.folder(),
        &project.sources(),
    )
    .expect("the output is declared");
    (planned, dir.join("build"))
}

#[test]
fn a_build_passes_its_steps_in_order_and_each_resource_of_a_set_passes_them_again() {
    let dir = tempfile::tempdir().unwrap();
    let (planned, root) = a_planned_pack(dir.path());
    let seen = RefCell::new(Vec::new());

    build_observed(&planned, &root, &ParamRemap::default(), None, &|step| {
        seen.borrow_mut().push(step)
    })
    .expect("the build goes through");
    assert_eq!(
        *seen.borrow(),
        [
            BuildStep::Cook,
            BuildStep::Write,
            BuildStep::ReadBack,
            BuildStep::Swap
        ],
        "plan is the caller's step, taken before the build is handed anything"
    );

    seen.borrow_mut().clear();
    let (built, stopped) = build_all_observed(
        &[planned.clone(), planned],
        &root,
        &ParamRemap::default(),
        None,
        &|step| seen.borrow_mut().push(step),
    );
    assert!(stopped.is_none() && built.len() == 2, "{stopped:?}");
    assert_eq!(seen.borrow().len(), 8, "{:?}", seen.borrow());
    assert!(
        BuildStep::ALL.windows(2).all(|pair| pair[0] < pair[1]),
        "ALL is in the order a build takes them"
    );
}

#[test]
fn a_build_that_stops_reports_no_step_it_never_reached() {
    let dir = tempfile::tempdir().unwrap();
    let (mut planned, root) = a_planned_pack(dir.path());
    // A file that is not where the plan says it is cannot be copied: the build stops while it writes.
    planned.files[0].origin = or_project::Origin::Copy {
        from: dir.path().join("gone.lua"),
        bytes: 3,
    };
    let seen = RefCell::new(Vec::new());
    let stopped = build_observed(&planned, &root, &ParamRemap::default(), None, &|step| {
        seen.borrow_mut().push(step)
    });
    assert!(stopped.is_err());
    assert_eq!(
        *seen.borrow(),
        [BuildStep::Cook, BuildStep::Write],
        "it stopped on the write, so it never read back or swapped"
    );
}

//! A project that links the folder of addon peds reads it into its own builds and writes nothing in it,
//! and the project that holds the folder is the only one that edits it.
//!
//! The ped's files only have names and bytes here: a plan hashes them and no reader opens them.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{peds_meta, plan, write};
use or_addonped::{module_folder, module_folder_to_edit, AddonPed, AddonPeds, Ped, PedFile};
use or_project::adopt::Recogniser;
use or_project::links::{add_link, links};
use or_project::{Container, Module, ModuleKind, Origin, Output, Project};

struct World {
    _dir: tempfile::TempDir,
    /// `shared/brand/modules/peds`, the folder of peds, inside the project `brand`.
    pack: PathBuf,
    brand: PathBuf,
    police: PathBuf,
}

/// Every file under `dir` with the bytes it holds.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut held = BTreeMap::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(next) = open.pop() {
        for entry in std::fs::read_dir(next).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                open.push(path);
            } else {
                held.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
    }
    held
}

/// A project `brand` that holds the peds of a folder, and a project `police` beside it that links that folder and holds nothing else.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let brand_folder = dir.path().join("shared").join("brand");
    let pack = brand_folder.join("modules").join("peds");
    let mut ped = Ped::new("my_ped");
    for file in ["my_ped.yft", "my_ped.ytd"] {
        write(&pack.join(file), file.as_bytes());
        ped.files.push(PedFile {
            path: file.into(),
            at: None,
        });
    }
    write(&pack.join("peds.meta"), peds_meta(&["my_ped"]).as_bytes());
    let mut model = AddonPeds::new();
    model.meta = Some("peds.meta".into());
    model.peds.push(ped);
    model.write(&pack).unwrap();
    let brand = brand_folder.join("brand.openrage-proj");
    let owned = Container {
        modules: vec![Module::addon_ped("peds", "modules/peds")],
        outputs: vec![Output::fivem_resource("fivem", "brand_pack", &["peds"])],
    };
    Project::create(&brand, &owned, &[]).unwrap();

    let police = dir
        .path()
        .join("work")
        .join("police")
        .join("police.openrage-proj");
    let mut container = Container {
        modules: Vec::new(),
        outputs: vec![Output::fivem_resource("fivem", "police_pack", &[])],
    };
    assert!(AddonPed.holds(&pack), "the recogniser knows the folder");
    add_link(
        &mut container,
        &police,
        ModuleKind::AddonPed,
        &pack,
        Some("brand"),
    )
    .unwrap();
    Project::create(&police, &container, &[]).unwrap();
    World {
        _dir: dir,
        pack,
        brand,
        police,
    }
}

#[test]
fn a_linked_ped_folder_is_planned_into_the_project_that_links_it_and_the_source_is_not_touched() {
    let world = world();
    let before = snapshot(world.brand.parent().unwrap());

    let project = Project::load(&world.police).unwrap();
    let linked = project
        .container
        .module("brand")
        .expect("the link is a module");
    assert!(linked.is_link() && linked.kind == ModuleKind::AddonPed);
    let plan = plan(&project.container, &project.folder());
    assert!(plan.is_buildable(), "{:#?}", plan.problems);
    let copies: Vec<&Path> = plan
        .files
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::Copy { from, .. } => Some(from.as_path()),
            _ => None,
        })
        .collect();
    assert!(
        copies.len() == 3 && copies.iter().all(|from| from.starts_with(&world.pack)),
        "the ped's two files and its data file are copied from the linked folder: {copies:#?}"
    );
    let figures: Vec<_> = plan.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(figures, ["my_ped"], "the ped is the plan's item here too");

    assert_eq!(
        snapshot(world.brand.parent().unwrap()),
        before,
        "planning reads the linked folder and writes nothing in the project that holds it"
    );
    assert!(!world.brand.parent().unwrap().join("build").exists());
}

#[test]
fn the_banner_names_the_project_that_writes_the_peds_and_only_it_may() {
    let world = world();
    let project = Project::load(&world.police).unwrap();
    let found = links(&project.container, &world.police);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].problem, None);
    assert!(
        found[0].banner().contains("brand.openrage-proj"),
        "{}",
        found[0].banner()
    );

    let linked = project.container.module("brand").unwrap();
    let refused = module_folder_to_edit(&world.police, &project.container, linked)
        .expect_err("the project that links the folder does not write it");
    assert!(
        refused.to_string().contains("brand.openrage-proj"),
        "{refused}"
    );
    assert!(
        module_folder(&project.folder(), linked).is_ok(),
        "reading it is allowed"
    );

    let source = Project::load(&world.brand).unwrap();
    let own = source.container.module("peds").unwrap();
    assert_eq!(
        module_folder_to_edit(&world.brand, &source.container, own).unwrap(),
        module_folder(&source.folder(), own).unwrap()
    );
}

#[test]
fn a_link_to_a_ped_folder_that_is_gone_stops_the_plan_with_a_sentence() {
    let world = world();
    std::fs::remove_dir_all(&world.pack).unwrap();
    let project = Project::load(&world.police).unwrap();
    let plan = plan(&project.container, &project.folder());
    assert!(!plan.is_buildable());
    assert!(
        plan.problems
            .iter()
            .any(|p| p.contains("brand") && p.contains("not there")),
        "{:#?}",
        plan.problems
    );
}

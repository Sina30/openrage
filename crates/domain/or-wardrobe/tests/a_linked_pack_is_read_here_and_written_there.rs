//! A project that links the folder of a pack reads it into its own builds and writes nothing in it,
//! and the pack's own project is the only one that edits it.
//!
//! The pack's files only have names here: a plan reads their bytes for a digest, never a reader.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, snapshot, FEET, JBIB};
use or_project::links::{add_link, links};
use or_project::{Container, Module, ModuleKind, Output, Project};
use or_wardrobe::{module_folder, module_folder_to_edit, plans, Ped, Wardrobe};

struct World {
    _dir: tempfile::TempDir,
    /// `brand/modules/uniforms`, the pack, inside the project `brand`.
    pack: PathBuf,
    brand: PathBuf,
    police: PathBuf,
}

fn put(folder: &Path, rel: &str, body: &[u8]) {
    let path = folder.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A project `brand` that holds a pack of two garments, and a project `police` beside it that links the pack.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let brand_folder = dir.path().join("shared").join("brand");
    let pack = brand_folder.join("modules").join("uniforms");
    put(&pack, "models/m0.ydd", b"model zero");
    put(&pack, "models/m1.ydd", b"model one");
    put(&pack, "textures/t0.ytd", b"texture zero");
    put(&pack, "textures/t1.ytd", b"texture one");
    let mut wardrobe = Wardrobe::new("brand");
    wardrobe.garments = vec![
        garment(
            Ped::Male,
            JBIB,
            Some(0),
            "models/m0.ydd",
            &["textures/t0.ytd"],
        ),
        garment(
            Ped::Female,
            FEET,
            Some(2),
            "models/m1.ydd",
            &["textures/t1.ytd"],
        ),
    ];
    wardrobe.write(&pack).unwrap();
    let brand = brand_folder.join("brand.openrage-proj");
    let owned = Container {
        modules: vec![Module::clothing("uniforms", "modules/uniforms")],
        outputs: vec![Output::fivem_resource("fivem", "brand_pack", &["uniforms"])],
    };
    Project::create(&brand, &owned, &[]).unwrap();

    let police_folder = dir.path().join("work").join("police");
    put(&police_folder, "client/main.lua", b"print('police')");
    let police = police_folder.join("police.openrage-proj");
    let mut container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::fivem_resource("fivem", "police_pack", &["files"])],
    };
    add_link(
        &mut container,
        &police,
        ModuleKind::Clothing,
        &pack,
        Some("brand"),
    )
    .unwrap();
    Project::create(
        &police,
        &container,
        &[police_folder.join("client/main.lua")],
    )
    .unwrap();
    World {
        _dir: dir,
        pack,
        brand,
        police,
    }
}

#[test]
fn a_linked_pack_is_planned_into_the_project_that_links_it_and_the_source_is_not_touched() {
    let world = world();
    let before = snapshot(world.brand.parent().unwrap());

    let project = Project::load(&world.police).unwrap();
    let planned = plans(&project, "fivem").expect("the build is declared");
    assert_eq!(planned.len(), 1);
    let plan = &planned[0];
    assert!(plan.is_buildable(), "{:#?}", plan.problems);

    let copies: Vec<&Path> = plan
        .files
        .iter()
        .filter_map(|f| match &f.origin {
            or_project::Origin::Copy { from, .. } => Some(from.as_path()),
            _ => None,
        })
        .collect();
    assert!(
        copies.iter().any(|from| from.starts_with(&world.pack)),
        "the garments are copied from the linked folder: {copies:#?}"
    );
    assert!(copies
        .iter()
        .any(|from| from.ends_with("client/main.lua") || from.ends_with("client\\main.lua")));
    // The figure is the one the pack's own project plans for it: nothing is added or lost by the link.
    let figure = |plan: &or_project::Plan| {
        plan.budgets
            .iter()
            .find(|b| b.name == "clothing_drawables")
            .map(|b| b.used)
    };
    let source = Project::load(&world.brand).unwrap();
    let own = &plans(&source, "fivem").unwrap()[0];
    assert!(
        figure(own).is_some_and(|used| used > 0),
        "{:#?}",
        own.budgets
    );
    assert_eq!(figure(plan), figure(own));

    assert_eq!(
        snapshot(world.brand.parent().unwrap()),
        before,
        "planning reads the linked folder and writes nothing in the project that holds it"
    );
    assert!(
        !world.brand.parent().unwrap().join("build").exists(),
        "the source project's builds are its own"
    );
}

#[test]
fn the_banner_names_the_project_that_writes_the_pack_and_only_it_may() {
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
        .expect_err("the project that links a pack does not write it");
    assert!(
        refused.to_string().contains("brand.openrage-proj"),
        "{refused}"
    );
    assert!(
        module_folder(&project.folder(), linked).is_ok(),
        "reading it is allowed"
    );

    // The project that holds the pack writes it.
    let source = Project::load(&world.brand).unwrap();
    let own = source.container.module("uniforms").unwrap();
    assert_eq!(
        module_folder_to_edit(&world.brand, &source.container, own).unwrap(),
        module_folder(&source.folder(), own).unwrap()
    );
}

#[test]
fn a_link_to_a_pack_that_is_gone_stops_the_plan_with_a_sentence_and_nothing_else_changes() {
    let world = world();
    std::fs::remove_dir_all(&world.pack).unwrap();
    let project = Project::load(&world.police).unwrap();
    let plan = &plans(&project, "fivem").unwrap()[0];
    assert!(!plan.is_buildable());
    assert!(
        plan.problems
            .iter()
            .any(|p| p.contains("brand") && p.contains("not there")),
        "{:#?}",
        plan.problems
    );
}

#[test]
fn two_projects_that_link_one_pack_claim_its_collections_once_each() {
    let world = world();
    // A project of its own clothing, in the collection the linked pack already claims.
    let mut project = Project::load(&world.police).unwrap();
    let own = world.police.parent().unwrap().join("own");
    let mut wardrobe = Wardrobe::new("brand");
    wardrobe.garments = vec![garment(Ped::Male, JBIB, Some(0), "m.ydd", &["t.ytd"])];
    put(&own, "m.ydd", b"m");
    put(&own, "t.ytd", b"t");
    wardrobe.write(&own).unwrap();
    project
        .container
        .modules
        .push(Module::clothing("own", "own"));
    project.container.outputs[0].take.push("own".into());

    let plan = &plans(&project, "fivem").unwrap()[0];
    assert!(
        plan.problems
            .iter()
            .any(|p| p.to_lowercase().contains("mp_m_brand")),
        "a name the game resolves once is one namespace across linked content too: {:#?}",
        plan.problems
    );
}

//! A module of particle sources goes through the one plan and the one build: each source cooks to `stream/<name>.ypt`, reloads through
//! the reader of the format, and the verifier reads the built resource back; a pack that breaks a rule is refused before anything is
//! written, and the cell of a singleplayer DLC is derived from what the content hands over.
//!
//! ⚠ The sources are made here, from lists the editor's own calls make: the clean checkout runs this, and the install-gated suite holds
//! the source form to the files of a mounted game.

mod common;

use std::path::{Path, PathBuf};

use or_particles::import::{add_files, Incoming};
use or_particles::model::Particles;
use or_particles::rules::rule_of;
use or_particles::verify::verify;
use or_particles::{contributions, inspect, Depth};
use or_project::{build_with, Container, Module, Origin, Output, OutputKind, Plan};

/// The module folder `dir/fx` holding two sources, `fx_a.particles.src` and `fx_b.particles.src`.
fn module(dir: &Path) -> PathBuf {
    let folder = dir.join("fx");
    for (name, effect) in [("fx_a", "spark_a"), ("fx_b", "spark_b")] {
        let bytes = common::a_ypt(name, effect);
        let file = format!("{name}.ypt");
        let added = add_files(
            &folder,
            &[Incoming {
                name: &file,
                bytes: &bytes,
                from_game: false,
                virtual_path: None,
            }],
        )
        .unwrap();
        assert_eq!(added.refused, Vec::<(String, String)>::new());
    }
    folder
}

fn container(kind: OutputKind) -> Container {
    let mut output = Output::fivem_resource("fivem", "sparks", &["fx"]);
    output.kind = kind;
    Container {
        modules: vec![Module::particles("fx", "fx")],
        outputs: vec![output],
    }
}

fn plan(container: &Container, folder: &Path) -> Plan {
    let contributions = or_particles::contributions(container, folder);
    or_project::plan_with(container, "fivem", folder, &contributions).expect("the output exists")
}

fn cooker(source: &Path, scratch: &Path) -> Result<PathBuf, Vec<String>> {
    or_source::cook::cook_source(source, scratch)
}

#[test]
fn a_pack_of_sources_plans_to_the_stream_folder_builds_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = module(dir.path());
    let container = container(OutputKind::FivemResource);
    let planned = plan(&container, dir.path());
    assert_eq!(planned.problems, Vec::<String>::new());

    let paths: Vec<&str> = planned.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["fxmanifest.lua", "stream/fx_a.ypt", "stream/fx_b.ypt"],
        "each cooked file at the folder FiveM streams from, and the manifest, by path"
    );
    assert!(planned
        .files
        .iter()
        .filter(|f| f.path.starts_with("stream/"))
        .all(|f| matches!(f.origin, Origin::Cook { .. })));
    assert_eq!(planned.items.len(), 2);

    let root = dir.path().join("build");
    let built = build_with(&planned, &root, &Default::default(), Some(&cooker))
        .expect("the build cooks, writes and reads back");
    assert_eq!(built.verification.problems, Vec::<String>::new());
    assert!(
        built
            .verification
            .reloaded
            .iter()
            .any(|r| r.path == "stream/fx_a.ypt"),
        "the particle file was reopened through its reader"
    );

    let model = Particles::read(&folder).unwrap();
    let facts = inspect(&folder, &model, Depth::Open);
    let found = verify(&model, &facts, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    assert_eq!(found.checks.len(), 2);

    // A second build of the same sources is the same build.
    let again = build_with(
        &plan(&container, dir.path()),
        &root,
        &Default::default(),
        Some(&cooker),
    )
    .expect("the second build");
    for file in ["stream/fx_a.ypt", "stream/fx_b.ypt"] {
        assert_eq!(
            std::fs::read(built.folder.join(file)).unwrap(),
            std::fs::read(again.folder.join(file)).unwrap(),
            "{file}"
        );
    }
}

#[test]
fn a_pack_that_breaks_a_rule_is_refused_by_the_plan_and_names_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let folder = module(dir.path());
    let mut model = Particles::read(&folder).unwrap();
    model
        .assets
        .push(or_particles::Asset::new("gone.particles.src"));
    model.write(&folder).unwrap();

    let planned = plan(&container(OutputKind::FivemResource), dir.path());
    let said = planned.problems.join("\n");
    assert!(
        said.contains("source-missing") && said.contains("gone.particles.src"),
        "{said}"
    );
    assert!(!rule_of("source-missing").unwrap().waivable);
}

#[test]
fn a_singleplayer_dlc_cell_is_derived_from_what_the_content_hands_over() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path());
    let container = container(OutputKind::FivemResource);
    let handed = contributions(&container, dir.path());
    assert_eq!(handed.len(), 1);
    assert_eq!(
        or_project::cannot_ship(&handed[0], &OutputKind::FivemResource),
        None
    );
    // A `.ypt` streams from `stream/`, and the game reads no type for it: the install keeps its own inside `ptfx.rpf` archives.
    let said = or_project::cannot_ship(&handed[0], &OutputKind::SingleplayerDlc)
        .expect("a file with no type locks the cell");
    assert!(said.contains("stream/fx_a.ypt"), "{said}");
}

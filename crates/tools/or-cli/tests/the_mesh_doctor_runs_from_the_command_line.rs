//! `openrage-cli wardrobe mesh` does what the Garment panel's buttons do: it looks and writes nothing, makes the levels of detail a model lacks as a new source the garment names, refuses a mirror it has no skeleton for, and says no to what it does not take.
//!
//! ⚠ Reads a ped component staged in the corpus; a machine without it skips, and says so.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::{Container, Module, Output, Project};
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::{Component, Garment, Ped, Slot, Wardrobe};

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string(), "mesh".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn staged(name: &str) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the ped component {name}"),
            "not staged".to_string(),
        );
    }
    found
}

/// A project whose one clothing content holds a jacket whose model is a source made of `file`.
fn a_project(dir: &Path, file: &Path) -> (PathBuf, PathBuf, String) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("models")).unwrap();
    models::extract_drawables(
        &std::fs::read(file).unwrap(),
        "model",
        &pack.join("models"),
        &Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton: None,
        },
    )
    .unwrap();
    let mut w = Wardrobe::new("demo");
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some(Slot::Component(Component::Jbib));
    g.model = Some("models/model.drawables.src".to_string());
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "clothing")],
        outputs: vec![Output::fivem_resource("fivem", "demo", &["wardrobe"])],
    };
    let project = dir.join("demo.openrage-proj");
    Project::create(&project, &container, &[]).unwrap();
    (project, pack, id)
}

fn model_of(pack: &Path) -> Option<String> {
    Wardrobe::read(pack).unwrap().garments[0].model.clone()
}

#[test]
fn the_look_writes_nothing_and_the_levels_are_made_as_a_new_source_the_garment_names() {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (project, pack, id) = a_project(dir.path(), &file);
    let project = project.to_string_lossy().into_owned();
    let old = model_of(&pack);

    assert_eq!(
        wardrobe(&[&project, "--garment", &id, "--check"]),
        ExitCode::SUCCESS
    );
    assert_eq!(model_of(&pack), old, "a look changes nothing");

    assert_eq!(
        wardrobe(&[&project, "--garment", &id, "--levels", "35,8"]),
        ExitCode::SUCCESS
    );
    let now = model_of(&pack).unwrap();
    assert_ne!(Some(now.clone()), old);
    assert!(pack.join(&now).is_dir() && pack.join(old.as_deref().unwrap()).is_dir());

    // Run again, the levels are there and nothing is made.
    assert_eq!(
        wardrobe(&[&project, "--garment", &id, "--levels", "35,8"]),
        ExitCode::SUCCESS
    );
    assert_eq!(model_of(&pack), Some(now));
}

#[test]
fn a_mirror_without_the_skeleton_is_refused_and_a_call_that_names_no_operation_is_a_usage_error() {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (project, pack, id) = a_project(dir.path(), &file);
    let project = project.to_string_lossy().into_owned();
    let old = model_of(&pack);

    assert_eq!(
        wardrobe(&[&project, "--garment", &id, "--mirror"]),
        ExitCode::FAILURE
    );
    assert_eq!(model_of(&pack), old, "the refusal changed nothing");

    assert_ne!(
        wardrobe(&[&project, "--garment", &id]),
        ExitCode::SUCCESS,
        "no operation is named"
    );
    assert_ne!(
        wardrobe(&[&project, "--garment", &id, "--check", "--rig"]),
        ExitCode::SUCCESS,
        "two are named"
    );
    assert_ne!(
        wardrobe(&[&project, "--garment", &id, "--levels", "150"]),
        ExitCode::SUCCESS,
        "a share past a hundred percent"
    );
    assert_ne!(
        wardrobe(&[&project, "--check"]),
        ExitCode::SUCCESS,
        "no garment is named"
    );
}

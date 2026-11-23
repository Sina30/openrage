//! `openrage-cli particles` lists the effects of a project's particle files, validates them and reads a build back, and the one `project`
//! plan and build take the particles content as they take any other.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn cli(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A `.ypt` of one effect that fires one emitter rule and one particle rule.
fn a_ypt(effect: &str) -> Vec<u8> {
    or_particle::sample::list_of("fx_test", &[effect])
}

/// A project with a particles content whose pack holds the file `fx_test`, which states the effect `effect`.
fn a_project_with_particles(dir: &Path, effect: &str) -> PathBuf {
    std::fs::write(
        dir.join("sparks.openrage-proj"),
        r#"{
  "documents": [],
  "game": "both",
  "modules": [{ "id": "particles", "path": "particles", "type": "particles" }],
  "open": [],
  "outputs": [{ "id": "fivem", "kind": "fivem-resource", "name": "sparks", "take": ["particles"] }],
  "schema": 4
}"#,
    )
    .unwrap();
    let folder = dir.join("particles");
    let added = or_particles::import::add_files(
        &folder,
        &[or_particles::import::Incoming {
            name: "fx_test.ypt",
            bytes: &a_ypt(effect),
            from_game: false,
            virtual_path: None,
        }],
    )
    .unwrap();
    assert_eq!(added.refused, Vec::<(String, String)>::new());
    dir.join("sparks.openrage-proj")
}

#[test]
fn a_particle_pack_is_listed_and_validated_and_a_word_that_is_no_command_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_particles(dir.path(), "fx_spark");
    assert_eq!(cli("particles", &["list", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        cli("particles", &["validate", text(&file)]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        cli("particles", &["validate", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    assert_ne!(cli("particles", &["dance"]), ExitCode::SUCCESS);
    assert_eq!(cli("particles", &[]), ExitCode::from(2));
}

#[test]
fn a_pack_with_a_missing_source_is_refused_and_so_is_a_project_with_no_particles() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_particles(dir.path(), "fx_spark");
    let folder = dir.path().join("particles");
    let mut model = or_particles::Particles::read(&folder).unwrap();
    model
        .assets
        .push(or_particles::Asset::new("gone.particles.src"));
    model.write(&folder).unwrap();
    assert_eq!(
        cli("particles", &["validate", text(&file)]),
        ExitCode::FAILURE
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::FAILURE);

    let plain = tempfile::tempdir().unwrap();
    assert_eq!(
        cli(
            "project",
            &["create", text(plain.path()), "--name", "plain"]
        ),
        ExitCode::SUCCESS
    );
    let file = plain.path().join("plain.openrage-proj");
    assert_eq!(
        cli("particles", &["validate", text(&file)]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_build_of_particles_is_read_back_against_the_files_and_a_missing_one_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_particles(dir.path(), "fx_spark");
    assert_eq!(
        cli("particles", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "nothing is built yet, so there is nothing to read"
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        cli("particles", &["verify", text(&file)]),
        ExitCode::SUCCESS
    );

    let shipped = dir
        .path()
        .join("build")
        .join("sparks")
        .join("stream")
        .join("fx_test.ypt");
    assert!(
        shipped.is_file(),
        "the particle file ships in the stream folder"
    );
    std::fs::remove_file(&shipped).unwrap();
    assert_eq!(
        cli("particles", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "the file the plan gave the source is not in the resource"
    );
}

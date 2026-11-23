//! `openrage-cli maps` lists what a project's map sets place and declare, validates them and reads a build back, and the one `project`
//! plan and build take the maps content as they take any other.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_maps::import::{add_files, Incoming};
use or_maps::sample::{map_file, types_file, Archetype, Entity};

fn cli(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A project with a maps content whose pack holds a map file placing two crates and a type file declaring them.
fn a_project_with_maps(dir: &Path) -> PathBuf {
    std::fs::write(
        dir.join("harbour.openrage-proj"),
        r#"{
  "documents": [],
  "game": "both",
  "modules": [{ "id": "maps", "path": "maps", "type": "maps" }],
  "open": [],
  "outputs": [{ "id": "fivem", "kind": "fivem-resource", "name": "harbour", "take": ["maps"] }],
  "schema": 4
}"#,
    )
    .unwrap();
    let map = map_file(
        "harbour",
        &[
            Entity::at("prop_crate_a", [10.0, 20.0, 0.5], 1),
            Entity::at("prop_crate_a", [14.0, 20.0, 0.5], 2),
        ],
        ([8.0, 18.0, -1.0], [16.0, 22.0, 2.0]),
    );
    let types = types_file(
        "harbour_types",
        &[Archetype::new("prop_crate_a", [-1.0; 3], [1.0; 3])],
    );
    let added = add_files(
        &dir.join("maps"),
        &[
            Incoming {
                name: "harbour.ymap",
                bytes: &map,
                from_game: false,
                virtual_path: None,
            },
            Incoming {
                name: "harbour_types.ytyp",
                bytes: &types,
                from_game: false,
                virtual_path: None,
            },
        ],
        None,
    )
    .unwrap();
    assert_eq!(added.refused, Vec::<(String, String)>::new());
    dir.join("harbour.openrage-proj")
}

#[test]
fn a_map_pack_is_listed_and_validated_and_a_word_that_is_no_command_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_maps(dir.path());
    assert_eq!(cli("maps", &["list", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("maps", &["validate", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        cli("maps", &["validate", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    assert_ne!(cli("maps", &["dance"]), ExitCode::SUCCESS);
    assert_eq!(cli("maps", &[]), ExitCode::from(2));
}

#[test]
fn a_pack_with_a_missing_source_is_refused_and_so_is_a_project_with_no_maps() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_maps(dir.path());
    let folder = dir.path().join("maps");
    let mut model = or_maps::Maps::read(&folder).unwrap();
    model.maps[0].sources.push("gone.metadata.src".into());
    model.write(&folder).unwrap();
    assert_eq!(cli("maps", &["validate", text(&file)]), ExitCode::FAILURE);

    let empty = tempfile::tempdir().unwrap();
    std::fs::write(
        empty.path().join("none.openrage-proj"),
        r#"{ "documents": [], "game": "both", "modules": [], "open": [], "outputs": [], "schema": 4 }"#,
    )
    .unwrap();
    assert_eq!(
        cli(
            "maps",
            &["list", text(&empty.path().join("none.openrage-proj"))]
        ),
        ExitCode::FAILURE
    );
}

#[test]
fn a_build_of_maps_is_read_back_against_the_sets_and_a_tampered_one_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_maps(dir.path());
    assert_eq!(
        cli("maps", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "nothing is built yet, so there is nothing to read"
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("maps", &["verify", text(&file)]), ExitCode::SUCCESS);

    let shipped = dir
        .path()
        .join("build")
        .join("harbour")
        .join("stream")
        .join("harbour.ymap");
    assert!(shipped.is_file(), "the map file ships in the stream folder");
    std::fs::write(&shipped, b"not a map").unwrap();
    assert_eq!(
        cli("maps", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "the file the plan gave the source does not read as a map"
    );
}

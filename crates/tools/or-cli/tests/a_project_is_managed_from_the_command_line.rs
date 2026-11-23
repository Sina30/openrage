//! `openrage-cli project` creates a project the way the editor does (a `sources/` folder, the game
//! recorded), adds content and builds, removes a build, cleans what builds left and writes the
//! redirect map, so a project is made and kept without the editor. `--output` still names a build.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;

fn project(args: &[&str]) -> ExitCode {
    let mut all = vec!["project".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn ids(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect()
}

/// A project of one script, made through the command line.
fn made(dir: &Path, extra: &[&str]) -> (PathBuf, PathBuf) {
    let folder = dir.join("pack");
    let script = folder.join("client.lua");
    write(&script, b"print('hi')\n");
    let mut args = vec!["create", text(&folder), text(&script)];
    args.extend_from_slice(extra);
    assert_eq!(project(&args), ExitCode::SUCCESS);
    (folder.clone(), folder.join("pack.openrage-proj"))
}

#[test]
fn create_makes_the_sources_folder_and_records_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &["--game", "enhanced"]);

    assert!(folder.join("sources").is_dir(), "no sources/ folder");
    let top = read_json(&file);
    assert_eq!(top["game"], "enhanced");
    assert_eq!(
        top["outputs"][0]["target"], "enhanced",
        "a build ships for the game's own edition, as the editor's New project does"
    );

    let both = dir.path().join("both");
    write(&both.join("a.lua"), b"a");
    assert_eq!(
        project(&["create", text(&both), text(&both.join("a.lua"))]),
        ExitCode::SUCCESS
    );
    let top = read_json(&both.join("both.openrage-proj"));
    assert_eq!(
        top["game"], "both",
        "the game is recorded when none is said"
    );
    assert!(top["outputs"][0].get("target").is_none());
    assert!(both.join("sources").is_dir());

    assert_eq!(
        project(&["create", text(&dir.path().join("x")), "--game", "xbox"]),
        ExitCode::from(2)
    );
}

#[test]
fn a_project_is_kept_from_the_command_line_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &["--game", "legacy"]);
    let more = folder.join("html").join("index.html");
    write(&more, b"<p>hi</p>");

    // Content: a file is added once, and one outside the project is refused.
    assert_eq!(
        project(&["add-content", text(&file), text(&more)]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&["add-content", text(&file), text(&more)]),
        ExitCode::SUCCESS
    );
    let documents = read_json(&file)["documents"].as_array().unwrap().len();
    assert_eq!(documents, 2, "the same file was added twice");
    let outside = dir.path().join("elsewhere.txt");
    write(&outside, b"x");
    assert_eq!(
        project(&["add-content", text(&file), text(&outside)]),
        ExitCode::FAILURE
    );

    // Builds: a second one takes all the content and the game's edition; a name is used once.
    assert_eq!(
        project(&["add-build", text(&file), "--name", "second_pack"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&["add-build", text(&file), "--name", "Second_Pack"]),
        ExitCode::FAILURE
    );
    let top = read_json(&file);
    assert_eq!(ids(&top["outputs"]), ["fivem", "second_pack"]);
    assert_eq!(top["outputs"][1]["take"], serde_json::json!(["files"]));
    assert_eq!(top["outputs"][1]["target"], "legacy");

    // Each build is built by its id; `--output` is the older spelling of the same flag.
    assert_eq!(
        project(&["build", text(&file), "--build", "fivem"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&["build", text(&file), "--output", "second_pack"]),
        ExitCode::SUCCESS
    );
    let built = folder.join("build");
    assert!(built.join("pack").is_dir() && built.join("second_pack").is_dir());
    assert!(built.join("second_pack/html/index.html").is_file());

    // A clean of one build leaves the other; a clean of all leaves none.
    assert_eq!(
        project(&["clean", text(&file), "--build", "second_pack"]),
        ExitCode::SUCCESS
    );
    assert!(!built.join("second_pack").exists());
    assert!(
        built.join("pack").is_dir(),
        "a clean of one build took another"
    );
    assert_eq!(project(&["clean", text(&file)]), ExitCode::SUCCESS);
    assert!(!built.join("pack").exists());
    assert_eq!(
        project(&["clean", text(&file), "--build", "nowhere"]),
        ExitCode::FAILURE,
        "a build the project does not declare"
    );

    // A build is removed by its id, once.
    assert_eq!(
        project(&["remove-build", text(&file), "second_pack"]),
        ExitCode::SUCCESS
    );
    assert_eq!(ids(&read_json(&file)["outputs"]), ["fivem"]);
    assert_eq!(
        project(&["remove-build", text(&file), "second_pack"]),
        ExitCode::FAILURE
    );
    assert_eq!(
        project(&["plan", text(&file), "--build", "second_pack"]),
        ExitCode::FAILURE
    );
}

/// What a build leaves for `resource`: its folder and the hidden marker that says OpenRage built it.
fn leave_build(root: &Path, resource: &str) {
    write(&root.join(resource).join("client.lua"), b"print('hi')\n");
    write(
        &root.join(format!(".{resource}.openrage-build")),
        b"OpenRage built the folder beside this file.\n",
    );
}

#[test]
fn a_clean_of_one_build_leaves_the_build_the_default_name_gives_the_second() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &[]);

    // With no --name, the second build is named for the project and a number, but not `pack_2`, where a split writes its second resource: `pack_v2`.
    assert_eq!(project(&["add-build", text(&file)]), ExitCode::SUCCESS);
    let top = read_json(&file);
    assert_eq!(top["outputs"][1]["name"], "pack_v2");
    assert_eq!(
        project(&["build", text(&file), "--build", "fivem"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&["build", text(&file), "--build", "pack_v2"]),
        ExitCode::SUCCESS
    );
    let built = folder.join("build");
    assert!(built.join("pack").is_dir() && built.join("pack_v2").is_dir());
    assert!(built.join(".pack_v2.openrage-items.json").is_file());

    assert_eq!(
        project(&["clean", text(&file), "--build", "fivem"]),
        ExitCode::SUCCESS
    );

    assert!(!built.join("pack").exists(), "the build named was left");
    assert!(
        built.join("pack_v2").is_dir(),
        "a clean of one build took the folder of the other"
    );
    assert!(
        built.join(".pack_v2.openrage-items.json").is_file(),
        "a clean of one build took what the other recorded"
    );
    assert!(
        built.join(".pack_v2.openrage-build").is_file(),
        "a clean of one build took the marker of the other"
    );
}

#[test]
fn a_clean_of_a_build_that_splits_takes_its_parts_and_not_a_build_declared_with_a_number() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &[]);
    assert_eq!(
        project(&["add-build", text(&file), "--name", "pack_2"]),
        ExitCode::SUCCESS
    );
    let mut top = read_json(&file);
    top["outputs"][0]["split"] = Value::Bool(true);
    write(&file, &serde_json::to_vec_pretty(&top).unwrap());
    let built = folder.join("build");
    // The parts of a split are `pack_1` and `pack_3` here, and `pack_2` is what the second build wrote.
    for resource in ["pack_1", "pack_2", "pack_3"] {
        leave_build(&built, resource);
    }

    assert_eq!(
        project(&["clean", text(&file), "--build", "fivem"]),
        ExitCode::SUCCESS
    );

    assert!(
        !built.join("pack_1").exists(),
        "a part of the split was left"
    );
    assert!(
        !built.join("pack_3").exists(),
        "a part of the split was left"
    );
    assert!(
        built.join("pack_2").is_dir() && built.join(".pack_2.openrage-build").is_file(),
        "a clean took the resource of a build that is declared with that name"
    );
    assert_eq!(project(&["clean", text(&file)]), ExitCode::SUCCESS);
    assert!(!built.join("pack_2").exists());
}

#[test]
fn clothing_content_is_added_with_a_folder_and_every_build_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &[]);

    assert_eq!(
        project(&["add-content", text(&file), "--kind", "clothing"]),
        ExitCode::SUCCESS
    );

    assert!(folder.join("clothing/wardrobe.json").is_file());
    let top = read_json(&file);
    assert_eq!(ids(&top["modules"]), ["files", "clothing"]);
    assert_eq!(top["modules"][1]["type"], "clothing");
    assert_eq!(
        top["outputs"][0]["take"],
        serde_json::json!(["files", "clothing"])
    );
    assert_eq!(
        project(&["add-content", text(&file), "--kind", "clothing"]),
        ExitCode::SUCCESS
    );
    let top = read_json(&file);
    assert_eq!(ids(&top["modules"]), ["files", "clothing", "clothing-2"]);
    assert_ne!(
        read_json(&folder.join("clothing/wardrobe.json"))["pack"],
        read_json(&folder.join("clothing-2/wardrobe.json"))["pack"],
        "two packs of one name claim the same collections"
    );
}

#[test]
fn the_redirect_map_is_written_from_the_record_of_the_build_before() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, file) = made(dir.path(), &[]);
    let map = dir.path().join("redirects.json");

    // No build has recorded anything yet: the map is empty, and the command says why on stderr.
    assert_eq!(
        project(&["redirects", text(&file), "--file", text(&map)]),
        ExitCode::SUCCESS
    );
    let empty = read_json(&map);
    assert_eq!(empty["format"], "openrage-redirects");
    assert_eq!(empty["redirects"], serde_json::json!([]));

    assert_eq!(project(&["build", text(&file)]), ExitCode::SUCCESS);
    assert!(folder.join("build/.pack.openrage-items.json").is_file());
    std::fs::remove_file(&map).unwrap();
    assert_eq!(
        project(&["redirects", text(&file), "--file", text(&map)]),
        ExitCode::SUCCESS
    );
    assert_eq!(read_json(&map)["redirects"], serde_json::json!([]));
    assert_eq!(
        project(&[
            "redirects",
            text(&file),
            "--file",
            text(&map),
            "--build",
            "nowhere"
        ]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_second_build_on_request_says_whether_the_bytes_are_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let (_, file) = made(dir.path(), &[]);

    assert_eq!(
        project(&["build", text(&file), "--reproducible"]),
        ExitCode::SUCCESS
    );
}

#[test]
fn a_verb_with_nothing_to_act_on_is_a_usage_error() {
    let usage = ExitCode::from(2);
    for verb in [
        "add-content",
        "add-build",
        "remove-build",
        "clean",
        "redirects",
    ] {
        assert_eq!(project(&[verb]), usage, "{verb}");
    }
    assert_eq!(project(&["remove-build", "p.openrage-proj"]), usage);
    assert_eq!(
        project(&["add-content", "p.openrage-proj", "--kind", "widgets"]),
        usage
    );
}

//! `openrage-cli project` names a second build so it cannot read as a part of a split, changes a build
//! the way the Builds window does (content, split, ceilings, file types), and links a whole project,
//! or just its Files content, whose own links it does not follow.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::container::SHIP_UNCHANGED_KEY;
use or_project::Project;
use serde_json::{json, Value};

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

fn json_of(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A project called `name` in its own folder under `dir`, holding the files named.
fn made(dir: &Path, name: &str, files: &[&str]) -> PathBuf {
    let folder = dir.join(name);
    let mut args = vec!["create".to_string(), text(&folder).to_string()];
    for file in files {
        write(&folder.join(file), file.as_bytes());
        args.push(text(&folder.join(file)).to_string());
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(project(&args), ExitCode::SUCCESS);
    folder.join(format!("{name}.openrage-proj"))
}

fn build_names(file: &Path) -> Vec<String> {
    json_of(file)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_second_build_is_named_for_what_it_is_and_never_as_a_part_of_a_split() {
    let dir = tempfile::tempdir().unwrap();
    let file = made(dir.path(), "pack", &["client.lua"]);

    for extra in [
        vec![],
        vec!["--kind", "dlc"],
        vec!["--target", "enhanced"],
        vec![],
    ] {
        let mut args = vec!["add-build", text(&file)];
        args.extend(extra);
        assert_eq!(project(&args), ExitCode::SUCCESS);
    }

    let names = build_names(&file);
    assert_eq!(
        names,
        ["pack", "pack_v2", "pack_dlc", "pack_enhanced", "pack_v3"]
    );
    assert!(
        !names.iter().any(|name| name.ends_with("_2")),
        "`pack_2` is where a split of `pack` writes its second resource: {names:?}"
    );
}

#[test]
fn a_build_is_changed_from_the_command_line_and_a_refusal_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = made(dir.path(), "pack", &["client.lua", "data/handling.meta"]);
    assert_eq!(
        project(&["add-build", text(&file), "--kind", "dlc"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&[
            "set-build",
            text(&file),
            "pack_dlc",
            "--name",
            "pack_pd",
            "--split",
            "on",
            "--max-files",
            "40",
            "--max-bytes",
            "1048576",
            "--limits",
            "clothing_drawables=128,metadata_entries=900",
            "--ship-unchanged",
            "on",
            "--target",
            "enhanced",
        ]),
        ExitCode::SUCCESS
    );
    let top = json_of(&file);
    let dlc = &top["outputs"][1];
    assert_eq!(dlc["id"], "pack_dlc", "a rename keeps the id");
    assert_eq!(dlc["name"], "pack_pd");
    assert_eq!(dlc["split"], true);
    assert_eq!(
        (dlc["max_files"].clone(), dlc["max_bytes"].clone()),
        (json!(40), json!(1_048_576))
    );
    assert_eq!(
        dlc["limits"],
        json!({ "clothing_drawables": 128, "metadata_entries": 900 })
    );
    assert_eq!(dlc[SHIP_UNCHANGED_KEY], true);
    assert_eq!(dlc["target"], "enhanced");

    // A ceiling and a figure are taken away with `none`, and so is the edition with `authored`.
    assert_eq!(
        project(&[
            "set-build",
            text(&file),
            "pack_dlc",
            "--max-files",
            "none",
            "--limits",
            "clothing_drawables=none",
            "--target",
            "authored",
        ]),
        ExitCode::SUCCESS
    );
    let dlc = json_of(&file)["outputs"][1].clone();
    assert!(dlc.get("max_files").is_none() && dlc.get("target").is_none());
    assert_eq!(dlc["limits"], json!({ "metadata_entries": 900 }));

    // The content a build takes is the list given.
    assert_eq!(
        project(&["set-build", text(&file), "pack_dlc", "--take", ""]),
        ExitCode::SUCCESS
    );
    assert_eq!(json_of(&file)["outputs"][1]["take"], json!([]));
    assert_eq!(
        project(&["set-build", text(&file), "pack_dlc", "--take", "files"]),
        ExitCode::SUCCESS
    );
    assert_eq!(json_of(&file)["outputs"][1]["take"], json!(["files"]));

    // Refusals: each leaves the file as it was.
    let before = std::fs::read(&file).unwrap();
    for (what, args) in [
        ("a name another build writes", vec!["--name", "PACK"]),
        ("content the project lacks", vec!["--take", "ghost"]),
        ("a total as a figure", vec!["--limits", "files=3"]),
    ] {
        let mut all = vec!["set-build", text(&file), "pack_dlc"];
        all.extend(args);
        assert_eq!(project(&all), ExitCode::FAILURE, "{what}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "{what} wrote");
    }
    for (what, args) in [
        ("a count that is not one", vec!["--max-files", "many"]),
        ("a switch that is neither", vec!["--split", "maybe"]),
        ("a pair with no value", vec!["--limits", "figure"]),
        ("nothing to change", vec![]),
    ] {
        let mut all = vec!["set-build", text(&file), "pack_dlc"];
        all.extend(args);
        assert_eq!(project(&all), ExitCode::from(2), "{what}");
    }
    assert_eq!(
        project(&["set-build", text(&file), "nope", "--split", "on"]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_ceiling_set_from_the_command_line_changes_the_plan_and_a_file_type_registers_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = made(dir.path(), "pack", &["a.lua", "b.lua", "c.lua"]);
    let planned = || {
        let loaded = Project::load(&file).unwrap();
        or_content::plans(&loaded, "fivem", &[]).expect("the build is declared")
    };
    assert_eq!(planned().len(), 1);
    assert!(planned()[0].is_buildable());

    assert_eq!(
        project(&["set-build", text(&file), "fivem", "--max-files", "3"]),
        ExitCode::SUCCESS
    );
    let blocked = planned();
    assert!(
        !blocked[0].is_buildable(),
        "a ceiling under the files the build writes stops it: {:?}",
        blocked[0].problems
    );

    assert_eq!(
        project(&["set-build", text(&file), "fivem", "--split", "on"]),
        ExitCode::SUCCESS
    );
    assert!(
        planned().len() > 1 || !planned()[0].is_buildable(),
        "a build over a ceiling that may split divides, or says what by itself is too big"
    );

    assert_eq!(
        project(&["add-build", text(&file), "--kind", "dlc"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&["set-file-type", text(&file), "a.lua", "HANDLING_FILE"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        json_of(&file)["modules"][0]["file_types"],
        json!({ "a.lua": "HANDLING_FILE" })
    );
    let loaded = Project::load(&file).unwrap();
    let dlc = or_content::plans(&loaded, "pack_dlc", &[]).unwrap();
    assert!(
        dlc[0]
            .registrations
            .iter()
            .any(|r| r.path == "a.lua" && r.file_type == "HANDLING_FILE"),
        "{:?}",
        dlc[0].registrations
    );
    assert_eq!(
        project(&["set-file-type", text(&file), "a.lua", "none"]),
        ExitCode::SUCCESS
    );
    assert!(json_of(&file)["modules"][0].get("file_types").is_none());
    assert_eq!(
        project(&["set-file-type", text(&file), "a.lua", "handling"]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_whole_project_is_linked_and_the_links_it_holds_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let logos = made(dir.path(), "logos", &["logo/a.lua"]);
    let brand = made(dir.path(), "brand", &["client/main.lua"]);
    let police = made(dir.path(), "police", &["server.lua"]);
    assert_eq!(
        project(&[
            "link",
            text(&brand),
            text(logos.parent().unwrap()),
            "--only",
            "files"
        ]),
        ExitCode::SUCCESS
    );

    assert_eq!(
        project(&["link", text(&police), text(brand.parent().unwrap())]),
        ExitCode::SUCCESS
    );
    let loaded = Project::load(&police).unwrap();
    let ids: Vec<&str> = loaded
        .container
        .modules
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["files", "brand"],
        "logos is brand's link, not police's"
    );
    assert_eq!(loaded.container.outputs[0].take, ["files", "brand"]);
    let shipped: Vec<String> = or_content::plans(&loaded, "fivem", &[]).unwrap()[0]
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect();
    assert!(
        shipped.contains(&"client/main.lua".to_string()),
        "{shipped:?}"
    );
    assert!(!shipped.contains(&"logo/a.lua".to_string()), "{shipped:?}");
    assert_eq!(project(&["links", text(&police)]), ExitCode::SUCCESS);

    // The Files content alone, under a name; a whole project has several links, so it takes no --id.
    let other = made(dir.path(), "other", &["x.lua"]);
    assert_eq!(
        project(&[
            "link",
            text(&police),
            text(other.parent().unwrap()),
            "--id",
            "x"
        ]),
        ExitCode::from(2)
    );
    assert_eq!(
        project(&[
            "link",
            text(&police),
            text(other.parent().unwrap()),
            "--only",
            "files",
            "--id",
            "x"
        ]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        project(&[
            "link",
            text(&police),
            text(other.parent().unwrap()),
            "--only",
            "files"
        ]),
        ExitCode::FAILURE,
        "a project's Files content is linked once"
    );
}

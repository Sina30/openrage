//! `openrage-cli project` adopts a folder that exists and lists what it could not place, links the
//! folder of a pack of another project, and makes a project from a template another one made.

use std::path::Path;
use std::process::ExitCode;

use or_project::{ModuleKind, Project};
use or_wardrobe::{layout, Component, Ped, Skin, Slot, Wardrobe};
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

fn json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A resource of the pack `harbor`, laid out as the engine lays one out, with files that only have names.
fn resource(folder: &Path) {
    let slot = Slot::Component(Component::Jbib);
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", slot);
    let model = layout::model_name(Ped::Male, "mp_m_harbor", slot, 0, Skin::Universal);
    let texture = layout::texture_name(Ped::Male, "mp_m_harbor", slot, 0, 0, "uni");
    write(&folder.join(&directory).join(model), b"model");
    write(&folder.join(&directory).join(texture), b"texture");
    write(&folder.join("fxmanifest.lua"), b"fx_version 'cerulean'\n");
    write(&folder.join("client/main.lua"), b"print('hi')");
    write(&folder.join("stream/harbor/odd.ydd"), b"odd");
}

#[test]
fn a_folder_that_exists_is_adopted_with_its_clothing_as_content_and_what_was_not_placed_is_recorded(
) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("harbor_resource");
    resource(&folder);

    assert_eq!(project(&["adopt", text(&folder)]), ExitCode::SUCCESS);
    let file = folder.join("harbor_resource.openrage-proj");
    assert!(file.is_file());
    assert!(
        folder.join("wardrobe.json").is_file(),
        "the pack is added beside the files"
    );
    assert_eq!(
        project(&["adopt", text(&folder)]),
        ExitCode::FAILURE,
        "a folder that holds a project is not adopted twice"
    );

    let loaded = Project::load(&file).unwrap();
    let kinds: Vec<&ModuleKind> = loaded.container.modules.iter().map(|m| &m.kind).collect();
    assert_eq!(kinds, [&ModuleKind::Clothing, &ModuleKind::Files]);
    assert_eq!(loaded.container.outputs[0].take, ["clothing", "files"]);
    let held: Vec<String> = loaded
        .members
        .iter()
        .map(|m| m.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        held,
        ["main.lua", "odd.ydd"],
        "what the pack does not claim stays Files"
    );
    let adopted = &json(&file)["adopted"];
    let listed: Vec<&str> = adopted["unplaced"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|u| u["path"].as_str())
        .collect();
    assert!(
        listed.contains(&"stream/harbor/odd.ydd") && listed.contains(&"fxmanifest.lua"),
        "{listed:?}"
    );

    // The adopted project plans, and its files are the pack's and the folder's.
    assert_eq!(project(&["plan", text(&file)]), ExitCode::SUCCESS);
}

#[test]
fn a_folder_nothing_recognises_is_adopted_as_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("scripts");
    write(&folder.join("client/main.lua"), b"x");
    write(&folder.join("stream/prop.ydr"), b"x");

    assert_eq!(
        project(&["adopt", text(&folder), "--name", "scripts_pack"]),
        ExitCode::SUCCESS
    );
    let loaded = Project::load(&folder.join("scripts_pack.openrage-proj")).unwrap();
    assert_eq!(loaded.container.modules.len(), 1);
    assert_eq!(loaded.container.modules[0].kind, ModuleKind::Files);
    assert_eq!(loaded.members.len(), 2);
    assert!(json(&folder.join("scripts_pack.openrage-proj"))
        .get("adopted")
        .is_none());
    assert!(!folder.join("wardrobe.json").exists());
}

#[test]
fn the_folder_of_a_pack_is_linked_read_and_unlinked_without_a_file_of_the_source_changing() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("brand").join("modules").join("uniforms");
    let mut wardrobe = Wardrobe::new("brand");
    wardrobe.garments.clear();
    write(&pack.join("placeholder.txt"), b"x");
    wardrobe.write(&pack).unwrap();
    write(&dir.path().join("brand").join("brand.openrage-proj"), b"{}");

    let folder = dir.path().join("police");
    write(&folder.join("client.lua"), b"x");
    assert_eq!(
        project(&["create", text(&folder), text(&folder.join("client.lua"))]),
        ExitCode::SUCCESS
    );
    let file = folder.join("police.openrage-proj");
    let source_before = std::fs::read(pack.join("wardrobe.json")).unwrap();
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();

    assert_eq!(
        project(&["link", text(&file), text(&pack), "--id", "brand"]),
        ExitCode::SUCCESS
    );
    assert_eq!(project(&["links", text(&file)]), ExitCode::SUCCESS);
    let loaded = Project::load(&file).unwrap();
    let linked = loaded.container.module("brand").expect("linked");
    assert!(linked.is_link());
    assert_eq!(linked.path(), Some("../brand/modules/uniforms"));
    assert_eq!(loaded.container.outputs[0].take, ["files", "brand"]);

    assert_eq!(project(&["plan", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        project(&["link", text(&file), text(&pack)]),
        ExitCode::FAILURE,
        "the same folder twice"
    );
    assert_eq!(
        project(&["link", text(&file), text(&folder)]),
        ExitCode::FAILURE,
        "a project's own folder is no link"
    );
    assert_eq!(
        project(&["link", text(&file), text(&dir.path().join("empty"))]),
        ExitCode::FAILURE,
        "a folder that holds no content this build knows is no link"
    );

    assert_eq!(
        project(&["unlink", text(&file), "brand"]),
        ExitCode::SUCCESS
    );
    assert!(Project::load(&file)
        .unwrap()
        .container
        .module("brand")
        .is_none());
    assert_eq!(
        project(&["unlink", text(&file), "files"]),
        ExitCode::FAILURE
    );
    assert_eq!(
        std::fs::read(pack.join("wardrobe.json")).unwrap(),
        source_before
    );
    assert!(pack.join("placeholder.txt").is_file());
}

#[test]
fn a_template_a_project_made_creates_the_same_project_in_another_folder() {
    let dir = tempfile::tempdir().unwrap();
    let made = dir.path().join("police");
    write(&made.join("client.lua"), b"x");
    assert_eq!(
        project(&[
            "create",
            text(&made),
            text(&made.join("client.lua")),
            "--target",
            "enhanced"
        ]),
        ExitCode::SUCCESS
    );
    let file = made.join("police.openrage-proj");
    or_project::project::edit_container(&file, |container| {
        container
            .modules
            .push(or_project::Module::clothing("uniforms", "modules/uniforms"));
        container.outputs[0].take.push("uniforms".into());
        Ok(())
    })
    .unwrap();
    let template = dir.path().join("police.openrage-template");
    assert_eq!(
        project(&[
            "template",
            text(&file),
            text(&template),
            "--description",
            "Police uniforms"
        ]),
        ExitCode::SUCCESS
    );
    assert!(template.is_file());

    let other = dir.path().join("harbor");
    assert_eq!(
        project(&[
            "create",
            text(&other),
            "--name",
            "harbor",
            "--template",
            text(&template)
        ]),
        ExitCode::SUCCESS
    );
    let loaded = Project::load(&other.join("harbor.openrage-proj")).unwrap();
    let ids: Vec<&str> = loaded
        .container
        .modules
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, ["files", "uniforms"]);
    assert_eq!(loaded.container.outputs[0].name, "harbor");
    assert_eq!(
        loaded.container.outputs[0].target,
        Some(or_project::Target::Enhanced),
        "what the build was set to travels"
    );
    assert!(loaded.members.is_empty(), "a template carries no file");
    let pack =
        Wardrobe::read(&other.join("modules").join("uniforms")).expect("an empty pack is made");
    assert_eq!(pack.pack, "harbor");
    assert_eq!(
        json(&other.join("harbor.openrage-proj"))["template"],
        "police"
    );

    assert_eq!(
        project(&[
            "create",
            text(&dir.path().join("x")),
            "--template",
            text(&template),
            "--kind",
            "dlc"
        ]),
        ExitCode::from(2),
        "a template states its builds"
    );
}

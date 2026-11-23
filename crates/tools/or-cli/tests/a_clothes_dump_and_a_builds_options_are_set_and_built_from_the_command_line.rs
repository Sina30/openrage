//! `openrage-cli wardrobe dump` writes the clothes dump of a pack, `project set-build` states the words and the clothing
//! options of a build, and `wardrobe build` writes a resource, a pack or a package as those options say and reads it back.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::{Container, Module, Output, OutputKind, Project};
use or_wardrobe::{Garment, Ped, Slot, Variation, Wardrobe};
use serde_json::{json, Value};

fn run(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

fn put(folder: &Path, rel: &str, bytes: &[u8]) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

/// A project in `dir/pack` holding a clothing pack of a jacket with two textures and a hat, with one build.
fn project(dir: &Path) -> PathBuf {
    let folder = dir.join("pack");
    put(
        &folder,
        "models/jacket.ydd",
        &or_source::sample::drawables_of("jacket"),
    );
    put(
        &folder,
        "models/hat.ydd",
        &or_source::sample::drawables_of("hat"),
    );
    for name in ["a", "b", "hat"] {
        put(
            &folder,
            &format!("textures/{name}.ytd"),
            &or_source::sample::textures_of(name),
        );
    }
    let garment = |slot: &str, model: &str, textures: &[&str]| {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(slot.parse::<Slot>().unwrap());
        g.position = Some(0);
        g.model = Some(model.to_string());
        g.variations = textures
            .iter()
            .map(|t| Variation {
                texture: Some(t.to_string()),
                ..Variation::default()
            })
            .collect();
        g
    };
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        "jbib",
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    ));
    w.garments
        .push(garment("p_head", "models/hat.ydd", &["textures/hat.ytd"]));
    w.write(&folder).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::of(
            OutputKind::FivemResource,
            "fivem",
            "demo",
            &["wardrobe"],
        )],
    };
    let file = folder.join("demo.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    file
}

fn json_of(file: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

#[test]
fn the_clothes_dump_is_written_as_json_and_as_a_lua_table_into_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = project(dir.path());
    let out = dir.path().join("dump");
    assert_eq!(
        run("wardrobe", &["dump", text(&file), "--out", text(&out)]),
        ExitCode::SUCCESS
    );
    let parsed: Value =
        serde_json::from_slice(&std::fs::read(out.join("clothes_dump.json")).unwrap()).unwrap();
    assert_eq!(parsed[0]["PedName"], "mp_m_freemode_01");
    assert_eq!(
        parsed[0]["ComponentVariations"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(parsed[0]["Props"].as_array().map(Vec::len), Some(1));
    let lua = std::fs::read_to_string(out.join("clothes_dump.lua")).unwrap();
    assert!(lua.contains("return {") && lua.contains("DlcCollectionName = \"mp_m_demo\""));
    // Printed, it is the same text; a content that is not there is a failure.
    assert_eq!(
        run("wardrobe", &["dump", text(&file), "--lua"]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        run("wardrobe", &["dump", text(&file), "--module", "ghost"]),
        ExitCode::FAILURE
    );
}

#[test]
fn the_words_and_the_clothing_options_of_a_build_are_set_kept_and_refused_when_they_are_not_options(
) {
    let dir = tempfile::tempdir().unwrap();
    let file = project(dir.path());
    assert_eq!(
        run(
            "project",
            &[
                "set-build",
                text(&file),
                "fivem",
                "--author",
                "Ann",
                "--version",
                "1.2.0",
                "--description",
                "Uniforms",
                "--clothing",
                "creature-metadata=off,collection-prefix=srv_,clothes-dump=both",
            ]
        ),
        ExitCode::SUCCESS
    );
    let build = json_of(&file)["outputs"][0].clone();
    assert_eq!(
        (&build["author"], &build["version"], &build["description"]),
        (&json!("Ann"), &json!("1.2.0"), &json!("Uniforms"))
    );
    assert_eq!(
        build["clothing"],
        json!({ "creature_metadata": false, "collection_prefix": "srv_", "clothes_dump": "both" })
    );

    // A word is taken away with none, and an option that goes back to its default is no key.
    assert_eq!(
        run(
            "project",
            &[
                "set-build",
                text(&file),
                "fivem",
                "--author",
                "none",
                "--clothing",
                "creature-metadata=on,collection-prefix=none,clothes-dump=off",
            ]
        ),
        ExitCode::SUCCESS
    );
    let build = json_of(&file)["outputs"][0].clone();
    assert!(
        build.get("author").is_none() && build.get("clothing").is_none(),
        "{build}"
    );

    let before = std::fs::read(&file).unwrap();
    for (what, option) in [
        ("an option nobody has", "nonsense=on"),
        ("a switch that is neither", "alternates=maybe"),
        ("a prefix with a space in it", "collection-prefix=a b"),
        ("a pair with no value", "alternates"),
    ] {
        assert_eq!(
            run(
                "project",
                &["set-build", text(&file), "fivem", "--clothing", option]
            ),
            ExitCode::from(2),
            "{what}"
        );
        assert_eq!(std::fs::read(&file).unwrap(), before, "{what} wrote");
    }
}

#[test]
fn a_build_with_options_is_written_as_they_say_and_a_pack_is_built_and_read_back_through_its_archives(
) {
    let dir = tempfile::tempdir().unwrap();
    let file = project(dir.path());
    assert_eq!(
        run(
            "project",
            &[
                "set-build",
                text(&file),
                "fivem",
                "--author",
                "Ann",
                "--clothing",
                "subfolders=off,clothes-dump=both",
            ]
        ),
        ExitCode::SUCCESS
    );
    let out = dir.path().join("out");
    assert_eq!(
        run(
            "wardrobe",
            &[
                "build",
                text(&file),
                "--build",
                "fivem",
                "--out",
                text(&out)
            ]
        ),
        ExitCode::SUCCESS
    );
    let live = out.join("demo");
    assert!(live.join("data/clothes_dump.json").is_file());
    assert!(live.join("data/clothes_dump.lua").is_file());
    let manifest = std::fs::read_to_string(live.join("fxmanifest.lua")).unwrap();
    assert!(manifest.contains("author 'Ann'"), "{manifest}");
    assert!(
        live.join(
            "stream/demo/mp_m_freemode_01_mp_m_demo/mp_m_freemode_01_mp_m_demo^jbib_000_u.ydd"
        )
        .is_file(),
        "no folder for the slot"
    );

    // The same pack as a singleplayer DLC: one file in the folder, checked through its archives.
    assert_eq!(
        run(
            "project",
            &[
                "set-build",
                text(&file),
                "fivem",
                "--kind",
                "dlc",
                "--name",
                "demo_dlc"
            ]
        ),
        ExitCode::SUCCESS
    );
    let out = dir.path().join("out_dlc");
    assert_eq!(
        run(
            "wardrobe",
            &[
                "build",
                text(&file),
                "--build",
                "fivem",
                "--out",
                text(&out)
            ]
        ),
        ExitCode::SUCCESS
    );
    assert!(out.join("demo_dlc/dlc.rpf").is_file());
}

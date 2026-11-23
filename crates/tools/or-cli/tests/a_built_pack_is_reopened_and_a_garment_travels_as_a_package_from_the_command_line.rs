//! `openrage-cli wardrobe reopen`, `package` and `add --package` do what the library does: a built singleplayer DLC comes back as a
//! project of sources, a garment leaves one pack as a package and enters another, and each says no to what it cannot take.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{build_all_with, Container, Cook, Module, Output, OutputKind, Project};
use or_wardrobe::Wardrobe;

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A project whose one clothing content holds the two jackets of a folder dropped on it, and which builds a singleplayer DLC.
fn project(dir: &Path, name: &str) -> PathBuf {
    let pack = dir.join("pack");
    or_wardrobe::edit::create(&pack, name).unwrap();
    let drop = dir.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let c = format!("mp_m_freemode_01_mp_m_{name}");
    for n in 0..2 {
        let tag = format!("jacket{n}");
        std::fs::write(
            drop.join(format!("{c}^jbib_00{n}_u.ydd")),
            or_source::sample::drawables_of(&tag),
        )
        .unwrap();
        std::fs::write(
            drop.join(format!("{c}^jbib_diff_00{n}_a_uni.ytd")),
            or_source::sample::textures_of(&tag),
        )
        .unwrap();
    }
    or_wardrobe::edit::add_clothes(&pack, &[drop]).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "pack")],
        outputs: vec![Output::of(
            OutputKind::SingleplayerDlc,
            "dlc",
            name,
            &["wardrobe"],
        )],
    };
    let path = dir.join(format!("{name}.openrage-proj"));
    Project::create(&path, &container, &[]).unwrap();
    path
}

#[test]
fn a_built_dlc_is_reopened_and_one_of_its_garments_is_packaged_and_added_to_another_pack() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path(), "demo");
    let loaded = Project::load(&path).unwrap();
    let plans = or_wardrobe::plans(&loaded, "dlc").unwrap();
    let out = tempfile::tempdir().unwrap();
    let cook = |source: &Path, scratch: &Path| or_source::cook::cook_source(source, scratch);
    let (built, stopped) = build_all_with(
        &plans,
        out.path(),
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );
    assert!(stopped.is_none(), "{stopped:?}");

    // The DLC comes back as a project of sources.
    let reopened = dir.path().join("reopened");
    assert_eq!(
        wardrobe(&["reopen", text(&built[0].folder), "--out", text(&reopened)]),
        ExitCode::SUCCESS
    );
    let again = reopened.join("demo.openrage-proj");
    assert!(again.is_file());
    let pack = Wardrobe::read(&reopened.join("clothing")).unwrap();
    assert_eq!(pack.garments.len(), 2);
    assert!(pack.garments.iter().all(|g| g
        .model
        .as_deref()
        .is_some_and(|m| m.ends_with(".drawables.src"))));

    // Into a folder that holds something, and from something that holds no pack, it says no.
    assert_eq!(
        wardrobe(&["reopen", text(&built[0].folder), "--out", text(&reopened)]),
        ExitCode::FAILURE
    );
    assert_eq!(
        wardrobe(&[
            "reopen",
            text(dir.path()),
            "--out",
            text(&dir.path().join("nothing"))
        ]),
        ExitCode::FAILURE
    );
    assert_eq!(
        wardrobe(&["reopen", text(&built[0].folder)]),
        ExitCode::from(2)
    );

    // A garment of the reopened pack is one package, and another pack takes it as an equal garment.
    let id = pack.garments[0].id.clone();
    let package = dir.path().join("jacket.garment");
    assert_eq!(
        wardrobe(&[
            "package",
            text(&again),
            "--garment",
            &id,
            "--out",
            text(&package)
        ]),
        ExitCode::SUCCESS
    );
    assert!(package.join(or_wardrobe::package::FILE).is_file());
    assert_eq!(
        wardrobe(&[
            "package",
            text(&again),
            "--garment",
            "nothing",
            "--out",
            text(&dir.path().join("x"))
        ]),
        ExitCode::FAILURE
    );

    let other = tempfile::tempdir().unwrap();
    let target = project(other.path(), "other");
    let target_pack = other.path().join("pack");
    let before = Wardrobe::read(&target_pack).unwrap().garments.len();
    assert_eq!(
        wardrobe(&["add", text(&target), "--package", text(&package)]),
        ExitCode::SUCCESS
    );
    let after = Wardrobe::read(&target_pack).unwrap();
    assert_eq!(after.garments.len(), before + 1);
    let added = after.garments.last().unwrap();
    assert_eq!(
        (added.ped, added.slot),
        (pack.garments[0].ped, pack.garments[0].slot),
        "the garment arrives with what it states"
    );
    // Nothing to add is a refusal, not a silent success.
    assert_eq!(wardrobe(&["add", text(&target)]), ExitCode::FAILURE);
}

#[test]
fn the_plan_of_a_pack_is_written_to_a_file_as_the_document_the_reference_describes() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path(), "demo");
    let file = dir.path().join("plan.json");

    assert_eq!(
        wardrobe(&["plan", text(&path), "--out", text(&file)]),
        ExitCode::SUCCESS
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let [plan] = written.as_array().unwrap().as_slice() else {
        panic!("one document for a build that does not divide: {written}");
    };
    assert_eq!(plan["format"], "openrage-wardrobe-plan");
    assert_eq!(plan["schema"], 1);
    assert_eq!(
        (plan["resource"].as_str(), plan["kind"].as_str()),
        (Some("demo"), Some("singleplayer-dlc"))
    );
    let loaded = Project::load(&path).unwrap();
    let wanted: Vec<String> = or_wardrobe::plans(&loaded, "dlc").unwrap()[0]
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let said: Vec<&str> = plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        said, wanted,
        "the files of the build, in the order of their paths"
    );
    assert_eq!(plan["totals"]["files"], said.len());
}

#[test]
fn a_drop_that_is_not_there_or_holds_nothing_is_refused_and_the_pack_is_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path(), "demo");
    let pack = dir.path().join("pack");
    let before = std::fs::read(pack.join("wardrobe.json")).unwrap();

    let nowhere = dir.path().join("nowhere").join("x.ydd");
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let held = dir.path().join("drop");
    for from in [vec![&nowhere], vec![&empty], vec![&held, &nowhere]] {
        let mut args = vec!["add", text(&path)];
        args.extend(from.iter().map(|f| text(f)));
        assert_eq!(wardrobe(&args), ExitCode::FAILURE, "{args:?}");
    }
    assert_eq!(
        std::fs::read(pack.join("wardrobe.json")).unwrap(),
        before,
        "a refusal changes nothing"
    );

    // What the pack holds already is said and is not a refusal: there the drop was read.
    assert_eq!(
        wardrobe(&["add", text(&path), text(&held)]),
        ExitCode::SUCCESS
    );
}

//! A module of weapons goes through the one plan and the one build: its metadata sources cook to the data folder, are registered
//! by that path, and reload through the weapon family; a pack that breaks a rule is refused before anything is written; and the
//! cells of the Builds window are derived from what the content hands over.
//!
//! ⚠ The sources are made here, from documents of this file's own with the install's names: the clean checkout runs this, and the
//! install-gated suites hold the same to the files of a mounted game.

use std::path::{Path, PathBuf};

use or_project::{build_with, Container, Module, Origin, Output, OutputKind, Plan};
use or_source::models::Extraction;
use or_weapons::edit::edit_weapon;
use or_weapons::model::{Weapon, Weapons};
use or_weapons::rules::rule_of;
use or_weapons::verify::verify;
use or_weapons::{contributions, inspect, Depth};

const WEAPONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTPISTOL</Name>
          <Model>w_pi_testpistol</Model>
          <Damage value="26.0" />
          <ClipSize value="12" />
          <AimingInfo ref="PISTOL_AIM" />
          <AttachPoints>
            <Item>
              <AttachBone>WAPClip</AttachBone>
              <Components>
                <Item><Name>COMPONENT_TESTPISTOL_CLIP_01</Name><Default value="true" /></Item>
              </Components>
            </Item>
          </AttachPoints>
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>TEST</Name>
</CWeaponInfoBlob>"#;

const COMPONENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponComponentInfoBlob>
  <Data />
  <Infos>
    <Item type="CWeaponComponentClipInfo">
      <Name>COMPONENT_TESTPISTOL_CLIP_01</Name>
      <Model>w_pi_testpistol_mag1</Model>
      <AttachBone>WAPClip</AttachBone>
      <ClipSize value="12" />
    </Item>
  </Infos>
  <InfoBlobName>TEST</InfoBlobName>
</CWeaponComponentInfoBlob>"#;

/// The module folder `dir/weapons` holding the sources `weapons.metadata.src` and `components.metadata.src` and a model naming the one weapon.
fn module(dir: &Path) -> PathBuf {
    let folder = dir.join("weapons");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, text) in [("weapons", WEAPONS), ("components", COMPONENTS)] {
        or_source::metadata::extract(
            text.as_bytes(),
            name,
            "meta",
            &folder,
            &Extraction {
                from_game: false,
                virtual_path: None,
                textures: false,
                skeleton: None,
            },
        )
        .unwrap_or_else(|p| panic!("{name}: {p:?}"));
    }
    let mut model = Weapons::new();
    let mut weapon = Weapon::new("weapon_testpistol");
    weapon.sources = vec![
        "weapons.metadata.src".to_string(),
        "components.metadata.src".to_string(),
    ];
    model.weapons.push(weapon);
    model.write(&folder).unwrap();
    folder
}

fn container(kind: OutputKind) -> Container {
    let mut output = Output::fivem_resource("fivem", "arsenal", &["weapons"]);
    output.kind = kind;
    Container {
        modules: vec![Module::weapons("weapons", "weapons")],
        outputs: vec![output],
    }
}

fn plan(container: &Container, folder: &Path) -> Plan {
    let contributions = or_weapons::contributions(container, folder);
    or_project::plan_with(container, "fivem", folder, &contributions).expect("the output exists")
}

fn cooker(source: &Path, scratch: &Path) -> Result<PathBuf, Vec<String>> {
    or_source::cook::cook_source(source, scratch)
}

#[test]
fn a_pack_of_metadata_sources_plans_to_the_data_folder_builds_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = module(dir.path());
    let container = container(OutputKind::FivemResource);
    let planned = plan(&container, dir.path());
    assert_eq!(planned.problems, Vec::<String>::new());

    let paths: Vec<&str> = planned.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "data/components.meta",
            "data/weapons.meta",
            "fxmanifest.lua"
        ],
        "each data file at the folder the content states, and the manifest, by path"
    );
    assert!(planned
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/"))
        .all(|f| matches!(f.origin, Origin::Cook { .. })));
    let manifest = match &planned
        .files
        .iter()
        .find(|f| f.path == "fxmanifest.lua")
        .expect("a manifest")
        .origin
    {
        Origin::Document(text) => text.clone(),
        other => panic!("the manifest is a document: {other:?}"),
    };
    for line in [
        "data_file 'WEAPONINFO_FILE' 'data/weapons.meta'",
        "data_file 'WEAPONCOMPONENTSINFO_FILE' 'data/components.meta'",
    ] {
        assert!(manifest.contains(line), "{line} is missing from {manifest}");
    }
    assert_eq!(planned.items.len(), 1);
    assert_eq!(planned.items[0].name, "weapon_testpistol");

    let root = dir.path().join("build");
    let built = build_with(&planned, &root, &Default::default(), Some(&cooker))
        .expect("the build cooks, writes and reads back");
    assert_eq!(built.verification.problems, Vec::<String>::new());
    let readers: Vec<String> = built
        .verification
        .reloaded
        .iter()
        .map(|r| format!("{} {}", r.path, r.reader))
        .collect();
    for wanted in [
        "data/weapons.meta CWeaponInfoBlob (XML)",
        "data/components.meta CWeaponComponentInfoBlob (XML)",
    ] {
        assert!(
            readers.iter().any(|r| r == wanted),
            "`{wanted}` was reopened through the family: {readers:?}"
        );
    }

    let model = Weapons::read(&folder).unwrap();
    let facts = inspect(&folder, &model, Depth::Open);
    let found = verify(&model, &facts, &built.folder);
    assert!(found.is_clean(), "{:?}", found.problems());
    assert!(found.checks.iter().any(|c| c.name == "weapons defined"));
}

#[test]
fn an_edit_through_the_family_changes_one_value_of_the_built_data_and_a_second_build_is_identical()
{
    let dir = tempfile::tempdir().unwrap();
    let folder = module(dir.path());
    let container = container(OutputKind::FivemResource);
    let root = dir.path().join("build");
    let first = build_with(
        &plan(&container, dir.path()),
        &root,
        &Default::default(),
        Some(&cooker),
    )
    .expect("the first build");
    let before = std::fs::read(first.folder.join("data/weapons.meta")).unwrap();

    let edited = edit_weapon(
        &folder.join("weapons.metadata.src"),
        "WEAPON_TESTPISTOL",
        |w| w.damage = 41.0,
    )
    .unwrap();
    assert!(edited.changed());
    let second = build_with(
        &plan(&container, dir.path()),
        &root,
        &Default::default(),
        Some(&cooker),
    )
    .expect("the second build");
    let after = std::fs::read(second.folder.join("data/weapons.meta")).unwrap();
    assert_ne!(before, after);
    let text = String::from_utf8(after).unwrap();
    assert!(text.contains("41"), "{text}");
    assert!(
        text.contains(r#"ref="PISTOL_AIM""#),
        "the reference arrived in the build: {text}"
    );

    // Undo is the text before, and the build after it is the build before it.
    or_weapons::edit::restore(&folder.join("weapons.metadata.src"), &edited.before).unwrap();
    let third = build_with(
        &plan(&container, dir.path()),
        &root,
        &Default::default(),
        Some(&cooker),
    )
    .expect("the third build");
    let again = std::fs::read(third.folder.join("data/weapons.meta")).unwrap();
    assert_eq!(again, before);
}

#[test]
fn a_pack_that_breaks_a_rule_is_refused_by_the_plan_and_names_the_weapon_and_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let folder = module(dir.path());
    let mut model = Weapons::read(&folder).unwrap();
    model.weapons[0]
        .sources
        .push("gone.drawable.src".to_string());
    let mut twin = Weapon::new("WEAPON_TESTPISTOL");
    twin.sources = vec!["weapons.metadata.src".to_string()];
    model.weapons.push(twin);
    model.write(&folder).unwrap();

    let planned = plan(&container(OutputKind::FivemResource), dir.path());
    let said = planned.problems.join("\n");
    assert!(
        said.contains("source-missing") && said.contains("gone.drawable.src"),
        "{said}"
    );
    assert!(said.contains("weapon-twice"), "{said}");
    assert!(!rule_of("weapon-twice").unwrap().waivable);
}

#[test]
fn a_singleplayer_dlc_cell_is_derived_from_what_the_content_hands_over() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path());
    let container = container(OutputKind::FivemResource);
    let handed = contributions(&container, dir.path());
    assert_eq!(handed.len(), 1);
    // Every data file states the type it registers as, and a model streams from `stream/`, which the game reads no type for.
    assert_eq!(
        or_project::cannot_ship(&handed[0], &OutputKind::FivemResource),
        None
    );
    assert_eq!(
        or_project::cannot_ship(&handed[0], &OutputKind::SingleplayerDlc),
        None,
        "a pack of data files that all state their type has nothing a DLC cannot register"
    );
    let mut with_a_model = handed[0].clone();
    with_a_model.files.push(or_project::PlannedFile {
        path: "stream/w_pi_testpistol.ydr".to_string(),
        origin: Origin::Document(String::new()),
    });
    let said = or_project::cannot_ship(&with_a_model, &OutputKind::SingleplayerDlc)
        .expect("a model with no type locks the cell");
    assert!(said.contains("stream/w_pi_testpistol.ydr"), "{said}");
}

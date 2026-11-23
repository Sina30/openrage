//! `openrage-cli weapons` validates the weapons of a project and reads a build back, and the one `project` plan and build take the
//! weapons content as they take any other.

use std::path::Path;
use std::process::ExitCode;

fn cli(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A project with a weapons content whose pack holds the weapon `WEAPON_TESTPISTOL`, defined by the metadata `weapons.meta`, and its model.
fn a_project_with_weapons(dir: &Path, defined_as: &str) -> std::path::PathBuf {
    std::fs::write(
        dir.join("arsenal.openrage-proj"),
        r#"{
  "documents": [],
  "game": "both",
  "modules": [{ "id": "weapons", "path": "weapons", "type": "weapons" }],
  "open": [],
  "outputs": [{ "id": "fivem", "kind": "fivem-resource", "name": "arsenal", "take": ["weapons"] }],
  "schema": 4
}"#,
    )
    .unwrap();
    let folder = dir.join("weapons");
    std::fs::create_dir_all(&folder).unwrap();
    let meta = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type=\"CWeaponInfo\">
          <Name>{defined_as}</Name>
          <Model>w_pi_testpistol</Model>
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>TEST</Name>
</CWeaponInfoBlob>"
    );
    or_source::metadata::extract(
        meta.as_bytes(),
        "weapons",
        "meta",
        &folder,
        &or_source::models::Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton: None,
        },
    )
    .expect("a weapon file is a metadata source");
    let mut model = or_weapons::model::Weapons::new();
    let mut weapon = or_weapons::model::Weapon::new("weapon_testpistol");
    weapon.sources = vec!["weapons.metadata.src".to_string()];
    model.weapons.push(weapon);
    model.write(&folder).unwrap();
    dir.join("arsenal.openrage-proj")
}

#[test]
fn a_weapon_pack_is_validated_and_a_word_that_is_no_command_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_weapons(dir.path(), "WEAPON_TESTPISTOL");
    assert_eq!(
        cli("weapons", &["validate", text(&file)]),
        ExitCode::SUCCESS,
        "the metadata defines the weapon the pack names"
    );
    assert_eq!(
        cli("weapons", &["validate", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    assert_ne!(cli("weapons", &["dance"]), ExitCode::SUCCESS);
    assert_eq!(cli("weapons", &[]), ExitCode::from(2));
}

#[test]
fn a_pack_whose_metadata_does_not_define_its_weapon_is_refused_and_so_is_a_project_with_no_weapons()
{
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_weapons(dir.path(), "WEAPON_SOMEONEELSE");
    assert_eq!(
        cli("weapons", &["validate", text(&file)]),
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
        cli("weapons", &["validate", text(&file)]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_build_of_weapons_is_read_back_against_the_weapons_and_a_tampered_one_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_weapons(dir.path(), "WEAPON_TESTPISTOL");
    assert_eq!(
        cli("weapons", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "nothing is built yet, so there is nothing to read"
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("weapons", &["verify", text(&file)]), ExitCode::SUCCESS);

    let built = dir.path().join("build").join("arsenal");
    let shipped = built.join("data").join("weapons.meta");
    assert!(shipped.is_file(), "the data file ships at the data folder");
    let tampered = std::fs::read_to_string(&shipped)
        .unwrap()
        .replace("WEAPON_TESTPISTOL", "WEAPON_SOMEONEELSE");
    std::fs::write(&shipped, tampered).unwrap();
    assert_eq!(
        cli("weapons", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "the data file no longer defines the weapon"
    );
}

#[test]
fn a_build_whose_manifest_does_not_register_its_data_file_fails_the_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = a_project_with_weapons(dir.path(), "WEAPON_TESTPISTOL");
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("weapons", &["verify", text(&file)]), ExitCode::SUCCESS);

    let manifest = dir
        .path()
        .join("build")
        .join("arsenal")
        .join("fxmanifest.lua");
    let written = std::fs::read_to_string(&manifest).unwrap();
    assert!(
        written.contains("data_file"),
        "the build registers the file: {written}"
    );
    let stripped: Vec<&str> = written
        .lines()
        .filter(|line| !line.contains("data_file"))
        .collect();
    std::fs::write(&manifest, stripped.join("\n")).unwrap();
    assert_eq!(
        cli("weapons", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "a build that does not tell the game to read the file cannot load the weapon"
    );
}

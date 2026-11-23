//! Adding weapons to a pack, and making a new weapon, are imports: they run on the import worker and show as that job, and each is one
//! step of History that Undo takes back with the sources it made and the weapons they gave the pack, and that Redo makes again.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, like `the_project_history`.

use std::path::{Path, PathBuf};

use or_weapons::Weapons;

use super::front_door::{NewProject, Template};
use super::the_project_history::{open, until, write};
use super::AppState;

const PISTOL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTPISTOL</Name>
          <Model>w_pi_testpistol</Model>
          <Damage value="26.0" />
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>TEST</Name>
</CWeaponInfoBlob>"#;

const RIFLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTRIFLE</Name>
          <Model>w_ar_testrifle</Model>
          <Damage value="40.0" />
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>TEST2</Name>
</CWeaponInfoBlob>"#;

/// The Weapons project `arsenal` in `dir`, opened in a state, and the folder of its pack.
fn a_weapons_project(dir: &Path) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Weapons, dir.to_path_buf());
    draft.set_name("arsenal".into());
    let file = draft.create().expect("a project").file;
    let mut state = open(&file);
    until(&mut state, "the pack to be read", |s| {
        s.weapons_pack().is_some()
    });
    let folder = state.weapons_pack().expect("the pack").folder.clone();
    (state, folder)
}

fn dropped(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join("drop").join(name);
    write(&path, text.as_bytes());
    path
}

fn names_of(folder: &Path) -> Vec<String> {
    Weapons::read(folder)
        .expect("the pack's model")
        .weapons
        .into_iter()
        .map(|weapon| weapon.name)
        .collect()
}

fn finished(state: &mut AppState) {
    until(state, "the import", |s| !s.workbench.imports.is_running());
}

#[test]
fn adding_a_weapon_file_returns_at_once_and_runs_as_the_import_job() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_weapons_project(dir.path());

    state.add_weapons(vec![dropped(dir.path(), "pistol.meta", PISTOL)]);
    assert!(
        state.workbench.imports.is_running(),
        "the decompile ran inside the frame that added the file"
    );
    assert!(
        state.project_import_job().is_some(),
        "the job list does not show the work"
    );
    assert!(
        !folder.join("pistol.metadata.src").exists(),
        "the source was made before the frame ended"
    );

    finished(&mut state);
    assert!(folder.join("pistol.metadata.src").is_dir());
    assert_eq!(names_of(&folder), ["WEAPON_TESTPISTOL"]);
}

#[test]
fn one_undo_takes_the_added_files_and_their_weapons_out_of_the_pack_and_one_redo_puts_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_weapons_project(dir.path());
    state.add_weapons(vec![dropped(dir.path(), "pistol.meta", PISTOL)]);
    finished(&mut state);
    assert_eq!(names_of(&folder), ["WEAPON_TESTPISTOL"]);

    let said = state
        .undo_project()
        .expect("the add is a step to take back");
    assert!(said.starts_with("Added"), "{said}");
    assert!(
        !folder.join("pistol.metadata.src").exists(),
        "Undo left the source the add made"
    );
    assert!(
        names_of(&folder).is_empty(),
        "Undo left a weapon whose source is gone: {:?}",
        names_of(&folder)
    );

    state.redo_project().expect("and make it again");
    assert!(folder.join("pistol.metadata.src").is_dir());
    assert_eq!(
        names_of(&folder),
        ["WEAPON_TESTPISTOL"],
        "Redo put the source back and the weapon its metadata defines with it"
    );
}

#[test]
fn a_new_weapon_is_an_import_step_that_one_undo_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_weapons_project(dir.path());

    state.new_weapon();
    assert!(
        state.workbench.imports.is_running(),
        "the new weapon was made inside the frame"
    );
    finished(&mut state);
    assert_eq!(names_of(&folder), ["WEAPON_NEW"]);
    assert!(folder.join("weapon_new.metadata.src").is_dir());

    let said = state.undo_project().expect("the new weapon is a step");
    assert!(said.contains("WEAPON_NEW"), "{said}");
    assert!(!folder.join("weapon_new.metadata.src").exists());
    assert!(names_of(&folder).is_empty());

    state.redo_project().expect("and make it again");
    assert_eq!(names_of(&folder), ["WEAPON_NEW"]);
}

#[test]
fn adding_weapons_keeps_the_history_of_the_weapons_and_a_change_taken_back_after_it_loses_none() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_weapons_project(dir.path());
    state.add_weapons(vec![dropped(dir.path(), "pistol.meta", PISTOL)]);
    finished(&mut state);
    until(&mut state, "the weapon to be listed", |s| {
        (s.weapons_pack().and_then(|p| p.read.as_ref().ok()))
            .is_some_and(|r| !r.model.weapons.is_empty())
    });
    let (module, id) = {
        let pack = state.weapons_pack().expect("the pack");
        let weapon = &pack.read.as_ref().expect("read").model.weapons[0];
        (pack.module.clone(), weapon.id.clone())
    };
    state.select_weapon(&module, Some(id));
    until(&mut state, "the weapon to be read", |s| {
        s.selected_weapon().is_some()
    });
    state.rename_selected_weapon("WEAPON_RENAMED");
    assert_eq!(names_of(&folder), ["WEAPON_RENAMED"]);
    assert_eq!(state.workbench.weapons.undo.len(), 1);

    state.add_weapons(vec![dropped(dir.path(), "rifle.meta", RIFLE)]);
    finished(&mut state);
    assert_eq!(
        state.workbench.weapons.undo.len(),
        1,
        "adding weapons emptied the history of the weapons"
    );

    state.undo_weapons_change();
    let mut held = names_of(&folder);
    held.sort();
    assert_eq!(
        held,
        ["WEAPON_TESTPISTOL", "WEAPON_TESTRIFLE"],
        "taking back the rename put an old copy of the pack's file over the weapons added since"
    );
}

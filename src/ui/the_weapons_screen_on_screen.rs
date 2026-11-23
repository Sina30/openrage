//! The Weapons screen as it is drawn: a weapon pack opens on it, it shows in Content, Open in and the start page through the table of task
//! screens alone, its empty state offers the one action, and a weapon edit is one step of Undo that round-trips.
//!
//! Read through AccessKit from whole frames of the shell, with the weapon files this file writes (a document of the shape the game's weapon
//! metadata has, with names of its own).

use std::path::{Path, PathBuf};

use super::frames_of_the_shell::{key, Shell};
use super::the_front_door_on_screen::{
    a_first_launch, a_project, click_on, names, shows, waits_for,
};
use super::the_project_screens_on_screen::{centre_of, until};
use crate::state::front_door::cards::read_card;
use crate::state::front_door::Template;
use crate::ui::dock::Workspace;

const WEAPONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTPISTOL</Name>
          <Model>w_pi_testpistol</Model>
          <Slot>SLOT_PISTOL</Slot>
          <Group>GROUP_PISTOL</Group>
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
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTRIFLE</Name>
          <Model>w_ar_testrifle</Model>
          <Group>GROUP_RIFLE</Group>
          <Damage value="40.0" />
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>TEST</Name>
</CWeaponInfoBlob>"#;

pub(super) fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// A shell on a weapon pack `arsenal`, which holds the two weapons of `WEAPONS` once the scan has read them.
pub(super) fn a_weapons_shell(dir: &Path) -> Shell {
    let file = a_project(dir, "arsenal", Template::Weapons);
    let mut shell = Shell::new(crate::state::ThemeChoice::Dark);
    shell.state.load_project_from(file);
    let dropped = write(&dir.join("drop").join("weapons.meta"), WEAPONS.as_bytes());
    shell.state.add_weapons(vec![dropped]);
    shell.state.active_workspace = Workspace::Weapons;
    shell.settled();
    until(&mut shell, "the weapons", |s| {
        s.state
            .weapons_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.model.weapons.len() == 2)
    });
    shell
}

#[test]
fn a_weapon_pack_opens_on_the_weapons_screen_and_shows_in_content_the_strip_and_the_start_page_through_the_table(
) {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let file = a_project(dir.path(), "arsenal", Template::Weapons);
    let card = read_card(&file);
    assert_eq!(
        card.content,
        ["Weapons · 0 weapons"],
        "the card counts in the row's word"
    );

    let mut shell = Shell::new(crate::state::ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    assert_eq!(shell.state.active_workspace, Workspace::Weapons);
    assert!(shell
        .state
        .workspaces_offered()
        .contains(&Workspace::Weapons));
    // ⚠ On the overview: Content counts the pack, and the Project panel is not repeated on a content's own screen.
    shell.state.active_workspace = Workspace::Project;
    until(&mut shell, "the pack to be counted", |s| {
        shows(s, "0 weapons")
    });
    assert!(
        shows(&shell, "Weapons  ·  0 weapons") || shows(&shell, "Weapons · 0 weapons"),
        "the Content panel counts the pack: {:?}",
        names(&shell)
    );
}

#[test]
fn the_empty_state_of_a_project_without_weapons_offers_the_one_action_and_it_gives_the_project_a_pack(
) {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let file = a_project(dir.path(), "gadgets", Template::Files);
    let mut shell = a_first_launch();
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Weapons;
    shell.settled();
    assert!(!shell.state.has_weapons());
    assert!(
        shows(&shell, "No weapons in this project"),
        "{:?}",
        names(&shell)
    );
    let add = "Add a weapon pack to this project";
    let at = super::the_project_screens_on_screen::centre_of(&shell, add);
    shell.click(at);
    shell.settled();
    assert!(
        shell.state.has_weapons(),
        "the action gave the project a pack"
    );
    waits_for(&mut shell, "Add weapons…");
}

#[test]
fn the_pack_lists_its_weapons_by_group_and_a_click_selects_one() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    for name in ["WEAPON_TESTPISTOL", "WEAPON_TESTRIFLE"] {
        assert!(shows(&shell, name), "{name}: {:?}", names(&shell));
    }
    assert!(shows(&shell, "GROUP_PISTOL") && shows(&shell, "GROUP_RIFLE"));
    let at = super::the_project_screens_on_screen::centre_of(&shell, "WEAPON_TESTPISTOL");
    shell.click(at);
    shell.settled();
    assert_eq!(
        shell.state.selected_weapon().map(|(_, w)| w.name.clone()),
        Some("WEAPON_TESTPISTOL".to_string())
    );
    assert!(
        shows(&shell, "Weapon field: Damage"),
        "the editor shows the value under the engine's name: {:?}",
        names(&shell)
    );
}

fn select(shell: &mut Shell, name: &str) {
    let (module, id) = {
        let pack = shell.state.weapons_pack().expect("a pack");
        let read = pack.read.as_ref().expect("a pack that reads");
        let weapon = read
            .model
            .weapons
            .iter()
            .find(|w| w.name == name)
            .expect("the weapon");
        (pack.module.clone(), weapon.id.clone())
    };
    shell.state.select_weapon(&module, Some(id));
    shell.settled();
}

/// The text of the weapon file of the pack, as the cook reads it.
fn the_weapon_file(shell: &Shell) -> String {
    let pack = shell.state.weapons_pack().expect("a pack");
    let dir = std::fs::read_dir(&pack.folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.to_string_lossy().ends_with(".metadata.src"))
        .expect("a weapon file source");
    std::fs::read_to_string(dir.join("metadata.xml")).unwrap()
}

#[test]
fn a_weapon_edit_is_one_step_of_undo_and_round_trips() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    select(&mut shell, "WEAPON_TESTPISTOL");
    let before = the_weapon_file(&shell);
    assert!(before.contains(r#"ref="PISTOL_AIM""#), "{before}");

    shell
        .state
        .edit_selected_weapon("Changed WEAPON_TESTPISTOL".to_string(), |w| w.damage = 33.5);
    let after = the_weapon_file(&shell);
    assert_ne!(before, after);
    assert!(after.contains("33.5"), "{after}");
    assert!(
        after.contains(r#"ref="PISTOL_AIM""#),
        "the reference arrived: {after}"
    );
    assert_eq!(
        shell.state.workbench.weapons.undo.len(),
        1,
        "one edit is one step"
    );

    shell.state.undo_weapons_change();
    assert_eq!(the_weapon_file(&shell), before, "Undo puts the file back");
    assert!(shell.state.workbench.weapons.undo.is_empty());
    shell.state.redo_weapons_change();
    assert_eq!(
        the_weapon_file(&shell),
        after,
        "Redo makes the change again"
    );

    // The edit reaches what the screen reads.
    until(&mut shell, "the scan to read the edit", |s| {
        s.state
            .workbench
            .weapons_read()
            .first()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| {
                read.facts
                    .source(&read.model.weapons[0].sources[0])
                    .is_some()
            })
    });
}

#[test]
fn the_list_header_takes_a_weapon_change_back_and_makes_it_again_by_buttons_that_name_it() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    select(&mut shell, "WEAPON_TESTPISTOL");
    let before = the_weapon_file(&shell);
    waits_for(&mut shell, "Undo weapon change");
    shell
        .state
        .edit_selected_weapon("Changed WEAPON_TESTPISTOL".to_string(), |w| w.damage = 33.5);
    shell.settled();
    let changed = the_weapon_file(&shell);
    assert_ne!(before, changed);
    // ⚠ The button names the change it takes back, so a person reads what Undo does before pressing it.
    click_on(&mut shell, "Undo \"Changed WEAPON_TESTPISTOL\"");
    shell.settled();
    assert_eq!(
        the_weapon_file(&shell),
        before,
        "the button took the change back"
    );
    click_on(&mut shell, "Redo \"Changed WEAPON_TESTPISTOL\"");
    shell.settled();
    assert_eq!(the_weapon_file(&shell), changed, "the button made it again");
}

#[test]
fn renaming_a_weapon_changes_its_model_and_its_file_in_one_step_and_a_taken_name_is_refused() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    select(&mut shell, "WEAPON_TESTPISTOL");
    shell.state.rename_selected_weapon("weapon_testrifle");
    assert!(
        the_weapon_file(&shell).contains("WEAPON_TESTPISTOL"),
        "a name held twice is refused"
    );
    shell.state.rename_selected_weapon("WEAPON_RENAMED");
    assert!(the_weapon_file(&shell).contains("<Name>WEAPON_RENAMED</Name>"));
    assert_eq!(shell.state.workbench.weapons.undo.len(), 1);
    shell.state.undo_weapons_change();
    assert!(the_weapon_file(&shell).contains("<Name>WEAPON_TESTPISTOL</Name>"));
}

/// A window tall enough to show every row of a group, so that a value far down the panel can be clicked.
const TALL: (u32, u32) = (1600, 2600);

/// Click the value named `field`, type `text` and press Enter, as a person who sets an exact number does.
fn type_into(shell: &mut Shell, field: &str, text: &str) {
    shell.settled_sized(TALL);
    let at = centre_of(shell, field);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(TALL, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(TALL, vec![button(false)]);
    shell.draw_sized(TALL, vec![egui::Event::Text(text.to_string())]);
    shell.draw_sized(TALL, vec![key(egui::Key::Enter)]);
    shell.settled_sized(TALL);
}

#[test]
fn a_number_typed_into_a_weapon_value_is_written_and_is_one_step_of_undo() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    select(&mut shell, "WEAPON_TESTPISTOL");
    let before = the_weapon_file(&shell);

    type_into(&mut shell, "Weapon field: Damage", "55");
    let after = the_weapon_file(&shell);
    assert_ne!(before, after, "the typed damage reached the file");
    assert!(after.contains(r#"<Damage value="55"#), "{after}");
    assert_eq!(
        shell.state.workbench.weapons.undo.len(),
        1,
        "typing a number is one step"
    );

    // A whole number is typed the same way.
    type_into(&mut shell, "Weapon field: KnockdownCount", "3");
    assert!(
        the_weapon_file(&shell).contains(r#"<KnockdownCount value="3""#),
        "{}",
        the_weapon_file(&shell)
    );
    assert_eq!(shell.state.workbench.weapons.undo.len(), 2);

    shell.state.undo_weapons_change();
    shell.state.undo_weapons_change();
    assert_eq!(the_weapon_file(&shell), before, "Undo takes both back");
}

/// The names of the weapons the model of the pack holds on disk.
fn weapons_on_disk(shell: &Shell) -> Vec<String> {
    let pack = shell.state.weapons_pack().expect("a pack");
    or_weapons::Weapons::read(&pack.folder)
        .expect("the model reads")
        .weapons
        .into_iter()
        .map(|w| w.name)
        .collect()
}

#[test]
fn adding_weapons_keeps_the_history_of_the_weapons_and_no_undo_of_it_drops_them() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    select(&mut shell, "WEAPON_TESTPISTOL");
    shell.state.rename_selected_weapon("WEAPON_RENAMED");
    assert_eq!(shell.state.workbench.weapons.undo.len(), 1);

    let other = WEAPONS
        .replace("WEAPON_TESTPISTOL", "WEAPON_OTHER")
        .replace("WEAPON_TESTRIFLE", "WEAPON_OTHERRIFLE");
    let dropped = write(
        &dir.path().join("drop").join("other.meta"),
        other.as_bytes(),
    );
    let import_ends = |s: &Shell| !s.state.workbench.imports.is_running();
    shell.state.add_weapons(vec![dropped]);
    until(&mut shell, "the weapons to be added", import_ends);
    shell.state.new_weapon();
    until(&mut shell, "the new weapon", import_ends);
    assert_eq!(
        shell.state.workbench.weapons.undo.len(),
        1,
        "adding weapons emptied the history of the weapons"
    );
    for name in ["WEAPON_OTHER", "WEAPON_OTHERRIFLE", "WEAPON_NEW"] {
        assert!(
            weapons_on_disk(&shell).contains(&name.to_string()),
            "{name}"
        );
    }

    shell.state.undo_weapons_change();
    shell.state.redo_weapons_change();
    let held = weapons_on_disk(&shell);
    for name in [
        "WEAPON_RENAMED",
        "WEAPON_OTHER",
        "WEAPON_OTHERRIFLE",
        "WEAPON_NEW",
    ] {
        assert!(
            held.contains(&name.to_string()),
            "{name} is still in the pack after Undo: {held:?}"
        );
    }
}

//! The commands of the Weapons screen: giving a project weapons, adding weapons to it, making one, taking one out, and taking the last change back.

use super::table::{a_project, none_if, Action, Spec, BASE};
use crate::platform::file_dialogs;
use crate::state::AppState;

const NO_WEAPONS: &str =
    tr_key!("The project holds no weapons: Project ▸ Add a weapon pack… adds some");

fn has_weapons(state: &AppState) -> Result<(), &'static str> {
    a_project(state)?;
    none_if(!state.has_weapons(), crate::i18n::tr(NO_WEAPONS))
}

const LINKED: &str = tr_key!(
    "These weapons are linked from another project, which alone edits them: open that project to change them"
);

fn writable_weapons(state: &AppState) -> Result<(), &'static str> {
    has_weapons(state)?;
    none_if(
        state.weapons_pack().is_some_and(|pack| pack.linked),
        crate::i18n::tr(LINKED),
    )
}

fn a_weapon_to_take_out(state: &AppState) -> Result<(), &'static str> {
    writable_weapons(state)?;
    none_if(
        state.selected_weapon().is_none(),
        tr!("Select a weapon on the Weapons screen first"),
    )
}

fn a_change_to_take_back(state: &AppState) -> Result<(), &'static str> {
    has_weapons(state)?;
    none_if(
        state.workbench.weapons.undo.is_empty(),
        tr!("No change to a weapon has been made to take back"),
    )
}

fn a_change_to_make_again(state: &AppState) -> Result<(), &'static str> {
    has_weapons(state)?;
    none_if(
        state.workbench.weapons.redo.is_empty(),
        tr!("No change to a weapon has been taken back to make again"),
    )
}

pub(super) const ADD_WEAPON_PACK: Spec = Spec {
    action: Action::AddWeaponPack,
    id: "add_weapon_pack",
    label: tr_key!("Add a weapon pack…"),
    category: tr_key!("Project ▸ More ▸ Weapons"),
    hover: tr_key!("Give the project weapons: an empty pack in a folder of its own, shipped by every build, and the Weapons screen to fill it. Add weapons… puts weapons into a pack that is there"),
    enabled: a_project,
    run: |state, _| state.add_weapon_content(),
    ..BASE
};

pub(super) const ADD_WEAPONS: Spec = Spec {
    action: Action::AddWeapons,
    id: "add_weapons",
    label: tr_key!("Add weapons…"),
    category: tr_key!("Project ▸ More ▸ Weapons"),
    hover: tr_key!("Imports into the weapons the models, textures and weapon files you pick: each game file is decompiled into a source of the pack, the weapons a weapon file defines join the pack, and a model or a texture goes to the weapon whose file names it. Dropping them on the Weapons screen does the same"),
    enabled: writable_weapons,
    run: |state, _| {
        if let Some(files) = file_dialogs::open_any_files() {
            state.add_weapons(files);
        }
    },
    ..BASE
};

pub(super) const NEW_WEAPON: Spec = Spec {
    action: Action::NewWeapon,
    id: "new_weapon",
    label: tr_key!("New weapon"),
    category: tr_key!("Project ▸ More ▸ Weapons"),
    hover: tr_key!("Add a weapon called WEAPON_NEW to the pack, in a weapon file of its own with every value at nought for you to set. Give it its real name and values on the Weapons screen"),
    enabled: writable_weapons,
    run: |state, _| state.new_weapon(),
    ..BASE
};

pub(super) const REMOVE_WEAPON: Spec = Spec {
    action: Action::RemoveWeapon,
    id: "remove_weapon",
    label: tr_key!("Remove weapon"),
    category: tr_key!("Project ▸ More ▸ Weapons"),
    hover: tr_key!("Take the weapon selected out of the pack. Its sources stay in the folder of the pack, where the rules say that no weapon names them; Undo puts it back"),
    enabled: a_weapon_to_take_out,
    run: |state, _| state.remove_selected_weapon(),
    ..BASE
};

pub(super) const UNDO_WEAPONS: Spec = Spec {
    action: Action::UndoWeapons,
    id: "undo_weapons",
    label: tr_key!("Undo weapon change"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Undo the last change to a weapon: a value, a name, a component or a weapon taken out of the pack. Adding weapons to the pack is one step of the project's History, which Undo import takes back"),
    enabled: a_change_to_take_back,
    label_now: Some(|state| match state.workbench.weapons.undo.last() {
        Some(last) => tr!("Undo \"{last}\"", last = last.label.as_str()),
        None => tr!("Undo weapon change").to_string(),
    }),
    run: |state, _| state.undo_weapons_change(),
    ..BASE
};

pub(super) const REDO_WEAPONS: Spec = Spec {
    action: Action::RedoWeapons,
    id: "redo_weapons",
    label: tr_key!("Redo weapon change"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Make again the change to a weapon that Undo took back"),
    enabled: a_change_to_make_again,
    label_now: Some(|state| match state.workbench.weapons.redo.last() {
        Some(last) => tr!("Redo \"{last}\"", last = last.label.as_str()),
        None => tr!("Redo weapon change").to_string(),
    }),
    run: |state, _| state.redo_weapons_change(),
    ..BASE
};

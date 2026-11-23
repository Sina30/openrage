//! The commands of the Wardrobe: giving a project clothing, adding clothes to it, and taking the last change back.

use super::table::{a_project, none_if, Action, Spec, BASE};
use crate::platform::file_dialogs;
use crate::state::workbench::Wanting;
use crate::state::AppState;
use crate::ui::dock::Workspace;

const NO_CLOTHING: &str =
    tr_key!("The project holds no clothing: Project ▸ Add a clothing pack… adds one");

fn has_clothing(state: &AppState) -> Result<(), &'static str> {
    a_project(state)?;
    none_if(!state.has_clothing(), crate::i18n::tr(NO_CLOTHING))
}

const LINKED: &str =
    tr_key!("This clothing is linked from another project, which alone edits it: open that project to add clothes");

const HELD_ELSEWHERE: &str =
    tr_key!("Another OpenRage window is editing this pack, so it is read-only here: close it there, or press Try again on the Wardrobe");

fn writable_clothing(state: &AppState) -> Result<(), &'static str> {
    has_clothing(state)?;
    none_if(state.wardrobe_is_linked(), crate::i18n::tr(LINKED))?;
    none_if(state.wardrobe_locked_out(), crate::i18n::tr(HELD_ELSEWHERE))
}

/// Why the garments picked cannot be taken out of the pack, none where they can.
pub(super) fn can_remove_garments(
    state: &AppState,
    garments: &[String],
) -> Result<(), &'static str> {
    writable_clothing(state)?;
    none_if(
        garments.is_empty(),
        tr!("Select a garment in the Wardrobe first"),
    )
}

/// Why the garments picked cannot be given a name, none where they can: one garment of a pack this window writes.
pub(super) fn can_rename_garment(
    state: &AppState,
    garments: &[String],
) -> Result<(), &'static str> {
    writable_clothing(state)?;
    none_if(
        garments.len() != 1,
        tr!("Pick one garment to give it a name"),
    )
}

fn a_change_to_take_back(state: &AppState) -> Result<(), &'static str> {
    has_clothing(state)?;
    none_if(
        state.workbench.wardrobe.undo.is_empty(),
        tr!("No drop, removal or change of a cloth has been made to take back"),
    )
}

pub(super) const ADD_CLOTHING: Spec = Spec {
    action: Action::AddClothing,
    id: "add_clothing",
    label: tr_key!("Add a clothing pack…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Give the project a clothing pack: an empty one in a folder of its own, shipped by every build, and the Wardrobe to fill it. Add clothes… puts garments into a pack that is there"),
    enabled: a_project,
    run: |state, _| state.add_clothing_content(),
    ..BASE
};

pub(super) const ADD_CLOTHES: Spec = Spec {
    action: Action::AddClothes,
    id: "add_clothes",
    label: tr_key!("Add clothes…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Imports into the project the models, textures and cloth you pick: each model becomes a garment with the files its name says go with it, and a game file is decompiled into a source of the project, where Open looks at a file where it is and changes nothing. Dropping them on the Wardrobe does the same; Add a folder of clothes… takes a whole folder"),
    enabled: writable_clothing,
    run: |state, _| {
        if let Some(files) = file_dialogs::open_any_files() {
            state.add_clothes(files);
        }
    },
    ..BASE
};

pub(super) const ADD_CLOTHES_FOLDER: Spec = Spec {
    action: Action::AddClothesFolder,
    id: "add_clothes_folder",
    label: tr_key!("Add a folder of clothes…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Imports into the project every model, texture and cloth in a folder you pick and in the folders under it, as a drop of the folder on the Wardrobe does: each model becomes a garment with the files its name says go with it, and a game file is decompiled into a source of the project, where Open looks at a file where it is and changes nothing"),
    enabled: writable_clothing,
    run: |state, _| {
        if let Some(folder) = file_dialogs::pick_folder(tr!("Add a folder of clothes")) {
            state.add_clothes(vec![folder]);
        }
    },
    ..BASE
};

/// Why the pack cannot be changed by a fix, none where it can: one this window writes, with a garment in it.
pub(super) fn a_pack_with_garments(state: &AppState) -> Result<(), &'static str> {
    writable_clothing(state)?;
    let held = (state.wardrobe_pack().and_then(|p| p.read.as_ref().ok()))
        .is_some_and(|read| !read.wardrobe.garments.is_empty());
    none_if(
        !held,
        tr!("The pack holds no garment yet: Add clothes… puts some in"),
    )
}

pub(super) const FIX_ALL_SAFE: Spec = Spec {
    action: Action::FixAllSafe,
    id: "fix_all_safe",
    label: tr_key!("Fix all safe"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Run repair, textures, order and mask over the pack, each reporting what it did in the Log, as one step of Undo: a model whose counts contradict their runs is repaired where that loses nothing, a texture is given its mips, its power of two and a format the game's clothing uses, the pins that clash are let go, and a mask whose skin bit disagrees with the garment's skin states it again. No picture is made smaller on purpose, no garment is taken out and no file is written over: a source that changes is copied. Run again it changes nothing"),
    enabled: a_pack_with_garments,
    run: |state, _| state.fix_all_safe(),
    ..BASE
};

pub(super) const OPTIMISE_TEXTURES: Spec = Spec {
    action: Action::OptimiseTextures,
    id: "optimise_textures",
    label: tr_key!("Optimise textures…"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Make the textures of the garments picked, or of the whole pack, lighter: BC1 or BC3 by alpha, mips, a power of two, and a size cut by a ratio or to a longest side. The textures embedded in the models are optimised too. A source is never changed in place: the garments name a lighter copy, the Log says what each texture came to, and Undo takes it back"),
    enabled: a_pack_with_garments,
    run: |state, _| state.ask_to_optimise(),
    ..BASE
};

pub(super) const REPAIR_MODEL: Spec = Spec {
    action: Action::RepairModel,
    id: "repair_model",
    label: tr_key!("Repair model"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Rewrite the counts of the model of the garment selected from the runs they describe, where that loses nothing: the model becomes a source made of the repaired file, the file it was is not touched, and the Log keeps what was rewritten. A model that cannot be repaired, such as a protected asset, is refused by name. Undo takes it back"),
    enabled: |state| {
        a_garment_to_change(state)?;
        none_if(
            the_garment(state).is_some_and(|(g, _)| g.model.is_none()),
            tr!("This place is held with no model to repair"),
        )
    },
    run: |state, _| {
        if let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) {
            state.repair_model(module, id);
        }
    },
    ..BASE
};

pub(super) const UNDO_CLOTHES: Spec = Spec {
    action: Action::UndoClothes,
    id: "undo_clothes",
    label: tr_key!("Undo clothes change"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Undo the last drop, removal or change of a cloth: a drop is taken back with the garments it added, the shop ids it read and the files it copied; a removed garment comes back with all it stated; a cloth is linked or unlinked again"),
    enabled: a_change_to_take_back,
    label_now: Some(|state| match state.workbench.wardrobe.undo_label() {
        Some(last) => tr!("Undo \"{last}\"", last = last),
        None => tr!("Undo clothes change").to_string(),
    }),
    run: |state, _| state.undo_last_change(),
    ..BASE
};

/// Why a cloth cannot be linked to the garment selected, none where it can.
fn a_garment_to_link_a_cloth_to(state: &AppState) -> Result<(), &'static str> {
    writable_clothing(state)?;
    let Some((pack, at)) = state.selected_garment() else {
        return Err(tr!("Select a garment in the Wardrobe first"));
    };
    let Ok(read) = &pack.read else {
        return Err(tr!("The pack could not be read"));
    };
    let garment = &read.wardrobe.garments[at];
    if garment.slot.is_some_and(or_wardrobe::Slot::is_prop) {
        return Err(tr!("A prop owns no cloth: the game reads none for it"));
    }
    none_if(
        garment.model.is_none(),
        tr!("This place is held with no model, so a cloth has nothing to be named after"),
    )
}

/// Why a cloth cannot be unlinked from the garment selected, none where it can.
fn a_garment_with_a_cloth(state: &AppState) -> Result<(), &'static str> {
    writable_clothing(state)?;
    let Some((pack, at)) = state.selected_garment() else {
        return Err(tr!("Select a garment in the Wardrobe first"));
    };
    let Ok(read) = &pack.read else {
        return Err(tr!("The pack could not be read"));
    };
    let garment = &read.wardrobe.garments[at];
    none_if(
        garment.cloth.is_none() && !garment.missing_cloth,
        tr!("This garment has no cloth"),
    )
}

/// The id of the garment selected in the Wardrobe.
pub(super) fn selected_id(state: &AppState) -> Option<String> {
    state
        .workbench
        .wardrobe
        .selected
        .as_ref()
        .map(|(_, id)| id.clone())
}

fn a_change_to_make_again(state: &AppState) -> Result<(), &'static str> {
    has_clothing(state)?;
    none_if(
        state.workbench.wardrobe.redo.is_empty(),
        tr!("Nothing taken back in the Wardrobe is left to make again"),
    )
}

pub(super) const REDO_CLOTHES: Spec = Spec {
    action: Action::RedoClothes,
    id: "redo_clothes",
    label: tr_key!("Redo clothes change"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Make again the last change to the clothes that was taken back: a drop comes back with its garments and the very files it copied. A new change drops what was taken back"),
    enabled: a_change_to_make_again,
    label_now: Some(|state| match state.redo_label() {
        Some(next) => tr!("Redo \"{next}\"", next = next),
        None => tr!("Redo clothes change").to_string(),
    }),
    run: |state, _| state.redo_last_change(),
    ..BASE
};

/// Whether Edit's Undo and Redo are the Wardrobe's: the workspace of the clothes joins the project's steps and the clothes' own changes in one.
pub(super) fn undo_is_the_wardrobes(state: &AppState) -> bool {
    state.project_path.is_some() && state.active_workspace == Workspace::Wardrobe
}

pub(super) fn can_undo_in_the_wardrobe(state: &AppState) -> Result<(), &'static str> {
    a_project(state)?;
    none_if(
        state.joined_undo_side().is_none(),
        tr!("Nothing to undo in the Wardrobe or the project yet"),
    )
}

pub(super) fn can_redo_in_the_wardrobe(state: &AppState) -> Result<(), &'static str> {
    a_project(state)?;
    none_if(
        state.joined_redo_side().is_none(),
        tr!("Nothing to redo in the Wardrobe or the project"),
    )
}

/// Why the pack shown cannot be put back to its last good version, none where it can.
fn a_pack_with_a_good_version(state: &AppState) -> Result<(), &'static str> {
    has_clothing(state)?;
    none_if(state.wardrobe_is_linked(), crate::i18n::tr(LINKED))?;
    none_if(state.wardrobe_locked_out(), crate::i18n::tr(HELD_ELSEWHERE))?;
    let Some(pack) = state.wardrobe_pack() else {
        return Err(tr!("The pack has not been read yet"));
    };
    if pack.read.is_ok() {
        return Err(tr!("The pack reads: there is nothing to put back"));
    }
    none_if(
        pack.last_good.is_none(),
        tr!("No earlier version of this pack reads"),
    )
}

pub(super) const RESTORE_LAST_GOOD: Spec = Spec {
    action: Action::RestoreLastGood,
    id: "restore_last_good",
    label: tr_key!("Restore the last good version"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Put the newest earlier version of the pack that reads in place of the wardrobe.json that does not. The text that did not read is kept in the pack's .openrage/unreadable folder"),
    enabled: a_pack_with_a_good_version,
    run: |state, _| {
        if let Some(module) = state.wardrobe_module_id() {
            state.restore_last_good(module);
        }
    },
    ..BASE
};

pub(super) const REVEAL_PACK: Spec = Spec {
    action: Action::RevealPack,
    id: "reveal_pack",
    label: tr_key!("Reveal the pack's wardrobe.json"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Show the pack's wardrobe.json in the file manager, to mend it by hand"),
    enabled: has_clothing,
    run: |state, _| {
        if let Some(pack) = state.wardrobe_pack() {
            crate::platform::shell::reveal(&pack.folder.join(or_wardrobe::FILE));
        }
    },
    ..BASE
};

pub(super) const LINK_CLOTH: Spec = Spec {
    action: Action::LinkCloth,
    id: "link_cloth",
    label: tr_key!("Link cloth…"),
    category: tr_key!("Project"),
    hover: tr_key!("Link a cloth (.yld) to the garment selected: choose one of the project's, one of another place (imported into the project, or only referenced) or type its id. It is shipped under the model's name, and the metadata states ownsCloth because of it"),
    enabled: a_garment_to_link_a_cloth_to,
    run: |state, _| {
        let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) else {
            return;
        };
        state.ask_for_a_file(Wanting::Cloth { module, garment: id });
    },
    ..BASE
};

pub(super) const UNLINK_CLOTH: Spec = Spec {
    action: Action::UnlinkCloth,
    id: "unlink_cloth",
    label: tr_key!("Unlink cloth"),
    category: tr_key!("Project"),
    hover: tr_key!("Take the cloth off the garment selected: the build ships none and states no ownsCloth for it. The file is not touched, and Undo links it again"),
    enabled: a_garment_with_a_cloth,
    run: |state, _| {
        if let Some(id) = selected_id(state) {
            state.unlink_cloth(id);
        }
    },
    ..BASE
};

/// The garment selected, when the pack it is in reads, with the place it takes.
pub(super) fn the_garment(state: &AppState) -> Option<(&or_wardrobe::Garment, Option<u32>)> {
    let (pack, at) = state.selected_garment()?;
    let read = pack.read.as_ref().ok()?;
    Some((
        &read.wardrobe.garments[at],
        read.allocation.of(at).map(|p| p.place),
    ))
}

const SELECT_FIRST: &str = tr_key!("Select a garment in the Wardrobe first");

/// Why the garment selected cannot be changed, none where it can.
pub(super) fn a_garment_to_change(state: &AppState) -> Result<(), &'static str> {
    writable_clothing(state)?;
    the_garment(state)
        .map(|_| ())
        .ok_or(crate::i18n::tr(SELECT_FIRST))
}

/// The texture of the garment selected that the dressed ped wears, which a command that names no texture acts on.
fn worn_texture(state: &AppState, garment: &or_wardrobe::Garment) -> usize {
    let worn = (state.workbench.wardrobe.dressed.variation.get(&garment.id)).copied();
    worn.unwrap_or(0)
        .min(garment.variations.len().saturating_sub(1))
}

pub(super) const REPLACE_MODEL: Spec = Spec {
    action: Action::ReplaceModel,
    id: "replace_model",
    label: tr_key!("Replace model…"),
    category: tr_key!("Project"),
    hover: tr_key!("Put another model in the place of the one of the garment selected, chosen from the project, from elsewhere (a .ydd is decompiled into a source of the project, or only referenced) or by its id: the garment keeps its place, textures and flags, the file it named is not touched, and Undo names it again"),
    enabled: a_garment_to_change,
    run: |state, _| {
        let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) else {
            return;
        };
        state.ask_for_a_file(Wanting::Model { module, garment: id });
    },
    ..BASE
};

pub(super) const USE_MODEL_SOURCE: Spec = Spec {
    action: Action::UseModelSource,
    id: "use_model_source",
    label: tr_key!("Use a model source folder…"),
    category: tr_key!("Project"),
    hover: tr_key!("Make the model of the garment selected a .drawables.src folder, taken as it is into the pack: a build cooks it into the .ydd it ships, and the garment keeps its place, textures and flags. Undo takes the copy out"),
    enabled: a_garment_to_change,
    run: |state, _| {
        let Some(id) = selected_id(state) else {
            return;
        };
        let picked = file_dialogs::pick_folder(tr!("Use Model Source Folder"));
        if let Some(folder) = picked {
            state.replace_model(id, folder);
        }
    },
    ..BASE
};

pub(super) const REPLACE_TEXTURE: Spec = Spec {
    action: Action::ReplaceTexture,
    id: "replace_texture",
    label: tr_key!("Replace texture…"),
    category: tr_key!("Project"),
    hover: tr_key!("Put an image, a .ytd or a textures source in the place of the texture the dressed ped wears, or a reference to one: an image becomes an authored textures source that compiles with the build, there is no convert button, and its tint and flags stay. The file it named is not touched, and Undo names it again"),
    enabled: |state| {
        a_garment_to_change(state)?;
        none_if(
            the_garment(state).is_some_and(|(g, _)| g.variations.is_empty()),
            tr!("This garment has no texture to replace"),
        )
    },
    run: |state, _| {
        let Some((id, texture)) =
            the_garment(state).map(|(g, _)| (g.id.clone(), worn_texture(state, g)))
        else {
            return;
        };
        if let Some(module) = state.wardrobe_module_id() {
            state.ask_for_a_file(Wanting::Texture {
                module,
                garment: id,
                variation: texture,
            });
        }
    },
    ..BASE
};

pub(super) const ADD_TEXTURE: Spec = Spec {
    action: Action::AddTexture,
    id: "add_texture",
    label: tr_key!("Add texture…"),
    category: tr_key!("Project"),
    hover: tr_key!("Add a texture to the garment selected as its next: a .ytd or an image becomes a source of the pack, and a source of the project or an id can be named instead. Undo takes it out"),
    enabled: a_garment_to_change,
    run: |state, _| {
        let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) else {
            return;
        };
        state.ask_for_a_file(Wanting::NewTexture { module, garment: id });
    },
    ..BASE
};

pub(super) const DUPLICATE_TO_OTHER_PED: Spec = Spec {
    action: Action::DuplicateToOtherPed,
    id: "duplicate_to_other_ped",
    label: tr_key!("Duplicate to the other ped"),
    category: tr_key!("Project"),
    hover: tr_key!("Copy the garment selected to the other freemode ped, with a copy of its model, cloth, textures and every flag: the copy takes the lowest free place of its slot, and ships the same model until Replace model… gives it the other ped's. Undo takes the copy out"),
    enabled: |state| {
        a_garment_to_change(state)?;
        none_if(
            the_garment(state).is_some_and(|(g, _)| g.ped.is_none()),
            tr!("Say which ped the garment dresses first"),
        )
    },
    label_now: Some(|state| match the_garment(state).and_then(|(g, _)| g.ped) {
        Some(ped) => match ped.other() {
            or_wardrobe::Ped::Male => tr!("Duplicate to the male ped"),
            or_wardrobe::Ped::Female => tr!("Duplicate to the female ped"),
        }
        .to_string(),
        None => tr!("Duplicate to the other ped").to_string(),
    }),
    run: |state, _| {
        if let Some((id, ped)) =
            the_garment(state).and_then(|(g, _)| Some((g.id.clone(), g.ped?)))
        {
            state.duplicate_garments(vec![id], ped.other());
        }
    },
    ..BASE
};

/// Why the garment selected cannot swap places with a neighbour, none where it can.
fn a_garment_with_a_place(state: &AppState) -> Result<(), &'static str> {
    a_garment_to_change(state)?;
    none_if(
        the_garment(state).is_some_and(|(_, place)| place.is_none()),
        tr!("This garment has no place yet: say its ped and its slot first"),
    )
}

pub(super) const MOVE_GARMENT_EARLIER: Spec = Spec {
    action: Action::MoveGarmentEarlier,
    id: "move_garment_earlier",
    label: tr_key!("Move garment earlier"),
    category: tr_key!("Project"),
    hover: tr_key!("Swap the place of the garment selected with the one before it in its slot. The name the game sees changes for both, and the next build's diff says so by their stable ids. Undo swaps them back"),
    enabled: a_garment_with_a_place,
    run: |state, _| {
        if let Some(id) = selected_id(state) {
            state.move_garment(id, or_wardrobe::edit::Toward::Earlier);
        }
    },
    ..BASE
};

pub(super) const MOVE_GARMENT_LATER: Spec = Spec {
    action: Action::MoveGarmentLater,
    id: "move_garment_later",
    label: tr_key!("Move garment later"),
    category: tr_key!("Project"),
    hover: tr_key!("Swap the place of the garment selected with the one after it in its slot. The name the game sees changes for both, and the next build's diff says so by their stable ids. Undo swaps them back"),
    enabled: a_garment_with_a_place,
    run: |state, _| {
        if let Some(id) = selected_id(state) {
            state.move_garment(id, or_wardrobe::edit::Toward::Later);
        }
    },
    ..BASE
};

pub(super) const EXPORT_GARMENT: Spec = Spec {
    action: Action::ExportGarment,
    id: "export_garment",
    label: tr_key!("Export garment files…"),
    category: tr_key!("Project"),
    hover: tr_key!("Write the files of the garment selected into a folder, under the names a build gives them: dropped on another pack they make the garment again. Nothing there is overwritten, and the pack is not changed"),
    enabled: |state| {
        has_clothing(state)?;
        the_garment(state).map(|_| ()).ok_or(crate::i18n::tr(SELECT_FIRST))
    },
    run: |state, _| {
        let Some(id) = selected_id(state) else {
            return;
        };
        let picked = file_dialogs::pick_folder(tr!("Export Files Into Folder"));
        if let Some(folder) = picked {
            state.export_garments(vec![id], folder);
        }
    },
    ..BASE
};

pub(super) const PACKAGE_GARMENT: Spec = Spec {
    action: Action::PackageGarment,
    id: "package_garment",
    label: tr_key!("Package garment…"),
    category: tr_key!("Project"),
    hover: tr_key!("Write the garment selected as one package in a folder of its own: its models and textures as sources, its settings as data, its cloth as it is. Another pack adds the package with Add package… as an equal garment. Nothing is overwritten, and the pack is not changed"),
    enabled: |state| {
        has_clothing(state)?;
        the_garment(state).map(|_| ()).ok_or(crate::i18n::tr(SELECT_FIRST))
    },
    run: |state, _| {
        let Some(id) = selected_id(state) else {
            return;
        };
        if let Some(folder) = file_dialogs::pick_folder(tr!("Package Garment Into Folder")) {
            state.package_garment(id, folder);
        }
    },
    ..BASE
};

pub(super) const REVIEW_VERSIONS: Spec = Spec {
    action: Action::ReviewVersions,
    id: "review_versions",
    label: tr_key!("Review versions…"),
    category: tr_key!("Project ▸ History"),
    hover: tr_key!("Put two versions of the pack side by side, read-only: the garments, textures and settings added, changed and removed between them, with pictures of both sides of each garment that differs. Nothing in the pack is changed"),
    enabled: has_clothing,
    run: |state, _| state.ask_to_review(),
    ..BASE
};

pub(super) const CREDIT_GARMENT: Spec = Spec {
    action: Action::CreditGarment,
    id: "credit_garment",
    label: tr_key!("Credit garment…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("State who made the garment selected and on what terms its sources may be used: it is written in the asset.json of each source folder the garment names, so it goes wherever the source goes, and a build that writes a credits file lists it"),
    enabled: |state| {
        writable_clothing(state)?;
        the_garment(state).map(|_| ()).ok_or(crate::i18n::tr(SELECT_FIRST))
    },
    run: |state, _| state.ask_to_credit(),
    ..BASE
};

pub(super) const EXPORT_PACK_PACKAGE: Spec = Spec {
    action: Action::ExportPackPackage,
    id: "export_pack_package",
    label: tr_key!("Export pack as a package…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Write the whole pack as one file another machine unpacks as an equal pack: its sources and its settings, never a compiled game file. The pack is not changed"),
    enabled: has_clothing,
    run: |state, _| {
        let name = state
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .map_or_else(|| "pack".to_string(), |read| read.wardrobe.pack.clone());
        if let Some(file) = file_dialogs::save_pack_package(&name) {
            state.export_pack_package(file);
        }
    },
    ..BASE
};

pub(super) const IMPORT_PACK_PACKAGE: Spec = Spec {
    action: Action::ImportPackPackage,
    id: "import_pack_package",
    label: tr_key!("Unpack a pack package…"),
    category: tr_key!("Project ▸ More ▸ Clothing"),
    hover: tr_key!("Unpack a pack package into a clothing folder of its own in the project and add it as a clothing content that every build ships. Its sources and settings come in as they were packaged"),
    enabled: a_project,
    run: |state, _| {
        if let Some(file) = file_dialogs::open_pack_package() {
            state.import_pack_package(file);
        }
    },
    ..BASE
};

pub(super) const ADD_PACKAGE: Spec = Spec {
    action: Action::AddPackage,
    id: "add_package",
    label: tr_key!("Add package…"),
    category: tr_key!("Project"),
    hover: tr_key!("Add the garment a package holds (Package garment… writes one) to the pack as an equal garment with an id of its own, its files copied in as sources. A shop id the pack already writes is let go and said. Undo takes the garment out"),
    enabled: writable_clothing,
    run: |state, _| {
        if let Some(folder) = file_dialogs::pick_folder(tr!("Add Garment Package")) {
            state.add_package(folder);
        }
    },
    ..BASE
};

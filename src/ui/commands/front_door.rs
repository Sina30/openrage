//! The commands of the front door: a new project, the start page, a project in a new window, and a folder made a project.

use super::table::{Action, Spec, BASE};
use crate::platform::file_dialogs;
use crate::state::front_door::{Offer, Template};

pub(super) const CREATE_PROJECT: Spec = Spec {
    action: Action::CreateProject,
    id: "create_project",
    label: tr_key!("New project…"),
    category: tr_key!("File"),
    hover: tr_key!(
        "Make a project from a template: an empty one, a clothing pack, loose files to ship \
            as a FiveM resource, or a folder you already have"
    ),
    run: |state, _| state.start_new_project(Template::Empty),
    ..BASE
};

pub(super) const START_PAGE: Spec = Spec {
    action: Action::StartPage,
    id: "start_page",
    label: tr_key!("Start page"),
    category: tr_key!("View"),
    hover: tr_key!(
        "Show the start page: new project, open project, recent projects, installs and keys"
    ),
    run: |state, _| state.show_start_page(),
    ..BASE
};

pub(super) const OPEN_PROJECT_IN_NEW_WINDOW: Spec = Spec {
    action: Action::OpenProjectInNewWindow,
    id: "open_project_in_new_window",
    label: tr_key!("Open project in new window…"),
    category: tr_key!("File"),
    hover: tr_key!("Open a project in a second OpenRage window: one window holds one project"),
    run: |state, _| {
        if let Some(path) = file_dialogs::open_project_file() {
            state.open_project_in_new_window(&path);
        }
    },
    ..BASE
};

pub(super) const OPEN_BUILT_PACK: Spec = Spec {
    action: Action::OpenBuiltPack,
    id: "open_built_pack",
    label: tr_key!("Import built pack…"),
    category: tr_key!("File"),
    hover: tr_key!("Import a built pack as a new project of sources: a singleplayer DLC (.rpf), an OIV package or a grzyClothTool project. \
            The sources are made in a new folder, and the pack is never changed"),
    enabled: |state| state.reopen_refused().map_or(Ok(()), Err),
    run: |state, _| {
        if let Some(built) = file_dialogs::pick_built_pack() {
            state.ask_to_reopen(built);
        }
    },
    ..BASE
};

pub(super) const MAKE_FOLDER_A_PROJECT: Spec = Spec {
    action: Action::MakeFolderAProject,
    id: "make_folder_a_project",
    label: tr_key!("Make this folder a project"),
    category: tr_key!("Project"),
    hover: tr_key!(
        "Write a project file in the folder open in the tree and open the project; \
            nothing in the folder is moved or rewritten"
    ),
    enabled: |state| match state.folder_offer() {
        Some(Offer::Make(_)) => Ok(()),
        Some(Offer::Open(_)) => Err(tr!("This folder already holds a project: open that one")),
        None => Err(tr!("Open a folder, with no project open, first")),
    },
    run: |state, _| {
        if let Some(Offer::Make(folder)) = state.folder_offer() {
            if let Err(why) = state.make_folder_a_project(&folder) {
                state.show_error_toast(why);
            }
        }
    },
    ..BASE
};

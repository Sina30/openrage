//! The table of task screens: one row for each screen that belongs to a kind of content, which every place that decides what such a screen shows reads.
//!
//! ⚠ A second task screen is a variant of [`Workspace`] listed in `Workspace::ALL`, a row here and its panels. No other place names its kind of content or its workspace to decide what it shows (`no_site_matches_on_the_kind_of_a_task_screen`), so the Content panel, Open in, the start page, the strip and the palette show a row without a line of their own.

use std::path::Path;

use egui_dock::{DockState, NodeIndex};
use or_project::{Module, ModuleKind};

use crate::state::front_door::Template;
use crate::state::workbench::{held, Workbench};
use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::dock::{Tab, Workspace};
use crate::ui::panels::registry::ids;

/// How a template starts a project on a task screen.
pub struct Starts {
    pub template: Template,
    /// The workspace buttons the template pins, the screen among them.
    pub pins: &'static [Workspace],
    /// What the template makes, which the hover of its chip on the start page says.
    pub goal: &'static str,
}

/// Everything the app decides about one task screen.
pub struct TaskScreen {
    /// The workspace that draws it, whose name a project file and a saved layout store.
    pub workspace: Workspace,
    /// The kind of content it shows, and that it is offered while the project holds.
    pub kind: ModuleKind,
    /// Whether the workspace is also the editor of a kind of file: it is offered with no project, keeps its own name and its own arrangement, and while a project holds the content the row adds a second arrangement, this row's layout.
    pub edits_files: bool,
    /// The name of the screen on its button and in the palette, and the sentence its hover says.
    pub label: &'static str,
    pub tooltip: &'static str,
    /// What the content is called on screen, and the word its count is said in.
    pub content: &'static str,
    pub count: &'static str,
    /// What Open in answers for content of another kind, and for a row of this kind of content.
    pub other_content: &'static str,
    pub its_rows: &'static str,
    /// What the screen says while the project holds none of its content, and the command that adds some with the words of its button.
    pub empty: &'static str,
    pub add: Action,
    pub add_button: &'static str,
    /// The line under that command on the Assets table of a project that holds nothing.
    pub landing_note: &'static str,
    /// What a drop on the screen says it does, and the command that does it.
    pub drop_hint: &'static str,
    pub drop: Action,
    /// The commands the Project menu lists under the content's name.
    pub menu: &'static [Action],
    /// The commands that take back and put back a change of this content, which Edit ▸ Undo and Redo are while the screen is on show; none where the screen's undo is not one stack of its own.
    pub undo: Option<(Action, Action)>,
    /// What the Details panel draws while the screen is in front: the thing selected on it; none where Details shows the fields of the open file.
    pub details: Option<fn(&mut egui::Ui, &mut AppState)>,
    /// The commands of the screen, drawn in the bar under the tabs while it is in front; none where the screen is the editor of a kind of file, whose bar is the file's.
    pub bar: Option<fn(&mut egui::Ui, &mut AppState)>,
    /// The arrangement of its panels, and the panels a saved arrangement must still hold to be kept.
    pub layout: fn() -> DockState<Tab>,
    pub must_dock: &'static [&'static str],
    /// The template that makes a project of this content, when one does.
    pub starts: Option<Starts>,
    /// The folder of a resource, ending in `/`, at which a data source of this content ships and a build registers it by that path (a text `.meta`, ruling V3 of the views design); none when the content keeps no such source.
    #[cfg_attr(not(test), allow(dead_code))]
    pub data_sources: Option<&'static str>,
    /// What a content holds, said in words, when counting the rows of the Assets table is not how it is counted.
    pub held: Option<fn(&Workbench, &Module) -> String>,
    /// How many a content holds as the disk says, for a card of the start page that opens nothing.
    pub on_disk: fn(&Path, &Module) -> Option<usize>,
    /// The content the screen was last told to show, which it shows while the project still holds it.
    pub showing: fn(&AppState) -> Option<&String>,
    /// Make the screen show the content `id`.
    pub show: fn(&mut AppState, &str),
    /// Whether the screen is offered with or without a project because of what the active document is.
    pub offered_by: fn(&AppState) -> bool,
}

static TABLE: [TaskScreen; 4] = [WARDROBE, WEAPONS, PARTICLES, MAPS];

const WARDROBE: TaskScreen = TaskScreen {
    workspace: Workspace::Wardrobe,
    kind: ModuleKind::Clothing,
    edits_files: false,
    label: tr_key!("Wardrobe"),
    tooltip: tr_key!(
        "A clothing pack: drop models and textures to add garments, and see what needs attention before a build"
    ),
    content: tr_key!("Clothing"),
    count: "garment",
    other_content: tr_key!("Only clothing content is shown in the Wardrobe"),
    its_rows: tr_key!("A garment is shown in the Wardrobe"),
    empty: tr_key!("No clothing in this project"),
    add: Action::AddClothing,
    add_button: tr_key!("Add a clothing pack to this project"),
    landing_note: tr_key!("An empty clothing pack, with the Wardrobe to fill it."),
    drop_hint: tr_key!("Drop clothes here"),
    drop: Action::AddClothes,
    menu: &[
        Action::AddClothing,
        Action::AddClothes,
        Action::AddClothesFolder,
        Action::RevealPack,
        Action::CreditGarment,
        Action::ExportPackPackage,
        Action::ImportPackPackage,
    ],
    // ⚠ None: its undo is the project's and the clothes' joined, which Edit ▸ Undo runs for it by name.
    undo: None,
    details: Some(crate::ui::panels::wardrobe::draw_garment),
    bar: Some(crate::ui::panels::wardrobe::draw_bar),
    layout: wardrobe_layout,
    must_dock: &[ids::DRESSED],
    starts: Some(Starts {
        template: Template::Clothing,
        pins: &[
            Workspace::Project,
            Workspace::Model,
            Workspace::Texture,
            Workspace::Wardrobe,
        ],
        goal: tr_key!("Drop models and textures: OpenRage names, numbers and checks them, and builds a FiveM resource."),    }),
    data_sources: None,
    held: Some(garments_held),
    on_disk: garments_on_disk,
    showing: |state| state.workbench.wardrobe.module.as_ref(),
    show: |state, id| state.workbench.wardrobe.module = Some(id.to_string()),
    offered_by: |_| false,
};

const WEAPONS: TaskScreen = TaskScreen {
    workspace: Workspace::Weapons,
    kind: ModuleKind::Weapons,
    edits_files: false,
    label: tr_key!("Weapons"),
    tooltip: tr_key!(
        "A weapon pack: drop weapon models, textures and weapon files to add weapons, change what defines each, and see what needs attention before a build"
    ),
    content: tr_key!("Weapons"),
    count: "weapon",
    other_content: tr_key!("Only weapons content is shown on the Weapons screen"),
    its_rows: tr_key!("A weapon is shown on the Weapons screen"),
    empty: tr_key!("No weapons in this project"),
    add: Action::AddWeaponPack,
    add_button: tr_key!("Add a weapon pack to this project"),
    landing_note: tr_key!("An empty weapon pack, with the Weapons screen to fill it."),
    drop_hint: tr_key!("Drop weapons here"),
    drop: Action::AddWeapons,
    menu: &[
        Action::AddWeaponPack,
        Action::AddWeapons,
        Action::NewWeapon,
        Action::RemoveWeapon,
    ],
    undo: Some((Action::UndoWeapons, Action::RedoWeapons)),
    details: Some(crate::ui::panels::weapons::draw_weapon),
    bar: Some(crate::ui::panels::weapons::draw_bar),
    layout: weapons_layout,
    must_dock: &[ids::WEAPON_PREVIEW],
    starts: Some(Starts {
        template: Template::Weapons,
        pins: &[
            Workspace::Project,
            Workspace::Model,
            Workspace::Texture,
            Workspace::Weapons,
        ],
        goal: tr_key!("Drop weapon models, textures and weapon files: OpenRage reads what they define, checks the pack and builds a FiveM resource."),    }),
    data_sources: Some(or_weapons::DATA_FOLDER),
    held: Some(crate::state::workbench::weapons_held),
    on_disk: crate::state::workbench::weapons_on_disk,
    showing: |state| state.workbench.weapons.module.as_ref(),
    show: |state, id| state.workbench.weapons.module = Some(id.to_string()),
    offered_by: |_| false,
};

const PARTICLES: TaskScreen = TaskScreen {
    workspace: Workspace::Particle,
    kind: ModuleKind::Particles,
    edits_files: true,
    label: tr_key!("Particle"),
    tooltip: tr_key!("Particle effects (.ypt)"),
    content: tr_key!("Particles"),
    count: "particle file",
    other_content: tr_key!("Only particles content is shown on the Particle screen"),
    its_rows: tr_key!("A particle file is shown on the Particle screen"),
    empty: tr_key!("No particle effects in this project"),
    add: Action::AddParticles,
    add_button: tr_key!("Add particle effects…"),
    landing_note: tr_key!("An empty pack of particle effects, with the Particle screen to fill it."),
    drop_hint: tr_key!("Drop particle files here"),
    drop: Action::AddParticles,
    menu: &[
        Action::AddParticlePack,
        Action::AddParticles,
        Action::NewParticles,
    ],
    undo: None,
    details: None,
    bar: None,
    layout: particles_layout,
    must_dock: &[ids::EFFECTS],
    starts: Some(Starts {
        template: Template::Particles,
        pins: &[Workspace::Project, Workspace::Particle, Workspace::Texture],
        goal: tr_key!("Drop .ypt files: OpenRage keeps each as rules and textures you can read and change, checks the pack and builds a FiveM resource."),    }),
    data_sources: None,
    held: Some(crate::state::workbench::particles_held),
    on_disk: crate::state::workbench::particles_on_disk,
    showing: |state| state.workbench.particles.module.as_ref(),
    show: |state, id| state.workbench.particles.module = Some(id.to_string()),
    offered_by: |_| false,
};

const MAPS: TaskScreen = TaskScreen {
    workspace: Workspace::Map,
    kind: ModuleKind::Maps,
    edits_files: false,
    label: tr_key!("Map"),
    tooltip: tr_key!(
        "A map pack: add .ymap and .ytyp files to see what they place, move, turn and scale each thing with a manipulator, and see what needs attention before a build"
    ),
    content: tr_key!("Maps"),
    count: "map set",
    other_content: tr_key!("Only maps content is shown on the Map screen"),
    its_rows: tr_key!("A map set is shown on the Map screen"),
    empty: tr_key!("No maps in this project"),
    add: Action::AddMapPack,
    add_button: tr_key!("Add a map to this project"),
    landing_note: tr_key!("An empty map pack, with the Map screen to fill it."),
    drop_hint: tr_key!("Drop map files here"),
    drop: Action::AddMaps,
    menu: &[
        Action::AddMapPack,
        Action::AddMaps,
        Action::AddEntity,
        Action::RemoveEntity,
        Action::DropToGround,
        Action::AddArchetype,
        Action::RemoveArchetype,
    ],
    undo: Some((Action::UndoMaps, Action::RedoMaps)),
    details: Some(crate::ui::panels::maps::draw_selection),
    bar: Some(crate::ui::panels::maps::draw_bar),
    layout: maps_layout,
    must_dock: &[ids::MAP_SCENE],
    starts: Some(Starts {
        template: Template::Maps,
        pins: &[
            Workspace::Project,
            Workspace::Model,
            Workspace::Texture,
            Workspace::Data,
            Workspace::Map,
        ],
        goal: tr_key!("Add .ymap and .ytyp files: OpenRage shows what they place, lets you move each thing with a manipulator, checks the pack and builds a FiveM resource."),    }),
    // ⚠ A map file is a binary metadata source and ships at `stream/` like every cooked source; this content keeps no text `.meta`.
    data_sources: None,
    held: Some(crate::state::workbench::maps_held),
    on_disk: crate::state::workbench::maps_on_disk,
    showing: |state| state.workbench.maps.module.as_ref(),
    show: |state, id| state.workbench.maps.module = Some(id.to_string()),
    // A scanned GTA VI map shows on it while it is the active document.
    offered_by: |state| state.scanned_map().is_some(),
};

/// The Map workspace: the maps at the left, the scene between the plan and the selection, and what needs attention along the bottom.
fn maps_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::MAP_SCENE]);
    let surface = state.main_surface_mut();
    // ⚠ No Project panel: the project's screens are tabs, and the panel beside each was the same list twice.
    let [center, _] = surface.split_left(NodeIndex::root(), 0.2, vec![Tab::MAPS]);
    let [center, _] = surface.split_right(center, 0.72, vec![Tab::DETAILS]);
    let [center, _] = surface.split_below(
        center,
        0.78,
        vec![Tab::PROBLEMS, Tab::BUILD, Tab::HISTORY, Tab::JOBS, Tab::LOG],
    );
    // The scene takes the larger part of the middle, with the plan beside it.
    let [center, _] = surface.split_right(center, 0.62, vec![Tab::MAP_PLAN]);
    state.set_focused_node_and_surface(egui_dock::NodePath {
        surface: egui_dock::SurfaceIndex::main(),
        node: center,
    });
    state
}

/// The Particle workspace while a project holds particles: the project and the effects of its files beside the viewport that plays the one chosen, the timeline and what needs attention under it, and the inspector on the right.
fn particles_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::VIEWPORT_3D]);
    let surface = state.main_surface_mut();
    let [center, left] = surface.split_left(NodeIndex::root(), 0.22, vec![Tab::PROJECT]);
    surface.split_below(left, 0.4, vec![Tab::EFFECTS, Tab::OUTLINE]);
    let [center, _] = surface.split_right(center, 0.72, vec![Tab::DETAILS, Tab::TEXTURE_PREVIEW]);
    let [center, _] = surface.split_below(
        center,
        0.74,
        vec![
            Tab::TIMELINE,
            Tab::PROBLEMS,
            Tab::BUILD,
            Tab::HISTORY,
            Tab::JOBS,
            Tab::LOG,
        ],
    );
    state.set_focused_node_and_surface(egui_dock::NodePath {
        surface: egui_dock::SurfaceIndex::main(),
        node: center,
    });
    state
}

/// The Weapons workspace: the weapons, the assembled weapon between them and the weapon selected, and what needs attention along the bottom.
fn weapons_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::WEAPONS]);
    let surface = state.main_surface_mut();
    let center = NodeIndex::root();
    let [center, _] = surface.split_right(center, 0.72, vec![Tab::DETAILS]);
    let [center, _] = surface.split_below(
        center,
        0.78,
        vec![Tab::PROBLEMS, Tab::BUILD, Tab::HISTORY, Tab::JOBS, Tab::LOG],
    );
    // The weapon takes the larger part of the middle, with the list of weapons beside it.
    let [center, _] = surface.split_right(center, 0.36, vec![Tab::WEAPON_PREVIEW]);
    state.set_focused_node_and_surface(egui_dock::NodePath {
        surface: egui_dock::SurfaceIndex::main(),
        node: center,
    });
    state
}

/// The Wardrobe workspace: the garments, the dressed ped between them and the garment, and what needs attention along the bottom.
fn wardrobe_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::WARDROBE]);
    let surface = state.main_surface_mut();
    let center = NodeIndex::root();
    let [center, _] = surface.split_right(center, 0.72, vec![Tab::DETAILS]);
    let [center, _] = surface.split_below(
        center,
        0.78,
        vec![Tab::PROBLEMS, Tab::BUILD, Tab::HISTORY, Tab::JOBS, Tab::LOG],
    );
    // The ped takes the larger part of the middle, with the list of garments beside it.
    let [center, _] = surface.split_right(center, 0.36, vec![Tab::DRESSED]);
    state.set_focused_node_and_surface(egui_dock::NodePath {
        surface: egui_dock::SurfaceIndex::main(),
        node: center,
    });
    state
}

/// What a clothing content holds as the last scan read its pack, in words.
///
/// ⚠ A garment is a row of its clothing content, so this is the count of the rows the table lists for it. A pack the scan has not reached yet is being read; one it reached and could not read says so.
fn garments_held(workbench: &Workbench, module: &Module) -> String {
    match (workbench.packs().iter()).find(|p| p.module == module.id) {
        None => tr!("Reading…").to_string(),
        Some(pack) => match &pack.read {
            Ok(read) => held(&module.kind, read.wardrobe.garments.len()),
            Err(_) => tr!("could not be read").to_string(),
        },
    }
}

/// How many garments the pack of a clothing content holds, none when its file cannot be read.
fn garments_on_disk(project: &Path, module: &Module) -> Option<usize> {
    let folder = or_wardrobe::module_folder(&or_project::project_folder(project), module).ok()?;
    or_wardrobe::Wardrobe::read(&folder)
        .ok()
        .map(|pack| pack.garments.len())
}

#[cfg(test)]
thread_local! {
    static STAND_IN: std::cell::Cell<Option<&'static TaskScreen>> = const { std::cell::Cell::new(None) };
}

/// Every row, in the order the buttons follow each other.
pub fn all() -> Vec<&'static TaskScreen> {
    let rows = TABLE.iter();
    #[cfg(test)]
    let rows = rows.chain(STAND_IN.with(std::cell::Cell::get));
    rows.collect()
}

/// The row of the screen `workspace`, none for a workspace that belongs to a kind of file.
pub fn of_workspace(workspace: Workspace) -> Option<&'static TaskScreen> {
    all().into_iter().find(|row| row.workspace == workspace)
}

/// The row of the screen `workspace` when it belongs to a kind of content and to no kind of file, none otherwise.
pub fn of_workspace_alone(workspace: Workspace) -> Option<&'static TaskScreen> {
    of_workspace(workspace).filter(|row| !row.edits_files)
}

/// The row that shows content of `kind`, none for a kind with no screen of its own.
pub fn of_kind(kind: &ModuleKind) -> Option<&'static TaskScreen> {
    all().into_iter().find(|row| row.kind == *kind)
}

/// The row a project made from `template` opens on.
pub fn of_template(template: Template) -> Option<&'static TaskScreen> {
    all()
        .into_iter()
        .find(|row| row.starts.as_ref().is_some_and(|s| s.template == template))
}

/// Run `then` with `row` as a second task screen on this thread, which a test uses to show that a row is all a screen takes.
#[cfg(test)]
pub fn with_a_second_row<R>(row: TaskScreen, then: impl FnOnce() -> R) -> R {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            STAND_IN.with(|row| row.set(None));
        }
    }
    let row: &'static TaskScreen = Box::leak(Box::new(row));
    STAND_IN.with(|slot| slot.set(Some(row)));
    let _restore = Restore;
    then()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wardrobe_is_the_row_of_clothing_and_of_the_clothing_pack_template() {
        let row = of_workspace(Workspace::Wardrobe).expect("the Wardrobe has a row");
        assert_eq!(row.kind, ModuleKind::Clothing);
        assert!(std::ptr::eq(row, of_kind(&ModuleKind::Clothing).unwrap()));
        assert!(std::ptr::eq(row, of_template(Template::Clothing).unwrap()));
        assert!(of_workspace(Workspace::Project).is_none());
        assert!(of_kind(&ModuleKind::Files).is_none());
        assert!(of_template(Template::Files).is_none());
        assert!(row.starts.as_ref().unwrap().pins.contains(&row.workspace));
    }

    #[test]
    fn a_row_is_the_only_one_for_its_workspace_and_kind_and_states_where_its_data_ships() {
        let rows = all();
        for (i, row) in rows.iter().enumerate() {
            for other in &rows[i + 1..] {
                assert_ne!(row.workspace, other.workspace, "two rows for one workspace");
                assert_ne!(row.kind, other.kind, "two rows for one kind of content");
            }
            assert!(Workspace::ALL.contains(&row.workspace));
            if let Some(path) = row.data_sources {
                assert!(
                    path.ends_with('/') && !path.starts_with('/'),
                    "`{path}` is a folder of the resource"
                );
            }
        }
    }

    #[test]
    fn undo_and_redo_on_a_screen_that_names_its_own_are_that_screens() {
        use crate::ui::commands;

        let mut state = AppState::default();
        state.project_path = Some(std::path::PathBuf::from("C:/mods/pack.openrage-proj"));
        let mut checked = 0;
        for row in all() {
            let Some((back, forth)) = row.undo else {
                continue;
            };
            state.active_workspace = row.workspace;
            for (edit, own) in [(Action::Undo, back), (Action::Redo, forth)] {
                let edit = commands::of(&state, edit).expect("Edit's command");
                let own = commands::of(&state, own).expect("the screen's command");
                assert_eq!(
                    (edit.enabled, &edit.label),
                    (own.enabled, &own.label),
                    "{:?}: the key of Undo does not reach the screen's own stack",
                    row.workspace
                );
            }
            checked += 1;
        }
        assert!(checked >= 2, "no row names an undo of its own");
    }

    #[test]
    fn a_panel_a_saved_arrangement_must_hold_is_in_the_rows_own_layout() {
        for row in all() {
            let layout = (row.layout)();
            for needed in row.must_dock {
                assert!(
                    layout.iter_all_tabs().any(|(_, tab)| tab.id() == *needed),
                    "{:?} requires `{needed}`, which its layout lacks",
                    row.workspace
                );
            }
        }
    }
}

//! Every item of the menu bar's menus, read from what egui draws, is a row of
//! the command palette with the same label, binding and enabled state.
//!
//! ⚠ Read through AccessKit, so a menu item drawn outside the command registry
//! fails here rather than passing because both sides consulted one list.

use std::collections::HashSet;

use egui::accesskit::{NodeId, Role};

use crate::state::plugins::InstalledPlugin;
use crate::state::{AppState, DocumentKind, PersistedBinding, Prefs};

const MENUS: [&str; 6] = ["File", "Edit", "View", "Project", "Automate", "Help"];

/// A widget one frame drew, as AccessKit describes it.
#[derive(Debug, Clone)]
struct Drawn {
    id: NodeId,
    role: Role,
    label: String,
    disabled: bool,
    centre: egui::Pos2,
}

/// A menu item: the menu path it was found under, its label with its binding, and whether it was disabled.
type Item = (String, String, bool);

fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    draw: &mut dyn FnMut(&mut egui::Ui),
) -> Vec<Drawn> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| draw(ui));
    output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(id, node)| {
                    let b = node.bounds()?;
                    Some(Drawn {
                        id: *id,
                        role: node.role(),
                        label: node.label()?.to_string(),
                        disabled: node.is_disabled(),
                        centre: egui::pos2(
                            ((b.x0 + b.x1) / 2.0) as f32,
                            ((b.y0 + b.y1) / 2.0) as f32,
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn click(ctx: &egui::Context, draw: &mut dyn FnMut(&mut egui::Ui), at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(ctx, vec![egui::Event::PointerMoved(at), press(true)], draw);
    frame(ctx, vec![press(false)], draw);
}

/// The items of the menu just opened at `path`, and of every submenu under it.
fn walk(
    ctx: &egui::Context,
    draw: &mut dyn FnMut(&mut egui::Ui),
    path: &str,
    before: &HashSet<NodeId>,
    items: &mut Vec<Item>,
) {
    let shown = frame(ctx, Vec::new(), draw);
    let here: HashSet<NodeId> = shown.iter().map(|d| d.id).collect();
    for drawn in shown.into_iter().filter(|d| !before.contains(&d.id)) {
        if let Some(name) = drawn
            .label
            .strip_suffix(egui::containers::menu::SubMenuButton::RIGHT_ARROW)
        {
            if !drawn.disabled {
                click(ctx, draw, drawn.centre);
                walk(
                    ctx,
                    draw,
                    &format!("{path} ▸ {}", name.trim_end()),
                    &here,
                    items,
                );
            }
        } else if matches!(drawn.role, Role::Button | Role::CheckBox) {
            items.push((path.to_string(), drawn.label, drawn.disabled));
        }
    }
}

/// Recent files, folders and projects, a project with a member, a document, a
/// plugin and two bindings the user chose, so that every submenu has items.
fn a_session_with_something_in_every_menu(dir: &std::path::Path) -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    let model = dir.join("effects.ypt");
    std::fs::write(&model, b"").expect("a recent file");
    let project = dir.join("mod.openrage-proj");
    let opened = crate::state::explorer::Place::new(&model, "").expect("a recent file");
    state.prefs.touch_recent_file(opened);
    state.prefs.recent_folders = vec![dir.to_path_buf()];
    state.prefs.recent_projects = vec![project.clone()];
    state.project_path = Some(project);
    let id = state.new_empty_document(DocumentKind::Ypt);
    if let Some(doc) = state.document_mut(id) {
        doc.path = Some(model);
    }
    state.set_active_document(id);
    state.plugins.scanned = Some(vec![InstalledPlugin {
        manifest: or_plugin::Manifest {
            id: "sample".into(),
            name: "Sample plugin".into(),
            abi: or_plugin::ABI_VERSION,
            wants: Vec::new(),
        },
        wasm: dir.join("plugin.wasm"),
        refused: None,
    }]);
    let bound = |ctrl, shift, key: &str| PersistedBinding {
        control: false,
        ctrl,
        shift,
        alt: false,
        key: key.into(),
    };
    let bindings = &mut state.prefs.shortcut_bindings;
    bindings.insert("open".into(), bound(true, true, "O"));
    bindings.insert("show_grid".into(), bound(false, false, "G"));
    state
}

/// The menus on the row of File, left to right.
fn menus_of_the_bar(state: &mut AppState) -> Vec<String> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut draw = |ui: &mut egui::Ui| crate::ui::panels::menu::draw(ui, state);
    let bar = frame(&ctx, Vec::new(), &mut draw);
    let row = (bar.iter())
        .find(|d| d.label == MENUS[0])
        .expect("the menu bar has File")
        .centre
        .y;
    let mut menus: Vec<&Drawn> = (bar.iter())
        .filter(|d| MENUS.contains(&d.label.as_str()) && (d.centre.y - row).abs() < 1.0)
        .collect();
    menus.sort_by(|a, b| a.centre.x.total_cmp(&b.centre.x));
    menus.into_iter().map(|d| d.label.clone()).collect()
}

#[test]
fn the_bar_has_the_same_menus_with_and_without_a_project() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut with = a_session_with_something_in_every_menu(dir.path());
    let mut without = AppState::from_prefs(Prefs::default());
    assert!(with.project_path.is_some() && without.project_path.is_none());
    assert_eq!(menus_of_the_bar(&mut with), MENUS);
    assert_eq!(menus_of_the_bar(&mut without), MENUS);
}

#[test]
fn every_menu_item_is_in_the_palette_with_its_label_binding_and_enabled_state() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut state = a_session_with_something_in_every_menu(dir.path());

    let mut items: Vec<Item> = Vec::new();
    for menu in MENUS {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut draw = |ui: &mut egui::Ui| crate::ui::panels::menu::draw(ui, &mut state);
        let bar = frame(&ctx, Vec::new(), &mut draw);
        let closed: HashSet<NodeId> = bar.iter().map(|d| d.id).collect();
        // ⚠ Other widgets share a menu's name (the title bar's project chip), and AccessKit
        // does not order nodes: the menu is the one on the row of the first menu, File.
        let row = bar
            .iter()
            .find(|d| d.label == MENUS[0])
            .unwrap_or_else(|| panic!("the menu bar has no {}", MENUS[0]))
            .centre
            .y;
        let title = bar
            .iter()
            .find(|d| d.label == menu && (d.centre.y - row).abs() < 1.0)
            .unwrap_or_else(|| panic!("the menu bar has no {menu}"))
            .centre;
        click(&ctx, &mut draw, title);
        walk(&ctx, &mut draw, menu, &closed, &mut items);
    }
    let paths: HashSet<&str> = items.iter().map(|(path, ..)| path.as_str()).collect();
    assert!(
        items.iter().any(|(path, label, disabled)| path == "File"
            && label.starts_with("Import built pack…")
            && !disabled),
        "File offers no Import built pack…: {items:?}"
    );
    for submenu in [
        "File ▸ Open recent",
        "Project ▸ Properties and sharing ▸ Remove from project",
        "View ▸ Panels",
        "View ▸ Workspace ▸ Buttons shown",
        "Project",
        "Project ▸ More ▸ Clothing",
        "Project ▸ Fix",
        "Project ▸ History",
        "Project ▸ Properties and sharing",
        "Automate ▸ Run plugin",
    ] {
        assert!(
            paths.contains(submenu),
            "the walk did not reach {submenu}: {items:?}"
        );
    }

    crate::ui::panels::command_palette::open(&mut state);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut draw =
        |ui: &mut egui::Ui| crate::ui::panels::command_palette::draw(ui.ctx(), &mut state);
    frame(&ctx, Vec::new(), &mut draw);
    let rows: HashSet<(String, bool)> = frame(&ctx, Vec::new(), &mut draw)
        .into_iter()
        .filter(|d| d.role == Role::Button)
        .map(|d| (d.label, d.disabled))
        .collect();

    let unlike: Vec<String> = items
        .iter()
        .filter(|(path, label, disabled)| !rows.contains(&(format!("{path}: {label}"), *disabled)))
        .map(|(path, label, disabled)| {
            let state = if *disabled { "disabled" } else { "enabled" };
            format!("{path} ▸ {label} ({state})")
        })
        .collect();
    assert!(
        unlike.is_empty(),
        "{} of {} menu items have no palette row alike:\n{}\n\nthe palette's rows:\n{rows:#?}",
        unlike.len(),
        items.len(),
        unlike.join("\n")
    );
}

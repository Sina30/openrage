//! Every button and control of the project screens has a row in the control inventory of the UX design, with its place and its reason.
//!
//! The screens are drawn with a clothing project open, in each of the states that bring a control in, and every control AccessKit
//! describes is looked up in the table. A control with no row fails the test; a row that has no place or no reason fails it too.

use std::collections::BTreeMap;

use egui::accesskit::Role;

use super::frames_of_the_shell::Shell;
use super::the_maps_screen_on_screen::a_maps_shell;
use super::the_particle_screen_on_screen::a_particles_shell;
use super::the_project_screens_on_screen::{a_project_shell, the_questions, until};
use super::the_weapons_screen_on_screen::a_weapons_shell;
use crate::state::workbench::Question;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;

const DOC: &str = include_str!("../../docs/direction/Workbench_UX_Design.md");
const SECTION: &str = "## Control inventory";

/// The kinds of widget that are a control.
fn is_control(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::ComboBox
            | Role::TextInput
            | Role::SpinButton
            | Role::Switch
    )
}

/// What a control is called in the table: no icon glyph, and a token (`<rule>`, `<step>`, `<asset>`, `<build>`, `<project>`) where the name of one stands.
fn normal(name: &str, data: &[(&str, String)]) -> String {
    let mut text: String = name.chars().filter(|c| !(''..='').contains(c)).collect();
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for (token, each) in data {
        if !each.is_empty() && text.contains(each.as_str()) {
            text = text.replace(each.as_str(), token);
        }
    }
    text.trim().to_string()
}

/// The rows of the inventory: the pattern of the first column, with its place and its reason.
fn inventory() -> Vec<(String, String, String)> {
    let section = DOC
        .split(SECTION)
        .nth(1)
        .unwrap_or_else(|| panic!("the UX design has no section `{SECTION}`"));
    let section = section.split("\n## ").next().unwrap_or(section);
    section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            assert!(
                cells.len() >= 3,
                "a row of the inventory has three cells: {line}"
            );
            (
                cells[0].trim_matches('`').to_string(),
                cells[1].to_string(),
                cells[2].to_string(),
            )
        })
        .collect()
}

/// Whether `name` is what the pattern `pattern` says, where `*` stands for any text.
fn matches(pattern: &str, name: &str) -> bool {
    // ⚠ The letters that name the textures of a garment are one row.
    if pattern == "[A-Z]" {
        return name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase());
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if parts.len() == 1 {
        return pattern == name;
    }
    if !name.starts_with(first) || !name.ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// The names of the data a screen shows, which a control's name may hold, the longest first.
fn data_names(shell: &Shell) -> Vec<(&'static str, String)> {
    let state = &shell.state;
    let mut data: Vec<(&'static str, String)> = Vec::new();
    data.extend(or_wardrobe::RULES.iter().map(|r| ("<rule>", r.sentence())));
    data.extend((state.workbench.history().iter()).map(|e| ("<step>", e.text.clone())));
    data.extend((state.workbench.rows().iter()).map(|r| ("<asset>", r.name.clone())));
    data.extend(
        (state.project_carried.container.outputs.iter()).map(|o| ("<build>", o.name.clone())),
    );
    data.extend(state.project_name().map(|n| ("<project>", n)));
    data.sort_by_key(|(_, each)| std::cmp::Reverse(each.len()));
    data
}

/// The names of the weapons and components the Weapons screen shows, which a control's name may hold, the longest first.
///
/// ⚠ Not the assets': the weapons folder of the pack is an asset called `weapons`, which would turn `Add weapons…` into `Add <asset>…`.
fn weapon_names(shell: &Shell) -> Vec<(&'static str, String)> {
    let mut data: Vec<(&'static str, String)> = Vec::new();
    data.extend(shell.state.project_name().map(|n| ("<project>", n)));
    if let Some(read) = shell
        .state
        .weapons_pack()
        .and_then(|p| p.read.as_ref().ok())
    {
        for weapon in &read.model.weapons {
            data.push(("<weapon>", weapon.name.clone()));
            let defined = or_weapons::rules::defined_by(weapon, &read.facts);
            for point in defined.iter().flat_map(|d| &d.attach_points) {
                data.extend(
                    point
                        .components
                        .iter()
                        .map(|(name, _)| ("<component>", name.clone())),
                );
            }
        }
    }
    data.sort_by_key(|(_, each)| std::cmp::Reverse(each.len()));
    data
}

/// The names of the effects and the files the Particle screen shows, which a control's name may hold, the longest first.
fn particle_names(shell: &Shell) -> Vec<(&'static str, String)> {
    let mut data: Vec<(&'static str, String)> = Vec::new();
    data.extend(shell.state.project_name().map(|n| ("<project>", n)));
    if let Some(read) = shell
        .state
        .particles_pack()
        .and_then(|p| p.read.as_ref().ok())
    {
        for row in read.effects() {
            data.push(("<effect>", row.effect.clone()));
            data.push(("<file>", row.file.clone()));
        }
    }
    data.sort_by_key(|(_, each)| std::cmp::Reverse(each.len()));
    data
}

fn collect(shell: &Shell, into: &mut BTreeMap<String, String>, seen_in: &str) {
    collect_named(shell, into, seen_in, &data_names(shell));
}

fn collect_weapons(shell: &Shell, into: &mut BTreeMap<String, String>, seen_in: &str) {
    collect_named(shell, into, seen_in, &weapon_names(shell));
}

fn collect_particles(shell: &Shell, into: &mut BTreeMap<String, String>, seen_in: &str) {
    collect_named(shell, into, seen_in, &particle_names(shell));
}

fn collect_named(
    shell: &Shell,
    into: &mut BTreeMap<String, String>,
    seen_in: &str,
    data: &[(&str, String)],
) {
    for widget in &shell.described {
        let Some(name) = widget.name.as_deref() else {
            continue;
        };
        if !is_control(widget.role) {
            continue;
        }
        let name = normal(name, data);
        if !name.is_empty() {
            into.entry(name)
                .or_insert_with(|| format!("{:?} in {seen_in}", widget.role));
        }
    }
}

/// What the shell draws round every workspace (the menu bar, the status bar, the strip of workspaces), which other tests hold.
fn chrome(shell: &mut Shell) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    shell.state.active_workspace = Workspace::Model;
    shell.settled();
    collect(shell, &mut found, "the shell");
    found
}

/// Draws the project screens in each state that brings a control in, and returns every control found with where it was seen first.
fn controls() -> BTreeMap<String, String> {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let round = chrome(&mut shell);
    let mut found = BTreeMap::new();

    let panels = [
        ids::ASSETS,
        ids::DETAILS,
        ids::PROBLEMS,
        ids::BUILD,
        ids::HISTORY,
        ids::JOBS,
        ids::PROJECT,
    ];
    for panel in panels {
        shell.state.active_workspace = Workspace::Project;
        shell
            .state
            .workspace_docks
            .reveal(Workspace::Project, panel);
        shell.settled();
        collect(
            &shell,
            &mut found,
            &format!("the Project workspace, {panel}"),
        );
    }

    // Properties of an asset: a file, a source and a garment each bring their own.
    let keys: Vec<String> = shell
        .state
        .workbench
        .rows()
        .iter()
        .map(|r| r.key.clone())
        .collect();
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::DETAILS);
    for key in keys {
        shell.state.select_asset(Some(key));
        shell.settled();
        collect(&shell, &mut found, "Properties of an asset");
    }

    // The Wardrobe, with and without a garment chosen.
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.state.select_asset(None);
    shell.settled();
    collect(&shell, &mut found, "the Wardrobe, no garment chosen");
    let module = (shell.state.project_carried.container.modules.iter())
        .find(|m| m.kind == or_project::ModuleKind::Clothing)
        .map(|m| m.id.clone())
        .expect("a clothing content");
    let garment = shell
        .state
        .workbench
        .packs()
        .first()
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|r| r.wardrobe.garments.first().map(|g| g.id.clone()))
        .expect("a garment");
    shell.state.select_garment(&module, Some(garment));
    shell.settled();
    collect(&shell, &mut found, "the Wardrobe, a garment chosen");

    // The questions the project screens ask.
    shell.state.active_workspace = Workspace::Project;
    shell.state.ask_about_builds(false);
    shell.settled();
    collect(&shell, &mut found, "the Builds window");
    shell.state.ask_about_builds(true);
    shell.settled();
    collect(&shell, &mut found, "the Builds window, adding a build");
    shell.state.ask_about_rules(module);
    shell.settled();
    collect(&shell, &mut found, "the Rules window");
    shell.state.workbench.asking = Some(Question::Sharing {
        concerns: Vec::new(),
    });
    shell.settled();
    collect(&shell, &mut found, "Check before sharing");
    for (title, ask) in the_questions(&shell) {
        ask(&mut shell);
        shell.settled();
        collect(&shell, &mut found, &format!("the question {title}"));
    }
    shell.state.ask_for_a_new_source("textures");
    shell.settled();
    collect(&shell, &mut found, "the New source question");
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.state.ask_to_optimise();
    shell.settled();
    collect(&shell, &mut found, "the Optimise question");
    shell.state.workbench.asking = None;
    shell.settled();
    until(&mut shell, "the frame", |_| true);
    found.retain(|name, _| !round.contains_key(name));
    found
}

/// Draws the Weapons screen with a weapon pack open, with no weapon chosen and with one that has components, and returns every control found.
fn weapon_controls() -> BTreeMap<String, String> {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_weapons_shell(dir.path());
    let mut round = BTreeMap::new();
    shell.state.active_workspace = Workspace::Model;
    shell.settled();
    collect_weapons(&shell, &mut round, "the shell");
    let mut found = BTreeMap::new();
    shell.state.active_workspace = Workspace::Weapons;
    let (module, ids) = {
        let pack = shell.state.weapons_pack().expect("a weapon pack");
        let ids: Vec<String> = match &pack.read {
            Ok(read) => read.model.weapons.iter().map(|w| w.id.clone()).collect(),
            Err(_) => Vec::new(),
        };
        (pack.module.clone(), ids)
    };
    shell.state.select_weapon(&module, None);
    shell.settled();
    collect_weapons(&shell, &mut found, "the Weapons screen, no weapon chosen");
    for id in ids {
        shell.state.select_weapon(&module, Some(id));
        shell.settled();
        collect_weapons(&shell, &mut found, "the Weapons screen, a weapon chosen");
    }
    found.retain(|name, _| !round.contains_key(name));
    found
}

/// Draws the Particle screen with a pack open, empty and holding a file with two effects, and returns every control found.
fn particle_controls() -> BTreeMap<String, String> {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_particles_shell(dir.path());
    let mut round = BTreeMap::new();
    shell.state.active_workspace = Workspace::Model;
    shell.settled();
    collect_particles(&shell, &mut round, "the shell");
    let mut found = BTreeMap::new();
    shell.state.active_workspace = Workspace::Particle;
    shell.settled();
    collect_particles(&shell, &mut found, "the Particle screen");
    found.retain(|name, _| !round.contains_key(name));
    found
}

/// Draws the Map screen with a pack open, with nothing chosen, with an entity chosen and with an archetype chosen, and returns every control found.
fn map_controls() -> BTreeMap<String, String> {
    use crate::state::workbench::{Pick, PickKind};
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_maps_shell(dir.path());
    let mut round = BTreeMap::new();
    shell.state.active_workspace = Workspace::Model;
    shell.settled();
    collect_named(&shell, &mut round, "the shell", &[]);
    let mut found = BTreeMap::new();
    shell.state.active_workspace = Workspace::Map;
    let (module, set) = {
        let (pack, set) = shell.state.shown_set().expect("a set");
        (pack.module.clone(), set.id.clone())
    };
    shell.state.select_in_maps(&module, &set, None);
    shell.settled();
    collect_named(&shell, &mut found, "the Map screen, nothing chosen", &[]);
    for (what, source, kind) in [
        (
            "an entity chosen",
            "harbour.metadata.src",
            PickKind::Entity(0),
        ),
        (
            "an archetype chosen",
            "harbour_types.metadata.src",
            PickKind::Archetype(0),
        ),
    ] {
        let pick = Pick {
            source: source.to_string(),
            kind,
        };
        shell.state.select_in_maps(&module, &set, Some(pick));
        shell.settled();
        collect_named(&shell, &mut found, &format!("the Map screen, {what}"), &[]);
    }
    found.retain(|name, _| !round.contains_key(name));
    found
}

#[test]
fn every_control_of_the_project_screens_has_a_row_with_its_place_and_its_reason() {
    let rows = inventory();
    for (control, place, reason) in &rows {
        assert!(
            !place.is_empty() && !reason.is_empty(),
            "the row of `{control}` has no place or no reason"
        );
    }
    let mut drawn = controls();
    drawn.extend(weapon_controls());
    drawn.extend(particle_controls());
    drawn.extend(map_controls());
    // ⚠ A walk that drew nothing would find nothing missing and pass.
    assert!(
        drawn.len() > 30,
        "only {} controls were drawn: {drawn:?}",
        drawn.len()
    );
    let missing: Vec<String> = drawn
        .into_iter()
        .filter(|(name, _)| !rows.iter().any(|(pattern, ..)| matches(pattern, name)))
        .map(|(name, seen)| format!("{name}   ({seen})"))
        .collect();
    assert!(
        missing.is_empty(),
        "{} controls have no row in `{SECTION}` of Workbench_UX_Design.md:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn a_row_matches_by_its_pattern_and_a_control_nobody_listed_is_missing() {
    assert!(matches("Add files…", "Add files…"));
    assert!(matches("Content: *", "Content: Add files…"));
    assert!(matches("* ●", "bare ●"));
    assert!(matches("[A-Z]", "B") && !matches("[A-Z]", "BB") && !matches("[A-Z]", "b"));
    assert!(!matches("Add files…", "Add clothing…"));
    assert!(!matches("Content: *", "Content"));
    assert_eq!(
        normal("\u{e256} tshirts ●", &[("<build>", "tshirts".to_string())]),
        "<build> ●"
    );
}

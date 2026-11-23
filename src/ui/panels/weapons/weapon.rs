//! The weapon selected: its name, the sources it is made of, every value its metadata states, its components and where each attaches, and the effects, sounds and animations it names.
//!
//! The values are the typed family's, read from the metadata source that defines the weapon and written back to it through
//! `or-weapons`, so a change is one step of Undo and what no value names stays in the file as it was.

use std::path::{Path, PathBuf};

use or_io::decl::File as _;
use or_io::meta::typed::weapon::{
    CWeaponComponentPointSComponent, CWeaponInfo, CWeaponInfoBlob, InfoAny,
};
use or_weapons::facts::SourceFact;

use super::action_button;
use super::fields::{rows, Cell, Group};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::commands::Action;
use crate::ui::dock::Workspace;
use crate::ui::theme;

/// The info of the weapon as its metadata states it, read once for the file as it stands.
pub(super) struct Info {
    /// The weapon and the file it was read from, with the file's length and time: what it was read for.
    key: (String, PathBuf, u64, Option<std::time::SystemTime>),
    pub(super) value: Result<CWeaponInfo, String>,
}

/// The info of the weapon called `name` in the metadata source `source`.
fn read(source: &Path, name: &str) -> Result<CWeaponInfo, String> {
    let bytes = std::fs::read(source.join(or_source::metadata::XML)).map_err(|e| e.to_string())?;
    let blob = CWeaponInfoBlob::from_bytes(&bytes).map_err(|e| e.to_string())?;
    blob.infos
        .into_iter()
        .flat_map(|group| group.infos)
        .find_map(|info| match info {
            InfoAny::CWeaponInfo(weapon) if weapon.name.eq_ignore_ascii_case(name) => Some(weapon),
            _ => None,
        })
        .ok_or_else(|| {
            tr!(
                "The weapon file defines no weapon called {name}",
                name = name
            )
            .to_string()
        })
}

/// What the panel kept of the info between frames, in the state of the workbench's panels.
#[derive(Default)]
pub(super) struct Kept(pub(super) Option<Info>);

impl crate::ui::panel_state::PanelState for Kept {
    const KEY: &'static str = "weapon_info";
}

/// A source of the weapon in words: what it is, and its name.
fn source_line(path: &str, source: Option<&SourceFact>) -> String {
    let what = match source {
        Some(s) if s.is_model() => tr!("Model"),
        Some(s) if s.is_textures() => tr!("Textures"),
        Some(s) if s.is_metadata() => tr!("Weapon file"),
        _ => tr!("Source"),
    };
    format!("{what}: {path}")
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_weapons() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    let Some((pack, weapon)) = state.selected_weapon() else {
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new(tr!(
                "Select a weapon on the Weapons screen to change what defines it"
            ))
            .color(theme::TEXT_SECONDARY),
        );
        return;
    };
    let (id, name) = (weapon.id.clone(), weapon.name.clone());
    let sources: Vec<String> = pack
        .read
        .as_ref()
        .map(|read| {
            weapon
                .sources
                .iter()
                .map(|path| source_line(path, read.facts.source(path)))
                .collect()
        })
        .unwrap_or_default();
    let defined_in = pack.read.as_ref().ok().and_then(|read| {
        or_weapons::rules::defining_source(weapon, &read.facts).map(|s| pack.folder.join(s))
    });
    let problems: Vec<(or_weapons::Severity, String)> = pack
        .read
        .as_ref()
        .map(|read| {
            read.findings
                .iter()
                .filter(|f| f.weapon.as_deref() == Some(id.as_str()) && f.waived.is_none())
                .map(|f| (f.severity, f.message.clone()))
                .collect()
        })
        .unwrap_or_default();
    let editable = !pack.linked;

    // The name, which is the model's and the metadata's at once.
    ui.add_space(theme::SPACING_HAIR);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("Name")).color(theme::TEXT_SECONDARY));
        let work = &mut state.workbench.weapons;
        let typed = match &mut work.renaming {
            Some((held, typed)) if *held == id => typed,
            slot => &mut slot.insert((id.clone(), name.clone())).1,
        };
        let field = ui.add_enabled(
            editable,
            egui::TextEdit::singleline(typed).desired_width(f32::INFINITY),
        );
        let field = named(field, tr!("Weapon name"));
        if field.lost_focus() {
            let new = typed.clone();
            work.renaming = None;
            state.rename_selected_weapon(&new);
        }
    });
    ui.label(
        egui::RichText::new(tr!("Id {id}", id = id.as_str()))
            .small()
            .color(theme::TEXT_SECONDARY),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (severity, line) in &problems {
                // A note says what the game may do and is not a fault.
                let colour = match severity {
                    or_weapons::Severity::Error => theme::ERROR.get(),
                    or_weapons::Severity::Warning => theme::WARNING.get(),
                    or_weapons::Severity::Info => theme::TEXT_SECONDARY.get(),
                };
                ui.colored_label(colour, line.replace('`', ""));
            }
            egui::CollapsingHeader::new(egui::RichText::new(tr!("Made of")).strong())
                .id_salt("weapon_sources")
                .default_open(true)
                .show(ui, |ui| {
                    if sources.is_empty() {
                        ui.label(
                            egui::RichText::new(tr!("The weapon names no source yet"))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    for line in &sources {
                        ui.label(line);
                    }
                });

            let Some(source) = defined_in else {
                ui.label(
                    egui::RichText::new(tr!(
                    "No weapon file of the pack defines this weapon, so there is no value to change"
                ))
                    .color(theme::TEXT_SECONDARY),
                );
                return;
            };
            let mut kept = state.panels.take::<Kept>();
            let key = (
                name.clone(),
                source.clone(),
                std::fs::metadata(source.join(or_source::metadata::XML)).map_or(0, |m| m.len()),
                std::fs::metadata(source.join(or_source::metadata::XML))
                    .and_then(|m| m.modified())
                    .ok(),
            );
            if kept.0.as_ref().is_none_or(|info| info.key != key) {
                kept.0 = Some(Info {
                    value: read(&source, &name),
                    key,
                });
            }
            let mut commit = false;
            if let Some(Info { value, .. }) = &mut kept.0 {
                match value {
                    Err(why) => {
                        ui.colored_label(theme::ERROR.get(), why.as_str());
                    }
                    Ok(info) => {
                        commit |= edit_values(ui, info, editable);
                        commit |= edit_components(ui, info, editable);
                        references(ui, state, info);
                    }
                }
            }
            if commit {
                if let Some(Info {
                    value: Ok(info), ..
                }) = &kept.0
                {
                    let edited = info.clone();
                    let label = tr!("Changed {name}", name = name.as_str());
                    state.edit_selected_weapon(label.to_string(), move |w| *w = edited);
                }
            }
            state.panels.put(kept);
        });
    ui.add_space(theme::SPACING_TIGHT);
    ui.horizontal(|ui| {
        action_button(ui, state, Action::RemoveWeapon, false);
    });
}

/// The least width a row's name keeps beside its control; a narrower panel puts the control on a line of its own.
const NAME_MIN: f32 = 90.0;

/// The width a row's control takes, which the name is given the rest of.
fn width_of(cell: &Cell<'_>, total: f32) -> f32 {
    let spacing = 4.0;
    match cell {
        Cell::Float(_) | Cell::Int(_) => 64.0,
        Cell::Flag(_) => 24.0,
        Cell::Text(_) => (total * 0.55).min(180.0),
        Cell::Vec2(_) => 2.0 * 64.0 + spacing,
        Cell::Vec3(_) => 3.0 * 64.0 + 2.0 * spacing,
    }
}

/// Whether a number has been let go: a drag ended, or a typed value was left by Enter, Tab or a click elsewhere.
///
/// ⚠ A click on a `DragValue` turns it into a text field, whose response never reports `drag_stopped`; leaving the field is `lost_focus`.
/// `changed` is no commit, because the value is set on every key typed.
fn let_go(response: &egui::Response) -> bool {
    response.drag_stopped() || response.lost_focus()
}

/// One row's control, `width` wide where it has a width; true once a change to it is let go.
fn cell(ui: &mut egui::Ui, cell: Cell<'_>, label: &str, width: f32) -> bool {
    let mut done = false;
    match cell {
        Cell::Float(v) => {
            let r = ui.add(egui::DragValue::new(v).speed(0.05));
            done |= let_go(&named(r, label));
        }
        Cell::Int(v) => {
            let r = ui.add(egui::DragValue::new(v).speed(0.2));
            done |= let_go(&named(r, label));
        }
        Cell::Flag(v) => {
            let r = ui.checkbox(v, "");
            done |= named(r, label).changed();
        }
        Cell::Text(v) => {
            let r = ui.add(egui::TextEdit::singleline(v).desired_width(width));
            done |= named(r, label).lost_focus();
        }
        Cell::Vec2(v) => {
            for part in [&mut v.x, &mut v.y] {
                let r = ui.add(egui::DragValue::new(part).speed(0.05));
                done |= let_go(&named(r, label));
            }
        }
        Cell::Vec3(v) => {
            for part in [&mut v.x, &mut v.y, &mut v.z] {
                let r = ui.add(egui::DragValue::new(part).speed(0.05));
                done |= let_go(&named(r, label));
            }
        }
    }
    done
}

/// One row: the name, cut short where the panel is narrow, and its control beside it, or under it when the panel leaves the name too little.
fn control(ui: &mut egui::Ui, row: super::fields::Row<'_>, editable: bool) -> bool {
    let label = tr!("Weapon field: {name}", name = row.name);
    let name = egui::RichText::new(row.name).color(theme::TEXT_SECONDARY);
    let total = ui.available_width();
    let spacing = ui.spacing().item_spacing.x;
    let width = width_of(&row.cell, total).min(total);
    let beside = total - width - spacing >= NAME_MIN;
    let mut done = false;
    ui.add_enabled_ui(editable, |ui| {
        if beside {
            ui.horizontal(|ui| {
                let room = egui::vec2(total - width - spacing, ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(
                    room,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        // ⚠ The name takes its whole room, so that every control starts at one place.
                        ui.set_min_width(room.x);
                        ui.add(egui::Label::new(name).truncate())
                            .on_hover_text(row.name)
                    },
                );
                done |= cell(ui, row.cell, &label, width);
            });
        } else {
            ui.add(egui::Label::new(name).truncate())
                .on_hover_text(row.name);
            ui.horizontal(|ui| done |= cell(ui, row.cell, &label, width));
        }
    });
    done
}

/// Every value the family types, by group; true once a change to one is let go.
fn edit_values(ui: &mut egui::Ui, info: &mut CWeaponInfo, editable: bool) -> bool {
    let mut committed = false;
    let mut buckets: Vec<(Group, Vec<super::fields::Row<'_>>)> =
        Group::ALL.iter().map(|g| (*g, Vec::new())).collect();
    for row in rows(info) {
        if let Some(at) = Group::ALL.iter().position(|g| *g == row.group) {
            buckets[at].1.push(row);
        }
    }
    for (group, held) in buckets {
        if held.is_empty() {
            continue;
        }
        let open = matches!(group, Group::Identity | Group::Damage);
        let title = format!("{}  ·  {}", group.label(), held.len());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("weapon_group", group as u8))
            .default_open(open)
            .show(ui, |ui| {
                for row in held {
                    committed |= control(ui, row, editable);
                }
            });
    }
    committed
}

/// The attachment points and the components that attach at each; true once a change to one is let go.
fn edit_components(ui: &mut egui::Ui, info: &mut CWeaponInfo, editable: bool) -> bool {
    let mut committed = false;
    egui::CollapsingHeader::new(
        egui::RichText::new(tr!("Components and where they attach")).strong(),
    )
    .id_salt("weapon_components")
    .default_open(true)
    .show(ui, |ui| {
        ui.add_enabled_ui(editable, |ui| {
            let mut drop_point: Option<usize> = None;
            for (at, point) in info.attach_points.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tr!("Bone")).color(theme::TEXT_SECONDARY));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut point.attach_bone).desired_width(120.0),
                    );
                    committed |= named(r, tr!("Attachment bone")).lost_focus();
                    let r = ui.button(tr!("Remove attachment point"));
                    if r.clicked() {
                        drop_point = Some(at);
                    }
                });
                let mut drop_component: Option<usize> = None;
                for (inside, component) in point.components.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(theme::SPACING_WIDE);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut component.name).desired_width(200.0),
                        );
                        committed |= named(r, tr!("Component name")).lost_focus();
                        let r = ui.checkbox(&mut component.default, tr!("Worn by default"));
                        committed |= r.changed();
                        if ui.button(tr!("Remove component")).clicked() {
                            drop_component = Some(inside);
                        }
                    });
                }
                if let Some(inside) = drop_component {
                    point.components.remove(inside);
                    committed = true;
                }
                if ui.button(tr!("Add component")).clicked() {
                    let mut component = CWeaponComponentPointSComponent::default();
                    component.name = "COMPONENT_NEW".to_string();
                    point.components.push(component);
                    committed = true;
                }
            }
            if let Some(at) = drop_point {
                info.attach_points.remove(at);
                committed = true;
            }
            if ui.button(tr!("Add attachment point")).clicked() {
                info.attach_points.push(Default::default());
                committed = true;
            }
        });
    });
    committed
}

/// What the weapon names that has a workspace of its own: effects, sounds, and the animations of its name.
fn references(ui: &mut egui::Ui, state: &mut AppState, info: &CWeaponInfo) {
    egui::CollapsingHeader::new(
        egui::RichText::new(tr!("Effects, sounds and animations")).strong(),
    )
    .id_salt("weapon_references")
    .default_open(false)
    .show(ui, |ui| {
        let named_effects: Vec<&str> = [
            info.fx.flash_fx.as_str(),
            info.fx.flash_fx_fp.as_str(),
            info.fx.muzzle_smoke_fx.as_str(),
            info.fx.shell_fx.as_str(),
            info.fx.tracer_fx.as_str(),
        ]
        .into_iter()
        .filter(|n| !n.is_empty())
        .collect();
        let sounds: Vec<&str> = [info.audio.as_str(), info.audio_collision_hash.as_str()]
            .into_iter()
            .filter(|n| !n.is_empty())
            .collect();
        for (label, names, button, workspace) in [
            (
                tr!("Effects"),
                named_effects,
                tr!("Open the effects in Particle"),
                Workspace::Particle,
            ),
            (
                tr!("Sounds"),
                sounds,
                tr!("Open the sounds in Data"),
                Workspace::Data,
            ),
            (
                tr!("Animations"),
                vec![info.name.as_str()],
                tr!("Open the animations in Animation"),
                Workspace::Animation,
            ),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_SECONDARY));
                ui.label(names.join(", "));
                if ui.button(button).clicked() {
                    state.active_workspace = workspace;
                }
            });
        }
    });
}

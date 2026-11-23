//! The map sets of the pack: the sources of the set shown, and under each map file its entities and under each type file its archetypes,
//! each row one click target, with what needs attention beside it.

use or_maps::{Role, Severity};

use super::{action_button, shown, step_button, MapsUi, STEP_BUTTON};
use crate::state::front_door::counted;
use crate::state::workbench::{Pick, PickKind};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::commands::Action;
use crate::ui::theme;

/// The height of a row of the list.
const ROW: f32 = 20.0;

/// One line of the list.
enum Row {
    /// A source of the set: a map file, a type file, a model or a texture dictionary.
    Source {
        path: String,
        label: String,
        kind: &'static str,
        /// Whether its entities or archetypes are folded away; none for a source that holds none.
        folded: Option<bool>,
        /// The worst thing wrong with the file that has not been set aside, with the first sentence that says it.
        trouble: Option<(Severity, String)>,
    },
    Entity {
        source: String,
        index: usize,
        text: String,
        interior: bool,
    },
    Archetype {
        source: String,
        index: usize,
        text: String,
        rooms: usize,
        portals: usize,
    },
}

fn kind_word(role: Option<Role>) -> &'static str {
    match role {
        Some(Role::Map) => tr!("map file"),
        Some(Role::Types) => tr!("type file"),
        Some(Role::Model) => tr!("model"),
        Some(Role::Textures) => tr!("textures"),
        None => tr!("source"),
    }
}

/// The rows of the shown set: its sources in the order the set names them, with the entities and archetypes the filter lets through under the ones that are not folded.
fn rows(state: &AppState, filter: &str) -> Option<Result<Vec<Row>, String>> {
    let pack = state.maps_pack()?;
    let read = match &pack.read {
        Ok(read) => read,
        Err(why) => return Some(Err(why.clone())),
    };
    let Some((_, set)) = state.shown_set() else {
        return Some(Ok(Vec::new()));
    };
    let filter = filter.trim().to_lowercase();
    let wanted = |text: &str| filter.is_empty() || text.to_lowercase().contains(&filter);
    let folded = &state.workbench.maps.folded;
    let mut out = Vec::new();
    for path in &set.sources {
        let Some(source) = read.facts.source(path) else {
            out.push(Row::Source {
                path: path.clone(),
                label: path.clone(),
                kind: tr!("source"),
                folded: None,
                trouble: read
                    .findings
                    .iter()
                    .find(|f| f.place.source.as_deref() == Some(path) && f.waived.is_none())
                    .map(|f| (f.severity, f.message.clone())),
            });
            continue;
        };
        let trouble = read
            .findings
            .iter()
            .filter(|f| f.place.source.as_deref() == Some(path.as_str()) && f.waived.is_none())
            .filter(|f| f.severity != Severity::Info)
            .max_by_key(|f| f.severity)
            .map(|f| (f.severity, f.message.clone()));
        let is_folded = folded.contains(path);
        let holds = source.map().is_some() || source.types().is_some();
        out.push(Row::Source {
            path: path.clone(),
            label: source.file_name(),
            kind: kind_word(source.role()),
            folded: holds.then_some(is_folded),
            trouble,
        });
        if is_folded {
            continue;
        }
        if let Some(map) = source.map() {
            for (index, entity) in map.entities.iter().enumerate() {
                let name = shown(state, &entity.archetype);
                let text = format!(
                    "#{index}  {name}  ({:.1}, {:.1}, {:.1})",
                    entity.position[0], entity.position[1], entity.position[2]
                );
                if wanted(&text) {
                    out.push(Row::Entity {
                        source: path.clone(),
                        index,
                        text,
                        interior: entity.interior,
                    });
                }
            }
        }
        if let Some(types) = source.types() {
            for (index, archetype) in types.archetypes.iter().enumerate() {
                let text = format!("#{index}  {}", shown(state, &archetype.name));
                if wanted(&text) {
                    out.push(Row::Archetype {
                        source: path.clone(),
                        index,
                        text,
                        rooms: archetype.interior.as_ref().map_or(0, |i| i.rooms.len()),
                        portals: archetype.interior.as_ref().map_or(0, |i| i.portals.len()),
                    });
                }
            }
        }
    }
    Some(Ok(out))
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if state.scanned_map().is_some() {
        return super::scanned::draw_list(ui, state);
    }
    if !state.has_maps() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    let mut shown_ui = state.panels.take::<MapsUi>();
    shown_ui.drop = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    header(ui, state);
    let filter = state.workbench.maps.filter.clone();
    match rows(state, &filter) {
        None => {
            ui.label(egui::RichText::new(tr!("Reading the maps…")).color(theme::TEXT_SECONDARY));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        Some(Err(why)) => {
            ui.colored_label(theme::ERROR.get(), why);
        }
        Some(Ok(rows)) if rows.is_empty() => {
            ui.add_space(theme::SPACING_DEFAULT);
            ui.label(
                egui::RichText::new(tr!(
                    "No map files in this pack yet: drop .ymap and .ytyp files here, or use Add map files…"
                ))
                .color(theme::TEXT_SECONDARY),
            );
        }
        Some(Ok(rows)) => body(ui, state, &rows),
    }
    ui.add_space(theme::SPACING_DEFAULT);
    state.panels.put(shown_ui);
}

/// The commands of the Map screen, in the bar under the tabs while the screen is in front: what adds map files, an entity and an archetype.
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_maps() {
        return;
    }
    action_button(ui, state, Action::AddMaps, true);
    action_button(ui, state, Action::AddEntity, false);
    action_button(ui, state, Action::AddArchetype, false);
}

/// The header: the set shown, and under it the filter with Undo and Redo at its end.
fn header(ui: &mut egui::Ui, state: &mut AppState) {
    set_picker(ui, state);
    ui.horizontal(|ui| {
        // ⚠ The filter leaves room for the two buttons beside it, so that neither wraps to a line of its own in a narrow panel.
        let room = 2.0 * (STEP_BUTTON + ui.spacing().item_spacing.x);
        let filter = &mut state.workbench.maps.filter;
        let field = ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text(tr!("Filter entities and archetypes"))
                .desired_width((ui.available_width() - room).max(60.0)),
        );
        named(field, tr!("Filter map entities and archetypes"));
        step_button(
            ui,
            state,
            Action::UndoMaps,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        );
        step_button(
            ui,
            state,
            Action::RedoMaps,
            egui_phosphor::regular::ARROW_CLOCKWISE,
        );
    });
    ui.add_space(theme::SPACING_HAIR);
}

/// Which set is shown, when the pack holds more than one.
fn set_picker(ui: &mut egui::Ui, state: &mut AppState) {
    let Some((pack, shown_set)) = state.shown_set() else {
        return;
    };
    let Ok(read) = pack.read.as_ref() else {
        return;
    };
    let sets: Vec<(String, String)> = read
        .model
        .maps
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    let module = pack.module.clone();
    let current = shown_set.id.clone();
    if sets.len() < 2 {
        return;
    }
    let mut chosen: Option<String> = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("Set")).color(theme::TEXT_SECONDARY));
        let name = sets
            .iter()
            .find(|(id, _)| *id == current)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let combo = egui::ComboBox::from_id_salt("map_set")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (id, name) in &sets {
                    if ui.selectable_label(*id == current, name).clicked() {
                        chosen = Some(id.clone());
                    }
                }
            });
        named(combo.response, tr!("Map set shown"));
    });
    if let Some(id) = chosen {
        state.select_in_maps(&module, &id, None);
        state.workbench.maps.needs_frame = true;
        state.workbench.maps.plan.fitted = false;
    }
}

fn body(ui: &mut egui::Ui, state: &mut AppState, rows: &[Row]) {
    let selected = state.workbench.maps.selected.clone();
    let module = state
        .maps_pack()
        .map(|p| p.module.clone())
        .unwrap_or_default();
    let set = state
        .shown_set()
        .map(|(_, set)| set.id.clone())
        .unwrap_or_default();
    let mut chosen: Option<Pick> = None;
    let mut toggled: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW, rows.len(), |ui, range| {
            for row in &rows[range] {
                match row {
                    Row::Source {
                        path,
                        label,
                        kind,
                        folded,
                        trouble,
                    } => {
                        ui.horizontal(|ui| {
                            let arrow = match folded {
                                Some(true) => egui_phosphor::regular::CARET_RIGHT,
                                Some(false) => egui_phosphor::regular::CARET_DOWN,
                                None => "",
                            };
                            let title = format!("{arrow} {label}  ·  {kind}");
                            let response =
                                ui.selectable_label(false, egui::RichText::new(title).strong());
                            let response = named(response, &format!("{label}, {kind}"));
                            if response.clicked() && folded.is_some() {
                                toggled = Some(path.clone());
                            }
                            if let Some((severity, why)) = trouble {
                                warning(ui, *severity, why);
                            }
                        });
                    }
                    Row::Entity {
                        source,
                        index,
                        text,
                        interior,
                    } => {
                        ui.horizontal(|ui| {
                            ui.add_space(theme::SPACING_WIDE);
                            let on = selected.as_ref().is_some_and(|p| {
                                p.source == *source && p.kind == PickKind::Entity(*index)
                            });
                            let response = ui.selectable_label(on, text);
                            let response = named(response, &tr!("Entity {text}", text = text));
                            if response.clicked() {
                                chosen = Some(Pick {
                                    source: source.clone(),
                                    kind: PickKind::Entity(*index),
                                });
                            }
                            if *interior {
                                ui.label(
                                    egui::RichText::new(tr!("interior"))
                                        .small()
                                        .color(theme::TEXT_SECONDARY),
                                );
                            }
                        });
                    }
                    Row::Archetype {
                        source,
                        index,
                        text,
                        rooms,
                        portals,
                    } => {
                        ui.horizontal(|ui| {
                            ui.add_space(theme::SPACING_WIDE);
                            let on = selected.as_ref().is_some_and(|p| {
                                p.source == *source && p.kind == PickKind::Archetype(*index)
                            });
                            let response = ui.selectable_label(on, text);
                            let response = named(response, &tr!("Archetype {text}", text = text));
                            if response.clicked() {
                                chosen = Some(Pick {
                                    source: source.clone(),
                                    kind: PickKind::Archetype(*index),
                                });
                            }
                            if *rooms > 0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}, {}",
                                        counted(*rooms, "room"),
                                        counted(*portals, "portal")
                                    ))
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                                );
                            }
                        });
                    }
                }
            }
        });
    if let Some(path) = toggled {
        let folded = &mut state.workbench.maps.folded;
        if !folded.remove(&path) {
            folded.insert(path);
        }
    }
    if let Some(pick) = chosen {
        state.select_in_maps(&module, &set, Some(pick));
    }
}

fn warning(ui: &mut egui::Ui, severity: Severity, why: &str) {
    let colour = match severity {
        Severity::Error => theme::ERROR.get(),
        _ => theme::WARNING.get(),
    };
    ui.label(egui::RichText::new(egui_phosphor::regular::WARNING).color(colour))
        .on_hover_text(why);
}

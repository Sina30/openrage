//! The weapons of the pack, grouped by the group their metadata states, each row one click target, with what needs attention beside it.

use std::collections::BTreeMap;

use or_weapons::Severity;

use super::{action_button, step_button, WeaponsUi, STEP_BUTTON};
use crate::state::front_door::counted;
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::commands::Action;
use crate::ui::theme;

/// One line of the list.
struct Row {
    id: String,
    name: String,
    group: String,
    components: usize,
    /// The worst thing wrong with the weapon that has not been set aside, with the first sentence that says it.
    trouble: Option<(Severity, String)>,
}

/// The rows of the pack: every weapon the filter lets through, in the order the pack states them.
fn rows(state: &AppState, filter: &str) -> Option<Result<Vec<Row>, String>> {
    let pack = state.weapons_pack()?;
    let read = match &pack.read {
        Ok(read) => read,
        Err(why) => return Some(Err(why.clone())),
    };
    let filter = filter.trim().to_lowercase();
    Some(Ok(read
        .model
        .weapons
        .iter()
        .filter(|w| filter.is_empty() || w.name.to_lowercase().contains(&filter))
        .map(|weapon| {
            let defined = or_weapons::rules::defined_by(weapon, &read.facts);
            let trouble = read
                .findings
                .iter()
                .filter(|f| f.weapon.as_deref() == Some(weapon.id.as_str()) && f.waived.is_none())
                .filter(|f| f.severity != Severity::Info)
                .max_by_key(|f| f.severity)
                .map(|f| (f.severity, f.message.clone()));
            Row {
                id: weapon.id.clone(),
                name: weapon.label.clone().unwrap_or_else(|| weapon.name.clone()),
                group: defined.map(|d| d.group.clone()).unwrap_or_default(),
                components: defined.map_or(0, |d| {
                    d.attach_points.iter().map(|p| p.components.len()).sum()
                }),
                trouble,
            }
        })
        .collect()))
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_weapons() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    let mut shown = state.panels.take::<WeaponsUi>();
    shown.drop = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    header(ui, state);
    let filter = state.workbench.weapons.filter.clone();
    match rows(state, &filter) {
        None => {
            ui.label(egui::RichText::new(tr!("Reading the weapons…")).color(theme::TEXT_SECONDARY));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        Some(Err(why)) => {
            ui.colored_label(theme::ERROR.get(), why);
        }
        Some(Ok(rows)) if rows.is_empty() => {
            ui.add_space(theme::SPACING_DEFAULT);
            let said = match filter.trim().is_empty() {
                true => tr!(
                    "No weapons in this pack yet: drop weapon models, textures and weapon files here, or use Add weapons…"
                ),
                false => tr!("No weapon is called like that"),
            };
            ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
        }
        Some(Ok(rows)) => body(ui, state, &rows),
    }
    ui.add_space(theme::SPACING_DEFAULT);
    state.panels.put(shown);
}

/// The commands of the Weapons screen, in the bar under the tabs while the screen is in front: what adds weapons, and what makes one.
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_weapons() {
        return;
    }
    action_button(ui, state, Action::AddWeapons, true);
    action_button(ui, state, Action::NewWeapon, false);
}

/// The header: the filter, with Undo and Redo at its end.
fn header(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // ⚠ The filter leaves room for the two buttons beside it, so that neither wraps to a line of its own in a narrow panel.
        let room = 2.0 * (STEP_BUTTON + ui.spacing().item_spacing.x);
        let filter = &mut state.workbench.weapons.filter;
        let field = ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text(tr!("Filter weapons by name"))
                .desired_width((ui.available_width() - room).max(60.0)),
        );
        named(field, tr!("Filter weapons"));
        step_button(
            ui,
            state,
            Action::UndoWeapons,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        );
        step_button(
            ui,
            state,
            Action::RedoWeapons,
            egui_phosphor::regular::ARROW_CLOCKWISE,
        );
    });
    ui.add_space(theme::SPACING_HAIR);
}

fn body(ui: &mut egui::Ui, state: &mut AppState, rows: &[Row]) {
    let mut groups: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        groups.entry(row.group.as_str()).or_default().push(row);
    }
    let selected = state.workbench.weapons.selected.clone();
    let module = state
        .weapons_pack()
        .map(|p| p.module.clone())
        .unwrap_or_default();
    let mut chosen: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (group, held) in &groups {
                let heading = match group.is_empty() {
                    true => tr!("No group").to_string(),
                    false => (*group).to_string(),
                };
                let title = format!("{heading}  ·  {}", counted(held.len(), "weapon"));
                egui::CollapsingHeader::new(egui::RichText::new(title).strong())
                    .id_salt(("weapons_group", *group))
                    .default_open(true)
                    .show(ui, |ui| {
                        for row in held {
                            let on = selected.as_deref() == Some(row.id.as_str());
                            ui.horizontal(|ui| {
                                let response = ui.selectable_label(on, &row.name);
                                let response = named(response, &row.name);
                                if response.clicked() {
                                    chosen = Some(row.id.clone());
                                }
                                if row.components > 0 {
                                    ui.label(
                                        egui::RichText::new(counted(row.components, "component"))
                                            .small()
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                }
                                if let Some((severity, why)) = &row.trouble {
                                    let colour = match severity {
                                        Severity::Error => theme::ERROR.get(),
                                        _ => theme::WARNING.get(),
                                    };
                                    ui.label(
                                        egui::RichText::new(egui_phosphor::regular::WARNING)
                                            .color(colour),
                                    )
                                    .on_hover_text(why);
                                }
                            });
                        }
                    });
            }
        });
    if let Some(id) = chosen {
        state.select_weapon(&module, Some(id));
    }
}

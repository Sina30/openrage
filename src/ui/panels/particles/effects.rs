//! The effects of the pack, one row each, grouped by the particle file that holds them, with what needs attention beside the file.

use or_particles::Severity;

use super::ParticlesUi;
use crate::state::front_door::counted;
use crate::state::workbench::EffectRow;
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::commands::{self, Action};
use crate::ui::panels::selection_ids::ypt_effect_id;
use crate::ui::theme;

/// The worst thing wrong with the file `source` that has not been set aside, with the first sentence that says it.
fn trouble(state: &AppState, source: &str) -> Option<(Severity, String)> {
    let read = state.particles_pack()?.read.as_ref().ok()?;
    let ids: Vec<&str> = read
        .model
        .assets
        .iter()
        .filter(|a| a.source == source)
        .map(|a| a.id.as_str())
        .collect();
    read.findings
        .iter()
        .filter(|f| f.waived.is_none() && f.severity != Severity::Info)
        .filter(|f| f.asset.as_deref().is_some_and(|id| ids.contains(&id)))
        .max_by_key(|f| f.severity)
        .map(|f| (f.severity, f.message.clone()))
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_particles() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    // ⚠ A document the effect wanted open may have arrived since the last frame.
    state.apply_wanted_effect();
    let mut shown = state.panels.take::<ParticlesUi>();
    shown.drop = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    match state.particles_pack().map(|pack| pack.read.clone()) {
        None => {
            ui.label(egui::RichText::new(tr!("Reading the effects…")).color(theme::TEXT_SECONDARY));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        Some(Err(why)) => {
            ui.colored_label(theme::ERROR.get(), why);
        }
        Some(Ok(pack)) if pack.model.assets.is_empty() => empty(ui, state),
        Some(Ok(pack)) => {
            header(ui, state);
            let filter = state.workbench.particles.filter.clone();
            let rows: Vec<EffectRow> = pack
                .effects()
                .into_iter()
                .filter(|row| {
                    filter.trim().is_empty()
                        || row
                            .effect
                            .to_lowercase()
                            .contains(&filter.trim().to_lowercase())
                })
                .collect();
            if rows.is_empty() {
                ui.add_space(theme::SPACING_DEFAULT);
                let said = match filter.trim().is_empty() {
                    true => tr!("The particle files of this pack hold no effect yet"),
                    false => tr!("No effect is called like that"),
                };
                ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
            } else {
                body(ui, state, &rows);
            }
        }
    }
    ui.add_space(theme::SPACING_DEFAULT);
    state.panels.put(shown);
}

/// What the screen says while the pack holds no particle file, and the one action that gives it some.
fn empty(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new(tr!("No particle effects in this project"))
            .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(theme::SPACING_DEFAULT);
    if let Some(command) = commands::of(state, Action::AddParticles) {
        let response = ui.add_enabled(
            command.enabled.is_ok(),
            egui::Button::new(command.label.clone()),
        );
        if commands::hints(response, &command).clicked() {
            commands::run(state, ui.ctx(), &command);
        }
    }
}

/// The header: what adds a list of effects, and the filter.
fn header(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        for (action, filled) in [(Action::AddParticles, true), (Action::NewParticles, false)] {
            let Some(command) = commands::of(state, action) else {
                continue;
            };
            let text = command.label.clone();
            let button = if filled {
                egui::Button::new(egui::RichText::new(text).color(theme::ON_ACCENT.get()))
                    .fill(theme::ACCENT.get())
            } else {
                egui::Button::new(text)
            };
            let response = ui.add_enabled(command.enabled.is_ok(), button);
            if commands::hints(response, &command).clicked() {
                commands::run(state, ui.ctx(), &command);
            }
        }
    });
    ui.add_space(theme::SPACING_HAIR);
    let filter = &mut state.workbench.particles.filter;
    let field = ui.add(
        egui::TextEdit::singleline(filter)
            .hint_text(tr!("Filter effects by name"))
            .desired_width(ui.available_width().max(60.0)),
    );
    named(field, tr!("Filter effects"));
    ui.add_space(theme::SPACING_HAIR);
}

fn body(ui: &mut egui::Ui, state: &mut AppState, rows: &[EffectRow]) {
    let module = state
        .particles_pack()
        .map(|p| p.module.clone())
        .unwrap_or_default();
    let mut files: Vec<(&str, &str, Vec<&EffectRow>)> = Vec::new();
    for row in rows {
        match files.iter_mut().find(|(source, ..)| *source == row.source) {
            Some((.., held)) => held.push(row),
            None => files.push((&row.source, &row.file, vec![row])),
        }
    }
    let mut chosen: Option<(String, usize)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (source, file, held) in &files {
                let shown = state.effect_shown_from(source);
                let title = format!("{file}  ·  {}", counted(held.len(), "effect"));
                egui::CollapsingHeader::new(egui::RichText::new(title).strong())
                    .id_salt(("particles_file", *source))
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some((severity, why)) = trouble(state, source) {
                            let colour = match severity {
                                Severity::Error => theme::ERROR.get(),
                                _ => theme::WARNING.get(),
                            };
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {why}",
                                    egui_phosphor::regular::WARNING
                                ))
                                .small()
                                .color(colour),
                            );
                        }
                        for row in held {
                            let on = shown == Some(ypt_effect_id(row.place));
                            let response = ui.selectable_label(on, &row.effect);
                            let response = named(response, &row.effect);
                            if response.clicked() {
                                chosen = Some((row.source.clone(), row.place));
                            }
                        }
                    });
            }
        });
    if let Some((source, place)) = chosen {
        state.show_effect(&module, &source, place);
    }
}

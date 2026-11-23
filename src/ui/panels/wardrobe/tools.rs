//! The tools of the dressed ped that change how it is drawn and what plays on it: the way of drawing, the level of detail, the tint, the clip it plays and the pictures of the pack; the ped's own parts are `parts.rs`'s and the light `light.rs`'s.
//!
//! ⚠ **Thin by design.** What is worn, which level is drawn and what a clip does to the skeleton are
//! the state's; this lays the controls out and hands what they asked for back to it.

use std::path::PathBuf;

use egui_phosphor::regular as icon;

use super::kit::{combo, combo_with, menu, tip};
use crate::platform::file_dialogs;
use crate::render::dressed::{level_name, LEVELS};
use crate::state::workbench::{ViewMode, PRESETS, TINTS, TURNS};
use crate::state::{AppState, GamePackStatus};
use crate::ui::theme;

/// The two rows under the ones that say who wears what: how it is drawn, then the parts, the motion and the pictures.
pub(super) fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    // A control is one line whatever the width: a row wraps between controls, not inside one.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    ui.horizontal_wrapped(|ui| {
        view_mode(ui, state);
        level(ui, state);
        tint(ui, state);
    });
    ui.horizontal_wrapped(|ui| {
        super::parts::hide_hair(ui, state);
        super::parts::own_parts(ui, state);
        animation(ui, state);
        pictures(ui, state);
    });
}

pub(super) fn small(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    )
}

/// A choice of the Draw menu: its name, and under it in small type what it draws.
fn choice_with_note(name: &str, note: &str) -> egui::text::LayoutJob {
    let (body, small) = (
        egui::FontId::proportional(14.0),
        egui::FontId::proportional(11.0),
    );
    let plain = |font_id| egui::text::TextFormat {
        font_id,
        color: egui::Color32::PLACEHOLDER,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, plain(body));
    job.append(&format!("\n{note}"), 0.0, plain(small));
    job
}

/// Why Shaded cannot be drawn yet, in a sentence, and what to do about it.
fn why_not_shaded(status: &GamePackStatus) -> String {
    match status {
        GamePackStatus::Missing => {
            tr!("The game's shaders are not baked yet: bake them in Automate ▸ Bake game shaders")
                .to_string()
        }
        other => other.short_label(),
    }
}

fn view_mode(ui: &mut egui::Ui, state: &mut AppState) {
    // ⚠ Shaded draws as Material until the game's shaders are baked, so it is not offered before: a choice that changes nothing is worse than none.
    let status = state.game_pack.status().clone();
    let baked = matches!(status, GamePackStatus::Ready { .. });
    let dressed = &mut state.workbench.wardrobe.dressed;
    // Two lines a choice, eight choices: taller than a list of one-line words, which would scroll.
    combo_with(
        ui,
        tr!("Draw"),
        dressed.view.name(),
        true,
        Some(340.0),
        |ui| {
            for mode in ViewMode::ALL {
                let offered = baked || mode != ViewMode::Shaded || dressed.view == mode;
                let chosen = dressed.view == mode;
                let row = ui.add_enabled(
                    offered,
                    egui::Button::selectable(chosen, choice_with_note(mode.name(), mode.note())),
                );
                row.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::RadioButton,
                        offered,
                        chosen,
                        mode.name(),
                    )
                });
                if row
                    .on_hover_text(mode.tip())
                    .on_disabled_hover_text(why_not_shaded(&status))
                    .clicked()
                {
                    dressed.view = mode;
                }
            }
        },
    )
    .on_hover_text(tr!(
        "How the 3D view draws the ped: with textures, as a shape, as edges, or as the vertex data"
    ));
}

/// The level of detail to draw, with what each costs for the garments worn.
fn level(ui: &mut egui::Ui, state: &mut AppState) {
    let dressed = &mut state.workbench.wardrobe.dressed;
    let (levels, held, worn) = match dressed.scene() {
        Some(scene) => (scene.levels, scene.held, scene.garments()),
        None => (Default::default(), Default::default(), 0),
    };
    let chosen = dressed.lod.min(LEVELS - 1);
    combo(ui, tr!("Level"), level_name(chosen), |ui| {
        for at in 0..LEVELS {
            let name = level_name(at);
            let cost = levels[at];
            let has = held[at] > 0;
            let words = match (has, held[at] < worn) {
                (false, _) => tr!("{name}: no garment worn has it", name = name),
                (true, false) => tr!("{name}: {triangles} triangles, {vertices} vertices", name = name, triangles = cost.triangles, vertices = cost.vertices),
                (true, true) => tr!(
                    "{name}: {triangles} triangles, {vertices} vertices; {held} of {worn} garments have it",
                    name = name,
                    triangles = cost.triangles,
                    vertices = cost.vertices,
                    held = held[at],
                    worn = worn
                ),
            };
            // ⚠ The first level stays offered whatever is worn: it is where every garment starts.
            let picked = ui
                .add_enabled_ui(has || at == 0, |ui| ui.selectable_label(chosen == at, words))
                .inner;
            if picked.clicked() {
                dressed.lod = at;
            }
        }
    })
    .on_hover_text(tr!("The level of detail of the garments worn; a garment with no model at a level is drawn at its first"));
}

fn tint(ui: &mut egui::Ui, state: &mut AppState) {
    let dressed = &mut state.workbench.wardrobe.dressed;
    let chosen = crate::i18n::tr(TINTS[dressed.tint.min(TINTS.len() - 1)].0);
    let response = combo(ui, tr!("Tint"), chosen, |ui| {
        for (i, (name, _)) in TINTS.iter().enumerate() {
            ui.selectable_value(&mut dressed.tint, i, crate::i18n::tr(name));
        }
    });
    tip(
        response,
        tr!("A colour that multiplies the garments' textures in this view; it is not written to the pack"),
    );
}

/// What the animation controls asked for.
pub(super) enum Play {
    Preset(usize),
    File(PathBuf),
    Clip(u32),
    Rest,
}

/// Carry out what the animation controls asked for.
pub(super) fn carry_out(state: &mut AppState, asked: Option<Play>) {
    let ped = state.dressed_ped();
    match asked {
        Some(Play::Preset(at)) => state.play_preset(at),
        Some(Play::File(path)) => state.play_clip_file(path),
        Some(Play::Clip(key)) => state.workbench.wardrobe.dressed.choose_clip(ped, key),
        Some(Play::Rest) => state.workbench.wardrobe.dressed.stop_clip(),
        None => {}
    }
}

/// The Motion menu: a clip of the game's default movement or of a file, with the one playing ticked.
fn animation(ui: &mut egui::Ui, state: &mut AppState) {
    let mut asked = None;
    let dressed = &state.workbench.wardrobe.dressed;
    let playing = dressed.motion.as_ref().map(|m| m.label.clone());
    let at_rest = playing.is_none() && !dressed.is_animating();
    let opened = menu(ui, tr!("Motion"), Some(icon::PERSON_SIMPLE_WALK), |ui| {
        for (at, preset) in PRESETS.iter().enumerate() {
            let label = crate::i18n::tr(preset.label);
            let now = playing.as_deref() == Some(label);
            if ui.selectable_label(now, label).clicked() {
                asked = Some(Play::Preset(at));
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button(tr!("Clip file…"))
            .on_hover_text(tr!("Play the first clip of a .ycd file on the ped"))
            .clicked()
        {
            let picked = file_dialogs::ask(tr!("Play a clip dictionary"), |dialog| {
                dialog
                    .add_filter(tr!("Clip dictionary"), &["ycd"])
                    .pick_file()
            });
            asked = picked.map(Play::File);
            ui.close();
        }
        if ui
            .selectable_label(at_rest, tr!("Rest pose"))
            .on_hover_text(tr!("The skeleton's own pose, with no clip playing"))
            .clicked()
        {
            asked = Some(Play::Rest);
            ui.close();
        }
    });
    opened.response.on_hover_text(tr!(
        "Play a clip of the game's default movement, or one from a file, on the ped's skeleton"
    ));
    carry_out(state, asked);
}

/// The catalogue: one picture of each garment of the pack, or several turned round it.
fn pictures(ui: &mut egui::Ui, state: &mut AppState) {
    let mut start = None;
    let mut end = false;
    match &state.workbench.wardrobe.dressed.catalogue {
        None => {
            menu(ui, tr!("Pictures"), Some(icon::CAMERA), |ui| {
                for turns in TURNS {
                    let words = match turns {
                        1 => tr!("One picture of each garment").to_string(),
                        n => tr!("{n} pictures round each garment", n = n),
                    };
                    if ui.button(words).clicked() {
                        start = Some(turns);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(tr!("Draw the garments of the pack, each alone as the preview wears it, into a folder you pick: WebP pictures on a clear ground, the same size and shading for each"));
        }
        Some(job) => {
            if job.is_finished() {
                let said = match job.failed.len() {
                    0 => tr!(
                        "{written} pictures in {out}, {turns} of each garment",
                        written = job.written,
                        out = job.out.display(),
                        turns = job.turns
                    ),
                    n => tr!(
                        "{written} pictures, {n} not drawn",
                        n = n,
                        written = job.written
                    ),
                };
                small(ui, said).on_hover_text(job.failed.join("\n"));
                end = ui.small_button(tr!("Done")).clicked();
            } else {
                ui.add(
                    egui::ProgressBar::new(job.done as f32 / job.total.max(1) as f32)
                        .desired_width(120.0)
                        .text(format!("{} of {}", job.done, job.total)),
                );
                end = ui
                    .small_button(tr!("Stop"))
                    .on_hover_text(tr!("Stop drawing; the pictures made stay in the folder"))
                    .clicked();
            }
        }
    }
    if let Some(turns) = start {
        if let Some(folder) = file_dialogs::pick_folder(tr!("Write the pictures into")) {
            if let Err(why) = state.start_catalogue(folder, turns) {
                state.show_info_toast(&why);
            }
        }
    }
    if end {
        state.end_catalogue();
    }
}

//! The light the dressed ped is seen in, and the model a garment with alternates is worn with: two controls of the preview's tools.
//!
//! ⚠ Thin by design. The light is `Dressed::light`, which the viewport reads each frame, and the hour is the viewport's own time of day;
//! the model is `Dressed::alternate`, which `or_wardrobe::dress` turns into the file a garment wears.

use egui_phosphor::regular as icon;

use super::kit::{combo, framed, menu};
use crate::state::workbench::light::{Light, STRONGEST};
use crate::state::workbench::ViewMode;
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// The widest a slider of the Light menu is drawn, so that the three line up.
const SLIDER: f32 = 150.0;

/// One row of the studio light: its name, a slider a screen reader speaks by `name`, and the number it stands at.
fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    (label, name, suffix): (&str, &str, &str),
) {
    ui.label(label);
    // ⚠ Without its own number: the box a slider shows beside itself is a control with no name to speak.
    let response = ui.add(egui::Slider::new(value, range).show_value(false));
    named(response, name);
    let size = egui::vec2(44.0, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| ui.label(format!("{value:.1}{suffix}")),
    );
    ui.end_row();
}

/// A heading of the Light menu with the views it lights under it.
fn section(ui: &mut egui::Ui, name: &str, lights: &str) {
    ui.label(egui::RichText::new(name).strong());
    ui.label(
        egui::RichText::new(lights)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// The hour of the game's day, which the sun, the sky and the ambient light of the Shaded view follow; true when it was moved.
fn time_of_day(ui: &mut egui::Ui, hour: &mut f32) -> bool {
    let field = ui.add(
        egui::DragValue::new(hour)
            .speed(0.05)
            .range(0.0..=24.0)
            .fixed_decimals(2)
            .suffix("h"),
    );
    named(field.clone(), tr!("Time of day")).on_hover_text(
        tr!("The hour of the game's day: the sun, the sky and the ambient light of the Shaded view follow it, and so does the studio light when it comes from the sun"),
    );
    field.changed()
}

/// The studio light of the plain views: where it comes from and how strongly it lights.
fn studio(ui: &mut egui::Ui, now: &mut Light, hour: &mut f32, hour_moved: &mut bool) {
    section(
        ui,
        tr!("Studio light"),
        tr!("Lights Solid, Material and the other plain views"),
    );
    let own = ui.radio_value(&mut now.sun, false, tr!("A direction I set"));
    named(own, tr!("Light from a direction I set"));
    let sun = ui.radio_value(&mut now.sun, true, tr!("The sun at the time of day"));
    named(sun, tr!("Light from the sun at the time of day")).on_hover_text(
        tr!("Light from where the sun stands at the time of day, which the Time of day of the Shaded bar sets too; with no timecycle read from the install the direction is kept"),
    );
    ui.spacing_mut().slider_width = SLIDER;
    egui::Grid::new("studio_light")
        .num_columns(3)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            // ⚠ Only the one that sets the direction: the sun's own direction cannot be moved by hand.
            if now.sun {
                ui.label(tr!("Time of day"));
                *hour_moved = time_of_day(ui, hour);
                ui.end_row();
            } else {
                slider(
                    ui,
                    &mut now.azimuth,
                    0.0..=360.0,
                    (tr!("Direction"), tr!("Light direction"), "°"),
                );
                slider(
                    ui,
                    &mut now.elevation,
                    -90.0..=90.0,
                    (tr!("Height"), tr!("Light height"), "°"),
                );
            }
            slider(
                ui,
                &mut now.strength,
                0.0..=STRONGEST,
                (tr!("Strength"), tr!("Light strength"), ""),
            );
        });
    let reset = ui
        .add_enabled(!now.is_the_first(), egui::Button::new(tr!("Reset light")))
        .on_hover_text(tr!(
            "Put the light back where the viewport had it before it could be set"
        ))
        .on_disabled_hover_text(tr!("The light has not been changed"));
    if reset.clicked() {
        *now = Light::default();
    }
}

/// The game's daylight of the Shaded view, and the way back to the plain views, which the studio light lights.
fn daylight(
    ui: &mut egui::Ui,
    hour: &mut f32,
    shadows: &mut bool,
    hour_moved: &mut bool,
    to_material: &mut bool,
) {
    section(ui, tr!("Game daylight"), tr!("Lights the Shaded view"));
    ui.horizontal(|ui| {
        ui.label(tr!("Time of day"));
        *hour_moved = time_of_day(ui, hour);
    });
    let check = ui.checkbox(shadows, tr!("Shadows"));
    named(check, tr!("Shadows")).on_hover_text(tr!(
        "Cast the sun's shadow of the ped on itself, as the game's passes draw it"
    ));
    ui.separator();
    ui.label(
        egui::RichText::new(tr!(
            "Solid, Material and the other plain views are lit by a studio light you set"
        ))
        .small()
        .color(theme::TEXT_SECONDARY),
    );
    let switch = ui.button(tr!("Switch Draw to Material")).on_hover_text(tr!(
        "Material draws the ped with the textures of its garments, lit by the studio light"
    ));
    *to_material = switch.clicked();
}

/// The Light menu: the light the view drawn now is lit by. Shaded has the game's daylight and the plain views a studio light, and each is shown only while it lights something.
pub(super) fn light(ui: &mut egui::Ui, state: &mut AppState) {
    let mut hour = state.drivers.time_of_day.hour;
    let dressed = &mut state.workbench.wardrobe.dressed;
    let shaded = dressed.view == ViewMode::Shaded;
    let mut now = dressed.light;
    let (mut hour_moved, mut to_material) = (false, false);
    let opened = menu(ui, tr!("Light"), Some(icon::SUN), |ui| {
        framed(ui);
        ui.set_min_width(280.0);
        match shaded {
            true => daylight(
                ui,
                &mut hour,
                &mut dressed.shadows,
                &mut hour_moved,
                &mut to_material,
            ),
            false => studio(ui, &mut now, &mut hour, &mut hour_moved),
        }
    });
    crate::ui::guide_links::tip(
        opened.response,
        tr!("The light of the dressed ped: the studio light of the plain views, or the game's daylight while Draw is Shaded"),
        crate::ui::guide_links::CHECK_ON_THE_PED,
    );
    dressed.light = now;
    if to_material {
        dressed.view = ViewMode::Material;
    }
    if hour_moved {
        state.drivers.time_of_day.set_hour(hour);
    }
}

/// The garment selected and the files of its alternates, when it has any.
pub(super) fn alternates_of_the_selected(state: &AppState) -> Option<(String, Vec<String>)> {
    state.selected_garment().and_then(|(pack, at)| {
        let g = &pack.read.as_ref().ok()?.wardrobe.garments[at];
        (!g.alternates.is_empty()).then(|| (g.id.clone(), g.alternates.clone()))
    })
}

/// The model the garment selected is worn with, when it has alternates: its own, or one of the numbered ones.
pub(super) fn alternate_model(ui: &mut egui::Ui, state: &mut AppState) {
    let Some((id, files)) = alternates_of_the_selected(state) else {
        return;
    };
    let dressed = &mut state.workbench.wardrobe.dressed;
    // The set of alternate variations that makes this garment wear one of its alternates, and the number it says.
    let driven = (dressed.driven.iter())
        .find(|d| d.target == or_wardrobe::alternate::Asset::garment(&id))
        .map(|d| (d.name.clone(), usize::from(d.alt), d.overridden));
    let chosen_by_hand = dressed.alternate.get(&id).copied();
    let now = chosen_by_hand.unwrap_or(0).min(files.len());
    let words = |n: usize| match n {
        0 => tr!("Main model").to_string(),
        n => tr!("Alternate {n}", n = n),
    };
    // The model the garment is worn with: the one chosen here, else the set's, else its own.
    let shown = match (&driven, chosen_by_hand) {
        (Some((_, alt, false)), None) => *alt,
        _ => now,
    };
    let (mut chosen, mut follow_the_set) = (shown, false);
    combo(ui, tr!("Model"), &words(shown), |ui| {
        if let Some((set, alt, _)) = &driven {
            let row = ui.selectable_label(chosen_by_hand.is_none(), tr!("As the set says: {alt}", alt = words(*alt)));
            follow_the_set = row.on_hover_text(tr!("Wear the model the set {set} says", set = set)).clicked();
        }
        for n in 0..=files.len() {
            let tip = match n {
                0 => tr!("The model of the garment").to_string(),
                n => tr!("{file}: the model the game shows in place of the garment's", file = files[n - 1]),
            };
            ui.selectable_value(&mut chosen, n, words(n)).on_hover_text(tip);
        }
    })
    .on_hover_text(tr!("Which of the garment's models the ped wears: its own, or an alternate the game shows in place of it"));
    if let Some((set, alt, overridden)) = &driven {
        let said = match overridden {
            false => tr!("by the set {set}", set = set),
            true => tr!("the set {set} says alternate {alt}", set = set, alt = alt),
        };
        ui.label(egui::RichText::new(said).small().color(crate::ui::theme::TEXT_SECONDARY))
            .on_hover_text(tr!("A set of alternate variations changes this garment's model while the garments that cause it are worn; a model chosen here is worn in place of the set's"));
    }
    if follow_the_set {
        dressed.alternate.remove(&id);
    } else if chosen != shown {
        match (chosen, &driven) {
            // ⚠ The main model is a choice held against a set, and the plain absence of one where none drives the garment.
            (0, None) => dressed.alternate.remove(&id),
            (n, _) => dressed.alternate.insert(id, n),
        };
    }
}

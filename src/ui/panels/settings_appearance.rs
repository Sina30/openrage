//! The Appearance page: theme, accent, interface scale and font.

use crate::state::AppState;
use crate::ui::theme;

const ACCENT_PRESETS: [(&str, [u8; 3]); 7] = [
    (
        tr_key!("Blue"),
        crate::state::AppearancePrefs::DEFAULT_ACCENT,
    ),
    (tr_key!("Teal"), [0x00, 0x99, 0x88]),
    (tr_key!("Green"), [0x3a, 0x9a, 0x3a]),
    (tr_key!("Amber"), [0xd0, 0x8a, 0x00]),
    (tr_key!("Red"), [0xd0, 0x3a, 0x3a]),
    (tr_key!("Purple"), [0x86, 0x4c, 0xd8]),
    (tr_key!("Pink"), [0xd0, 0x4a, 0x9a]),
];

/// The Language row: System default, then each language in its own words; the choice applies from the next frame.
fn language_choice(ui: &mut egui::Ui, state: &mut AppState) {
    let mut chosen = state.prefs.language.clone();
    let system = crate::i18n::system_default();
    let shown = match &chosen {
        Some(code) => crate::i18n::languages()
            .iter()
            .find(|l| l.code.eq_ignore_ascii_case(code))
            .map_or_else(|| code.clone(), |l| l.name.clone()),
        None => tr!("System default ({language})", language = system.name),
    };
    let combo = egui::ComboBox::from_id_salt("appearance_language")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut chosen,
                None,
                tr!("System default ({language})", language = system.name),
            );
            for language in crate::i18n::languages() {
                ui.selectable_value(
                    &mut chosen,
                    Some(language.code.clone()),
                    language.name.as_str(),
                );
            }
        });
    crate::ui::accessible_name::named(combo.response, tr!("Language")).on_hover_text(tr!(
        "The language of the menus, screens and messages. System default follows the language Windows shows; a language is a file in the languages folder of the configuration."
    ));
    if chosen != state.prefs.language {
        state.set_language(chosen);
    }
}

pub fn draw_appearance_page(ui: &mut egui::Ui, state: &mut AppState) {
    use crate::state::ThemeChoice;

    ui.heading(tr!("Appearance"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "Applies to every window at once and is remembered across launches. The \
             3D viewport keeps its own dark background."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_DEFAULT);

    let mut next = state.prefs.appearance.clone();

    egui::Grid::new("appearance_grid")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label(tr!("Theme"));
            ui.horizontal(|ui| {
                for choice in ThemeChoice::ALL {
                    ui.radio_value(&mut next.theme, choice, choice.label());
                }
            });
            ui.end_row();

            ui.label(tr!("Accent"));
            ui.horizontal(|ui| {
                for (name, rgb) in ACCENT_PRESETS {
                    let colour = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let selected = next.accent == rgb;
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, theme::RADIUS_SMALL, colour);
                    if selected {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            5.0,
                            egui::Stroke::new(2.0_f32, theme::TEXT_PRIMARY),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.on_hover_text(crate::i18n::tr(name)).clicked() {
                        next.accent = rgb;
                    }
                }
                ui.add_space(theme::SPACING_SNUG);
                let any = ui.color_edit_button_srgb(&mut next.accent);
                crate::ui::accessible_name::named(any, tr!("Any accent colour"))
                    .on_hover_text(tr!("Any colour"));
            });
            ui.end_row();

            ui.label(tr!("Interface scale"));
            let scale = egui::ComboBox::from_id_salt("appearance_scale")
                .selected_text(format!("{}%", next.ui_scale_percent))
                .show_ui(ui, |ui| {
                    for step in crate::state::AppearancePrefs::SCALE_STEPS {
                        ui.selectable_value(&mut next.ui_scale_percent, step, format!("{step}%"));
                    }
                });
            crate::ui::accessible_name::named(scale.response, tr!("Interface scale"))
                .on_hover_text(tr!(
                    "On top of the scale Windows reports for this display. 100% is the \
                     display's own."
                ));
            ui.end_row();

            ui.label(tr!("Font"));
            let font = egui::ComboBox::from_id_salt("appearance_font")
                .selected_text(next.font.label())
                .show_ui(ui, |ui| {
                    for font in crate::app::gpu::available_fonts() {
                        let label = font.label();
                        ui.selectable_value(&mut next.font, font, label);
                    }
                });
            crate::ui::accessible_name::named(font.response, tr!("Font")).on_hover_text(tr!(
                "System fonts are listed only if installed. One that goes missing later \
                     falls back to the default, and the Log says so."
            ));
            ui.end_row();

            ui.label(tr!("Motion"));
            ui.checkbox(&mut next.reduce_motion, tr!("Reduce motion"))
                .on_hover_text(tr!(
                    "Sections, switches and scrolling change at once, with no animation."
                ));
            ui.end_row();

            ui.label(tr!("Language"));
            language_choice(ui, state);
            ui.end_row();
        });

    ui.add_space(theme::SPACING_LOOSE);
    if ui
        .add_enabled(
            next != crate::state::AppearancePrefs::default(),
            egui::Button::new(tr!("Restore defaults")),
        )
        .clicked()
    {
        next = crate::state::AppearancePrefs::default();
    }

    let unsaved = egui::Id::new("appearance_unsaved_while_held");
    let held = ui.input(|i| i.pointer.any_down());
    let pending = ui.data(|d| d.get_temp::<bool>(unsaved).unwrap_or(false));
    match save_step(next != state.prefs.appearance, held, pending) {
        SaveStep::Preview => {
            state.preview_appearance(next);
            ui.data_mut(|d| d.insert_temp(unsaved, true));
        }
        SaveStep::Save => {
            state.set_appearance(next);
            ui.data_mut(|d| d.remove::<bool>(unsaved));
        }
        SaveStep::SaveWhatWasPreviewed => {
            state.persist_appearance();
            ui.data_mut(|d| d.remove::<bool>(unsaved));
        }
        SaveStep::Nothing => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveStep {
    Preview,
    Save,
    SaveWhatWasPreviewed,
    Nothing,
}

/// A colour dragged in the accent picker changes every frame, so while the
/// pointer is held a change only restyles, and `prefs.json` is written once
/// it is released.
fn save_step(changed: bool, held: bool, pending: bool) -> SaveStep {
    match (changed, held, pending) {
        (true, true, _) => SaveStep::Preview,
        (true, false, _) => SaveStep::Save,
        (false, false, true) => SaveStep::SaveWhatWasPreviewed,
        _ => SaveStep::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_saves_once_when_it_is_released() {
        // Ten frames of dragging, then the release frame, then idle frames.
        let mut pending = false;
        let mut saves = 0;
        let frames = [(true, true); 10]
            .into_iter()
            .chain([(false, false)])
            .chain([(false, false); 5]);
        for (changed, held) in frames {
            match save_step(changed, held, pending) {
                SaveStep::Preview => pending = true,
                SaveStep::Save | SaveStep::SaveWhatWasPreviewed => {
                    saves += 1;
                    pending = false;
                }
                SaveStep::Nothing => {}
            }
        }
        assert_eq!(saves, 1);
        assert!(!pending);
    }

    fn frame(ctx: &egui::Context, state: &mut AppState, n: usize, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 800.0),
            )),
            time: Some(n as f64 / 30.0),
            events,
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| draw_appearance_page(ui, state));
        if let Some(update) = out.platform_output.accesskit_update {
            let wells: Vec<egui::Rect> = update
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::ColorWell)
                .filter_map(|(_, node)| node.bounds())
                .map(|b| {
                    egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    )
                })
                .collect();
            if let Some(well) = wells.first() {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("test_well"), *well));
            }
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// Drives the real page through real frames and counts the writes: calling
    /// `state.set_appearance(next)` unconditionally writes once per dragged
    /// frame and fails this.
    #[test]
    fn dragging_the_custom_accent_writes_prefs_once_on_release() {
        let mut state = AppState::default();
        state.prefs.appearance = crate::state::AppearancePrefs::default();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut n = 0;
        let mut step = |state: &mut AppState, events: Vec<egui::Event>| {
            frame(&ctx, state, n, events);
            n += 1;
        };
        step(&mut state, vec![]);
        step(&mut state, vec![]);
        let well: egui::Rect = ctx
            .data(|d| d.get_temp(egui::Id::new("test_well")))
            .expect("the page draws the custom accent button");

        let at = well.center();
        step(
            &mut state,
            vec![egui::Event::PointerMoved(at), button(at, true)],
        );
        step(&mut state, vec![button(at, false)]);
        step(&mut state, vec![]);
        let popup = ctx
            .layer_id_at(egui::pos2(at.x, well.bottom() + 40.0))
            .and_then(|layer| ctx.memory(|m| m.area_rect(layer.id)))
            .expect("clicking the button opens the picker below it");

        let before = state.prefs.appearance.accent;
        let writes_before = crate::state::prefs::writes_on_this_thread();
        let start = egui::pos2(
            popup.left() + popup.width() * 0.3,
            popup.bottom() - popup.height() * 0.2,
        );
        step(
            &mut state,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        for i in 1..=10 {
            let p = start + egui::vec2(i as f32 * 6.0, -(i as f32) * 2.0);
            step(&mut state, vec![egui::Event::PointerMoved(p)]);
        }
        assert_ne!(
            state.prefs.appearance.accent, before,
            "the drag moved the accent, or this test measured nothing"
        );
        assert_eq!(
            crate::state::prefs::writes_on_this_thread() - writes_before,
            0,
            "prefs.json was written while the picker was still held"
        );

        let end = start + egui::vec2(60.0, -20.0);
        step(&mut state, vec![button(end, false)]);
        for _ in 0..3 {
            step(&mut state, vec![]);
        }
        assert_eq!(
            crate::state::prefs::writes_on_this_thread() - writes_before,
            1,
            "one drag is one write"
        );
    }

    #[test]
    fn a_click_saves_at_once() {
        // A preset swatch, the scale and the font all change on release.
        assert_eq!(save_step(true, false, false), SaveStep::Save);
        assert_eq!(save_step(false, false, false), SaveStep::Nothing);
        assert_eq!(save_step(false, true, false), SaveStep::Nothing);
    }
}

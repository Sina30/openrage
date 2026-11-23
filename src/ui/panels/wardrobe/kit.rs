//! The small pieces the tools of the dressed ped are drawn with: a tooltip that leads to the guide, a named drop-down, a named menu with its arrow and one choice among a few as one joined bar.

use egui_phosphor::regular as icon;

use crate::ui::accessible_name::named;
use crate::ui::theme;

/// `response` with `text` as its tooltip, ending with the guide section that explains the dressed ped.
pub(super) fn tip(response: egui::Response, text: &str) -> egui::Response {
    crate::ui::guide_links::tip(response, text, crate::ui::guide_links::CHECK_ON_THE_PED)
}

/// The width a drop-down is given: its choice is cut to it, and one with a longer choice grows past it.
const COMBO_WIDTH: f32 = 84.0;

/// The width of `text` on one line in the font of a label.
fn width_of(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_string(), font, egui::Color32::WHITE)
        .size()
        .x
}

/// A drop-down named for what it chooses, which a screen reader speaks; its label stands level with it.
///
/// ⚠ Whole or on the next row of a wrapped row: a drop-down inside its own row never wraps, so one that did not fit ran past the edge of the panel.
pub(super) fn combo(
    ui: &mut egui::Ui,
    name: &str,
    shown: &str,
    items: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    combo_with(ui, name, shown, true, None, items)
}

/// [`combo`] that can be greyed (its response says it is disabled, and the caller gives the reason) and whose list is as tall as `height` before it scrolls.
pub(super) fn combo_with(
    ui: &mut egui::Ui,
    name: &str,
    shown: &str,
    enabled: bool,
    height: Option<f32>,
    items: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // The arrow and the margins of the button come on top of the choice it shows.
    let button = COMBO_WIDTH.max(width_of(ui, shown) + 30.0);
    let needs = width_of(ui, name) + button + ui.spacing().item_spacing.x * 2.0;
    if ui.available_width() < needs {
        ui.end_row();
    }
    let combo = ui
        .horizontal(|ui| {
            ui.label(name);
            ui.add_enabled_ui(enabled, |ui| {
                egui::ComboBox::from_id_salt(("dressed", name))
                    .width(COMBO_WIDTH)
                    .height(height.unwrap_or(ui.spacing().combo_height))
                    .selected_text(shown)
                    .show_ui(ui, items)
            })
            .inner
        })
        .inner;
    combo
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, name));
    combo.response
}

/// A button that opens a menu, drawn with the arrow that says so and named for a screen reader without it.
///
/// ⚠ Not inside `add_enabled_ui`: a scope of its own does not know where a wrapped row breaks.
pub(super) fn menu<R>(
    ui: &mut egui::Ui,
    name: &str,
    glyph: Option<&str>,
    items: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let text = match glyph {
        Some(glyph) => format!("{glyph} {name} {}", icon::CARET_DOWN),
        None => format!("{name} {}", icon::CARET_DOWN),
    };
    let opened = ui.menu_button(text, items);
    named(opened.response.clone(), name);
    opened
}

/// A menu that opens with its controls drawn as controls: a menu draws its widgets with no frame, which makes a button, a tick box or a slider read as text.
pub(super) fn framed(ui: &mut egui::Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = widgets.inactive.bg_fill;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, theme::outline());
}

/// One part of a [`segments`] bar.
pub(super) struct Part<'a> {
    pub text: &'a str,
    pub chosen: bool,
    pub tip: &'a str,
    /// Why it cannot be chosen now, when it cannot.
    pub off: Option<&'a str>,
}

/// One choice among a few, drawn as one joined bar whose chosen part is filled; the part clicked.
pub(super) fn segments(ui: &mut egui::Ui, parts: &[Part]) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        let round = ui.visuals().widgets.inactive.corner_radius;
        for (at, part) in parts.iter().enumerate() {
            let (first, last) = (at == 0, at + 1 == parts.len());
            let corners = egui::CornerRadius {
                nw: if first { round.nw } else { 0 },
                sw: if first { round.sw } else { 0 },
                ne: if last { round.ne } else { 0 },
                se: if last { round.se } else { 0 },
            };
            let response = ui.add_enabled(
                part.off.is_none(),
                egui::Button::new(part.text)
                    .selected(part.chosen)
                    .corner_radius(corners),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::RadioButton,
                    part.off.is_none(),
                    part.chosen,
                    part.text,
                )
            });
            let response = response.on_disabled_hover_text(part.off.unwrap_or_default());
            if tip(response, part.tip).clicked() {
                clicked = Some(at);
            }
        }
    });
    clicked
}

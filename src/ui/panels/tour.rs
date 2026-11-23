//! The tour: a small window in a corner that says what the screen it has brought forward is for, with Back, Next and a way out.
//!
//! ⚠ In a corner and not over the middle, so the screen it talks about stays in view; the last step's button opens the guide.

use crate::state::front_door::{Corner, TOUR};
use crate::state::AppState;
use crate::ui::theme;

const TITLE: &str = tr_key!("Tour");

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(at) = state.front_door.tour else {
        return;
    };
    let Some(step) = TOUR.get(at) else {
        state.end_tour();
        return;
    };
    let last = at + 1 == TOUR.len();
    // ⚠ Below the menu bar or above the status bar, whichever corner covers nothing the step is about: a card that hides Fix… on the step that teaches it teaches nothing.
    let anchor = match step.corner {
        Corner::TopRight => (egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 40.0)),
        Corner::BottomRight => (egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0)),
    };
    let (mut back, mut next, mut end) = (false, false, false);
    egui::Window::new(crate::i18n::tr(TITLE))
        .id(egui::Id::new("the_tour"))
        .collapsible(false)
        .resizable(false)
        .anchor(anchor.0, anchor.1)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.set_max_width(340.0);
            ui.label(
                egui::RichText::new(tr!(
                    "Step {step} of {steps}",
                    step = at + 1,
                    steps = TOUR.len()
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            ui.label(egui::RichText::new(crate::i18n::tr(step.title)).strong());
            ui.add(egui::Label::new(crate::i18n::tr(step.words)).wrap());
            ui.add_space(theme::SPACING_DEFAULT);
            ui.horizontal(|ui| {
                back = ui
                    .add_enabled(at > 0, egui::Button::new(tr!("Back")))
                    .clicked();
                let go = if last {
                    tr!("Open the guide")
                } else {
                    tr!("Next")
                };
                let button =
                    egui::Button::new(egui::RichText::new(go).color(theme::ON_ACCENT.get()))
                        .fill(theme::ACCENT.get());
                next = ui.add(button).clicked();
                end = ui.button(tr!("End the tour")).clicked();
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        end = true;
    }
    if back {
        state.tour_back();
    } else if next {
        state.tour_next();
    } else if end {
        state.end_tour();
    }
}

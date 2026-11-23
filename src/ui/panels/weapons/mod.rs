//! The Weapons screen's panels: the weapons of a pack, the weapon selected, and the weapon assembled.
//!
//! ⚠ **Thin by design.** Every rule, every read and every write is `or-weapons`'s and runs through `state.workbench.weapons`; a
//! panel reads what the last scan left and asks for a change.

mod fields;
mod list;
mod preview;
mod weapon;

pub use list::bar as draw_bar;
pub use list::draw as draw_list;
pub use preview::draw as draw_preview;
pub use weapon::draw as draw_weapon;

use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::panel_state::PanelState;

/// What the panels keep between frames.
#[derive(Default)]
pub struct WeaponsUi {
    /// The list's rectangle in the pass it was last drawn, which is where a drop is the Weapons screen's.
    drop: Option<(egui::Rect, u64)>,
    /// The same for the assembled weapon, which a drop of weapon files lands on as well.
    drop_preview: Option<(egui::Rect, u64)>,
}

impl PanelState for WeaponsUi {
    const KEY: &'static str = "weapons";
}

/// The list's rectangle as drawn this frame, none when it was not drawn.
pub fn drop_rect(state: &AppState) -> Option<egui::Rect> {
    state.panels.peek::<WeaponsUi>()?.drop.map(|(rect, _)| rect)
}

/// Whether a drop at `at` lands on the Weapons screen's list or its assembled weapon, as drawn in this pass.
pub fn accepts_drop_at(ctx: &egui::Context, state: &AppState, at: egui::Pos2) -> bool {
    let pass = ctx.cumulative_pass_nr();
    state.panels.peek::<WeaponsUi>().is_some_and(|ui| {
        [ui.drop, ui.drop_preview]
            .into_iter()
            .flatten()
            .any(|(rect, drawn)| drawn == pass && rect.contains(at))
    })
}

/// The width of the icon buttons that take a change back and make it again.
pub(super) const STEP_BUTTON: f32 = 30.0;

/// The button that takes the last change to a weapon back or makes it again: an icon of one width, named for the change.
pub(super) fn step_button(ui: &mut egui::Ui, state: &mut AppState, action: Action, glyph: &str) {
    let Some(command) = commands::of(state, action) else {
        return;
    };
    let button = egui::Button::new(glyph).min_size(egui::vec2(STEP_BUTTON, 0.0));
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    let response = crate::ui::accessible_name::named(response, &command.label);
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
    }
}

/// `action` as a button: its label, greyed with the reason when it cannot run; true once it ran.
pub(super) fn action_button(
    ui: &mut egui::Ui,
    state: &mut AppState,
    action: Action,
    filled: bool,
) -> bool {
    let Some(command) = commands::of(state, action) else {
        return false;
    };
    let text = command.label.clone();
    let button = if filled {
        egui::Button::new(egui::RichText::new(text).color(crate::ui::theme::ON_ACCENT.get()))
            .fill(crate::ui::theme::ACCENT.get())
    } else {
        egui::Button::new(text)
    };
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
        return true;
    }
    false
}

use crate::state::AppState;
use crate::ui::panel_state::PanelState;

use super::settings::{SettingsCategory, SettingsUiState};

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    /// Asked for again since the window last came forward.
    raise: bool,
}

impl PanelState for SettingsWindow {
    const KEY: &'static str = "settings";
}

pub fn is_settings_open(state: &AppState) -> bool {
    state
        .panels
        .peek::<SettingsWindow>()
        .is_some_and(|s| s.open)
}

pub fn set_settings_open(state: &mut AppState, open: bool) {
    state.panels.get::<SettingsWindow>().open = open;
}

/// Open the window, or bring it forward when it is open behind the editor.
pub fn open_settings(state: &mut AppState) {
    let window = state.panels.get::<SettingsWindow>();
    window.open = true;
    window.raise = true;
}

pub fn open_settings_at(state: &mut AppState, category: SettingsCategory) {
    open_settings(state);
    let page = state.panels.get::<SettingsUiState>();
    page.category = category;
    page.query.clear();
}

/// Whether the window was asked for since this last answered, which it answers once.
pub fn take_raise(state: &mut AppState) -> bool {
    std::mem::take(&mut state.panels.get::<SettingsWindow>().raise)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_settings_while_it_is_open_brings_its_window_forward() {
        let mut state = AppState::default();
        open_settings(&mut state);
        take_raise(&mut state);
        open_settings(&mut state);
        assert!(
            take_raise(&mut state),
            "opening it again left the window where it was"
        );
        assert!(!take_raise(&mut state), "one opening raises it once");
        open_settings_at(&mut state, SettingsCategory::Shortcuts);
        assert!(take_raise(&mut state));
    }

    #[test]
    fn opening_at_a_category_selects_it_and_clears_the_search() {
        let mut state = AppState::default();
        state.panels.get::<SettingsUiState>().query = "keys".to_string();
        open_settings_at(&mut state, SettingsCategory::Shortcuts);
        assert!(is_settings_open(&state));
        let page = state.panels.get::<SettingsUiState>();
        assert_eq!(page.category, SettingsCategory::Shortcuts);
        assert!(
            page.query.is_empty(),
            "a stale query would filter the page the caller asked to land on"
        );
    }

    /// ⚠ The window is an operating-system window with no close control of its own in the page: every other window and question closes on Escape.
    #[test]
    fn escape_closes_the_settings_window() {
        let mut state = AppState::default();
        open_settings(&mut state);
        let ctx = egui::Context::default();
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 580.0),
            )),
            events: vec![escape],
            ..Default::default()
        });
        super::super::settings::draw_settings_window_body(&ctx, &mut state);
        let _ = ctx.end_pass();
        assert!(!is_settings_open(&state), "Escape left the window open");
    }
}

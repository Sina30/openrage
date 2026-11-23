use crate::state::AppState;
use crate::ui::theme;

#[derive(Default)]
pub struct AboutModal {
    pub open: bool,
}

impl crate::ui::panel_state::PanelState for AboutModal {
    const KEY: &'static str = "about";
}

pub fn open_about(state: &mut AppState) {
    state.panels.get::<AboutModal>().open = true;
}

pub fn draw_about_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.panels.get::<AboutModal>().open {
        return;
    }
    let mut keep_open = true;
    egui::Window::new(tr!("About OpenRage"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut keep_open)
        .show(ctx, |ui| draw_body(ui, state));
    if !keep_open {
        state.panels.get::<AboutModal>().open = false;
    }
}

fn draw_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.set_min_width(380.0);
    ui.vertical_centered(|ui| {
        ui.add_space(theme::SPACING_DEFAULT);
        ui.add_sized(
            egui::vec2(72.0, 72.0),
            crate::ui::branding::app_logo_image(72.0),
        );
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(egui::RichText::new("OpenRage").size(22.0).strong());
        ui.label(
            egui::RichText::new(crate::i18n::tr(crate::ui::branding::TAGLINE))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_DEFAULT);

        let version_line = tr!("Version {version}", version = env!("CARGO_PKG_VERSION"));
        ui.label(egui::RichText::new(&version_line).monospace());
        if let Some(active) = state.prefs.active_game {
            ui.label(
                egui::RichText::new(tr!("Targeting: {active}", active = active.label()))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        ui.add_space(theme::SPACING_DEFAULT);

        ui.label(
            egui::RichText::new(crate::i18n::tr(crate::ui::branding::DESCRIPTION))
                .small()
                .weak(),
        );
        ui.add_space(theme::SPACING_DEFAULT);

        ui.horizontal(|ui| {
            if ui.button(tr!("Copy version info")).clicked() {
                let info = format!(
                    "OpenRage {} (target: {})",
                    env!("CARGO_PKG_VERSION"),
                    state
                        .prefs
                        .active_game
                        .map(|g| g.label())
                        .unwrap_or("unset")
                );
                ui.ctx().copy_text(info);
                state.show_toast(tr!("Version info copied to clipboard"));
            }
            if ui.button(tr!("Keyboard shortcuts")).clicked() {
                crate::ui::panels::settings::open_settings_at(
                    state,
                    crate::ui::panels::settings::SettingsCategory::Shortcuts,
                );
                state.panels.get::<AboutModal>().open = false;
            }
            if ui.button(tr!("Close")).clicked() {
                state.panels.get::<AboutModal>().open = false;
            }
        });
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!("Third-party licenses: see LICENSE-third-party.md"))
                .small()
                .weak(),
        );
        ui.add_space(theme::SPACING_TIGHT);
    });
}

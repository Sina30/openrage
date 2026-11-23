//! The plugins page and the consent prompt a plugin raises before it first runs.

use super::question;
use or_plugin::{SettingKind, SettingValue};

use crate::platform::file_dialogs;
use crate::state::plugins::{plugins_dir, Consent};
use crate::state::AppState;
use crate::ui::theme;

pub fn draw_plugins_page(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Plugins"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "A plugin is a directory holding plugin.json and plugin.wasm. Before it \
             first runs you are shown what its manifest declares, and your answer is \
             remembered here until you revoke it."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_SNUG);
    ui.horizontal(|ui| {
        let dir = plugins_dir().map_or_else(
            || "no configuration directory".to_string(),
            |d| d.display().to_string(),
        );
        ui.label(egui::RichText::new(dir).monospace());
        if ui.button(tr!("Read again")).clicked() {
            state.rescan_plugins();
        }
    });
    ui.horizontal(|ui| {
        let from_folder = ui
            .button(tr!("Install from folder…"))
            .on_hover_text(tr!("Copy a folder that holds plugin.json and plugin.wasm into the plugins directory. Nothing runs until you say yes to what it asks for"));
        let from_file = ui
            .button(tr!("Install from file…"))
            .on_hover_text(tr!("Pick the plugin.json or the plugin.wasm of a plugin; its folder is copied into the plugins directory"));
        let picked = if from_folder.clicked() {
            file_dialogs::pick_folder(tr!("Install plugin from a folder"))
        } else if from_file.clicked() {
            file_dialogs::pick_plugin_file()
        } else {
            None
        };
        if let Some(source) = picked {
            match state.install_plugin(&source) {
                Ok(name) => state.show_toast(tr!("Installed {name}", name = name)),
                Err(why) => state.show_error_toast(why),
            }
        }
    });

    let plugins = state.installed_plugins().to_vec();
    if plugins.is_empty() {
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!("No plugins are installed.")).color(theme::TEXT_SECONDARY),
        );
        return;
    }
    for plugin in &plugins {
        let id = plugin.manifest.id.as_str();
        ui.add_space(theme::SPACING_SNUG);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&plugin.manifest.name).strong());
            ui.label(egui::RichText::new(id).monospace().weak());
        });
        if let Some(why) = &plugin.refused {
            ui.label(egui::RichText::new(why).color(theme::WARNING));
        }
        // ⚠ A plugin built for an older interface loads and runs as it was built, and the page says so.
        for note in plugin.notes() {
            ui.label(
                egui::RichText::new(note)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
        for capability in &plugin.manifest.wants {
            ui.label(format!("• {}", capability.asks_for()));
        }
        ui.horizontal(|ui| match state.prefs.plugin_consent.get(id) {
            Some(Consent::Granted(_)) => {
                ui.label(tr!("Granted."));
                if ui.button(tr!("Revoke")).clicked() {
                    state.revoke_plugin_consent(id);
                }
            }
            Some(Consent::Declined) => {
                ui.label(tr!("Declined."));
                if ui.button(tr!("Ask again")).clicked() {
                    state.revoke_plugin_consent(id);
                }
            }
            None => {
                ui.label(egui::RichText::new(tr!("Not asked yet.")).weak());
            }
        });
        if let Some(report) = state.plugin_report(id) {
            for line in report {
                let capability = line
                    .arg("capability")
                    .map(|c| format!(" {c}"))
                    .unwrap_or_default();
                let colour = if line.severity > or_io::Severity::Info {
                    theme::WARNING
                } else {
                    theme::TEXT_SECONDARY
                };
                ui.label(
                    egui::RichText::new(format!("[{}{capability}] {}", line.key, line.message))
                        .small()
                        .color(colour),
                );
            }
        }
    }
}

/// The page of the settings a loaded plugin declared, which the preferences keep.
pub fn draw_plugin_settings_page(ui: &mut egui::Ui, state: &mut AppState, plugin: &str) {
    let name = (state.plugin_setting_pages().into_iter())
        .find(|(id, _)| id == plugin)
        .map(|(_, name)| name);
    let Some(name) = name else {
        ui.label(
            egui::RichText::new(tr!("This plugin is not loaded.")).color(theme::TEXT_SECONDARY),
        );
        return;
    };
    ui.heading(&name);
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "The settings this plugin declared; the plugin reads them when it needs them."
        ))
        .small()
        .weak(),
    );
    let specs = (state.plugin_registrations(plugin)).map_or_else(Vec::new, |r| r.settings.clone());
    for spec in specs {
        let Some(current) = state.plugin_setting(plugin, &spec.id) else {
            continue;
        };
        ui.add_space(theme::SPACING_SNUG);
        let changed = match (&spec.kind, current) {
            (SettingKind::Bool { .. }, SettingValue::Bool(mut value)) => ui
                .checkbox(&mut value, spec.label.as_str())
                .changed()
                .then_some(SettingValue::Bool(value)),
            (SettingKind::Number { min, max, .. }, SettingValue::Number(mut value)) => {
                if min < max {
                    let slider =
                        egui::Slider::new(&mut value, *min..=*max).text(spec.label.as_str());
                    ui.add(slider)
                        .changed()
                        .then_some(SettingValue::Number(value))
                } else {
                    ui.label(format!("{}: {value}", spec.label));
                    None
                }
            }
            (SettingKind::Text { .. }, SettingValue::Text(mut value)) => ui
                .horizontal(|ui| {
                    ui.label(spec.label.as_str());
                    ui.add(egui::TextEdit::singleline(&mut value)).changed()
                })
                .inner
                .then_some(SettingValue::Text(value)),
            _ => None,
        };
        if let Some(value) = changed {
            state.set_plugin_setting(plugin, &spec.id, value);
        }
    }
    // ⚠ Not while a slider is dragged, which changes the value every frame.
    if !ui.input(|i| i.pointer.any_down()) {
        state.persist_plugin_settings();
    }
}

/// What a plugin declared, shown before it runs; nothing runs until it is answered.
pub fn draw_consent_modal(ctx: &egui::Context, state: &mut AppState) {
    let Some(plugin) = state.plugins.pending_consent.as_ref() else {
        return;
    };
    let mut answer = None;
    question(ctx, tr!("Allow this plugin?"), |ui| {
        ui.set_min_width(420.0);
        ui.label(egui::RichText::new(tr!("{name} asks to:", name = plugin.manifest.name)).strong());
        ui.add_space(theme::SPACING_TIGHT);
        for capability in &plugin.manifest.wants {
            ui.horizontal(|ui| {
                ui.label(format!("• {}", capability.asks_for()));
                ui.label(egui::RichText::new(capability.key()).small().weak());
            });
        }
        let edits_a_document = plugin
            .manifest
            .wants
            .iter()
            .any(|c| c.writes() && !matches!(c, or_plugin::Capability::WriteFiles));
        if edits_a_document {
            ui.add_space(theme::SPACING_SNUG);
            ui.label(tr!(
                "A change still passes the editor's edit mode, and nothing is \
                 written to disk until you save."
            ));
        }
        if plugin
            .manifest
            .wants
            .contains(&or_plugin::Capability::WriteFiles)
        {
            ui.add_space(theme::SPACING_SNUG);
            ui.label(tr!(
                "The files it writes into your project are written when it runs, all \
                 together or none, and Undo in the project takes them back as one step."
            ));
        }
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "Your answer is remembered; change it in Automate ▸ Run plugin ▸ Plugin settings…."
            ))
            .weak(),
        );
        ui.add_space(theme::SPACING_DEFAULT);
        ui.horizontal(|ui| {
            if ui.button(tr!("Allow")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Decline")).clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(grant) = answer {
        state.answer_plugin_consent(grant);
    }
}

/// The Plugins panel, which Automate opens: what is installed and what each was allowed, then the settings each loaded plugin declared.
pub fn draw_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_plugins_page(ui, state);
            for (plugin, _) in state.plugin_setting_pages() {
                ui.separator();
                draw_plugin_settings_page(ui, state, &plugin);
            }
        });
}

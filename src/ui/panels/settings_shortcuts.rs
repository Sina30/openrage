use crate::state::AppState;
use crate::ui::commands;
use crate::ui::shortcuts;
use crate::ui::theme;

const CAPTURE_KEY: &str = "settings_shortcut_capture";
const CONFLICT_KEY: &str = "settings_shortcut_conflict";

pub fn reset_transient(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove::<Option<commands::Action>>(egui::Id::new(CAPTURE_KEY));
        d.remove::<PendingConflict>(egui::Id::new(CONFLICT_KEY));
    });
}

#[derive(Clone)]
struct PendingConflict {
    new_action: commands::Action,
    existing: commands::Action,
    mods: shortcuts::ShortcutMods,
    key: egui::Key,
}

pub fn draw_shortcuts_page(ui: &mut egui::Ui, state: &mut AppState, query_lc: &str) {
    ui.heading(tr!("Keyboard shortcuts"));
    ui.add_space(theme::SPACING_HAIR);
    let primary = shortcuts::primary_modifier_name();
    ui.label(
        egui::RichText::new(tr!(
            "Click a row's Edit button, then press the keys. A shortcut follows the \
             character its key types on your layout. One without {primary} or Alt does \
             nothing while you type in a field, and Tab, Space, Enter and Backspace \
             need {primary}, Shift or Alt.",
            primary = primary
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_DEFAULT);

    ui.horizontal(|ui| {
        let has_overrides = shortcuts::has_overrides(&state.prefs);
        let restore_btn = egui::Button::new(tr!("Restore all defaults"));
        if ui
            .add_enabled(has_overrides, restore_btn)
            .on_hover_text(tr!(
                "Drop every user-customised binding and revert to the compile-time defaults."
            ))
            .clicked()
        {
            shortcuts::restore_defaults(&mut state.prefs);
            crate::state::save_prefs(&state.prefs);
            state.show_toast(tr!("All keyboard shortcuts restored to defaults."));
            ui.ctx().data_mut(|d| {
                d.remove::<Option<commands::Action>>(egui::Id::new(CAPTURE_KEY));
                d.remove::<PendingConflict>(egui::Id::new(CONFLICT_KEY));
            });
        }
    });
    ui.add_space(theme::SPACING_SNUG);
    ui.separator();
    ui.add_space(theme::SPACING_SNUG);

    let capture_id = egui::Id::new(CAPTURE_KEY);
    let conflict_id = egui::Id::new(CONFLICT_KEY);
    let capturing: Option<commands::Action> = ui
        .ctx()
        .data(|d| d.get_temp::<Option<commands::Action>>(capture_id))
        .unwrap_or(None);
    let pending_conflict: Option<PendingConflict> = ui
        .ctx()
        .data(|d| d.get_temp::<PendingConflict>(conflict_id));

    let rows: Vec<&commands::Spec> = commands::COMMANDS
        .iter()
        .filter(|spec| query_lc.is_empty() || matches_query(spec, &state.prefs, query_lc))
        .collect();

    if rows.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(tr!(
                    "No shortcuts match \"{query_lc}\"",
                    query_lc = query_lc
                ))
                .color(theme::TEXT_SECONDARY),
            );
        });
        return;
    }

    let mut to_capture: Option<Option<commands::Action>> = None;
    let mut to_reset: Option<commands::Action> = None;
    let mut conflict_to_set: Option<Option<PendingConflict>> = None;

    // ⚠ A long action name wraps: at its full width it pushed Edit and Reset out of the page.
    let name_width = (ui.available_width() - 260.0).max(160.0);
    egui::Grid::new("shortcuts_table")
        .num_columns(4)
        .max_col_width(name_width)
        .spacing([theme::SPACING_RELAXED, theme::SPACING_TIGHT])
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(tr!("Action")).strong());
            ui.label(egui::RichText::new(tr!("Binding")).strong());
            ui.label("");
            ui.label("");
            ui.end_row();

            for spec in &rows {
                let action = spec.action;
                let name = ui.label(name_of(spec));
                if !spec.hover.is_empty() {
                    name.on_hover_text(crate::i18n::tr(spec.hover));
                }

                if capturing == Some(action) {
                    ui.colored_label(
                        theme::INFO,
                        egui::RichText::new(tr!("Press a combo… (Esc to cancel)")).italics(),
                    );
                } else {
                    let labels = shortcuts::labels_for(action, &state.prefs);
                    let label_text = if labels.is_empty() {
                        tr!("(unbound)").to_string()
                    } else {
                        // One a line: side by side, a second binding pushes the
                        // Reset column out of the window.
                        labels.join("\n")
                    };
                    let mut text = egui::RichText::new(label_text).monospace();
                    if shortcuts::is_overridden(&state.prefs, action) {
                        text = text.color(theme::ACCENT_TEXT);
                    } else if labels.is_empty() {
                        text = text.color(theme::TEXT_DISABLED);
                    }
                    ui.label(text);
                }

                let edit_label = if capturing == Some(action) {
                    tr!("Cancel")
                } else {
                    tr!("Edit")
                };
                if ui.button(edit_label).clicked() {
                    if capturing == Some(action) {
                        to_capture = Some(None);
                    } else {
                        to_capture = Some(Some(action));
                        conflict_to_set = Some(None);
                    }
                }

                let can_reset = shortcuts::is_overridden(&state.prefs, action);
                if ui
                    .add_enabled(can_reset, egui::Button::new(tr!("Reset")))
                    .on_hover_text(tr!("Restore this shortcut to its default"))
                    .clicked()
                {
                    to_reset = Some(action);
                }
                ui.end_row();
            }
        });

    if let Some(action) = capturing {
        if let Some((mods, key)) = next_combo(ui.ctx()) {
            if key == egui::Key::Escape && !mods.ctrl && !mods.shift && !mods.alt {
                to_capture = Some(None);
            } else if !shortcuts::is_valid_combo(mods, key) {
                state.show_error_toast(tr!(
                    "Tab, Space, Enter and Backspace need Ctrl, Shift or Alt."
                ));
                to_capture = Some(None);
            } else {
                match shortcuts::set_binding(&mut state.prefs, action, mods, key) {
                    Ok(()) => {
                        crate::state::save_prefs(&state.prefs);
                        state.show_toast(tr!(
                            "Bound {action} to {shortcuts}.",
                            action = sc_description(action),
                            shortcuts = shortcuts::format_binding(mods, key)
                        ));
                        to_capture = Some(None);
                    }
                    Err(conflict) => {
                        conflict_to_set = Some(Some(PendingConflict {
                            new_action: action,
                            existing: conflict.existing,
                            mods,
                            key,
                        }));
                        to_capture = Some(None);
                    }
                }
            }
        }
    }

    if let Some(conflict) = pending_conflict.clone() {
        ui.add_space(theme::SPACING_DEFAULT);
        egui::Frame::group(ui.style())
            .fill(theme::BG_L2.get())
            .show(ui, |ui| {
                ui.colored_label(
                    theme::WARNING,
                    tr!(
                        "{shortcuts} is already bound to “{existing}”.",
                        shortcuts = shortcuts::format_binding(conflict.mods, conflict.key),
                        existing = sc_description(conflict.existing)
                    ),
                );
                ui.label(
                    egui::RichText::new(tr!(
                        "Overwriting will leave the previous action without a binding."
                    ))
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button(tr!("Overwrite")).clicked() {
                        shortcuts::force_set_binding(
                            &mut state.prefs,
                            conflict.new_action,
                            conflict.mods,
                            conflict.key,
                        );
                        crate::state::save_prefs(&state.prefs);
                        state.show_toast(tr!(
                            "Bound {new_action} to {shortcuts}; {existing} is now unbound.",
                            new_action = sc_description(conflict.new_action),
                            shortcuts = shortcuts::format_binding(conflict.mods, conflict.key),
                            existing = sc_description(conflict.existing)
                        ));
                        conflict_to_set = Some(None);
                    }
                    if ui.button(tr!("Cancel")).clicked() {
                        conflict_to_set = Some(None);
                    }
                });
            });
    }

    if let Some(next) = to_capture {
        ui.ctx().data_mut(|d| d.insert_temp(capture_id, next));
    }
    if let Some(action) = to_reset {
        shortcuts::reset_binding(&mut state.prefs, action);
        crate::state::save_prefs(&state.prefs);
        state.show_toast(tr!(
            "Reset {action} to its default.",
            action = sc_description(action)
        ));
    }
    if let Some(next) = conflict_to_set {
        match next {
            Some(c) => ui.ctx().data_mut(|d| {
                d.insert_temp(conflict_id, c);
            }),
            None => ui.ctx().data_mut(|d| {
                d.remove::<PendingConflict>(conflict_id);
            }),
        }
    }
}

fn sc_description(action: commands::Action) -> &'static str {
    commands::spec(action).map_or(tr!("(unknown action)"), |spec| crate::i18n::tr(spec.label))
}

/// A command as the palette names it: where it sits, then its label.
fn name_of(spec: &commands::Spec) -> String {
    format!(
        "{}: {}",
        crate::i18n::tr(spec.category),
        crate::i18n::tr(spec.label)
    )
}

/// Whether the settings search names this command, its hover text or its binding.
pub fn matches_query(spec: &commands::Spec, prefs: &crate::state::Prefs, query_lc: &str) -> bool {
    [name_of(spec), crate::i18n::tr(spec.hover).to_string()]
        .into_iter()
        .chain(shortcuts::label_for(spec.action, prefs))
        .any(|text| text.to_lowercase().contains(query_lc))
}

fn next_combo(ctx: &egui::Context) -> Option<(shortcuts::ShortcutMods, egui::Key)> {
    ctx.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Some(((*modifiers).into(), *key)),
            _ => None,
        })
    })
}

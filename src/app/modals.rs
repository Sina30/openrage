use egui::Context;

use crate::state::front_door::spoken;
use crate::state::toasts::Toast;
use crate::state::{
    AfterClose, AppState, CloseAnswer, EnhancedOpenDecision, PendingExportFixes,
    RetailEditDecision, ToastType, Toasts,
};
use crate::ui::panels::jobs::JobList;
use crate::ui::panels::{question, question_escaped, question_titled};
use crate::ui::theme;

/// Everything the editor window draws on top of the shell.
///
/// ⚠ One list rather than calls at the call site, so `--self-check` reaches
/// the same set the window does. The focus ring is last: a dialog paints its
/// frame when it is drawn, over anything drawn into its layer before it.
pub(crate) fn draw_editor_overlays(ctx: &Context, app_state: &mut AppState) {
    if app_state.show_error_dialog {
        draw_error_dialog(ctx, app_state);
    }
    crate::ui::panels::settings::draw_keys_prompt_modal(ctx, app_state);
    draw_retail_edit_confirm_modal(ctx, app_state);
    draw_export_fixes_modal(ctx, app_state);
    draw_enhanced_open_modal(ctx, app_state);
    draw_close_prompt(ctx, app_state);
    draw_reload_prompt(ctx, app_state);
    crate::ui::panels::settings_plugins::draw_consent_modal(ctx, app_state);
    // ⚠ The tour's card is a window of the bottom right corner, which the stack of toasts would cover: its Next and End the tour must stay where they are pressed.
    let tour = (app_state.front_door.tour.is_some())
        .then(|| ctx.memory(|m| m.area_rect(egui::Id::new("the_tour"))))
        .flatten();
    let job_list = app_state
        .panels
        .peek::<JobList>()
        .and_then(|list| list.drawn);
    let dodge: Vec<egui::Rect> = job_list.into_iter().chain(tour).collect();
    if let Some(action) = draw_toasts(ctx, &mut app_state.toasts, &dodge) {
        app_state.run_toast_action(action);
    }
    draw_crash_notice(ctx);
    crate::ui::focus_ring::draw(ctx);
    crate::ui::accessible_name::name_the_focus(ctx);
}

/// A panic's report: where it is, what it says, and a button to copy it.
pub(super) fn draw_crash_notice(ctx: &Context) {
    use crate::diagnostics::crash;

    let Some(notice) = crash::pending() else {
        return;
    };
    let (title, intro) = if notice.from_last_session {
        (
            tr!("OpenRage closed unexpectedly"),
            tr!(
                "The last session ended with an internal error. The report below says \
             what happened; paste it into a bug report."
            ),
        )
    } else {
        (
            tr!("OpenRage hit an internal error"),
            tr!(
                "Part of OpenRage failed. The editor may keep working, but whatever that \
             part was doing did not finish. The report below says what happened; \
             paste it into a bug report."
            ),
        )
    };
    let mut close = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .default_width(640.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.label(intro);
            ui.add_space(theme::SPACING_SNUG);
            match &notice.path {
                Some(path) => {
                    ui.label(tr!("Saved to:"));
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .monospace()
                            .color(theme::TEXT_PRIMARY),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new(tr!(
                            "It could not be saved to disk, so this window is the only copy. \
                             Copy it before closing."
                        ))
                        .color(theme::WARNING),
                    );
                }
            }
            ui.label(
                egui::RichText::new(tr!(
                    "It stays on this computer. Nothing has been sent anywhere."
                ))
                .small()
                .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(theme::SPACING_SNUG);
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut notice.report.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            ui.add_space(theme::SPACING_DEFAULT);
            ui.horizontal(|ui| {
                if ui.button(tr!("Copy report")).clicked() {
                    ui.ctx().copy_text(notice.report.clone());
                }
                if let Some(dir) = notice.path.as_deref().and_then(std::path::Path::parent) {
                    if ui.button(tr!("Open folder")).clicked() {
                        crate::platform::shell::open_folder(dir);
                    }
                }
                if ui.button(tr!("Close")).clicked() {
                    close = true;
                }
            });
        });
    if close {
        crash::dismiss();
    }
}

/// The button that goes on, filled, with the keyboard on it from the first frame its question `key` is drawn.
///
/// ⚠ Asked for until it has it, as a modal takes away the focus the first frame asked for; [`answered`] makes the next question ask again. Enter then does what a click on it does.
fn primary(ui: &mut egui::Ui, text: &str, key: &str) -> egui::Response {
    let button = ui.add(
        egui::Button::new(egui::RichText::new(text).color(theme::ON_ACCENT.get()))
            .fill(theme::ACCENT.get()),
    );
    let asked = egui::Id::new(("primary_focused", key));
    if !ui.data(|d| d.get_temp::<bool>(asked)).unwrap_or(false) {
        button.request_focus();
        if button.has_focus() {
            ui.data_mut(|d| d.insert_temp(asked, true));
        }
    }
    button
}

/// The question `key` is answered: its button takes the keyboard again the next time it is asked.
fn answered(ctx: &Context, key: &str) {
    ctx.data_mut(|d| d.remove::<bool>(egui::Id::new(("primary_focused", key))));
}

pub(super) fn draw_error_dialog(ctx: &Context, app_state: &mut AppState) {
    let title = app_state.error_dialog_title.clone();
    let mut close = false;
    question_titled(ctx, tr!("Error"), &title, 460.0, |ui| {
        ui.add_space(theme::SPACING_SNUG);
        // ⚠ The plain sentence when there is one; what the reader said is the details, and the Log's.
        let said =
            (app_state.error_dialog_plain.as_ref()).unwrap_or(&app_state.error_dialog_message);
        ui.add(egui::Label::new(said).wrap());
        ui.add_space(theme::SPACING_RELAXED);
        ui.horizontal(|ui| {
            close |= primary(ui, tr!("OK"), "error").clicked();
            if ui
                .button(tr!("Copy details"))
                .on_hover_text(tr!(
                    "Copy the title and the exact words of the error, for a report"
                ))
                .clicked()
            {
                let details = format!("{title}\n{}", app_state.error_dialog_message);
                ui.ctx().copy_text(details);
            }
        });
    });
    if close || question_escaped(ctx, tr!("Error")) {
        answered(ctx, "error");
        app_state.clear_error();
    }
}

pub(super) fn draw_retail_edit_confirm_modal(ctx: &Context, app_state: &mut AppState) {
    let prompt = match app_state.pending_retail_edit.as_ref() {
        Some(p) => p.clone(),
        None => return,
    };
    let mut decision: Option<RetailEditDecision> = None;
    question(ctx, tr!("Modify base-game file?"), |ui| {
        ui.set_min_width(420.0);
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "You are about to {verb} a base-game file:",
                verb = prompt.verb
            ))
            .strong(),
        );
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(prompt.target_path.display().to_string())
                .monospace()
                .color(theme::WARNING),
        );
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(tr!("This file ships with the game install. Changes here \
             affect your real Rockstar install — make sure you \
             know what you're doing."));
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!(
                "A copy of the original is kept beside it as .bak, once, \
                 the first time OpenRage writes to it."
            ))
            .weak(),
        );
        ui.add_space(theme::SPACING_RELAXED);
        ui.horizontal(|ui| {
            if ui.button(tr!("Yes")).clicked() {
                decision = Some(RetailEditDecision::YesSession);
            }
            if ui.button(tr!("No")).clicked() {
                decision = Some(RetailEditDecision::No);
            }
        });
        ui.add_space(theme::SPACING_TIGHT);
        // ⚠ No "never ask again" box: a boolean set once in a dialog could
        // not be found again. The edit mode, which shows in Settings and in
        // the menu bar, decides whether base-game writes are allowed at all,
        // and a Yes ends when it changes.
        ui.label(
            egui::RichText::new(tr!(
                "Yes allows base-game writes until the edit mode changes or \
                 OpenRage closes. The edit mode, in the menu bar, decides whether \
                 they are allowed at all."
            ))
            .weak(),
        );
    });
    if let Some(d) = decision {
        app_state.apply_pending_retail_edit(d);
    }
}

pub(super) fn draw_export_fixes_modal(ctx: &Context, app_state: &mut AppState) {
    let Some(pending) = app_state.pending_export_fixes.as_ref() else {
        return;
    };
    let drag_out = matches!(pending, PendingExportFixes::DragOut);
    let remember_id = egui::Id::new("export_fixes_remember");
    let mut remember = ctx.data(|d| d.get_temp::<bool>(remember_id).unwrap_or(false));
    let mut decision: Option<bool> = None;
    question(ctx, tr!("Export"), |ui| {
        ui.set_min_width(380.0);
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "Do you want to apply OpenRage export fixes (this will alter the bytes)?"
            ))
            .strong(),
        );
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!(
                "Fixed files are OpenRage's canonical form, which exports again \
                 byte for byte. Without the fixes the files keep their original bytes."
            ))
            .weak(),
        );
        if drag_out {
            ui.add_space(theme::SPACING_TIGHT);
            ui.label(tr!("Drag the rows out again to export them."));
        }
        ui.add_space(theme::SPACING_DEFAULT);
        ui.checkbox(&mut remember, tr!("Remember my choice"))
            .on_hover_text(tr!("The Editing page of Settings changes it later"));
        ui.add_space(theme::SPACING_DEFAULT);
        ui.horizontal(|ui| {
            if ui.button(tr!("Apply fixes")).clicked() {
                decision = Some(true);
            }
            if ui.button(tr!("Keep original bytes")).clicked() {
                decision = Some(false);
            }
        });
    });
    ctx.data_mut(|d| d.insert_temp(remember_id, remember));
    if let Some(apply) = decision {
        ctx.data_mut(|d| d.remove::<bool>(remember_id));
        app_state.apply_export_fixes(apply, remember);
    }
}

pub(super) fn draw_enhanced_open_modal(ctx: &Context, app_state: &mut AppState) {
    let prompt = match app_state.pending_enhanced_open.as_ref() {
        Some(p) => p.clone(),
        None => return,
    };
    let mut decision: Option<EnhancedOpenDecision> = None;
    let label = prompt.game.label();
    question(ctx, &tr!("Open {label} file?", label = label), |ui| {
        ui.set_min_width(460.0);
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "This is a {label} file. Opening it will mount {label}.",
                label = label
            ))
            .strong(),
        );
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(
                prompt
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| prompt.path.display().to_string()),
            )
            .monospace(),
        );
        ui.label(
            egui::RichText::new(tr!("resource version {version}", version = prompt.version))
                .small()
                .weak(),
        );
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(tr!(
            "{label} files are edited and saved as {label} — they are not \
             converted. Your currently open documents stay as they are.",
            label = label
        ));
        if !prompt.enhanced_root_known {
            ui.add_space(theme::SPACING_SNUG);
            ui.label(
                egui::RichText::new(tr!(
                    "No {label} install folder is configured, so nothing \
                     will be mounted. The file will still open.",
                    label = label
                ))
                .color(theme::WARNING),
            );
        }
        ui.add_space(theme::SPACING_RELAXED);
        ui.horizontal(|ui| {
            if ui
                .button(tr!("Yes, switch to {label}", label = label))
                .on_hover_text(tr!(
                    "Make {label} the active edition and open this file",
                    label = label
                ))
                .clicked()
            {
                decision = Some(EnhancedOpenDecision::Switch);
            }
            if ui.button(tr!("Cancel")).clicked() {
                decision = Some(EnhancedOpenDecision::Cancel);
            }
        });
        ui.add_space(theme::SPACING_TIGHT);
    });
    if let Some(d) = decision {
        app_state.apply_pending_enhanced_open(d);
    }
}

/// How many of the steps made since a file was saved the close question lists before it counts the rest.
const STEPS_LISTED: usize = 3;

/// The question a close asks before it drops unsaved work.
///
/// ⚠ Hidden while the saves a Save answer started run, so the prompts they
/// raise (where an archive entry goes, the edit gate) are the ones on top.
pub(super) fn draw_close_prompt(ctx: &Context, app_state: &mut AppState) {
    let Some(pending) = app_state.pending_close.as_ref().filter(|p| !p.saving) else {
        return;
    };
    let before = match pending.then {
        AfterClose::Nothing => "closing",
        AfterClose::NewSession => tr!("starting a new session"),
        AfterClose::OpenProject(_) => tr!("opening the project"),
        AfterClose::CloseProject => tr!("closing the project"),
        AfterClose::SwitchGame(..) => tr!("switching the game"),
        AfterClose::ClearGameFolder(_) => tr!("closing the install"),
        AfterClose::Exit => "exiting",
    };
    // The name, where it is (for the hover; none for a document never saved) and what was done to it since it was saved.
    let mut files: Vec<(String, Option<String>, Vec<String>)> = app_state
        .unsaved_documents(&pending.documents)
        .into_iter()
        .filter_map(|id| app_state.document(id))
        .map(|doc| {
            let place = doc.path.as_ref().map(|path| path.display().to_string());
            let steps = doc.history.since_saved().into_iter().map(str::to_string);
            (doc.file_name(), place, steps.collect())
        })
        .collect();
    if let Some(project) = app_state
        .project_path
        .as_ref()
        .filter(|_| app_state.loses_project(&pending.then))
    {
        let name = project.file_name().unwrap_or(project.as_os_str());
        let steps = (app_state.workbench.unsaved_steps().into_iter())
            .map(str::to_string)
            .collect();
        files.push((
            name.to_string_lossy().into_owned(),
            Some(project.display().to_string()),
            steps,
        ));
    }
    let changes: usize = files.iter().map(|(.., steps)| steps.len()).sum();
    let drops = match changes {
        0 => tr!("Don't save drops these changes.").to_string(),
        1 => tr!("Don't save drops this change.").to_string(),
        n => tr!("Don't save drops these {n} changes.", n = n),
    };
    let mut answer = None;
    question(ctx, tr!("Unsaved changes"), |ui| {
        ui.set_min_width(420.0);
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!("Save your changes before {before}?", before = before))
                .strong(),
        );
        ui.add_space(theme::SPACING_SNUG);
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .show(ui, |ui| {
                for (name, place, steps) in &files {
                    // ⚠ The name once, and where it is on hover: a path under a name that already says which file is a second line to read.
                    match place {
                        Some(place) => ui.label(name).on_hover_text(place),
                        None => ui.label(tr!("{name} (never saved)", name = name)),
                    };
                    for step in steps.iter().take(STEPS_LISTED) {
                        let text = egui::RichText::new(spoken(step))
                            .small()
                            .color(theme::TEXT_SECONDARY);
                        ui.add(egui::Label::new(text).wrap());
                    }
                    if steps.len() > STEPS_LISTED {
                        let more = tr!("and {count} more", count = steps.len() - STEPS_LISTED);
                        ui.label(
                            egui::RichText::new(more)
                                .small()
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
            });
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(egui::RichText::new(drops).weak());
        ui.add_space(theme::SPACING_DEFAULT);
        ui.horizontal(|ui| {
            if primary(ui, tr!("Save"), "close").clicked() {
                answer = Some(CloseAnswer::Save);
            }
            if ui.button(tr!("Don't save")).clicked() {
                answer = Some(CloseAnswer::DontSave);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(CloseAnswer::Cancel);
            }
        });
    });
    if answer.is_none() && question_escaped(ctx, tr!("Unsaved changes")) {
        answer = Some(CloseAnswer::Cancel);
    }
    if let Some(answer) = answer {
        answered(ctx, "close");
        app_state.answer_close(answer);
    }
}

/// A file changed on disk while it holds unsaved edits: reload over them, or keep them.
pub(super) fn draw_reload_prompt(ctx: &Context, app_state: &mut AppState) {
    if app_state.reload_conflicts.is_empty() {
        return;
    }
    let mut answer = None;
    // ⚠ A file OpenRage cooked itself from a saved source is refreshed, and nobody else changed it: the words say so.
    let (refreshed, changed): (Vec<&std::path::PathBuf>, Vec<&std::path::PathBuf>) = app_state
        .reload_conflicts
        .iter()
        .partition(|path| app_state.workbench.refreshed.contains(*path));
    question(ctx, tr!("Changed on disk"), |ui| {
        ui.set_min_width(420.0);
        ui.add_space(theme::SPACING_SNUG);
        for (heading, paths) in [
            (
                tr!("Refreshed from the source while it has unsaved changes here:"),
                &refreshed,
            ),
            (
                tr!("Changed on disk while it has unsaved changes here:"),
                &changed,
            ),
        ] {
            if paths.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(heading).strong());
            ui.add_space(theme::SPACING_TIGHT);
            for path in paths {
                ui.label(egui::RichText::new(path.display().to_string()).monospace());
            }
            ui.add_space(theme::SPACING_TIGHT);
        }
        ui.add_space(theme::SPACING_TIGHT);
        ui.horizontal(|ui| {
            let take = if changed.is_empty() {
                tr!("Use the refreshed file")
            } else {
                tr!("Reload and lose my changes")
            };
            if ui.button(take).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Keep my changes")).clicked() {
                answer = Some(false);
            }
        });
        ui.add_space(theme::SPACING_TIGHT);
        let kept = if changed.is_empty() {
            tr!("Kept, they overwrite the cooked file at the next save, and a source whose input has not changed does not cook it again.")
        } else {
            tr!("Kept, they overwrite the file on disk at the next save.")
        };
        ui.label(egui::RichText::new(kept).weak());
    });
    if let Some(reload) = answer {
        app_state.answer_reload_conflicts(reload);
    }
}

/// A toast's width in points; a longer message wraps and the toast grows.
const TOAST_WIDTH: f32 = 320.0;

/// The share of its life a toast spends fading out; until then it is opaque.
const TOAST_FADE: f32 = 0.15;

/// The rows a toast's message may take before it is cut; the Log keeps all of it.
const TOAST_MAX_ROWS: usize = 6;

/// The same for as long as the toast shows, so egui keeps its measured size.
fn toast_area_id(toast: &Toast) -> egui::Id {
    egui::Id::new(("toast", toast.created_at, &toast.message))
}

/// A toast's fill, text, accent and icon at `level`, in the active theme.
///
/// ⚠ The theme's own text on a panel tinted toward the level, never white on
/// the level's colour, which on the success green reads at 2.2:1.
fn toast_style(level: ToastType) -> (egui::Color32, egui::Color32, egui::Color32, &'static str) {
    use egui_phosphor::regular as icon;
    let (accent, glyph) = match level {
        ToastType::Success => (theme::SUCCESS, icon::CHECK_CIRCLE),
        ToastType::Info => (theme::INFO, icon::INFO),
        ToastType::Warning => (theme::WARNING, icon::WARNING),
        ToastType::Error => (theme::ERROR, icon::X_CIRCLE),
    };
    let accent = accent.get();
    let fill = theme::BG_L2.get().lerp_to_gamma(accent, 0.2);
    (fill, theme::TEXT_PRIMARY.get(), accent, glyph)
}

/// The toasts from the bottom right, oldest lowest, each on the measured top of the one below.
pub(super) fn draw_toasts(
    ctx: &Context,
    toasts: &mut Toasts,
    dodge: &[egui::Rect],
) -> Option<crate::state::toasts::ToastAction> {
    let screen = ctx.content_rect();
    // Left of the job list and of the tour's card while they are open.
    let right = (dodge.iter()).fold(screen.right() - 20.0, |right, rect| {
        right.min(rect.left() - 8.0)
    });
    // Clear of the status bar.
    let mut bottom = screen.bottom() - 28.0;
    let mut dismissed = None;
    let mut pressed = None;
    for (i, toast) in toasts.iter().enumerate() {
        let (fill, text, accent, glyph) = toast_style(toast.toast_type);
        let shown = egui::Area::new(toast_area_id(toast))
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(egui::pos2(right, bottom))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.multiply_opacity(toast.remaining_fraction() / TOAST_FADE);
                // Each button the toast carries, with where it is and whether it was clicked.
                let mut buttons: Vec<(crate::state::toasts::ToastAction, egui::Rect, bool)> =
                    Vec::new();
                let frame = egui::Frame::NONE
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0_f32, accent))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_MEDIUM))
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(TOAST_WIDTH - 24.0);
                        ui.horizontal_top(|ui| {
                            ui.label(egui::RichText::new(glyph).color(accent));
                            let mut message = egui::text::LayoutJob::simple(
                                toast.message.clone(),
                                egui::TextStyle::Body.resolve(ui.style()),
                                text,
                                f32::INFINITY,
                            );
                            message.wrap.max_rows = TOAST_MAX_ROWS;
                            ui.add(egui::Label::new(message).wrap());
                        });
                        if toast.action.is_some() {
                            ui.horizontal(|ui| {
                                for action in toast.action.into_iter().chain(toast.also) {
                                    let response = ui.small_button(action.label());
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            action.label(),
                                        )
                                    });
                                    buttons.push((action, response.rect, response.clicked()));
                                }
                            });
                        }
                    });
                let over = ui.interact(
                    frame.response.rect,
                    ui.id().with("dismiss"),
                    egui::Sense::click(),
                );
                // ⚠ The area over the whole toast is made after its buttons, so it takes the click a button was pressed with.
                let hit = (over.interact_pointer_pos())
                    .and_then(|at| buttons.iter().find(|(_, rect, _)| rect.contains(at)))
                    .or_else(|| buttons.iter().find(|(_, _, clicked)| *clicked))
                    .map(|(action, _, _)| *action);
                (
                    over.clicked() || buttons.iter().any(|(_, _, clicked)| *clicked),
                    hit,
                )
            });
        let (clicked, on_a_button) = shown.inner;
        if clicked {
            dismissed = Some(i);
            if on_a_button.is_some() {
                pressed = on_a_button;
            }
        }
        bottom = shown.response.rect.top() - 8.0;
    }
    if let Some(i) = dismissed {
        toasts.dismiss(i);
    }
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppearancePrefs, ThemeChoice};
    use crate::ui::theme::contrast;

    const LEVELS: [ToastType; 4] = [
        ToastType::Success,
        ToastType::Info,
        ToastType::Warning,
        ToastType::Error,
    ];

    /// ⚠ Falsified by white text on the level's own colour: on the success
    /// green it reads at 2.2:1.
    #[test]
    fn a_toast_reads_at_4_5_to_1_in_both_themes() {
        let _serial = theme::serial();
        let ctx = egui::Context::default();
        for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
            let appearance = AppearancePrefs {
                theme: choice,
                ..AppearancePrefs::default()
            };
            theme::apply_theme(&ctx, &appearance);
            let mut fills = std::collections::HashSet::new();
            for level in LEVELS {
                let (fill, text, accent, _) = toast_style(level);
                let ratio = contrast(text, fill);
                assert!(
                    ratio >= 4.5,
                    "{choice:?} {level:?}: the text reads at {ratio:.2}:1"
                );
                let ratio = contrast(accent, fill);
                assert!(
                    ratio >= 3.0,
                    "{choice:?} {level:?}: the icon shows at {ratio:.2}:1"
                );
                fills.insert(fill);
            }
            assert_eq!(
                fills.len(),
                LEVELS.len(),
                "{choice:?}: each level has its own colour"
            );
        }
        theme::apply_theme(&ctx, &AppearancePrefs::default());
    }

    /// ⚠ Falsified by stacking at a fixed height, where a message that wraps
    /// runs into the toast above it, or by no cap on the rows, where a long
    /// one pushes the stack off the screen.
    #[test]
    fn toasts_of_different_heights_stack_without_overlapping() {
        let _serial = theme::serial();
        let ctx = crate::app::gpu::configured_egui_context(None, &AppearancePrefs::default());
        let mut toasts = Toasts::default();
        toasts.success("Saved weapons.ytd");
        toasts.error(
            "Could not save weapons.ytd: another program holds the file open, so nothing was \
             written and the document keeps its changes",
        );
        toasts.info("Copied");
        toasts.warning(format!(
            "60 files could not be imported: {}",
            "C:\\mods\\a folder with a long name\\vehicles.ytd; ".repeat(60)
        ));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                draw_toasts(ui.ctx(), &mut toasts, &[]);
            });
        }

        let rects: Vec<egui::Rect> = toasts
            .iter()
            .map(|t| {
                ctx.memory(|m| m.area_rect(toast_area_id(t)))
                    .expect("drawn")
            })
            .collect();
        assert_eq!(rects.len(), 4);
        assert!(
            rects[1].height() > rects[0].height(),
            "the long message wraps onto a second line: {rects:?}"
        );
        assert!(
            rects[3].height() < 4.0 * rects[0].height(),
            "a message of any length stays a few rows tall: {rects:?}"
        );
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "toasts overlap: {rects:?}");
            }
            assert!(
                screen.contains_rect(*a),
                "a toast left the screen: {rects:?}"
            );
        }
    }
}

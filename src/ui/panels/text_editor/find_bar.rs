use egui::{Align2, Frame, Margin, Vec2};

use crate::ui::theme;

use super::editor_state::{FindMode, FindState};

#[derive(Default, Debug)]
pub struct FindBarOutcome {
    pub query_changed: bool,
    pub go_next: bool,
    pub go_prev: bool,
    pub replace_one: bool,
    pub replace_all: bool,
    pub cancel: bool,
    pub close: bool,
}

/// `replace_hint` is the replace command's binding, which the mode toggle names; `rope_revision` is the text's, which a search is of.
pub fn draw(
    ui: &mut egui::Ui,
    body: egui::Rect,
    state: &mut FindState,
    rope_revision: u64,
    replace_hint: Option<&str>,
) -> FindBarOutcome {
    let mut out = FindBarOutcome::default();

    let id = ui.id().with("text_editor_find_bar");
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .movable(false)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(body.right_top() + Vec2::new(-12.0, 12.0));
    let inner_response = area.show(ui.ctx(), |ui| {
        let frame = Frame::group(ui.style())
            .fill(theme::BG_L1.get())
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
            .inner_margin(Margin::symmetric(8, 6));
        // The rows wrap within what is left of `body` once 12 points clear each side.
        let room = body.width() - 24.0 - frame.total_margin().sum().x;
        frame
            .show(ui, |ui| {
                ui.set_max_width(room.max(0.0));
                ui.set_min_width(room.clamp(0.0, 420.0));
                ui.vertical(|ui| {
                    draw_find_row(ui, state, &mut out, replace_hint);
                    if matches!(state.mode, FindMode::Replace) {
                        draw_replace_row(ui, state, &mut out);
                    }
                    draw_status_row(ui, state, rope_revision, &mut out);
                });
            })
            .response
    });

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        out.close = true;
    }
    let bar_focused = inner_response.response.contains_pointer()
        || ui.memory(|m| {
            m.focused().is_some_and(|fid| {
                fid == id || fid.short_debug_format().contains("text_editor_find")
            })
        });
    if bar_focused {
        let (enter, shift_enter) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::Enter) && !i.modifiers.shift,
                i.key_pressed(egui::Key::Enter) && i.modifiers.shift,
            )
        });
        if enter {
            out.go_next = true;
        }
        if shift_enter {
            out.go_prev = true;
        }
    }

    out
}

fn draw_find_row(
    ui: &mut egui::Ui,
    state: &mut FindState,
    out: &mut FindBarOutcome,
    replace_hint: Option<&str>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Find")
                .color(theme::TEXT_SECONDARY)
                .small(),
        );
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .id_salt("text_editor_find_query")
                .desired_width(200.0)
                .hint_text("text or pattern"),
        );
        if resp.changed() {
            out.query_changed = true;
        }
        if state.want_focus_query {
            resp.request_focus();
            state.want_focus_query = false;
        }

        if toggle(ui, "Aa", state.case_sensitive, "Match case").clicked() {
            state.case_sensitive = !state.case_sensitive;
            out.query_changed = true;
        }
        if toggle(ui, "\\b", state.whole_word, "Whole word").clicked() {
            state.whole_word = !state.whole_word;
            out.query_changed = true;
        }
        if toggle(ui, ".*", state.regex, "Regex").clicked() {
            state.regex = !state.regex;
            out.query_changed = true;
        }
        if state.has_selection
            && toggle(
                ui,
                "In sel",
                state.in_selection,
                "Search inside selection only",
            )
            .clicked()
        {
            state.in_selection = !state.in_selection;
            out.query_changed = true;
        }

        if ui
            .small_button("▲")
            .on_hover_text("Previous match (Shift+Enter)")
            .clicked()
        {
            out.go_prev = true;
        }
        if ui
            .small_button("▼")
            .on_hover_text("Next match (Enter)")
            .clicked()
        {
            out.go_next = true;
        }
        let mode_glyph = match state.mode {
            FindMode::Find => "↳",
            FindMode::Replace => "↵",
        };
        let toggle_hint = match replace_hint {
            Some(keys) => format!("Toggle Find ↔ Replace ({keys})"),
            None => "Toggle Find ↔ Replace".to_string(),
        };
        if ui
            .small_button(mode_glyph)
            .on_hover_text(toggle_hint)
            .clicked()
        {
            state.mode = match state.mode {
                FindMode::Find => FindMode::Replace,
                FindMode::Replace => FindMode::Find,
            };
        }
        if ui
            .small_button(egui_phosphor::regular::X)
            .on_hover_text("Close (Esc)")
            .clicked()
        {
            out.close = true;
        }
    });
}

fn draw_replace_row(ui: &mut egui::Ui, state: &mut FindState, out: &mut FindBarOutcome) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Replace")
                .color(theme::TEXT_SECONDARY)
                .small(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.replacement)
                .id_salt("text_editor_find_replacement")
                .desired_width(200.0)
                .hint_text(if state.regex {
                    "replacement (use $1, $2, ${name})"
                } else {
                    "replacement"
                }),
        );
        if ui
            .small_button("Replace")
            .on_hover_text("Replace the active match (then advance)")
            .clicked()
        {
            out.replace_one = true;
        }
        if ui
            .small_button("Replace All")
            .on_hover_text("Replace every match")
            .clicked()
        {
            out.replace_all = true;
        }
    });
}

fn draw_status_row(
    ui: &mut egui::Ui,
    state: &FindState,
    rope_revision: u64,
    out: &mut FindBarOutcome,
) {
    ui.horizontal_wrapped(|ui| {
        // ⚠ Nothing is missing until a search has run: one waits for a pause in the typing.
        let count_text = if state.matches.is_empty() {
            if state.searching {
                "Searching…".to_string()
            } else if state.error.is_some()
                || state.query.is_empty()
                || !state.searched(rope_revision)
            {
                String::new()
            } else {
                "No results".to_string()
            }
        } else {
            let active = state
                .active
                .map(|i| i + 1)
                .unwrap_or(if state.matches.is_empty() { 0 } else { 1 });
            let suffix = if state.truncated {
                "+"
            } else if state.searching {
                " (searching…)"
            } else {
                ""
            };
            format!("{active} of {}{}", state.matches.len(), suffix)
        };
        let color = if state.error.is_some() {
            theme::ERROR
        } else {
            theme::TEXT_SECONDARY
        };
        ui.label(egui::RichText::new(&count_text).color(color).small());

        if let Some(err) = state.error.as_deref() {
            ui.label(egui::RichText::new(err).color(theme::ERROR).small());
        }
        if state.searching && ui.small_button("Cancel").clicked() {
            out.cancel = true;
        }
    });
}

fn toggle(ui: &mut egui::Ui, label: &str, on: bool, tip: &str) -> egui::Response {
    ui.add(egui::Button::selectable(on, label))
        .on_hover_text(tip)
}

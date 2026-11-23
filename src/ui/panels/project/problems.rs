//! Problems: what is wrong with the project, in one list, each with what can be done about it.
//!
//! The list can be filtered by severity and by words, ordered, gathered under a heading for each rule, content or severity,
//! and written out as it is shown; the rules of a clothing content are set from here.

use super::builds_editor::named_choice;
use super::{fitted, severity_colour, REFRESH};
use crate::state::workbench::{
    problem_about, problem_content, problem_heading, problem_rows, Finding, Fix, Place,
    ProblemGroup, ProblemRow, ProblemShow, ProblemSort, ProblemView, Scope,
};
use crate::state::AppState;
use crate::ui::panel_state::PanelState;
use crate::ui::theme;

/// What the person asked the list to show, kept while the project is open.
#[derive(Default)]
struct Shown {
    view: ProblemView,
}

impl PanelState for Shown {
    const KEY: &'static str = "problems-view";
}

/// What the toolbar asked for this frame.
#[derive(Default)]
struct Asked {
    export: bool,
    rules: bool,
    help: bool,
}

fn toolbar(ui: &mut egui::Ui, state: &AppState, view: &mut ProblemView) -> Asked {
    let mut asked = Asked::default();
    toolbar_row(ui, state, view, &mut asked);
    asked
}

/// ⚠ A row that wraps, so a narrow panel shows every control on a second line and none is cut off; the two lists are as wide in every state, so a longer choice does not move the buttons after them.
fn toolbar_row(ui: &mut egui::Ui, state: &AppState, view: &mut ProblemView, asked: &mut Asked) {
    ui.horizontal_wrapped(|ui| {
        for show in ProblemShow::ALL {
            let on = view.show == show;
            if ui
                .add(egui::Button::new(show.label()).selected(on))
                .clicked()
            {
                view.show = show;
            }
        }
        let field = ui.add(
            egui::TextEdit::singleline(&mut view.text)
                .hint_text(tr!("Filter problems"))
                .desired_width(120.0),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Filter problems"))
        });
        // ⚠ A list does not wrap by itself: it starts the next row when it would not fit this one, so no control is pushed past the panel's edge.
        if ui.available_size_before_wrap().x < 100.0 {
            ui.end_row();
        }
        let group = egui::ComboBox::from_id_salt("problems-group")
            .width(100.0)
            .selected_text(view.group.label())
            .show_ui(ui, |ui| {
                for group in ProblemGroup::ALL {
                    ui.selectable_value(&mut view.group, group, group.label());
                }
            });
        named_choice(&group.response, tr!("Group the problems"), view.group.label());
        if ui.available_size_before_wrap().x < 128.0 {
            ui.end_row();
        }
        let sort = egui::ComboBox::from_id_salt("problems-sort")
            .width(128.0)
            .selected_text(view.sort.label())
            .show_ui(ui, |ui| {
                for sort in ProblemSort::ALL {
                    ui.selectable_value(&mut view.sort, sort, sort.label());
                }
            });
        named_choice(&sort.response, tr!("Order the problems"), view.sort.label());
        if ui
            .button(tr!("Export…"))
            .on_hover_text(tr!("Write the problems shown to a file: text, CSV or JSON by its name"))
            .clicked()
        {
            asked.export = true;
        }
        if state.has_clothing()
            && ui
                .button(tr!("Rules…"))
                .on_hover_text(tr!("Set which rules the clothing is held to, how serious each is and where its limit stands"))
                .clicked()
        {
            asked.rules = true;
        }
    });
}

/// Whether the project holds nothing a problem could be about yet: no file and no garment.
fn nothing_to_check(state: &AppState) -> bool {
    state.workbench.rows().is_empty()
        && (state.workbench.packs().iter()).all(|p| {
            p.read
                .as_ref()
                .is_ok_and(|r| r.wardrobe.garments.is_empty())
        })
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if state.project_path.is_none() {
        theme::empty_message(ui, tr!("No project is open, so it has no problems"));
        return;
    }
    ui.ctx().request_repaint_after(REFRESH);
    let findings: Vec<Finding> = state.workbench.findings().to_vec();
    if findings.is_empty() {
        ui.centered_and_justified(|ui| {
            let said = if !state.workbench.is_scanned() {
                tr!("Reading the project…")
            } else if nothing_to_check(state) {
                tr!("Nothing to check yet: add clothes or files")
            } else {
                tr!("No problems: every file is there and every build plan is clear")
            };
            ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
        });
        return;
    }
    let mut shown = state.panels.take::<Shown>();
    let mut asked = toolbar(ui, state, &mut shown.view);
    let container = state.project_carried.container.clone();
    let rows = problem_rows(&findings, &shown.view, &container);
    let kept = rows
        .iter()
        .filter(|row| matches!(row, ProblemRow::Finding(_)))
        .count();
    let mut said = state.workbench.counts(Scope::Project).said();
    if shown.view.filters() {
        said = tr!("{said}; {kept} shown", said = said, kept = kept);
    }
    // ⚠ The `?` stands with the sentence it explains and not at the end of the toolbar, where a narrow panel left it alone on a row of its own.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            asked.help = super::help_button(ui, super::GUIDE_PROBLEMS);
        });
    });
    ui.separator();
    if rows.is_empty() {
        ui.label(
            egui::RichText::new(tr!("No problem matches what is asked for"))
                .color(theme::TEXT_SECONDARY),
        );
    }

    let mut fix: Option<Fix> = None;
    let mut select: Option<String> = None;
    let mut build: Option<String> = None;
    let mut garment: Option<(String, String)> = None;
    let mut laid = state.panels.take::<Laid>();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // ⚠ A message is as tall as its words need at this width, so every row's height is measured, and only the rows in view are drawn.
            let width = ui.available_width();
            laid.fit(ui, &rows, width);
            let heights = &laid.heights;
            ui.set_height(heights.iter().sum());
            let origin = ui.max_rect().min;
            let mut top = 0.0;
            for ((row, height), lines) in rows.iter().zip(heights).zip(&laid.lines) {
                let (above, below) = (top + height < viewport.min.y, top > viewport.max.y);
                if !above && !below {
                    let rect = egui::Rect::from_min_size(
                        origin + egui::vec2(0.0, top),
                        egui::vec2(width, *height),
                    );
                    match row {
                        ProblemRow::Heading { .. } => {
                            // ⚠ A band with its count first, so a heading is not one more message.
                            ui.painter()
                                .rect_filled(rect, theme::RADIUS_SMALL, theme::BG_L2.get());
                            let inner = rect.shrink2(egui::vec2(8.0, 4.0));
                            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                                ui.label(egui::RichText::new(lines).strong());
                            });
                        }
                        ProblemRow::Finding(finding) => {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "● {}",
                                            finding.severity.label()
                                        ))
                                        .color(severity_colour(finding.severity)),
                                    );
                                    // The content a file or a garment is in, by the name the Content panel gives it, so a row of a project of many contents is not a guess; a heading that says it already, over a list grouped by content, is not said again on every row.
                                    let named = container.modules.len() > 1
                                        && shown.view.group != ProblemGroup::Content;
                                    if let (
                                        true,
                                        Place::Asset { .. } | Place::Garment { id: Some(_), .. },
                                    ) = (named, &finding.place)
                                    {
                                        ui.label(
                                            egui::RichText::new(problem_content(
                                                finding, &container,
                                            ))
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                    }
                                    let about = ui.link(problem_about(finding, &container));
                                    if about.clicked() {
                                        match &finding.place {
                                            Place::Build(id) => build = Some(id.clone()),
                                            Place::Asset { key, .. } => select = Some(key.clone()),
                                            Place::Garment {
                                                content,
                                                id: Some(id),
                                                ..
                                            } => garment = Some((content.clone(), id.clone())),
                                            _ => {}
                                        }
                                    }
                                    // ⚠ The buttons on the first line, from the right: the message is the line under, as wide as the panel.
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if let Some(chosen) =
                                                super::fixes::buttons(ui, finding, true)
                                            {
                                                fix = Some(chosen);
                                            }
                                        },
                                    );
                                });
                                ui.add(egui::Label::new(lines).wrap());
                            });
                        }
                    }
                }
                top += height;
            }
        });

    state.panels.put(laid);
    let view = shown.view.clone();
    state.panels.put(shown);
    if asked.export {
        state.export_problems(&view);
    }
    if asked.help {
        state.front_door.show_guide(super::GUIDE_PROBLEMS);
    }
    if asked.rules {
        if let Some(module) = state.wardrobe_module_id() {
            state.ask_about_rules(module);
        }
    }
    if let Some(key) = select {
        state.select_asset(Some(key));
    }
    if let Some(id) = build {
        state.workbench.selected_output = Some(id);
    }
    if let Some((content, id)) = garment {
        state.show_garment(&content, id);
    }
    if let Some(fix) = fix {
        super::fixes::apply(state, fix);
    }
}

/// The height of the first line of a row: what it is about, and its buttons.
const FIRST_LINE: f32 = 24.0;
/// The space under a row's message.
const BELOW: f32 = 8.0;
/// The height of a heading over a group that says its words in one line.
const HEADING: f32 = 26.0;
/// The space a heading takes around the lines of its words.
const HEADING_PAD: f32 = 10.0;

/// The messages and headings as the panel's width lays them out, kept while neither the width nor the words change.
///
/// ⚠ A message and a heading are broken at a space or after a path separator and are as tall as their lines need (a heading
/// is a rule's whole summary when the list is grouped by rule), so a long list is laid out once and not on every frame.
#[derive(Default)]
struct Laid {
    width: f32,
    /// What each row says, and whether it is a heading.
    texts: Vec<(bool, String)>,
    lines: Vec<String>,
    heights: Vec<f32>,
}

impl PanelState for Laid {
    const KEY: &'static str = "problems-layout";
}

impl Laid {
    fn fit(&mut self, ui: &egui::Ui, rows: &[ProblemRow], width: f32) {
        let texts: Vec<(bool, String)> = rows
            .iter()
            .map(|row| match row {
                ProblemRow::Heading { label, counts } => (true, problem_heading(label, *counts)),
                ProblemRow::Finding(f) => (false, crate::state::front_door::spoken(&f.text)),
            })
            .collect();
        if (self.width - width).abs() < 0.5 && self.texts == texts {
            return;
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        self.width = width;
        self.lines = texts
            .iter()
            .map(|(_, text)| fitted(ui, text, &font, width))
            .collect();
        self.heights = texts
            .iter()
            .zip(&self.lines)
            .map(|((heading, _), lines)| {
                let said =
                    ui.painter()
                        .layout(lines.clone(), font.clone(), egui::Color32::WHITE, width);
                match heading {
                    true => HEADING.max(said.size().y + HEADING_PAD),
                    false => FIRST_LINE + said.size().y + BELOW,
                }
            })
            .collect();
        self.texts = texts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::workbench::Counts;

    /// A heading over a group of `count` warnings, which is the summary of a rule when the list is grouped by rule.
    fn heading(label: &str, count: usize) -> ProblemRow<'static> {
        ProblemRow::Heading {
            label: label.to_string(),
            counts: Counts {
                errors: 0,
                warnings: count,
            },
        }
    }

    #[test]
    fn a_heading_as_long_as_a_sentence_is_as_tall_as_its_lines_and_a_short_one_keeps_its_height() {
        let long = "The medium level of detail of a model has more than the limit's share of the vertices of its high one, so it saves too little";
        let rows = [heading(long, 1), heading("Texture size", 3)];
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut laid = Laid::default();
            // ⚠ A panel is a few hundred points wide, and a rule's summary is more than one line of them.
            laid.fit(ui, &rows, 300.0);

            let font = egui::TextStyle::Body.resolve(ui.style());
            let line = ui.fonts_mut(|fonts| fonts.row_height(&font));
            let lines = laid.lines[0].lines().count();
            assert!(
                lines >= 2,
                "the heading wraps at this width: {:?}",
                laid.lines
            );
            assert!(
                laid.heights[0] >= lines as f32 * line,
                "{lines} lines of {line} do not fit in {}",
                laid.heights[0]
            );
            assert_eq!(laid.heights[1], HEADING, "a short heading is one line");
        });
    }
}

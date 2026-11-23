//! Import this pack as a new project?: the question a built pack asks, naming the folder the project is made in.
//!
//! ⚠ Import, as every game file is: a pack becomes sources in a new folder and the pack is never changed. The refusal says it in one sentence at the size of the question; the reader's own words are its hover.

use crate::platform::file_dialogs;
use crate::state::workbench::{what_it_is, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_titled};
use crate::ui::theme;

/// What the question is known by to the shell and to Escape, whatever file it is about: a modal that changed its identity with its heading would be a new window.
const KEY: &str = "Import built pack";

/// A text field named for a screen reader; true once Enter was pressed in it.
fn field(ui: &mut egui::Ui, text: &mut String, name: &str, hint: &str) -> (egui::Response, bool) {
    let field = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(300.0),
    );
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    (field, entered)
}

/// Whether the question asks for the pack's name: only when the reader has refused the one the pack states.
fn asks_for_a_pack_name(pack: &str, error: &Option<String>) -> bool {
    !pack.is_empty()
        || error
            .as_deref()
            .is_some_and(|why| why.contains("name of the pack"))
}

/// The refusal as a sentence, and the reader's own words, which go in a hover.
fn sentence_and_details(why: &str) -> (&str, Option<&str>) {
    match why.split_once('\n') {
        Some((sentence, details)) => (sentence, Some(details)),
        None => (why, None),
    }
}

pub fn draw(ctx: &egui::Context, state: &mut AppState, asked: Question) {
    let Question::Reopen {
        built,
        mut place,
        mut name,
        mut pack,
        mut error,
        focus,
    } = asked
    else {
        return;
    };
    let file = built
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut answer: Option<bool> = None;
    let title = tr!("Import {file} as a new project?", file = file);
    question_titled(ctx, KEY, &title, 560.0, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&file).strong())
                .on_hover_text(built.display().to_string());
            ui.label(tr!("is {what}.", what = what_it_is(&built)));
        });
        ui.add(
            egui::Label::new(
                tr!("Every model and texture in the pack becomes a source you can edit, in a new folder. \
                 The pack and your other files are never changed."),
            )
            .wrap(),
        );
        ui.add_space(theme::SPACING_SNUG);
        egui::Grid::new("reopen_fields")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr!("Name"));
                let (named, entered) = field(ui, &mut name, tr!("Name of the project folder"), "");
                // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
                if focus || (!named.has_focus() && name.is_empty()) {
                    named.request_focus();
                }
                if named.changed() {
                    error = None;
                }
                if entered && !name.trim().is_empty() {
                    answer = Some(true);
                }
                ui.end_row();

                ui.label(tr!("Create in"));
                ui.horizontal(|ui| {
                    let (placed, _) =
                        field(ui, &mut place, tr!("Folder the project is made in"), "");
                    if placed.changed() {
                        error = None;
                    }
                    if ui.button(tr!("Browse…")).clicked() {
                        if let Some(picked) =
                            file_dialogs::pick_folder(tr!("Choose Folder For The New Project"))
                        {
                            place = picked.display().to_string();
                            error = None;
                        }
                    }
                });
                ui.end_row();

                if asks_for_a_pack_name(&pack, &error) {
                    ui.label(tr!("Pack name"));
                    let (_, _) = field(
                        ui,
                        &mut pack,
                        tr!("Name of the pack"),
                        tr!("a lower case name"),
                    );
                    ui.end_row();
                }
            });
        if name.trim().is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("Name the new project to go on"))
                        .color(theme::TEXT_SECONDARY),
                )
                .wrap(),
            );
        }
        // ⚠ A row of its own height whether it holds a refusal or not, so the buttons stay under the pointer when one comes.
        let reserved = 2.0 * ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), reserved),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                // ⚠ A child ui is allocated what it used: the height it reserves is asked for here.
                ui.set_min_height(reserved);
                if let Some(why) = &error {
                    let (sentence, details) = sentence_and_details(why);
                    let said = ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "{} {sentence}",
                                egui_phosphor::regular::WARNING
                            ))
                            .color(theme::ERROR),
                        )
                        .wrap(),
                    );
                    if let Some(details) = details {
                        said.on_hover_text(details);
                    }
                }
            },
        );
        ui.horizontal(|ui| {
            // ⚠ Filled as every front-door button that goes on is; waiting for a name it is a plain button with its reason on hover.
            let named = !name.trim().is_empty();
            let go = if named {
                egui::Button::new(
                    egui::RichText::new(tr!("Import as project")).color(theme::ON_ACCENT.get()),
                )
                .fill(theme::ACCENT.get())
            } else {
                egui::Button::new(tr!("Import as project"))
            };
            if ui
                .add_enabled(named, go)
                .on_disabled_hover_text(tr!("Name the new project first"))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, KEY) {
        answer = Some(false);
    }
    let asked = Question::Reopen {
        built,
        place,
        name,
        pack,
        error,
        focus: false,
    };
    match answer {
        Some(true) => state.answer_reopen(asked),
        Some(false) => state.workbench.asking = None,
        None => state.workbench.asking = Some(asked),
    }
}

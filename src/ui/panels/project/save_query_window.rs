//! Save query: the question that names a query the pack keeps as a group of the list.

use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Save a query");

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    query: String,
    mut name: String,
    mut error: Option<String>,
    focus: bool,
) {
    let mut answer: Option<bool> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 380.0, |ui| {
        ui.set_max_width(480.0);
        ui.add(
            egui::Label::new(tr!("The query {query} is kept in the pack under a name, and the list offers it as a group. It is read again each time, so a garment tagged later joins it.", query = query))
            .wrap(),
        );
        ui.add_space(theme::SPACING_SNUG);
        ui.horizontal(|ui| {
            ui.label(tr!("Name"));
            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    true,
                    tr!("Name of the saved query"),
                )
            });
            // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
            if focus || (!field.has_focus() && name.is_empty()) {
                field.request_focus();
            }
            if field.changed() {
                error = None;
            }
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !name.trim().is_empty()
            {
                answer = Some(true);
            }
        });
        if let Some(why) = &error {
            ui.add(egui::Label::new(egui::RichText::new(why).small().color(theme::ERROR)).wrap());
        }
        ui.add_space(theme::SPACING_DEFAULT);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!name.trim().is_empty(), egui::Button::new(tr!("Save")))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(false);
    }
    match answer {
        Some(true) => match state.query_problem(&module, &name, &query) {
            None => {
                state.workbench.asking = None;
                state.save_query(module, name, query);
            }
            Some(why) => {
                state.workbench.asking = Some(Question::SaveQuery {
                    module,
                    query,
                    name,
                    error: Some(why),
                    focus: true,
                });
            }
        },
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::SaveQuery {
                module,
                query,
                name,
                error,
                focus: false,
            });
        }
    }
}

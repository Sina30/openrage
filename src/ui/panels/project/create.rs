//! New: the question that names a source started empty. The source is made under `sources/`, selected in the Assets table, and then filled.

use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

/// Why `name` cannot be the name of the new source's folder, in a sentence; none for a name that can be, and for none typed yet.
fn name_fault(name: &str) -> Option<String> {
    let name = name.trim();
    or_source::asset::unsafe_file_name(name)?;
    if name.is_empty() {
        return None;
    }
    Some(if name.ends_with(['.', ' ']) {
        tr!("A name cannot end with a dot or a space").to_string()
    } else if name
        .chars()
        .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        tr!("A name cannot hold any of these: / \\ : * ? \" < > |").to_string()
    } else {
        tr!(
            "{name} is a name Windows keeps for itself: choose another",
            name = name
        )
    })
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    kind: String,
    label: String,
    mut name: String,
    mut error: Option<String>,
    focus: bool,
) {
    let title = tr!("New {kind}", kind = label.to_lowercase());
    let mut answer: Option<bool> = None;
    // ⚠ Said before Create is pressed, so the button waits for a name that can be one.
    let fault = name_fault(&name);
    question_within(ctx, &title, 380.0, |ui| {
        ui.set_max_width(480.0);
        ui.add(
            egui::Label::new(
                tr!("An empty texture dictionary in your project. Put pictures (PNG, TGA, TIFF, BMP, JPG or DDS) in its textures folder to fill it."),
            )
            .wrap(),
        );
        ui.add_space(theme::SPACING_SNUG);
        ui.horizontal(|ui| {
            ui.label(tr!("Name"));
            let field_id = egui::Id::new("new_source_name");
            let field = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .id(field_id)
                    .desired_width(260.0),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    true,
                    tr!("Name of the new source"),
                )
            });
            // ⚠ Asked for until it has it: a modal takes away the focus that the first frame asked for.
            if focus || (!field.has_focus() && name.is_empty()) {
                field.request_focus();
            }
            // The name offered is selected, so typing replaces it.
            if focus {
                if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), field_id) {
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(name.chars().count()),
                    );
                    edit.cursor.set_char_range(Some(all));
                    edit.store(ui.ctx(), field_id);
                }
            }
            if field.changed() {
                error = None;
            }
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !name.trim().is_empty()
                && fault.is_none()
            {
                answer = Some(true);
            }
        });
        if let Some(why) = fault.as_ref().or(error.as_ref()) {
            ui.add(egui::Label::new(egui::RichText::new(why).color(theme::ERROR)).wrap());
        }
        ui.add_space(theme::SPACING_DEFAULT);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !name.trim().is_empty() && fault.is_none(),
                    egui::Button::new(tr!("Create")),
                )
                .on_disabled_hover_text(match fault {
                    Some(_) => tr!("Choose a name a folder can have"),
                    None => tr!("Type a name first"),
                })
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if question_escaped(ctx, &title) {
        answer = Some(false);
    }
    match answer {
        Some(true) => match state.create_source(&kind, &name) {
            Ok(_) => state.workbench.asking = None,
            Err(why) => {
                state.workbench.asking = Some(Question::NewSource {
                    kind,
                    label,
                    name,
                    error: Some(why),
                    focus: true,
                });
            }
        },
        Some(false) => state.dismiss_new_source(),
        None => {
            state.workbench.asking = Some(Question::NewSource {
                kind,
                label,
                name,
                error,
                focus: false,
            });
        }
    }
}

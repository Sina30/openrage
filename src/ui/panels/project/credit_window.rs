//! Credit: the question that states who made the sources of a garment and on what terms, in the `asset.json` of each, for the credits file of a build.

use super::readable::primary;
use crate::state::workbench::{CreditDraft, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Credit garment");

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    garment: String,
    mut draft: CreditDraft,
    had: bool,
) {
    let name = state.garment_display_name(&module, &garment);
    let mut answer: Option<bool> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 480.0, |ui| {
        ui.add(
            egui::Label::new(tr!("Who made {name}, and on what terms it may be used. Saved with the source files, so it goes wherever they go.", name = name))
            .wrap(),
        )
        .on_hover_text(tr!("It is written in the asset.json of each source folder the garment names; a build that writes a credits file lists it"));
        ui.add_space(theme::SPACING_SNUG);
        egui::Grid::new("credit_fields")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for (label, hint, value, name) in [
                    (
                        tr!("Author"),
                        tr!("who made it"),
                        &mut draft.author,
                        tr!("Author of the sources"),
                    ),
                    (
                        tr!("Link"),
                        tr!("where it was found"),
                        &mut draft.url,
                        tr!("Link to the sources"),
                    ),
                    (
                        tr!("Licence"),
                        tr!("such as CC-BY-4.0"),
                        &mut draft.licence,
                        tr!("Licence of the sources"),
                    ),
                ] {
                    ui.label(label);
                    // ⚠ Lighter and slanted: the hint of a field that is empty must not read as what was typed.
                    let field = ui.add(
                        egui::TextEdit::singleline(value)
                            .desired_width(340.0)
                            .hint_text(
                                egui::RichText::new(hint)
                                    .italics()
                                    .color(theme::TEXT_SECONDARY),
                            ),
                    );
                    field.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name)
                    });
                    ui.end_row();
                }
            });
        ui.add_space(theme::SPACING_DEFAULT);
        // ⚠ Said above the buttons and in the body size: this is the one edit of the Wardrobe that Undo does not take back.
        ui.add(
            egui::Label::new(tr!(
                "A credit is written into the source files, so Undo does not take it back."
            ))
            .wrap(),
        );
        ui.add_space(theme::SPACING_SNUG);
        let stated = draft != CreditDraft::default();
        ui.horizontal(|ui| {
            let save = ui.add_enabled(stated, primary(tr!("Save credit")));
            if save
                .on_disabled_hover_text(tr!("Type who made it, where it was found or its licence first: to take a credit away, use Remove credit"))
                .clicked()
            {
                answer = Some(true);
            }
            let remove = ui.add_enabled(had, egui::Button::new(tr!("Remove credit")));
            if remove
                .on_hover_text(tr!("Take the credit out of the source files: the author, the link and the licence are cleared"))
                .on_disabled_hover_text(tr!("These sources state no credit"))
                .clicked()
            {
                draft = CreditDraft::default();
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
        Some(true) => {
            state.workbench.asking = None;
            state.credit_garment(module, garment, draft);
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::Credit {
                module,
                garment,
                draft,
                had,
            })
        }
    }
}

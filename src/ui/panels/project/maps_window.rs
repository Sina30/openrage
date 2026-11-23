//! The maps window: a normal map and a specular map made of the diffuse textures of the garments picked, or of the whole pack, listed before they are made.
//!
//! The window edits a draft and Make is one step of Undo, so ticking a box is not a write. Nothing here knows how a map is made: `or-source` does,
//! and the lines the window lists are the plan it gives for the draft.

use crate::state::workbench::{MapsDraft, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Make normal and specular maps");
/// How many lines of the plan the window shows before it says there are more.
const SHOWN: usize = 24;

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    garments: Option<Vec<String>>,
    mut draft: MapsDraft,
    mut lines: Vec<String>,
) {
    let was = draft;
    let mut answer: Option<bool> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 540.0, |ui| {
        ui.label(match &garments {
            Some(ids) => tr!(
                "For the diffuse textures of the {count} garment(s) picked.",
                count = ids.len()
            ),
            None => tr!("For the diffuse textures of every garment in the pack.").to_string(),
        });
        ui.label(
            egui::RichText::new(
                tr!("A map is a new picture in a copy of the texture's source, beside the diffuse; the garments name the copy, the source it was is untouched and Undo takes the copy away. The maps are an estimate from the brightness of the diffuse: an artist's own maps replace them by name."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_SNUG);
        ui.checkbox(&mut draft.normal, tr!("Normal map")).on_hover_text(
            tr!("The brightness of the diffuse read as a height; green points down the picture, as the game's own maps do"),
        );
        ui.checkbox(&mut draft.specular, tr!("Specular map")).on_hover_text(
            tr!("Shine that rises with the brightness of the diffuse; the falloff and the Fresnel term are the same everywhere"),
        );
        ui.horizontal(|ui| {
            ui.label(tr!("Strength"));
            let mut strength = draft.strength as f32 / 10.0;
            let field = ui.add(
                egui::DragValue::new(&mut strength)
                    .range(0.1..=20.0)
                    .speed(0.1),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    true,
                    tr!("Normal map strength"),
                )
            });
            field.on_hover_text(tr!(
                "How steeply the brightness of the diffuse tilts the normal map"
            ));
            draft.strength = (strength * 10.0).round().max(1.0) as u32;
        });
        ui.separator();
        ui.label(tr!("What would be made"));
        egui::ScrollArea::vertical()
            .id_salt("maps_plan")
            .max_height(220.0)
            .show(ui, |ui| {
                for line in lines.iter().take(SHOWN) {
                    ui.label(egui::RichText::new(line).small().monospace());
                }
                if lines.len() > SHOWN {
                    ui.label(
                        egui::RichText::new(tr!(
                            "and {count} more lines",
                            count = lines.len() - SHOWN
                        ))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                    );
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(draft.asks(), egui::Button::new(tr!("Make")))
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
    if draft != was {
        lines = state.maps_words(&module, garments.as_deref(), &draft);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.make_maps(module, garments, draft);
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::Maps {
                module,
                garments,
                draft,
                lines,
            })
        }
    }
}

//! The texture optimiser's window: what to do to the textures of the garments picked, or of the whole pack.
//!
//! The window edits a draft and Optimise makes it one step of Undo, so ticking a box is not a write. Nothing here knows what
//! a texture comes to: the optimiser says that when it has run, in the Log.

use super::readable::primary;
use crate::state::front_door::counted;
use crate::state::workbench::{Cut, Draft, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Optimise textures");

/// What the optimiser leaves alone, said once, for the hover of the sentence that says nothing of yours changes.
const LEFT_ALONE: &str = tr_key!("A texture that stores the mips under its top level is cut by dropping the top levels, and the levels it keeps are as they were. The normal and specular maps embedded in the models are made lighter the same way, each model becoming a new source the garment names. A compiled .ytd, a picture and a reference are listed in the Log and left as they are.");

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    garments: Option<Vec<String>>,
    mut draft: Draft,
) {
    let picked = state.picked_garments();
    let in_the_pack = state
        .wardrobe_pack()
        .and_then(|pack| pack.read.as_ref().ok())
        .map_or(0, |read| read.wardrobe.garments.len());
    let mut whole_pack = garments.is_none();
    let mut answer: Option<bool> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 500.0, |ui| {
        ui.label(tr!("Make the textures of these garments lighter:"));
        if !picked.is_empty() {
            let words = match picked.len() {
                1 => tr!("The garment picked").to_string(),
                n => tr!("The {n} garments picked", n = n),
            };
            ui.radio_value(&mut whole_pack, false, words);
        }
        ui.radio_value(
            &mut whole_pack,
            true,
            tr!(
                "The whole pack ({garments})",
                garments = counted(in_the_pack, "garment")
            ),
        );
        ui.add_space(theme::SPACING_SNUG);
        for (on, label, hint) in [
            (
                &mut draft.format,
                tr!("Store each picture in the smallest format that keeps it"),
                tr!("A texture stored uncompressed becomes BC1 when it is opaque and BC3 when it is not; a BC7 texture does too in a pack built for Legacy, whose DirectX 10 cards read none"),
            ),
            (
                &mut draft.mips,
                tr!("Add the smaller copies the game draws at a distance"),
                tr!("These are the mips: a texture of more than 4 pixels that stores a single mip is given the chain, under the blocks it has"),
            ),
            (
                &mut draft.power_of_two,
                tr!("Round odd sizes to 256, 512, 1024, 2048…"),
                tr!("A side that is not a power of two goes to the nearest one"),
            ),
        ] {
            let said = label.to_string();
            let response = ui.checkbox(on, label).on_hover_text(hint);
            let checked = *on;
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, said.clone())
            });
        }
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(tr!("Make pictures smaller:"));
        ui.horizontal_wrapped(|ui| {
            for cut in Cut::CHOICES {
                ui.radio_value(&mut draft.cut, cut, cut.label());
            }
        });
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "Your originals are never changed; Undo brings the old textures back."
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        )
        .on_hover_text(crate::i18n::tr(LEFT_ALONE));
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(draft.asks(), primary(tr!("Optimise")))
                .on_disabled_hover_text(tr!("Choose at least one fix or a smaller size"))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    let garments = match whole_pack || picked.is_empty() {
        true => None,
        false => Some(picked),
    };
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.optimise_textures(module, garments, draft);
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::Optimise {
                module,
                garments,
                draft,
            })
        }
    }
}

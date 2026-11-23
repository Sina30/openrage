//! The mesh doctor in the Garment panel: what the models of the garment hold, and the steps that give them levels of detail, mend their rig, clean their weights and turn them over.
//!
//! ⚠ Thin by design: `or-wardrobe::mesh` looks and changes, `state.workbench.wardrobe.mesh` runs it off the UI thread, and this
//! asks. Each step makes a new source the garment names, and the viewport can draw the one it replaced beside it.

use or_wardrobe::Garment;

use super::garment::command_button;
use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::theme;

/// How many lines of what a look found the panel shows before it says there are more.
const SHOWN: usize = 14;

/// What each repair does, in a line under its button.
const REPAIRS: [(Action, &str); 4] = [
    (
        Action::RepairRig,
        tr_key!("Mends bone names and weights no skeleton knows."),
    ),
    (
        Action::CleanWeights,
        tr_key!("Drops tiny weights and makes the rest add up."),
    ),
    (Action::MirrorModel, tr_key!("Turns left and right over.")),
    (
        Action::CropUvs,
        tr_key!("Cuts the textures to the part the model uses."),
    ),
];

/// What the doctor's controls asked for, carried out once the panel is drawn.
#[derive(Default)]
pub(super) struct Asked {
    /// The commands the buttons ran.
    pub run: Vec<Action>,
    /// The shares of the Medium and Low levels and whether a level that is there is made again, when the controls changed them.
    pub set: Option<(u8, u8, bool)>,
    /// Draw the model the last step replaced in place of the one that took its place, or not.
    pub show_before: Option<bool>,
}

/// A weak line under a control: what it does.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// The doctor's section for garment `g`.
pub(super) fn draw(ui: &mut egui::Ui, state: &AppState, g: &Garment, asked: &mut Asked) {
    if g.model.is_none() {
        return;
    }
    ui.add_space(theme::SPACING_DEFAULT);
    // Closed until asked for, since the doctor's buttons are for a garment a person means to change and a panel that opens on them is longer for every other; open, by its own state, while a look or a step before stands for the garment, which is what the person asked to see.
    let desk = &state.workbench.wardrobe.mesh;
    let standing = desk.looked.as_ref().is_some_and(|l| l.garment == g.id)
        || desk
            .before
            .as_ref()
            .is_some_and(|b| b.garment == g.id && g.model.as_deref() == Some(b.now.as_str()));
    let folded = egui::CollapsingHeader::new(tr!("Mesh doctor"))
        .id_salt(("mesh doctor", standing))
        .default_open(standing)
        .show(ui, |ui| body(ui, state, g, asked));
    // ⚠ A fold that opens at the foot of a panel shows nothing of itself: bring it up, once, when it opens.
    let was = ui.id().with(tr!("mesh doctor was standing"));
    let before = ui.data_mut(|d| d.get_temp::<bool>(was)).unwrap_or(false);
    ui.data_mut(|d| d.insert_temp(was, standing));
    if (standing && !before) || (folded.header_response.clicked() && folded.body_response.is_some())
    {
        folded.header_response.scroll_to_me(Some(egui::Align::TOP));
    }
}

fn body(ui: &mut egui::Ui, state: &AppState, g: &Garment, asked: &mut Asked) {
    let desk = &state.workbench.wardrobe.mesh;
    ui.horizontal_wrapped(|ui| {
        if command_button(ui, state, Action::CheckMesh) {
            asked.run.push(Action::CheckMesh);
        }
        if state.is_looking_at_a_mesh() {
            ui.spinner();
        }
    });
    if let Some(looked) = desk.looked.as_ref().filter(|l| l.garment == g.id) {
        match &looked.result {
            Ok(found) => {
                // One line a model, by the part of its file name that tells it apart, with the whole path in its hover.
                let mut shown = 0;
                let mut more = 0;
                egui::ScrollArea::vertical()
                    .id_salt("mesh_findings")
                    .max_height(170.0)
                    .show(ui, |ui| {
                        for model in &found.models {
                            for (at, line) in model.lines().into_iter().enumerate() {
                                if shown >= SHOWN {
                                    more += 1;
                                    continue;
                                }
                                shown += 1;
                                let label = ui.label(egui::RichText::new(line).small());
                                if at == 0 {
                                    label.on_hover_text(&model.file);
                                }
                            }
                        }
                        if more > 0 {
                            ui.label(
                                egui::RichText::new(tr!("and {more} more lines", more = more))
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                    });
            }
            Err(why) => {
                ui.label(egui::RichText::new(why).small().color(theme::ERROR));
            }
        }
    }

    // The levels to add: the share of the High level's triangles each keeps, beside the button that adds them.
    ui.add_space(theme::SPACING_DEFAULT);
    caption(ui, tr!("Levels to add, in percent of High"));
    let (mut medium, mut low, mut replace) = (desk.medium, desk.low, desk.replace);
    ui.horizontal_wrapped(|ui| {
        for (label, share, name) in [
            (
                tr!("Medium"),
                &mut medium,
                tr!("Medium level share in percent"),
            ),
            (tr!("Low"), &mut low, tr!("Low level share in percent")),
        ] {
            ui.label(label);
            let field = ui.add(egui::DragValue::new(share).range(0..=100).suffix("%"));
            field
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, name));
            field.on_hover_text(tr!(
                "The share of the High level's triangles the {label} level keeps; 0 makes none",
                label = label
            ));
        }
        ui.checkbox(&mut replace, "Make again").on_hover_text(tr!(
            "Make again a level the model has already; off, the levels it has are left as they are"
        ));
    });
    if (medium, low, replace) != (desk.medium, desk.low, desk.replace) {
        asked.set = Some((medium, low, replace));
    }
    if command_button(ui, state, Action::AddLevels) {
        asked.run.push(Action::AddLevels);
    }

    ui.add_space(theme::SPACING_DEFAULT);
    caption(
        ui,
        tr!("Repairs: each makes a new source, and Undo takes it back"),
    );
    for (action, what) in REPAIRS {
        if command_button(ui, state, action) {
            asked.run.push(action);
        }
        caption(ui, crate::i18n::tr(what));
    }

    // The step before: the viewport draws the model it replaced, or the one that took its place.
    if let Some(before) = desk
        .before
        .as_ref()
        .filter(|b| b.garment == g.id && g.model.as_deref() == Some(b.now.as_str()))
    {
        let mut shown = before.shown;
        ui.checkbox(
            &mut shown,
            tr!("Show the model as it was before the last step"),
        )
        .on_hover_text(tr!(
            "The viewport draws {was} in place of {now}; the pack is not changed",
            was = before.was,
            now = before.now
        ));
        if shown != before.shown {
            asked.show_before = Some(shown);
        }
    }
}

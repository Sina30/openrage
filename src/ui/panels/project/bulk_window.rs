//! The bulk edit window: one change to every garment a query lists, with what it would change in how many garments said before it is made.
//!
//! The window edits a draft and Apply is one step of Undo, so choosing a change is not a write. Nothing here knows how a garment changes:
//! `or-wardrobe` does, and the lines the window lists are its dry run of the draft.

use or_wardrobe::Ped;

use super::readable::{arrows, primary};
use crate::state::front_door::counted;
use crate::state::workbench::{BulkDraft, BulkKind, Question};
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

/// How many garments of the dry run the window lists before it says there are more.
const SHOWN: usize = 12;

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    query: String,
    ids: Vec<String>,
    mut draft: BulkDraft,
    mut run: Option<std::sync::Arc<or_wardrobe::bulk::DryRun>>,
) {
    let was = draft.clone();
    let mut answer: Option<bool> = None;
    let title = tr!(
        "Edit these {garments}",
        garments = counted(ids.len(), "garment")
    );
    question_within(ctx, &title, 620.0, |ui| {
        ui.label(tr!("The garments the query {query} lists.", query = query));
        ui.add_space(theme::SPACING_SNUG);
        ui.horizontal(|ui| {
            ui.label(tr!("Change to make"));
            let chosen = egui::ComboBox::from_id_salt("bulk_kind")
                .selected_text(draft.kind.label())
                .show_ui(ui, |ui| {
                    for kind in BulkKind::ALL {
                        ui.selectable_value(&mut draft.kind, kind, kind.label());
                    }
                });
            chosen.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr!("Change to make"))
            });
        });
        if draft.kind.needs_text() {
            ui.horizontal(|ui| {
                ui.label(match draft.kind {
                    BulkKind::SetGroup => tr!("Group"),
                    _ => tr!("Tag"),
                });
                let field = ui.add(
                    egui::TextEdit::singleline(&mut draft.text)
                        .desired_width(220.0)
                        .hint_text(tr!("one word or name")),
                );
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        tr!("Tag or group to change"),
                    )
                });
            });
        }
        if draft.kind == BulkKind::SetPed {
            ui.horizontal(|ui| {
                for (ped, words) in [(Ped::Male, tr!("Male")), (Ped::Female, tr!("Female"))] {
                    ui.selectable_value(&mut draft.ped, ped, words);
                }
            });
        }
        ui.separator();
        match &run {
            None => {
                ui.label(
                    egui::RichText::new(tr!("Type the word, and this says what would change"))
                        .color(theme::TEXT_SECONDARY),
                );
            }
            Some(run) => {
                ui.label(egui::RichText::new(run.sentence()).strong());
                egui::ScrollArea::vertical()
                    .id_salt("bulk_plan")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        // The garment is the strongest thing in a row, and what happens to it comes second.
                        egui::Grid::new("bulk_changes")
                            .num_columns(2)
                            .striped(true)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                for changed in run.changed.iter().take(SHOWN) {
                                    ui.label(&changed.name);
                                    ui.vertical(|ui| {
                                        for line in &changed.lines {
                                            ui.label(
                                                egui::RichText::new(arrows(line))
                                                    .color(theme::TEXT_SECONDARY),
                                            );
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                        if run.changed.len() > SHOWN {
                            ui.label(
                                egui::RichText::new(tr!(
                                    "and {count} more garments",
                                    count = run.changed.len() - SHOWN
                                ))
                                .small()
                                .color(theme::TEXT_SECONDARY),
                            );
                        }
                        for (name, why) in run.refused.iter().take(SHOWN) {
                            ui.label(
                                egui::RichText::new(format!("{name} {why}"))
                                    .small()
                                    .color(theme::ERROR),
                            );
                        }
                    });
            }
        }
        ui.separator();
        let worth = run.as_ref().is_some_and(|r| r.is_worth_making());
        ui.horizontal(|ui| {
            let apply = ui
                .add_enabled(worth, primary(tr!("Apply")))
                .on_disabled_hover_text(
                    match draft.kind.needs_text() && draft.text.trim().is_empty() {
                        true => tr!("Type the word first"),
                        false => tr!("These garments already state it, or one of them refuses it"),
                    },
                );
            if apply.clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
        ui.label(
            egui::RichText::new(tr!("One Undo takes the whole change back."))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    });
    if question_escaped(ctx, &title) {
        answer = Some(false);
    }
    // ⚠ Made when the draft changes: a dry run copies the pack, and a window that made one for each frame would copy it for each frame.
    if draft != was {
        run = state.bulk_dry_run(&module, &ids, &draft);
    }
    match answer {
        Some(true) => {
            state.workbench.asking = None;
            state.bulk_edit(module, ids, draft);
        }
        Some(false) => state.workbench.asking = None,
        None => {
            state.workbench.asking = Some(Question::Bulk {
                module,
                query,
                ids,
                draft,
                shown: run,
            })
        }
    }
}

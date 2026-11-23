//! Selecting an array's records, saving one as a preset and applying a preset to the selection.

use std::cell::RefCell;

use egui_phosphor::regular as ph;
use or_io::meta::Value;

use crate::state::presets::{record_of, MetaStep, RecordSelection};
use crate::state::{AppState, DocumentId};
use crate::ui::theme;

use super::ctx::EditorCtx;

/// What the visual editor knows of the selection and presets while the document is borrowed.
#[derive(Default)]
pub struct RecordUi {
    pub document: DocumentId,
    pub selection: RecordSelection,
    /// Each loaded preset's name and record type, in the library's order.
    pub presets: Vec<(String, String)>,
    /// Copied records wait for a paste.
    pub pastes: bool,
    actions: RefCell<Vec<RecordAction>>,
}

pub enum RecordAction {
    Select {
        array: Vec<MetaStep>,
        indices: Vec<usize>,
    },
    Toggle {
        array: Vec<MetaStep>,
        index: usize,
    },
    Save {
        array: Vec<MetaStep>,
        index: usize,
        name: String,
        fields: Vec<String>,
    },
    Apply {
        preset: usize,
    },
    Copy,
    Paste,
}

/// The record whose fields the save dialog is choosing among.
#[derive(Clone)]
struct SaveDialog {
    array: Vec<MetaStep>,
    index: usize,
    name: String,
    chosen: Vec<String>,
}

fn dialog_id() -> egui::Id {
    egui::Id::new("meta_preset_save_dialog")
}

impl RecordUi {
    pub fn of(state: &AppState, document: DocumentId) -> Self {
        Self {
            document,
            selection: state.records_selected.clone(),
            presets: state
                .presets
                .list
                .iter()
                .map(|p| (p.name.clone(), p.type_name.clone()))
                .collect(),
            pastes: state.records_to_paste(),
            actions: RefCell::default(),
        }
    }

    fn push(&self, action: RecordAction) {
        self.actions.borrow_mut().push(action);
    }

    pub fn take(&self) -> Vec<RecordAction> {
        self.actions.take()
    }
}

/// The selection count, select all or none, and the apply menu, above an array of records.
pub fn array_header(ui: &mut egui::Ui, ctx: &EditorCtx, items: &[Value]) {
    if !items.iter().any(|item| record_of(item).is_some()) {
        return;
    }
    save_dialog(ui, ctx, items);
    let records = &ctx.records;
    let selected = records
        .selection
        .in_array(records.document, &ctx.path)
        .len();
    if ui
        .small_button(ph::CHECK_SQUARE)
        .on_hover_text("Select every record")
        .clicked()
    {
        records.push(RecordAction::Select {
            array: ctx.path.clone(),
            indices: (0..items.len()).collect(),
        });
    }
    if selected == 0 {
        return;
    }
    ui.label(
        egui::RichText::new(format!("{selected} selected"))
            .small()
            .color(theme::ACCENT_TEXT),
    );
    if ui.small_button("None").clicked() {
        records.push(RecordAction::Select {
            array: ctx.path.clone(),
            indices: Vec::new(),
        });
    }
    if ui
        .small_button("Copy")
        .on_hover_text("Copy the selected records, to paste into this file or another")
        .clicked()
    {
        records.push(RecordAction::Copy);
    }
    if ui
        .add_enabled(records.pastes, egui::Button::new("Paste").small())
        .on_hover_text("Paste the copied records after the last one selected")
        .on_disabled_hover_text("Copy records first")
        .clicked()
    {
        records.push(RecordAction::Paste);
    }
    ui.menu_button(egui::RichText::new("Apply preset").small(), |ui| {
        if records.presets.is_empty() {
            ui.label("No preset saved yet");
        }
        for (preset, (name, type_name)) in records.presets.iter().enumerate() {
            if ui.button(format!("{name}  ({type_name})")).clicked() {
                records.push(RecordAction::Apply { preset });
                ui.close_kind(egui::UiKind::Menu);
            }
        }
    });
}

/// Drop the selection if it is in this array and the array was reordered.
pub fn forget_selection_if(ctx: &EditorCtx, reordered: bool) {
    // ⚠ The selection is by index, so after a delete or a move it would name other records.
    let records = &ctx.records;
    if reordered
        && !records
            .selection
            .in_array(records.document, &ctx.path)
            .is_empty()
    {
        records.push(RecordAction::Select {
            array: ctx.path.clone(),
            indices: Vec::new(),
        });
    }
}

/// A record's selection box and its save-as-preset button, in its row header.
pub fn item_controls(ui: &mut egui::Ui, ctx: &EditorCtx, index: usize, item: &Value) {
    let Some(record) = record_of(item) else {
        return;
    };
    let records = &ctx.records;
    let mut on = records
        .selection
        .in_array(records.document, &ctx.path)
        .contains(&index);
    if ui.checkbox(&mut on, "").changed() {
        records.push(RecordAction::Toggle {
            array: ctx.path.clone(),
            index,
        });
    }
    if ui
        .small_button(ph::BOOKMARK_SIMPLE)
        .on_hover_text("Save fields of this record as a preset")
        .clicked()
    {
        let dialog = SaveDialog {
            array: ctx.path.clone(),
            index,
            name: String::new(),
            chosen: record.fields.iter().map(|(n, _)| n.clone()).collect(),
        };
        ui.ctx().data_mut(|d| d.insert_temp(dialog_id(), dialog));
    }
}

/// The save dialog, when it was opened on a record of this array.
fn save_dialog(ui: &mut egui::Ui, ctx: &EditorCtx, items: &[Value]) {
    let Some(mut dialog) = ui.ctx().data(|d| d.get_temp::<SaveDialog>(dialog_id())) else {
        return;
    };
    if dialog.array != ctx.path {
        return;
    }
    let Some(record) = items.get(dialog.index).and_then(record_of) else {
        ui.ctx().data_mut(|d| d.remove::<SaveDialog>(dialog_id()));
        return;
    };
    let mut open = true;
    let mut done = false;
    egui::Window::new(format!("Save a {} preset", record.type_name))
        .open(&mut open)
        .resizable(true)
        .default_width(320.0)
        .show(ui.ctx(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut dialog.name).hint_text("Preset name"));
            ui.label(
                egui::RichText::new("The chosen fields and their values here:")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (name, _) in &record.fields {
                        let mut on = dialog.chosen.contains(name);
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                dialog.chosen.push(name.clone());
                            } else {
                                dialog.chosen.retain(|n| n != name);
                            }
                        }
                    }
                });
            let ready = !dialog.name.trim().is_empty() && !dialog.chosen.is_empty();
            if ui.add_enabled(ready, egui::Button::new("Save")).clicked() {
                ctx.records.push(RecordAction::Save {
                    array: dialog.array.clone(),
                    index: dialog.index,
                    name: dialog.name.trim().to_string(),
                    fields: dialog.chosen.clone(),
                });
                done = true;
            }
        });
    ui.ctx().data_mut(|d| {
        if open && !done {
            d.insert_temp(dialog_id(), dialog);
        } else {
            d.remove::<SaveDialog>(dialog_id());
        }
    });
}

/// Carry out what the editor asked for once the document is no longer borrowed.
pub fn apply(state: &mut AppState, document: DocumentId, actions: Vec<RecordAction>) {
    for action in actions {
        match action {
            RecordAction::Select { array, indices } => {
                state.records_selected.set(document, &array, indices)
            }
            RecordAction::Toggle { array, index } => {
                state.records_selected.toggle(document, &array, index)
            }
            RecordAction::Save {
                array,
                index,
                name,
                fields,
            } => match state.save_preset(document, &array, index, &name, &fields) {
                Ok(path) => state.show_toast(format!("Saved preset {name} to {}", path.display())),
                Err(why) => state.show_error_toast(format!("The preset was not saved: {why}")),
            },
            RecordAction::Apply { preset } => match state.apply_preset(document, preset) {
                Ok(report) => {
                    for (_, why) in &report.refused {
                        tracing::warn!("preset not applied: {why}");
                    }
                    let mut message = format!("Applied the preset to {} record(s)", report.applied);
                    if let Some((_, why)) = report.refused.first() {
                        message.push_str(&format!(
                            "; {} refused ({why}{}); the log lists each",
                            report.refused.len(),
                            if report.refused.len() > 1 {
                                ", …"
                            } else {
                                ""
                            }
                        ));
                        state.show_warning_toast(message);
                    } else {
                        state.show_toast(message);
                    }
                }
                Err(why) => state.show_error_toast(format!("The preset was not applied: {why}")),
            },
            RecordAction::Copy | RecordAction::Paste => {
                let done = match action {
                    RecordAction::Copy => state.copy_records(document),
                    _ => state.paste_records(document),
                };
                match done {
                    Ok(said) => state.show_info_toast(said),
                    Err(why) => state.show_error_toast(why),
                }
            }
        }
    }
}

//! The picker a name field opens over the names the install's index holds.

use or_io::meta::FieldHint;

use crate::state::name_index::NameIndexCategory;
use crate::ui::theme;

use super::ctx::EditorCtx;

pub(super) fn picker_category(hint: FieldHint) -> Option<NameIndexCategory> {
    match hint {
        FieldHint::ModelName => Some(NameIndexCategory::Model),
        FieldHint::TextureName => Some(NameIndexCategory::Texture),
        FieldHint::ClipSetName => Some(NameIndexCategory::ClipSet),
        FieldHint::EffectRuleName => Some(NameIndexCategory::Effect),
        _ => None,
    }
}

pub(super) fn open_picker(ctx: &egui::Context, category: NameIndexCategory) {
    ctx.data_mut(|d| {
        d.insert_temp(picker_open_id(category), true);
        d.insert_temp::<String>(picker_query_id(category), String::new());
    });
}

pub(super) fn take_picker_choice(
    ctx: &egui::Context,
    category: NameIndexCategory,
) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(picker_choice_id(category)))
}

fn picker_open_id(category: NameIndexCategory) -> egui::Id {
    egui::Id::new(("meta_picker_open", category as u8))
}

fn picker_query_id(category: NameIndexCategory) -> egui::Id {
    egui::Id::new(("meta_picker_query", category as u8))
}

fn picker_choice_id(category: NameIndexCategory) -> egui::Id {
    egui::Id::new(("meta_picker_choice", category as u8))
}

pub(super) fn draw_picker_modal(ui: &mut egui::Ui, ctx: &EditorCtx, category: NameIndexCategory) {
    let open = ui.ctx().data(|d| {
        d.get_temp::<bool>(picker_open_id(category))
            .unwrap_or(false)
    });
    if !open {
        return;
    }
    let mut still_open = true;
    let mut query: String = ui.ctx().data(|d| {
        d.get_temp::<String>(picker_query_id(category))
            .unwrap_or_default()
    });
    let mut choice: Option<String> = None;
    egui::Window::new(category.label())
        .open(&mut still_open)
        .resizable(true)
        .default_width(420.0)
        .show(ui.ctx(), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(f32::INFINITY)
                    .hint_text("Search…"),
            );
            ui.separator();
            let names = ctx.name_index.names(category);
            let needle = query.to_ascii_lowercase();
            let filtered: Vec<&String> = names
                .iter()
                .filter(|n| needle.is_empty() || n.to_ascii_lowercase().contains(&needle))
                .take(500)
                .collect();
            if filtered.is_empty() {
                let message = if names.is_empty() {
                    category.producer_hint()
                } else {
                    "(no matching entries)"
                };
                ui.colored_label(theme::TEXT_DIM, message);
            } else {
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for name in filtered {
                            if ui
                                .selectable_label(false, egui::RichText::new(name).monospace())
                                .clicked()
                            {
                                choice = Some(name.clone());
                            }
                        }
                    });
            }
        });
    ui.ctx().data_mut(|d| {
        d.insert_temp(picker_query_id(category), query);
    });
    if let Some(picked) = choice {
        ui.ctx().data_mut(|d| {
            d.insert_temp(picker_choice_id(category), picked);
            d.insert_temp(picker_open_id(category), false);
        });
    }
    if !still_open {
        ui.ctx().data_mut(|d| {
            d.insert_temp(picker_open_id(category), false);
        });
    }
}

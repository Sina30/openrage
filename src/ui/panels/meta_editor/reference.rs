//! Reference fields: a choice among the names `ReferenceNames` offers.

use egui_phosphor::regular as ph;
use or_io::meta::instance::value::HashedString;
use or_io::meta::{Field, StringKind, Value};

/// An index into an interior's rooms or entities, offered as their names.
pub(super) fn draw_index(
    ui: &mut egui::Ui,
    field: &Field,
    names: &[String],
    noun: &str,
    value: &mut Value,
    dirty: &mut bool,
) {
    let current = match value {
        Value::UInt(v) => *v,
        Value::Int(v) => (*v).max(0) as u64,
        _ => 0,
    };
    let label = |i: u64| match names.get(i as usize) {
        Some(name) if !name.is_empty() => format!("{i}: {name}"),
        Some(_) => i.to_string(),
        None => format!("{i} (no such {noun})"),
    };
    egui::ComboBox::from_id_salt(("meta_index", field.name.as_str()))
        .selected_text(label(current))
        .show_ui(ui, |ui| {
            for i in 0..names.len() as u64 {
                if ui.selectable_label(i == current, label(i)).clicked() && i != current {
                    *value = match value {
                        Value::Int(_) => Value::Int(i as i64),
                        _ => Value::UInt(i),
                    };
                    *dirty = true;
                }
            }
        });
}

/// A menu of the names a hashed field can take, beside its text box.
pub(super) fn draw_name_choice(
    ui: &mut egui::Ui,
    names: &[HashedString],
    hover: &str,
    kind: StringKind,
    value: &mut Value,
    dirty: &mut bool,
) {
    ui.menu_button(ph::LIST, |ui| {
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for name in names {
                    let label = if name.raw.is_empty() {
                        name.to_literal()
                    } else {
                        name.raw.clone()
                    };
                    if ui
                        .selectable_label(false, egui::RichText::new(label).monospace())
                        .clicked()
                    {
                        // ⚠ The name's own hash is kept: its `raw` came from the
                        // name index and may not hash back to it.
                        *value = match kind {
                            StringKind::Plain | StringKind::AtString => {
                                Value::String(name.raw.clone())
                            }
                            _ => Value::Hash(name.clone()),
                        };
                        *dirty = true;
                        ui.close_kind(egui::UiKind::Menu);
                    }
                }
            });
    })
    .response
    .on_hover_text(hover);
}

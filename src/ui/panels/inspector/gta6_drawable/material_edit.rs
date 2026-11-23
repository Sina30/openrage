//! A GTA VI drawable's materials edited as GTA V's material editor edits one: each constant's words
//! and each bound texture slot's key, each change a step of undo.

use std::sync::Arc;

use or_io::gta6::headerless::{ParameterValue, Shader};

use super::*;
use crate::state::gta6_edit::{drawable_code, texture_key, MaterialEdit};
use crate::state::gta6_material_declared::declared_default;
use crate::state::gta6_resource::{
    drawable_lines_stamp, drawable_of, effect_def, parameter_rows, texture_name,
};

/// Each shader's parameter rows: a name, whether the words are floats, and the default the effect
/// sets for a constant of its size.
type Rows = Arc<Vec<Vec<Row>>>;
/// A parameter row: its name, whether its words are floats, and its effect's default.
type Row = (String, bool, Option<Vec<u32>>);

/// `rows` of `shaders`, each given the default its effect sets, as a Reset sets it.
fn with_defaults(shaders: &[Shader], rows: Vec<Vec<(String, bool)>>) -> Vec<Vec<Row>> {
    (shaders.iter().zip(rows))
        .map(|(s, rows)| {
            let effect = effect_def(s.name_hash);
            (s.parameters.iter().zip(rows))
                .map(|(p, (name, floats))| {
                    let default = match (&p.value, &effect) {
                        (ParameterValue::Constant { words, .. }, Some(e)) => {
                            declared_default(e, p.hash, words.len())
                        }
                        _ => None,
                    };
                    (name, floats, default)
                })
                .collect()
        })
        .collect()
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    selection: Option<u64>,
) {
    let Some(DocPayload::Gta6Drawable(resource)) = state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let resource = Arc::clone(resource);
    let (Some(code), Some(drawable)) = (
        drawable_code(&resource, selection),
        drawable_of(&resource, selection),
    ) else {
        return;
    };
    // Built again only when the body, the drawable chosen, or what names its parameters changes.
    let stamp = (
        drawable_lines_stamp(state),
        Arc::as_ptr(&resource) as usize,
        selection,
    );
    let id = egui::Id::new(("gta6_material_rows", doc_id));
    let cached = ui.data(|d| d.get_temp::<((u64, usize, Option<u64>), Rows)>(id));
    let rows = match cached.filter(|(s, _)| *s == stamp) {
        Some((_, rows)) => rows,
        None => {
            let named = parameter_rows(&drawable.shaders);
            let rows: Rows = Arc::new(with_defaults(&drawable.shaders, named));
            ui.data_mut(|d| d.insert_temp(id, (stamp, rows.clone())));
            rows
        }
    };
    let mut edits = Vec::new();
    egui::CollapsingHeader::new(tr!("Edit materials"))
        .id_salt(("gta6_material_edit", doc_id))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr!("A constant keeps its size, so nothing moves. An empty slot given a key takes a binding and a key record of its own at the end of the virtual section, and takes only a key whose name the app knows or that another slot of the drawable binds. Save As writes the body as a loose resource."))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            for (i, (s, rows)) in drawable.shaders.iter().zip(rows.iter()).enumerate() {
                ui.add_space(crate::ui::theme::SPACING_TIGHT);
                ui.strong(tr!("Shader {index}", index = i));
                for (k, (p, (name, floats, default))) in s.parameters.iter().zip(rows).enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(name);
                        match &p.value {
                            ParameterValue::Texture { slot } => {
                                let slot = usize::from(*slot);
                                let held = s.texture_keys.get(slot).copied().flatten();
                                if let Some(key) = key_field(ui, (doc_id, i, slot), held) {
                                    edits.push((i, MaterialEdit::TextureKey { slot, key }));
                                }
                            }
                            ParameterValue::Constant { words, .. } => {
                                for (w, word) in words.iter().enumerate() {
                                    if let Some(word) = word_field(ui, *word, *floats) {
                                        let mut words = words.clone();
                                        words[w] = word;
                                        let edit = MaterialEdit::Constant { parameter: k, words };
                                        edits.push((i, edit));
                                    }
                                }
                                let reset = default.as_ref().filter(|d| *d != words);
                                if let Some(default) = reset {
                                    let button = ui.small_button(tr!("Reset"));
                                    let hint = tr!("Set the effect's default");
                                    if button.on_hover_text(hint).clicked() {
                                        let words = default.clone();
                                        let edit = MaterialEdit::Constant { parameter: k, words };
                                        edits.push((i, edit));
                                    }
                                }
                            }
                        }
                    });
                }
            }
        });
    // ⚠ An edit needs the body alone: this panel's hold on it goes first.
    drop(resource);
    for (shader, edit) in edits {
        if let Err(why) = state.edit_gta6_material(doc_id, code, shader, edit) {
            state.show_warning_toast(why);
        }
    }
}

/// A constant's word, as a float where its effect declares floats; the word typed, if any.
fn word_field(ui: &mut egui::Ui, word: u32, floats: bool) -> Option<u32> {
    if floats {
        let mut value = f32::from_bits(word);
        let edited = drag_value(ui, &mut value, |d| d.speed(0.01).max_decimals(4)).inner;
        return edited.then(|| value.to_bits());
    }
    let mut value = word;
    let edited = drag_value(ui, &mut value, |d| d.hexadecimal(8, false, true)).inner;
    edited.then_some(value)
}

/// A texture slot's key as the name it is known by, empty for a slot that binds none, typed until
/// the field lets go; the key typed.
fn key_field(ui: &mut egui::Ui, at: (DocumentId, usize, usize), key: Option<u64>) -> Option<u64> {
    let id = egui::Id::new(("gta6_texture_key", at));
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| key.map(texture_name).unwrap_or_default());
    let field = egui::TextEdit::singleline(&mut text)
        .desired_width(220.0)
        .hint_text(tr!("unbound"));
    let response = ui.add(field);
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if !response.lost_focus() {
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    texture_key(&text).filter(|typed| Some(*typed) != key)
}

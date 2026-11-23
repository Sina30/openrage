//! A GTA VI drawable's materials laid out anew as GTA V's material editor changes one: a shader's
//! effect, and a parameter or a texture slot added or removed, each a step of undo.

use std::sync::Arc;

use or_io::gta5::shaders::shader_defs::ShaderDef;
use or_io::gta6::headerless::{ParameterValue, Shader};

use super::*;
use crate::state::gta6_edit::drawable_code;
use crate::state::gta6_material_declared::{declared_shader, missing, with_declared};
use crate::state::gta6_material_layout::{typed_hash, LayoutEdit};
use crate::state::gta6_resource::{drawable_of, effect_def, effect_name};

/// What the row that adds a parameter holds while it is typed.
#[derive(Clone)]
struct Adding {
    name: String,
    buffer: u8,
    offset: u16,
    words: usize,
}

impl Default for Adding {
    fn default() -> Self {
        Self {
            name: String::new(),
            buffer: 0,
            offset: 0,
            words: 1,
        }
    }
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
    let (mut edits, mut refused) = (Vec::new(), Vec::new());
    let mut failed: Vec<String> = Vec::new();
    let mut hash_of = |text: &str| {
        let hash = typed_hash(text);
        if hash.is_none() {
            refused.push(tr!("Type a name, or hash_ and eight hex digits."));
        }
        hash
    };
    egui::CollapsingHeader::new(tr!("Change effects and parameters"))
        .id_salt(("gta6_material_layout", doc_id))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr!("A changed effect, or a parameter or texture slot added or removed, gives the shader new parameter records and a new parameter block at the end of the body's virtual section when it is saved, and the old ones stay as bytes nothing names. An added constant starts at zero and an added texture slot binds no texture. Type an effect or a parameter by its name, or as hash_ and eight hex digits."))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            for (i, s) in drawable.shaders.iter().enumerate() {
                ui.add_space(crate::ui::theme::SPACING_TIGHT);
                ui.horizontal(|ui| {
                    ui.strong(tr!("Shader {index}", index = i));
                    let text = typed(ui, ("gta6_effect", doc_id, i), effect_name(s.name_hash));
                    if let Some(hash) = text.and_then(|t| hash_of(&t)) {
                        if hash != s.name_hash {
                            let (edit, why) = effect_change(s, hash);
                            edits.push((i, edit));
                            failed.extend(why);
                        }
                    }
                });
                if let Some(effect) = effect_def(s.name_hash) {
                    let picked = declared_pick(ui, (doc_id, i), s, &effect);
                    match picked {
                        Some(Ok(edit)) => edits.push((i, edit)),
                        Some(Err(why)) => failed.push(why),
                        None => {}
                    }
                }
                for (k, p) in s.parameters.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("hash_{:08X}", p.hash));
                        let place = match &p.value {
                            ParameterValue::Texture { slot } => {
                                tr!("texture slot {slot}", slot = slot)
                            }
                            ParameterValue::Constant { buffer, offset, .. } => tr!(
                                "buffer {buffer} +{offset}",
                                buffer = buffer,
                                offset = format!("{offset:#x}"),
                            ),
                        };
                        ui.label(egui::RichText::new(place).small().color(theme::TEXT_SECONDARY));
                        if ui.small_button(tr!("Remove")).clicked() {
                            edits.push((i, LayoutEdit::Remove(k)));
                        }
                    });
                }
                let id = egui::Id::new(("gta6_material_adding", doc_id, i));
                let mut adding = ui.data(|d| d.get_temp::<Adding>(id)).unwrap_or_default();
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut adding.name)
                        .desired_width(160.0)
                        .hint_text("hash_00000000");
                    ui.add(field);
                    if ui.button(tr!("Add a texture slot")).clicked() {
                        if let Some(hash) = hash_of(&adding.name) {
                            edits.push((i, LayoutEdit::AddTexture(hash)));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("Buffer, byte offset and words"));
                    drag_value(ui, &mut adding.buffer, |d| d.range(0..=63));
                    drag_value(ui, &mut adding.offset, |d| d.range(0..=0x3FFC).speed(4.0));
                    drag_value(ui, &mut adding.words, |d| d.range(1..=1023));
                    if ui.button(tr!("Add a constant")).clicked() {
                        if let Some(hash) = hash_of(&adding.name) {
                            let edit = LayoutEdit::AddConstant {
                                hash,
                                buffer: adding.buffer,
                                offset: adding.offset,
                                words: adding.words,
                            };
                            edits.push((i, edit));
                        }
                    }
                });
                ui.data_mut(|d| d.insert_temp(id, adding));
            }
        });
    // ⚠ An edit needs the body alone: this panel's hold on it goes first.
    drop(resource);
    for (shader, edit) in edits {
        if let Err(why) = state.edit_gta6_material_layout(doc_id, code, shader, edit) {
            state.show_warning_toast(why);
        }
    }
    for why in refused {
        state.show_warning_toast(why);
    }
    for why in failed {
        state.show_warning_toast(why);
    }
}

/// The change of `s`'s effect to `hash`: an effect the app has read gives the shader the
/// parameters it declares; any other, or one whose parameters are not placed, changes the effect
/// alone, with why its parameters were not taken.
fn effect_change(s: &Shader, hash: u32) -> (LayoutEdit, Option<String>) {
    let Some(effect) = effect_def(hash) else {
        return (LayoutEdit::Effect(hash), None);
    };
    match declared_shader(s, &effect) {
        Ok(shader) => {
            let edit = LayoutEdit::Declared {
                shader: Box::new(shader),
                effect: true,
            };
            (edit, None)
        }
        Err(why) => (LayoutEdit::Effect(hash), Some(why)),
    }
}

/// A list of what `effect` declares and `s` lacks; the addition of the one picked, if any.
fn declared_pick(
    ui: &mut egui::Ui,
    at: (DocumentId, usize),
    s: &Shader,
    effect: &ShaderDef,
) -> Option<Result<LayoutEdit, String>> {
    let absent = missing(s, effect);
    if absent.is_empty() {
        return None;
    }
    let mut picked = None;
    egui::ComboBox::from_id_salt(("gta6_material_declared", at))
        .selected_text(tr!("Add from the effect's list"))
        .show_ui(ui, |ui| {
            for (hash, name, texture) in &absent {
                if ui.selectable_label(false, name.as_str()).clicked() {
                    picked = Some((*hash, *texture));
                }
            }
        });
    let (hash, texture) = picked?;
    if texture {
        return Some(Ok(LayoutEdit::AddTexture(hash)));
    }
    Some(
        with_declared(s, effect, hash).map(|laid| LayoutEdit::Declared {
            shader: Box::new(laid),
            effect: false,
        }),
    )
}

/// A field showing `shown` that keeps what is typed until it lets go; the text typed, if any.
fn typed(ui: &mut egui::Ui, at: impl std::hash::Hash, shown: String) -> Option<String> {
    let id = egui::Id::new(at);
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or(shown);
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if !response.lost_focus() {
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    Some(text)
}

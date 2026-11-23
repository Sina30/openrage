//! A GTA VI material's effect changed, and a parameter or a texture slot added or removed, as GTA
//! V's material editor makes those changes, each a step of undo; a save lays out anew the shaders
//! whose parameters no longer fit where they were read.

use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{shader_layout_holds, Parameter, ParameterValue, Shader};

use super::edit_history::Change;
use super::gta6_edit::shader_mut;
use super::{AppState, DocumentId};

/// What an edit of a GTA VI material's layout sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LayoutEdit {
    /// The effect, by the hash of its name.
    Effect(u32),
    /// Parameter `k` removed, and its texture slot with it where it is a texture.
    Remove(usize),
    /// A texture record of this hash, in a slot of its own that binds no texture.
    AddTexture(u32),
    /// A constant of this hash at `offset` bytes of `buffer`, of `words` zero words.
    AddConstant {
        hash: u32,
        buffer: u8,
        offset: u16,
        words: usize,
    },
    /// The shader as its effect declares it, made by `gta6_material_declared`: a changed effect
    /// given its parameters where `effect`, else one parameter added from the effect's list.
    Declared { shader: Box<Shader>, effect: bool },
}

/// The hash an effect or a parameter takes from what was typed: `hash_` and eight hex digits, else
/// the hash of a name.
pub(crate) fn typed_hash(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.get(..5).filter(|p| p.eq_ignore_ascii_case("hash_")) {
        Some(_) => u32::from_str_radix(&text[5..], 16).ok(),
        None => (!text.is_empty()).then(|| name_hash32(text.as_bytes())),
    }
}

/// `edit` made to `shader`, or why it is not; `shader` is left as it was when it is not.
fn apply(shader: &mut Shader, edit: LayoutEdit) -> Result<(), String> {
    if let LayoutEdit::AddTexture(hash) | LayoutEdit::AddConstant { hash, .. } = &edit {
        if shader.parameters.iter().any(|p| p.hash == *hash) {
            return Err(tr!("The shader already holds that parameter.").to_string());
        }
    }
    match edit {
        LayoutEdit::Effect(hash) => shader.name_hash = hash,
        LayoutEdit::Remove(k) => {
            if k >= shader.parameters.len() {
                return Err(tr!("That is no parameter of this shader.").to_string());
            }
            if let ParameterValue::Texture { slot } = shader.parameters.remove(k).value {
                if usize::from(slot) < shader.texture_keys.len() {
                    shader.texture_keys.remove(usize::from(slot));
                }
                for p in &mut shader.parameters {
                    if let ParameterValue::Texture { slot: at } = &mut p.value {
                        if *at > slot {
                            *at -= 1;
                        }
                    }
                }
            }
        }
        LayoutEdit::AddTexture(hash) => {
            let slot = shader.texture_keys.len();
            let value = ParameterValue::Texture {
                slot: u8::try_from(slot).unwrap_or(u8::MAX),
            };
            let at = slot.min(shader.parameters.len());
            shader.parameters.insert(at, Parameter { hash, value });
            shader.texture_keys.push(None);
        }
        LayoutEdit::AddConstant {
            hash,
            buffer,
            offset,
            words,
        } => {
            let value = ParameterValue::Constant {
                buffer,
                offset,
                // Past 1,023 words the layout check refuses it, so no more are made.
                words: vec![0; words.min(1 << 10)],
            };
            shader.parameters.push(Parameter { hash, value });
        }
        LayoutEdit::Declared { shader: laid, .. } => *shader = *laid,
    }
    Ok(())
}

impl AppState {
    /// Shader `shader` of the drawable `code` finds in `doc` given `edit`, as a step of its history.
    pub(crate) fn edit_gta6_material_layout(
        &mut self,
        doc: DocumentId,
        code: Option<u32>,
        shader: usize,
        edit: LayoutEdit,
    ) -> Result<(), String> {
        let label = match &edit {
            LayoutEdit::Effect(_) | LayoutEdit::Declared { effect: true, .. } => {
                tr!("Change a material's effect")
            }
            LayoutEdit::Remove(_) => tr!("Remove a material parameter"),
            _ => tr!("Add a material parameter"),
        };
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let held = shader_mut(&mut document.payload, code, shader).ok_or_else(|| {
            tr!("The material is not edited while a task reads the body; try again once it ends.")
        })?;
        let before = held.clone();
        apply(held, edit)?;
        if let Err(why) = shader_layout_holds(held) {
            *held = before;
            return Err(tr!("The material is not laid out so: {why}", why = why));
        }
        let change = Change::part(
            format!("gta6 material layout {code:?} {shader}"),
            before,
            move |p| shader_mut(p, code, shader),
        );
        self.record(doc, label, change);
        // Each change of the layout is a step of its own, not taken into the one before it.
        if let Some(document) = self.document_mut(doc) {
            document.history.seal();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "gta6_material_layout_tests.rs"]
mod tests;

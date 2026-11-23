//! Edits of a scanned GTA VI body that move nothing, made as GTA V's editors make them: a drawable's
//! constants and texture keys, and a scanned dictionary texture's pixels in its own size, format
//! and mips. A save reads the body again on its worker, writes the values shown over its sections
//! in place, and writes those as a loose resource.

use std::path::Path;
use std::sync::Arc;

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{RgbaImage, TextureSource};
use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Role, Rpf9Sections,
};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta6::hash::{name_hash32, name_hash64};
use or_io::gta6::headerless::{
    write_materials_with, Entry, Parameter, ParameterValue, Resource, Shader,
};

use super::document::Document;
use super::edit_history::Change;
use super::gta6_resource_save::EditedSave;
use super::texture_edit::TextureEdit;
use super::{AppState, DocPayload, DocumentId};

/// The most bytes a save reads of a body again.
const BODY_LIMIT: u64 = 1 << 30;

/// An edit of a body's sections, of the role its signature gives, made on the worker that writes them.
pub type EditSections =
    dyn Fn(Option<Rpf9Role>, &mut Rpf9Sections) -> Result<(), String> + Send + Sync;

/// What an edit of a GTA VI material sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaterialEdit {
    /// Parameter `parameter`'s words, as many as it holds.
    Constant { parameter: usize, words: Vec<u32> },
    /// The key of texture slot `slot`; an empty slot takes only a key whose 32-bit name hash is stated.
    TextureKey { slot: usize, key: u64 },
}

/// The 32-bit name hash of the texture `key` names, where the app knows that name.
pub(crate) fn named_word(key: u64) -> Option<u32> {
    let name = super::ttpf_document::known_name(key)?;
    (name_hash64(name.as_bytes()) == key).then(|| name_hash32(name.as_bytes()))
}

/// The drawable `selection` shows of `resource`, by the code a save finds it under: `None` for its
/// own or its fragment's, a dictionary entry's code; `None` outright where it shows none.
pub(crate) fn drawable_code(resource: &Resource, selection: Option<u64>) -> Option<Option<u32>> {
    use crate::ui::panels::selection_ids::{dict_entry_unpack_part, is_dict_entry_selection};
    match resource {
        Resource::Drawable(_) | Resource::DrawableWithBound(_) => Some(None),
        Resource::Dictionary(_) => {
            let id = selection.filter(|s| is_dict_entry_selection(*s))?;
            Some(Some(dict_entry_unpack_part(id).0))
        }
        _ => None,
    }
}

/// The key a texture slot takes from what was typed: `hash_` and sixteen hex digits, else the hash of
/// a name.
pub(crate) fn texture_key(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.get(..5).filter(|p| p.eq_ignore_ascii_case("hash_")) {
        Some(_) => u64::from_str_radix(&text[5..], 16).ok(),
        None => (!text.is_empty()).then(|| name_hash64(text.as_bytes())),
    }
}

/// Shader `index` of the drawable `code` finds in `payload`, while no other holder of the body is
/// reading it.
pub(super) fn shader_mut(
    payload: &mut DocPayload,
    code: Option<u32>,
    index: usize,
) -> Option<&mut Shader> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    let drawable = match (Arc::get_mut(resource)?, code) {
        (Resource::Drawable(d), None) => d,
        (Resource::DrawableWithBound(r), None) => &mut r.drawable,
        (Resource::Dictionary(dict), Some(code)) => {
            (dict.entries.iter_mut()).find_map(|(c, e)| match e {
                Ok(Entry::Drawable(d)) if *c == code => Some(&mut **d),
                _ => None,
            })?
        }
        _ => return None,
    };
    drawable.shaders.get_mut(index)
}

/// `edit` made to `shader`, or why it would move something; `stated` says whether a name or a slot
/// of the drawable states the key's 32-bit name hash, which the slot's key record is given.
fn apply(shader: &mut Shader, edit: MaterialEdit, stated: bool) -> Result<(), String> {
    match edit {
        MaterialEdit::Constant { parameter, words } => {
            let Some(Parameter {
                value: ParameterValue::Constant { words: held, .. },
                ..
            }) = shader.parameters.get_mut(parameter)
            else {
                return Err(tr!("That parameter is no constant of this shader.").to_string());
            };
            if held.len() != words.len() {
                return Err(
                    tr!("A constant keeps its size: another would move the sections.").to_string(),
                );
            }
            *held = words;
        }
        MaterialEdit::TextureKey { slot, key } => match shader.texture_keys.get_mut(slot) {
            // A slot's key record states its key's 32-bit name hash, so every slot needs one.
            Some(held) if stated || *held == Some(key) => *held = Some(key),
            Some(_) => {
                return Err(tr!("No name the app knows and no slot of this drawable states this texture's 32-bit name hash, so no slot is bound to it.").to_string());
            }
            None => {
                let why = tr!("That parameter is no texture slot of this shader.");
                return Err(why.to_string());
            }
        },
    }
    Ok(())
}

impl AppState {
    /// Shader `shader` of the drawable `code` finds in `doc` given `edit`, as a step of its history.
    pub(crate) fn edit_gta6_material(
        &mut self,
        doc: DocumentId,
        code: Option<u32>,
        shader: usize,
        edit: MaterialEdit,
    ) -> Result<(), String> {
        let label = match &edit {
            MaterialEdit::Constant { .. } => tr!("Set a material value").to_string(),
            MaterialEdit::TextureKey { key, .. } => tr!(
                "Bind {name}",
                name = super::gta6_resource::texture_name(*key)
            ),
        };
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let stated = match (&edit, &document.payload) {
            (MaterialEdit::TextureKey { key, .. }, DocPayload::Gta6Drawable(resource)) => {
                named_word(*key).is_some()
                    || (shaders_of(resource).iter())
                        .filter(|(c, _)| *c == code)
                        .flat_map(|(_, shaders)| shaders)
                        .any(|s| s.texture_keys.contains(&Some(*key)))
            }
            _ => false,
        };
        let held = shader_mut(&mut document.payload, code, shader).ok_or_else(|| {
            tr!("The material is not edited while a task reads the body; try again once it ends.")
        })?;
        let before = held.clone();
        apply(held, edit, stated)?;
        let change = Change::part(
            format!("gta6 material {code:?} {shader}"),
            before,
            move |p| shader_mut(p, code, shader),
        );
        self.record(doc, label, change);
        Ok(())
    }
}

/// Whether `doc` is a scanned GTA VI texture dictionary, shown as a Legacy one.
pub(crate) fn is_scanned_dictionary(doc: &Document) -> bool {
    doc.read_only == Some(super::rpf9_scan::READ_ONLY_BODY)
        && matches!(
            doc.payload,
            DocPayload::Resource(or_io::Asset::TextureDict(_))
        )
}

/// Texture `name` of the scanned GTA VI dictionary `doc` replaced from `image` in its own size,
/// format and mips: the texture it held and what changed; `None` for any other document.
pub(super) fn replace_scanned_texture(
    doc: &mut Document,
    name: &str,
    image: &RgbaImage,
    format: FormatChoice,
    options: &TextureBuildOptions,
) -> Option<Result<(GrcTexture, TextureEdit), String>> {
    if !is_scanned_dictionary(doc) {
        return None;
    }
    // An array's or a cube's slice is replaced alone: the one the preview shows.
    let shown = super::ttpf_document::shown_scanned_slice(doc, name);
    let slices = match &doc.payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => dict.get(name),
        _ => None,
    };
    let slices = slices.map(|t| super::ttpf_document::scanned_slices(doc, t));
    let DocPayload::Resource(or_io::Asset::TextureDict(dict)) = &mut doc.payload else {
        return None;
    };
    let (Some(texture), Some(slices)) = (dict.get_mut(name), slices) else {
        return Some(Err(tr!("{name} is not in this dictionary", name = name)));
    };
    // A volume of one level holds one z-plane a slice, replaced alone as an array's slice is.
    if texture.image_type == super::ttpf_document::VOLUME_IMAGE && texture.mip_count > 1 {
        return Some(Err(tr!(
            "{name} is a volume texture, whose slices each hold part of every level, so it is not replaced from an image.",
            name = name
        )));
    }
    let outcome = match slices {
        1 => replaced(texture, image, format, options),
        _ => replaced_slice(texture, (shown, slices), image, format, options),
    };
    Some(outcome.map(|before| {
        let edit = TextureEdit::Imported {
            name: name.to_string(),
            format: texture.pixel_format(),
            mips: u32::from(texture.mip_count),
            replaced: true,
        };
        (before, edit)
    }))
}

/// Slice `slice` of the `slices` `texture` holds one after another encoded from `image`, as
/// [`replaced`] encodes a texture of one slice; the texture it was.
fn replaced_slice(
    texture: &mut GrcTexture,
    (slice, slices): (u32, u32),
    image: &RgbaImage,
    format: FormatChoice,
    options: &TextureBuildOptions,
) -> Result<GrcTexture, String> {
    let slice = slice.min(slices.saturating_sub(1));
    let mut one = super::ttpf_document::scanned_slice(texture.clone(), slice, slices);
    replaced(&mut one, image, format, options)?;
    // `replaced` keeps the slice's length, and the slices fit the texture's store whole.
    let each = one.backing_store.len();
    let at = each * slice as usize;
    let before = texture.clone();
    texture.backing_store[at..at + each].copy_from_slice(&one.backing_store);
    Ok(before)
}

/// `texture` encoded from `image` in its own size, format and mips; the texture it was.
pub(super) fn replaced(
    texture: &mut GrcTexture,
    image: &RgbaImage,
    format: FormatChoice,
    options: &TextureBuildOptions,
) -> Result<GrcTexture, String> {
    let own = texture.pixel_format();
    let (width, height) = (u32::from(texture.width), u32::from(texture.height));
    if (image.width, image.height) != (width, height) {
        return Err(tr!(
            "{name} is {width}×{height}: a scanned GTA VI texture is replaced only from an image of its own size, as another would move the sections.",
            name = texture.name,
            width = width,
            height = height,
        ));
    }
    if matches!(format, FormatChoice::Fixed(f) if f != own) || !or_io::authoring::is_encodable(own)
    {
        return Err(tr!(
            "A scanned GTA VI texture is encoded only to its own format, {format}, and only where OpenRage encodes that format.",
            format = format!("{own:?}"),
        ));
    }
    if texture.backing_store.is_empty() {
        return Err(tr!(
            "{name} does not read, so its pixels are not replaced.",
            name = texture.name
        ));
    }
    let before = texture.clone();
    let options = TextureBuildOptions {
        mip_levels: Some(u32::from(texture.mip_count)),
        ..options.clone()
    };
    let encoded = texture.set_pixels(own, image, &options);
    // A Legacy store pads to whole units of the size mask; a scanned chain keeps its own length.
    use or_io::gta5::texture_dict::gen8::PAYLOAD_SIZE_MASK as MASK;
    let held = before.backing_store.len();
    let unit = (MASK & MASK.wrapping_neg()) as usize;
    if encoded.is_ok() && texture.backing_store.len() == held.next_multiple_of(unit) {
        texture.backing_store.truncate(held);
    }
    if encoded.is_err()
        || texture.mip_count != before.mip_count
        || texture.backing_store.len() != before.backing_store.len()
    {
        let why = encoded.err().map(|e| e.to_string());
        *texture = before;
        return Err(why.unwrap_or_else(|| {
            tr!("The image encodes to other mips than the texture holds, which would move the sections.").to_string()
        }));
    }
    texture.format = before.format;
    texture.info_bits = before.info_bits;
    Ok(before)
}

/// The GTA VI drawable and texture dictionary rows' `edited_sections` where `document` holds a GTA
/// VI edit: its body read again on the worker and written over with the values it shows; `None`
/// where it holds none.
pub(super) fn edited_save(document: &Document) -> Option<Result<EditedSave, String>> {
    if !document.history.can_undo() {
        return None;
    }
    let (archive, offset) = super::rpf9_scan::scanned_body(document.path.as_deref()?)?;
    let edit: Arc<EditSections> = match &document.payload {
        DocPayload::Gta6Drawable(resource) => {
            let drawables = shaders_of(resource);
            let filters = super::gta6_filter_edit::values_of(resource);
            Arc::new(move |role, sections| {
                let role = role.ok_or_else(|| tr!("The body's role is not known.").to_string())?;
                for (code, shaders) in &drawables {
                    write_materials_with(role, sections, *code, shaders, &named_word)
                        .map_err(|e| e.to_string())?;
                }
                super::gta6_filter_edit::write(sections, &filters)
            })
        }
        DocPayload::Resource(or_io::Asset::TextureDict(dict))
            if is_scanned_dictionary(document) =>
        {
            let textures = super::gta6_dictionary_write::held(dict);
            Arc::new(move |_, sections| super::gta6_dictionary_write::write(sections, &textures))
        }
        _ => return None,
    };
    Some(Ok(EditedSave::Body {
        archive: archive.to_path_buf(),
        offset,
        edit,
    }))
}

/// Each drawable of `resource` with its shaders, by the code a save finds it under.
fn shaders_of(resource: &Resource) -> Vec<(Option<u32>, Vec<Shader>)> {
    match resource {
        Resource::Drawable(d) => vec![(None, d.shaders.clone())],
        Resource::DrawableWithBound(r) => vec![(None, r.drawable.shaders.clone())],
        Resource::Dictionary(_) => super::gta6_resource::dictionary_drawables(resource)
            .map(|(code, d)| (Some(code), d.shaders.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The body at `offset` of `archive` read again, `edit` made to its sections, as a loose resource.
pub(super) fn written(archive: &Path, offset: u64, edit: &EditSections) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let (find, bytes) = read_rpf9_body(&file, offset, BODY_LIMIT).map_err(|e| e.to_string())?;
    let mut sections = Rpf9Sections::of(&find, &bytes).ok_or_else(|| {
        tr!("The body is not a resource, so no edit of it is written.").to_string()
    })?;
    let role = match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    };
    edit(role, &mut sections)?;
    write_rpf9_resource(&sections).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "gta6_edit_tests.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "a_scanned_array_s_shown_slice_is_replaced_alone.rs"]
mod slices;

//! A scanned GTA VI drawable's embedded texture replaced from an image in any size, format and
//! mips, embedded from the texture a slot binds, or unembedded, as a GTA V drawable's are in the
//! preview: each a step of undo, drawn at once, and written by a save over the body read again.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::authoring::{encode_texture_with_options, FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{max_mip_levels, RgbaImage, TextureSource as _};
use or_io::container::rpf::Rpf9Sections;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta6::headerless::{
    embed_texture, key_word, read_drawable_head, reshape_embedded_texture, unembed_texture,
    write_embedded_texture, HeldTexture, ModelTexture, Resource, TexturePlanError,
};
use or_io::gta6::Texture;

use super::document::Document;
use super::edit_history::{Change, Swap};
use super::gta6_edit::EditSections;
use super::gta6_resource::{drawable_of, texture_name};
use super::gta6_resource_save::EditedSave;
use super::texture_edit::TextureEdit;
use super::{AppState, DocPayload, DocumentId};

/// What a document holds in place of the texture a body embeds under a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Held {
    /// Pixels in any shape, of the texture the body embeds or of one it does not.
    Texture(Texture),
    /// None: the texture was unembedded.
    Removed,
}

/// What each scanned body's embedded textures were edited to, by its document's path and the
/// texture's key.
// ponytail: one map for the whole app, as the heads are; an undo swaps one texture's entry.
static EDITED: Mutex<BTreeMap<PathBuf, BTreeMap<u64, Held>>> = Mutex::new(BTreeMap::new());

fn edits() -> MutexGuard<'static, BTreeMap<PathBuf, BTreeMap<u64, Held>>> {
    EDITED.lock().unwrap_or_else(|p| p.into_inner())
}

/// Forget the edits of the body at `path`, which opens anew with none.
pub(super) fn forget(path: &Path) {
    edits().remove(path);
}

/// What the document holds for the embedded texture `key` of the body at `path`, if it edited it.
pub(super) fn edited(path: &Path, key: u64) -> Option<Held> {
    edits().get(path)?.get(&key).cloned()
}

/// The pixels the embedded texture `key` of the body at `path` was replaced with, if it was.
#[cfg(test)]
pub(super) fn replaced(path: &Path, key: u64) -> Option<Vec<u8>> {
    match edited(path, key)? {
        Held::Texture(texture) => Some(texture.pixels),
        Held::Removed => None,
    }
}

/// The body's path, whether it is a fragment, the key of the texture `name` a slot of the drawable
/// of `doc` binds, and how many slots bind it.
fn bound_key(state: &AppState, doc: DocumentId, name: &str) -> Option<(PathBuf, bool, u64, usize)> {
    let document = state.document(doc)?;
    let DocPayload::Gta6Drawable(resource) = &document.payload else {
        return None;
    };
    let fragment = match **resource {
        Resource::Drawable(_) => false,
        Resource::DrawableWithBound(_) => true,
        _ => return None,
    };
    let path = document.path.clone()?;
    let drawable = drawable_of(resource, None)?;
    let keys = || (drawable.shaders.iter()).flat_map(|s| s.texture_keys.iter().flatten());
    let key = *keys().find(|key| texture_name(**key).eq_ignore_ascii_case(name))?;
    let slots = keys().filter(|k| **k == key).count();
    Some((path, fragment, key, slots))
}

/// The texture the document shows embedded under `key` in the body at `path`, if it reads, and
/// whether it is a 2D texture of one slice; `None` where none is embedded.
fn shown(path: &Path, key: u64) -> Option<(Option<Texture>, bool)> {
    match edited(path, key) {
        Some(Held::Texture(texture)) => return Some((Some(texture), true)),
        Some(Held::Removed) => return None,
        None => {}
    }
    let head = super::gta6_head::head_of(path)?;
    let found = ((*head).as_ref().ok()?.textures.iter())
        .flatten()
        .find(|t| t.key == key)?;
    let d = &found.texture;
    let one_slice = d.dimension == 1 && d.depth <= 1;
    Some((d.texture.as_ref().ok().cloned(), one_slice))
}

/// The keys the document shows embedded in the body at `path`.
fn shown_keys(path: &Path) -> BTreeSet<u64> {
    let head = super::gta6_head::head_of(path);
    let read = (head.iter()).flat_map(|h| (**h).as_ref().ok().map(|h| &h.textures));
    let mut keys: BTreeSet<u64> = read.flatten().flatten().map(|t| t.key).collect();
    for (key, held) in edits().get(path).into_iter().flatten() {
        match held {
            Held::Texture(_) => keys.insert(*key),
            Held::Removed => keys.remove(key),
        };
    }
    keys
}

/// The body's path, whether it is a fragment, and the key of the texture `name` that the drawable
/// of `doc` binds and shows embedded; `None` where it embeds none of that name.
fn embedded_key(state: &AppState, doc: DocumentId, name: &str) -> Option<(PathBuf, bool, u64)> {
    let (path, fragment, key, _) = bound_key(state, doc, name)?;
    is_shown(&path, key).then_some((path, fragment, key))
}

/// Whether [`shown`] finds a texture, without copying its pixels: the preview asks every frame.
fn is_shown(path: &Path, key: u64) -> bool {
    match edits().get(path).and_then(|held| held.get(&key)) {
        Some(Held::Texture(_)) => return true,
        Some(Held::Removed) => return false,
        None => {}
    }
    super::gta6_head::head_of(path).is_some_and(|head| {
        (*head)
            .as_ref()
            .is_ok_and(|h| h.textures.iter().flatten().any(|t| t.key == key))
    })
}

/// Whether the drawable of `doc` embeds the texture `name` it binds.
pub(super) fn embeds(state: &AppState, doc: DocumentId, name: &str) -> bool {
    embedded_key(state, doc, name).is_some()
}

/// The texture of `key` as a save tiles it.
fn planned(key: u64, texture: &Texture) -> Result<ModelTexture, String> {
    let side = |pixels: u32| u16::try_from(pixels).map_err(|e| e.to_string());
    let held = HeldTexture {
        name: format!("hash_{key:016x}"),
        width: side(texture.width)?,
        height: side(texture.height)?,
        format: texture.format.format(),
        srgb: texture.format.is_srgb(),
        mips: u8::try_from(texture.levels).map_err(|e| e.to_string())?,
        pixels: texture.pixels.clone(),
    };
    held.tiled(None).map_err(|e| e.to_string())
}

/// `read`, the texture embedded as `name`, encoded from `image`: in its own shape where the image,
/// the format and the mips asked are its own, else in the image's size and the format and mips
/// asked, `Automatic` keeping its own format.
fn reshaped(
    name: &str,
    read: Texture,
    image: &RgbaImage,
    format: FormatChoice,
    options: &TextureBuildOptions,
) -> Result<Texture, String> {
    let own = read.format.format();
    let target = match format {
        FormatChoice::Fixed(f) => f,
        FormatChoice::Automatic => own,
    };
    let most = max_mip_levels(image.width, image.height);
    let mips = options
        .mip_levels
        .unwrap_or(read.levels)
        .clamp(1, most.max(1));
    if (image.width, image.height) == (read.width, read.height)
        && (target, mips) == (own, read.levels)
    {
        let mut texture = super::ttpf_document::grc(name.to_string(), read.clone())?;
        super::gta6_edit::replaced(&mut texture, image, format, options)?;
        let pixels = texture.backing_store;
        return Ok(Texture { pixels, ..read });
    }
    let options = TextureBuildOptions {
        mip_levels: Some(mips),
        ..options.clone()
    };
    let encoded = encode_texture_with_options(target, image.width, image.height, image, &options)
        .map_err(|e| e.to_string())?;
    let format = or_io::gta6::Format::of_grc(target, options.srgb)
        .ok_or_else(|| TexturePlanError::Format { format: target }.to_string())?;
    let texture = Texture {
        width: image.width,
        height: image.height,
        format,
        levels: u32::from(encoded.mip_count),
        faces: 1,
        pixels: encoded.data,
    };
    // A shape no dictionary texture tiles is refused now, not at the save.
    planned(0, &texture)?;
    Ok(texture)
}

/// The pack texture `texture` as the embedded texture of `key`, its levels top first.
fn of_grc(key: u64, texture: &GrcTexture) -> Result<Texture, String> {
    let format = texture.pixel_format();
    let srgb = texture.info_bits & 1 != 0;
    let unread = || {
        tr!(
            "{name} does not read, so its pixels are not replaced.",
            name = texture.name
        )
    };
    let (width, height) = (u32::from(texture.width), u32::from(texture.height));
    let levels = u32::from(texture.mip_count.max(1));
    let side = |pixels: u32, level: u32| pixels.checked_shr(level).unwrap_or(0).max(1);
    let len: usize = (0..levels)
        .map(|m| format.mip_size(side(width, m), side(height, m)) as usize)
        .sum();
    let pixels = texture
        .backing_store
        .get(..len)
        .ok_or_else(unread)?
        .to_vec();
    let format = or_io::gta6::Format::of_grc(format, srgb)
        .ok_or_else(|| TexturePlanError::Format { format }.to_string())?;
    let texture = Texture {
        width,
        height,
        format,
        levels,
        faces: 1,
        pixels,
    };
    planned(key, &texture)?;
    Ok(texture)
}

/// A step's entry for one embedded texture: the one the map does not hold now, `None` for none.
struct Step {
    path: PathBuf,
    key: u64,
    other: Option<Held>,
}

impl Swap for Step {
    fn swap(&mut self, _: &mut DocPayload) -> bool {
        let mut held = edits();
        let textures = held.entry(self.path.clone()).or_default();
        let now = match self.other.take() {
            Some(entry) => textures.insert(self.key, entry),
            None => textures.remove(&self.key),
        };
        self.other = now;
        true
    }
}

impl AppState {
    /// The texture `name` that the drawable `doc` embeds replaced from the image at `image`, in its
    /// own shape or the one `format` and `options` ask, as a step of undo; `None` where it embeds
    /// none of that name.
    pub(super) fn replace_embedded_texture(
        &mut self,
        doc: DocumentId,
        name: &str,
        image: &Path,
        format: FormatChoice,
        options: &TextureBuildOptions,
    ) -> Option<Result<TextureEdit, String>> {
        let (path, _, key) = embedded_key(self, doc, name)?;
        Some(self.replaced_embedded(doc, (path, key), name, image, (format, options)))
    }

    fn replaced_embedded(
        &mut self,
        doc: DocumentId,
        (path, key): (PathBuf, u64),
        name: &str,
        image: &Path,
        (format, options): (FormatChoice, &TextureBuildOptions),
    ) -> Result<TextureEdit, String> {
        let (read, one_slice) = shown(&path, key)
            .ok_or_else(|| tr!("{name} is not in this dictionary", name = name))?;
        if !one_slice {
            return Err(tr!(
                "{name} holds several slices, so it keeps its size, format and mips: only a 2D texture of one slice takes others.",
                name = name
            ));
        }
        let read = read.ok_or_else(|| {
            tr!(
                "{name} does not read, so its pixels are not replaced.",
                name = name
            )
        })?;
        let rgba = or_io::authoring::texture_import::load(image)
            .map_err(|e| format!("{}: {e}", image.display()))?;
        let texture = reshaped(name, read, &rgba, format, options)?;
        let edit = TextureEdit::Imported {
            name: name.to_string(),
            format: texture.format.format(),
            mips: texture.levels,
            replaced: true,
        };
        let label = tr!("Replace {name}", name = name);
        self.held_step(doc, (path, key), Held::Texture(texture), label);
        Ok(edit)
    }

    /// The texture `texture` that the drawable `doc` binds embedded in its shader group, as a step
    /// of undo; `None` where `doc` is no scanned GTA VI drawable binding it.
    pub(crate) fn embed_gta6_texture(
        &mut self,
        doc: DocumentId,
        texture: &GrcTexture,
    ) -> Option<Result<TextureEdit, String>> {
        let (path, _, key, _) = bound_key(self, doc, &texture.name)?;
        let name = texture.name.clone();
        let others = shown_keys(&path).into_iter().any(|k| k != key);
        if others {
            return Some(Err(tr!(
                "This drawable already embeds another texture, and a dictionary of two was never measured, so {name} is not embedded.",
                name = name
            )));
        }
        let replaced = shown(&path, key).is_some();
        let held = match of_grc(key, texture) {
            Ok(held) => held,
            Err(why) => return Some(Err(why)),
        };
        let label = tr!("Embed {name}", name = name);
        self.held_step(doc, (path, key), Held::Texture(held), label);
        Some(Ok(TextureEdit::Embedded { name, replaced }))
    }

    /// The texture `name` that the drawable `doc` embeds unembedded, as a step of undo; its slots
    /// keep its key. `None` where `doc` is no scanned GTA VI drawable binding it.
    pub(crate) fn unembed_gta6_texture(
        &mut self,
        doc: DocumentId,
        name: &str,
    ) -> Option<Result<TextureEdit, String>> {
        let (path, _, key, slots) = bound_key(self, doc, name)?;
        if shown(&path, key).is_none() {
            return Some(Err(tr!(
                "{name} is not carried by this drawable",
                name = name
            )));
        }
        let label = tr!("Stop carrying {name}", name = name);
        self.held_step(doc, (path, key), Held::Removed, label);
        Some(Ok(TextureEdit::Unembedded {
            name: name.to_string(),
            orphaned: slots,
        }))
    }

    /// `held` taken in place of what the document held for `key` of the body at `path`, as a step
    /// of undo named `label`, and drawn at once.
    fn held_step(
        &mut self,
        doc: DocumentId,
        (path, key): (PathBuf, u64),
        held: Held,
        label: String,
    ) {
        let was = edits().entry(path.clone()).or_default().insert(key, held);
        let change = Change::Part {
            key: Some(format!("gta6 embedded texture {key:016X}")),
            parts: vec![Box::new(Step {
                path,
                key,
                other: was,
            })],
        };
        self.record(doc, label, change);
        if let Some(document) = self.document_mut(doc) {
            document.history.seal();
        }
        self.texture_cache.forget_document(doc);
    }
}

/// Whether the body `sections` hold embeds `key` in the shape of `texture`; `None` where it
/// embeds no texture of that key.
fn same_shape(
    sections: &Rpf9Sections,
    fragment: bool,
    key: u64,
    texture: &Texture,
) -> Result<Option<bool>, String> {
    let (v, p) = (&sections.virtual_section, &sections.physical);
    let head = read_drawable_head(v, Some(p), fragment).map_err(|e| e.to_string())?;
    let found = (head.textures.iter()).flatten().find(|t| t.key == key);
    Ok(found.map(|t| {
        (t.texture.texture.as_ref()).is_ok_and(|r| {
            let shape = |x: &Texture| (x.width, x.height, x.format, x.levels, x.faces);
            shape(r) == shape(texture)
        })
    }))
}

/// `held` written over the drawable `sections` hold: each texture unembedded first, then each held
/// in place where the body embeds it in that shape, else re-planned or embedded anew.
fn write_held(
    sections: &mut Rpf9Sections,
    fragment: bool,
    held: &[(u64, Held)],
) -> Result<(), String> {
    for (key, _) in held.iter().filter(|(_, h)| *h == Held::Removed) {
        let texture = read_drawable_head(
            &sections.virtual_section,
            Some(&sections.physical),
            fragment,
        )
        .map_err(|e| e.to_string())?;
        if texture.textures.iter().flatten().any(|t| t.key == *key) {
            unembed_texture(sections, fragment, *key).map_err(|e| e.to_string())?;
        }
    }
    for (key, held) in held {
        let Held::Texture(texture) = held else {
            continue;
        };
        match same_shape(sections, fragment, *key, texture)? {
            Some(true) => {
                let (v, p) = (&sections.virtual_section, &mut sections.physical);
                write_embedded_texture(v, p, fragment, *key, &texture.pixels)
                    .map_err(|e| e.to_string())?;
            }
            Some(false) => {
                let model = planned(*key, texture)?;
                reshape_embedded_texture(sections, fragment, *key, &model)
                    .map_err(|e| e.to_string())?;
            }
            None => {
                let model = planned(*key, texture)?;
                let word = (super::gta6_edit::named_word(*key))
                    .or_else(|| key_word(&sections.virtual_section, *key))
                    .ok_or_else(|| {
                        tr!(
                            "No name or bound entry states the 32-bit name hash of {name}, so it is not embedded.",
                            name = texture_name(*key)
                        )
                    })?;
                embed_texture(sections, fragment, *key, word, &model).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// The GTA VI drawable row's `edited_sections` before its glTF imports: its materials, physics and
/// bones, then each embedded texture replaced, re-planned, embedded or unembedded.
pub(super) fn edited_save(document: &Document) -> Option<Result<EditedSave, String>> {
    let save = super::gta6_bone_edit::edited_save(document)?;
    let DocPayload::Gta6Drawable(resource) = &document.payload else {
        return Some(save);
    };
    let fragment = matches!(**resource, Resource::DrawableWithBound(_));
    let held: Vec<(u64, Held)> = (document.path.as_deref())
        .and_then(|p| {
            let textures = edits();
            let held = textures.get(p)?;
            Some(held.iter().map(|(k, v)| (*k, v.clone())).collect())
        })
        .unwrap_or_default();
    if held.is_empty() {
        return Some(save);
    }
    Some(save.map(|save| match save {
        EditedSave::Body {
            archive,
            offset,
            edit,
        } => {
            let edit: Arc<EditSections> = Arc::new(move |role, sections| {
                edit(role, sections)?;
                write_held(sections, fragment, &held)
            });
            EditedSave::Body {
                archive,
                offset,
                edit,
            }
        }
        other => other,
    }))
}

#[cfg(test)]
#[path = "gta6_embedded_edit_tests.rs"]
mod tests;

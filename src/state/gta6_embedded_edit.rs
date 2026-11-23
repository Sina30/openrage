//! A scanned GTA VI drawable's embedded texture replaced from an image in its own size, format and
//! mips, as a GTA V drawable's embedded texture is replaced in the preview: a step of undo, drawn
//! at once, and written into its own tiles over the body read again by a save.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::TextureSource;
use or_io::gta6::headerless::{write_embedded_texture, Resource};

use super::document::Document;
use super::edit_history::{Change, Swap};
use super::gta6_edit::EditSections;
use super::gta6_resource::{drawable_of, texture_name};
use super::gta6_resource_save::EditedSave;
use super::texture_edit::TextureEdit;
use super::{AppState, DocPayload, DocumentId};

/// The pixels each scanned body's embedded textures were replaced with, by its document's path
/// and the texture's key; levels top first, as the reader gives them.
// ponytail: one map for the whole app, as the heads are; an undo swaps one texture's pixels.
static REPLACED: Mutex<BTreeMap<PathBuf, BTreeMap<u64, Vec<u8>>>> = Mutex::new(BTreeMap::new());

fn replacements() -> MutexGuard<'static, BTreeMap<PathBuf, BTreeMap<u64, Vec<u8>>>> {
    REPLACED.lock().unwrap_or_else(|p| p.into_inner())
}

/// Forget the replaced pixels of the body at `path`, which opens anew with none.
pub(super) fn forget(path: &Path) {
    replacements().remove(path);
}

/// The pixels the embedded texture `key` of the body at `path` was replaced with, if it was.
pub(super) fn replaced(path: &Path, key: u64) -> Option<Vec<u8>> {
    replacements().get(path)?.get(&key).cloned()
}

/// The body's path, whether it is a fragment, and the key of the texture `name` that the drawable
/// of `doc` binds and its own shader group embeds; `None` where it embeds none of that name.
fn embedded_key(state: &AppState, doc: DocumentId, name: &str) -> Option<(PathBuf, bool, u64)> {
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
    let key = (drawable.shaders.iter())
        .flat_map(|s| s.texture_keys.iter().flatten())
        .copied()
        .find(|key| texture_name(*key).eq_ignore_ascii_case(name))?;
    let head = super::gta6_head::head_of(&path)?;
    let head = (*head).as_ref().ok()?;
    let embeds = head.textures.iter().flatten().any(|t| t.key == key);
    embeds.then_some((path, fragment, key))
}

/// Whether the drawable of `doc` embeds the texture `name` it binds.
pub(super) fn embeds(state: &AppState, doc: DocumentId, name: &str) -> bool {
    embedded_key(state, doc, name).is_some()
}

/// A step's pixels of one embedded texture: the ones the map does not hold now, `None` for those
/// the body holds.
struct Pixels {
    path: PathBuf,
    key: u64,
    other: Option<Vec<u8>>,
}

impl Swap for Pixels {
    fn swap(&mut self, _: &mut DocPayload) -> bool {
        let mut held = replacements();
        let textures = held.entry(self.path.clone()).or_default();
        let now = match self.other.take() {
            Some(pixels) => textures.insert(self.key, pixels),
            None => textures.remove(&self.key),
        };
        self.other = now;
        true
    }
}

impl AppState {
    /// The texture `name` that the drawable `doc` embeds replaced from the image at `image` in its
    /// own size, format and mips, as a step of undo; `None` where it embeds none of that name.
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
        let head = super::gta6_head::head_of(&path)
            .ok_or_else(|| tr!("{name} is not in this dictionary", name = name))?;
        let embedded = ((*head).as_ref().ok())
            .and_then(|h| h.textures.iter().flatten().find(|t| t.key == key))
            .ok_or_else(|| tr!("{name} is not in this dictionary", name = name))?;
        let held = &embedded.texture;
        if held.depth > 1 || held.dimension != 1 {
            return Err(tr!(
                "{name} holds several slices, so it keeps its size, format and mips: only a 2D texture of one slice takes others.",
                name = name
            ));
        }
        let unread = |_| {
            tr!(
                "{name} does not read, so its pixels are not replaced.",
                name = name
            )
        };
        let mut read = (held.texture.as_ref()).map_err(unread)?.clone();
        let was = replaced(&path, key);
        if let Some(edited) = &was {
            read.pixels.clone_from(edited);
        }
        let rgba = or_io::authoring::texture_import::load(image)
            .map_err(|e| format!("{}: {e}", image.display()))?;
        let mut texture = super::ttpf_document::grc(name.to_string(), read)?;
        super::gta6_edit::replaced(&mut texture, &rgba, format, options)?;
        let edit = TextureEdit::Imported {
            name: name.to_string(),
            format: texture.pixel_format(),
            mips: u32::from(texture.mip_count),
            replaced: true,
        };
        let mut textures = replacements();
        textures
            .entry(path.clone())
            .or_default()
            .insert(key, texture.backing_store);
        drop(textures);
        let change = Change::Part {
            key: Some(format!("gta6 embedded texture {key:016X}")),
            parts: vec![Box::new(Pixels {
                path,
                key,
                other: was,
            })],
        };
        self.record(doc, tr!("Replace {name}", name = name), change);
        if let Some(document) = self.document_mut(doc) {
            document.history.seal();
        }
        self.texture_cache.forget_document(doc);
        Ok(edit)
    }
}

/// The GTA VI drawable row's `edited_sections` before its glTF imports: its materials, physics and
/// bones, then each embedded texture replaced, written into its own tiles.
pub(super) fn edited_save(document: &Document) -> Option<Result<EditedSave, String>> {
    let save = super::gta6_bone_edit::edited_save(document)?;
    let DocPayload::Gta6Drawable(resource) = &document.payload else {
        return Some(save);
    };
    let fragment = matches!(**resource, Resource::DrawableWithBound(_));
    let held: Vec<(u64, Vec<u8>)> = (document.path.as_deref())
        .and_then(|p| {
            let textures = replacements();
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
                for (key, pixels) in &held {
                    let (v, p) = (&sections.virtual_section, &mut sections.physical);
                    write_embedded_texture(v, p, fragment, *key, pixels)
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
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

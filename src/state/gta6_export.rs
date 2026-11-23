//! A scanned GTA VI drawable to glTF: the scene `or-cli` builds of it, skinned to the skeleton read
//! with the body, each texture its keys resolve to in an open document carried as an image.

use std::sync::Arc;

use or_interchange::Scene;
use or_io::gta6::headerless::{Resource, Skeleton};

use super::gta6_resource::{drawable_of, effect_name, texture_name};
use super::textures::Texture;
use super::{AppState, DocPayload, DocumentId};

/// What a GTA VI drawable's export reads, gathered when it is asked for.
#[derive(Clone)]
pub(super) struct Gta6Export {
    body: Arc<Resource>,
    /// The selection that picks a dictionary's drawable.
    chosen: Option<u64>,
    skeleton: Option<Skeleton>,
    /// Each texture a key of the drawable resolves to, by the name the key is known by.
    textures: Vec<(String, Texture<'static>)>,
}

impl Gta6Export {
    /// The export of `doc`, a scanned GTA VI body; `None` for any other document.
    pub(super) fn of(state: &AppState, doc: DocumentId) -> Option<Self> {
        let document = state.document(doc)?;
        let DocPayload::Gta6Drawable(body) = &document.payload else {
            return None;
        };
        let chosen = state.selection.selected(doc);
        let path = document.path.as_deref();
        let skeleton = match &**body {
            Resource::Drawable(_) | Resource::DrawableWithBound(_) => {
                super::rpf9_scan::scanned_skeleton(path?).ok()
            }
            // A dictionary's entry is skinned to the skeleton it holds itself, where it holds one.
            Resource::Dictionary(_) => {
                use crate::ui::panels::selection_ids::{
                    dict_entry_unpack_part, is_dict_entry_selection,
                };
                let code = chosen.filter(|s| is_dict_entry_selection(*s));
                let code = code.map(|s| dict_entry_unpack_part(s).0);
                code.and_then(|code| super::gta6_entry_skeletons::of(path?, code))
            }
            _ => None,
        };
        let mut textures = Vec::new();
        if let Some(d) = drawable_of(body, chosen) {
            for key in d
                .shaders
                .iter()
                .flat_map(|s| s.texture_keys.iter().flatten())
            {
                let name = texture_name(*key);
                if textures.iter().any(|(n, _)| *n == name) {
                    continue;
                }
                let found = super::texture_resolution::resolve(state, None, chosen, &name);
                if let Some(found) = found {
                    textures.push((name, found.texture.into_owned()));
                }
            }
        }
        Some(Self {
            body: Arc::clone(body),
            chosen,
            skeleton,
            textures,
        })
    }

    /// The scene of the drawable chosen, named `stem`; `None` where none is chosen.
    pub(super) fn scene(&self, stem: &str, every_lod: bool) -> Option<Scene> {
        let drawable = drawable_of(&self.body, self.chosen)?;
        let names = or_cli::Gta6Names {
            effect: &effect_name,
            texture: &texture_name,
        };
        let mut scene =
            or_cli::gta6_scene(drawable, self.skeleton.as_ref(), stem, every_lod, &names);
        or_cli::gta6_carry_images(&mut scene, &self.textures);
        Some(scene)
    }
}

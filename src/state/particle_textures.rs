//! The textures a particle list holds or its models carry, as the browser lists them and the preview reads them.

use or_io::codec::texture::GrcTextureFormat;

use super::particle_target::ParticleTarget;
use super::textures::rows::row;
use super::textures::{DocumentTexture, Texture, TextureListing, TextureSource, NO_DICTIONARY};
use super::DocPayload;
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, ypt_active_model_for_textures, ypt_model_texture_lookup,
    ypt_model_texture_names, ypt_texture_id, YPT_MODEL_KIND_TEXTURE,
};

/// The textures of the model `selection` names where it names one, else the list's own.
pub(super) fn list<T: ParticleTarget>(
    payload: &DocPayload,
    selection: Option<u64>,
) -> TextureListing {
    let Some(asset) = T::list(payload) else {
        return TextureListing::Nothing(NO_DICTIONARY);
    };
    if let Some(model_hash) = ypt_active_model_for_textures(payload, selection) {
        let names = ypt_model_texture_names(asset, model_hash);
        if names.is_empty() {
            return TextureListing::Nothing("This model has no textures");
        }
        let textures = (names.iter().enumerate())
            .map(|(index, name)| {
                let found = ypt_model_texture_lookup(asset, model_hash, name).map(T::wrap);
                let (width, height, format) = found.map_or((0, 0, GrcTextureFormat::None), |t| {
                    (u32::from(t.width()), u32::from(t.height()), t.format())
                });
                DocumentTexture {
                    name: name.clone(),
                    width,
                    height,
                    format,
                    selection_id: dict_entry_part_id(
                        model_hash,
                        YPT_MODEL_KIND_TEXTURE,
                        index as u32,
                    ),
                }
            })
            .collect();
        return TextureListing::Textures {
            textures,
            source: TextureSource::Entry { hash: model_hash },
        };
    }
    match asset.texture_dict.as_deref() {
        Some(dict) => TextureListing::Textures {
            textures: (dict.entries.iter().flatten())
                .map(|t| row::<T>(t, ypt_texture_id(T::name_of(t))))
                .collect(),
            source: TextureSource::Document,
        },
        None => TextureListing::Nothing("This particle file has no embedded textures"),
    }
}

/// The texture `name` of `source` in a particle list, where the preview decodes it.
pub(super) fn at<'p, T: ParticleTarget>(
    payload: &'p DocPayload,
    source: TextureSource,
    name: &str,
) -> Option<Texture<'p>> {
    let asset = T::list(payload)?;
    let found = match source {
        TextureSource::Document => {
            let dict = asset.texture_dict.as_deref()?;
            dict.entries
                .iter()
                .flatten()
                .find(|t| T::name_of(t) == name)
                .map(|t| &**t)
        }
        TextureSource::Entry { hash } => ypt_model_texture_lookup(asset, hash, name),
        TextureSource::Root => None,
    };
    found.filter(|t| T::decodes(t)).map(T::wrap)
}

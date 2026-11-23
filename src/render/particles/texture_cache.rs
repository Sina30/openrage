use std::collections::HashMap;
use std::sync::Arc;

use or_io::codec::texture::{decode_level, ChannelView, TextureSource};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PtxFxList;
use or_renderer::game_shaders::mesh::Image;

pub struct ParticleTextureCache {
    /// The first level of each texture a batch has named, as the game's own passes bind it; a name the dictionary lacks is kept as `None`.
    pub(super) images: HashMap<String, Option<Arc<Image>>>,
    pub doc_id: Option<crate::state::DocumentId>,
    /// A scanned GTA VI list's sprites, by rule.
    pub(super) gta6: super::gta6::Gta6Sprites,
}

impl ParticleTextureCache {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            doc_id: None,
            gta6: Default::default(),
        }
    }

    pub fn invalidate_for_doc(&mut self, doc_id: crate::state::DocumentId) {
        if self.doc_id != Some(doc_id) {
            self.images.clear();
            self.gta6 = Default::default();
            self.doc_id = Some(doc_id);
        }
    }

    /// The texture `texture_name` of the effect list's dictionary, decoded once, for the game's own passes to bind.
    pub fn get_or_decode_image(
        &mut self,
        asset: &PtxFxList,
        texture_name: &str,
    ) -> Option<Arc<Image>> {
        if let Some(image) = self.images.get(texture_name) {
            return image.clone();
        }
        let lower = texture_name.to_lowercase();
        let image = asset
            .texture_dict
            .as_deref()
            .and_then(|dict| {
                dict.entries
                    .iter()
                    .flatten()
                    .find(|t| t.name.to_lowercase() == lower)
            })
            .and_then(|t| decode_image(t))
            .map(Arc::new);
        self.images.insert(texture_name.to_string(), image.clone());
        image
    }
}

/// The first level of `grc` as 8-bit RGBA in the preview's channel order.
fn decode_image(grc: &GrcTexture) -> Option<Image> {
    let rgba = decode_level(grc, 0)
        .ok()?
        .with_channel_view(ChannelView::preview_default(grc.pixel_format()));
    if rgba.data.is_empty() {
        return None;
    }
    Some(Image {
        width: rgba.width,
        height: rgba.height,
        rgba: rgba.data,
        srgb: rgba.is_srgb,
    })
}

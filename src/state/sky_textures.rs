//! The maps the game's sky and its water draw from, read out of the install's `skydome.ytd` and `graphics.ytd`: the stars, the face of the moon, and the noise the clouds are cut from; the ripples and the foam.

use std::path::Path;
use std::sync::Arc;

use or_io::container::RpfCryptoProvider;
use or_renderer::game_shaders::dome::SkyTextures;
use or_renderer::game_shaders::drawable::first_level;
use or_renderer::game_shaders::mesh::Image;
use or_renderer::game_shaders::water::WaterTextures;

use super::textures::Texture;
use super::{DocPayload, DocumentKind};

/// The name of each map in the sky's dictionary.
const STARS: &str = "starfield";
const MOON: &str = "moon-new";
const NOISE: &str = "baseperlinnoise3channel";
const DETAIL: &str = "noise16_p";
const DITHER: &str = "dither";

/// The name of each map in the water's dictionary.
const BUMP: &str = "waterbump";
const BUMP2: &str = "waterbump2";
const FOAM: &str = "waterfoam";

/// The first level of each of `names` in `bytes`, a dictionary as the install holds it, in the order asked; a map it lacks or one that will not decode is `None`.
fn first_levels(
    file: &str,
    bytes: &[u8],
    crypto: Arc<dyn RpfCryptoProvider>,
    names: &[&str],
) -> Vec<Option<Image>> {
    let payload =
        super::document_io::parse_payload(Path::new(file), DocumentKind::Ytd, bytes, crypto);
    tracing::info!(file, ok = payload.is_ok(), len = bytes.len());
    let Ok(DocPayload::Resource(asset)) = payload else {
        return vec![None; names.len()];
    };
    let textures: Vec<Texture> = match &asset {
        or_io::Asset::TextureDict(dict) => (dict.entries.iter().flatten())
            .map(|t| Texture::Owned(std::borrow::Cow::Borrowed(&**t)))
            .collect(),
        or_io::Asset::TextureDictGen9(dict) => (dict.entries.iter().flatten())
            .map(|t| Texture::Enhanced(std::borrow::Cow::Borrowed(&**t)))
            .collect(),
        _ => Vec::new(),
    };
    names
        .iter()
        .map(|name| {
            textures
                .iter()
                .find(|t| t.name().eq_ignore_ascii_case(name))
                .and_then(|t| first_level(t))
        })
        .collect()
}

/// The stars and the moon of `bytes`, the dictionary as the install holds it; a map it lacks or one that will not decode is left out.
pub(super) fn decode(bytes: &[u8], crypto: Arc<dyn RpfCryptoProvider>) -> SkyTextures {
    let [stars, moon, noise, detail, dither] = first_levels(
        "skydome.ytd",
        bytes,
        crypto,
        &[STARS, MOON, NOISE, DETAIL, DITHER],
    )
    .try_into()
    .expect("a map for each name");
    SkyTextures {
        stars,
        moon,
        noise,
        detail,
        dither,
    }
}

/// The ripples and the foam of `bytes`, the dictionary of the shader library as the install holds it.
pub(super) fn decode_water(bytes: &[u8], crypto: Arc<dyn RpfCryptoProvider>) -> WaterTextures {
    let [bump, bump2, foam] = first_levels("graphics.ytd", bytes, crypto, &[BUMP, BUMP2, FOAM])
        .try_into()
        .expect("a map for each name");
    WaterTextures { bump, bump2, foam }
}

#[cfg(test)]
mod tests {
    use or_io::codec::texture::GrcTextureFormat;
    use or_io::container::NoOpCryptoProvider;
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::PgDictionary;
    use or_io::Asset;

    use super::*;

    /// A `.ytd` holding an 8×8 texture for each of `names`.
    fn dictionary(names: &[&str]) -> Vec<u8> {
        let mut dict = PgDictionary::<GrcTexture>::default();
        for name in names {
            let mut image = GrcTexture {
                name: (*name).to_string(),
                width: 8,
                height: 8,
                depth: 1,
                mip_stride: 16,
                format: GrcTextureFormat::DXT1.to_raw_format(),
                mip_count: 1,
                backing_store: vec![0; 32],
                ..Default::default()
            };
            image.set_template_type(1 << 28);
            dict.insert(image);
        }
        Asset::TextureDict(dict).to_bytes().expect("a dictionary")
    }

    fn crypto() -> Arc<dyn RpfCryptoProvider> {
        Arc::new(NoOpCryptoProvider::new())
    }

    #[test]
    fn the_water_takes_its_ripples_and_its_foam_from_the_maps_of_those_names_and_leaves_out_one_it_lacks(
    ) {
        let all = decode_water(&dictionary(&[BUMP, BUMP2, FOAM, "other"]), crypto());
        for map in [&all.bump, &all.bump2, &all.foam] {
            let map = map.as_ref().expect("a map");
            assert_eq!((map.width, map.height), (8, 8));
        }
        let none = decode_water(&dictionary(&[FOAM]), crypto());
        assert!(none.bump.is_none() && none.bump2.is_none() && none.foam.is_some());
        // Bytes that are no dictionary leave all out
        let nothing = decode_water(b"not a dictionary", crypto());
        assert!(nothing.bump.is_none() && nothing.bump2.is_none() && nothing.foam.is_none());
    }

    #[test]
    fn the_sky_still_takes_its_five_maps_by_name() {
        let sky = decode(&dictionary(&[STARS, MOON, NOISE, DETAIL, DITHER]), crypto());
        assert!(sky.stars.is_some() && sky.moon.is_some() && sky.noise.is_some());
        assert!(sky.detail.is_some() && sky.dither.is_some());
        assert!(decode(&dictionary(&[BUMP]), crypto()).stars.is_none());
    }
}

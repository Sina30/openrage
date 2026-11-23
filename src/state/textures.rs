use std::borrow::Cow;

use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{gen8, gen9, PgDictionary, RmcDrawable};

use super::capabilities;
use super::document::DocPayload;

/// A texture the preview, the cache and the viewport read: a Legacy or an Enhanced owned entry.
// One lookup's answer, borrowed but for that view or a preview's copy, so the
// owned variant's size is not worth a box.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Texture<'p> {
    Owned(Cow<'p, gen8::GrcTexture>),
    Enhanced(Cow<'p, gen9::GrcTextureG9>),
}

impl Texture<'_> {
    fn pixels(&self) -> &dyn or_io::codec::texture::TextureSource {
        match self {
            Self::Owned(t) => t.as_ref(),
            Self::Enhanced(t) => t.as_ref(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Owned(t) => &t.name,
            Self::Enhanced(t) => &t.name,
        }
    }

    pub fn width(&self) -> u16 {
        match self {
            Self::Owned(t) => t.width,
            Self::Enhanced(t) => t.width,
        }
    }

    pub fn height(&self) -> u16 {
        match self {
            Self::Owned(t) => t.height,
            Self::Enhanced(t) => t.height,
        }
    }

    pub fn format(&self) -> GrcTextureFormat {
        self.pixels().pixel_format()
    }

    pub fn mip_count(&self) -> u8 {
        match self {
            Self::Owned(t) => t.mip_count,
            Self::Enhanced(t) => t.mip_levels,
        }
    }

    pub fn has_pixel_data(&self) -> bool {
        !self.pixels().payload().is_empty()
    }

    pub fn into_owned(self) -> Texture<'static> {
        match self {
            Self::Owned(t) => Texture::Owned(Cow::Owned(t.into_owned())),
            Self::Enhanced(t) => Texture::Enhanced(Cow::Owned(t.into_owned())),
        }
    }
}

impl or_io::codec::texture::TextureSource for Texture<'_> {
    fn pixel_format(&self) -> GrcTextureFormat {
        self.pixels().pixel_format()
    }

    fn is_srgb(&self) -> bool {
        self.pixels().is_srgb()
    }

    fn stored_size(&self) -> (u32, u32) {
        self.pixels().stored_size()
    }

    fn stored_levels(&self) -> u32 {
        self.pixels().stored_levels()
    }

    fn payload(&self) -> &[u8] {
        self.pixels().payload()
    }
}

/// A Legacy `.ytd`, as the owned dictionary it opens into.
pub fn legacy_dictionary(payload: &DocPayload) -> Option<&PgDictionary<gen8::GrcTexture>> {
    match payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => Some(dict),
        _ => None,
    }
}

/// A Legacy `.ytd`'s owned dictionary, to edit.
pub fn legacy_dictionary_mut(
    payload: &mut DocPayload,
) -> Option<&mut PgDictionary<gen8::GrcTexture>> {
    match payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => Some(dict),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureSource {
    Document,

    Root,

    Entry { hash: u32 },
}

#[derive(Debug, Clone)]
pub struct DocumentTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: GrcTextureFormat,

    pub selection_id: u64,
}

#[derive(Debug)]
pub enum TextureListing {
    Textures {
        textures: Vec<DocumentTexture>,
        source: TextureSource,
    },

    Nothing(&'static str),
}

#[derive(Debug)]
pub enum TextureResolution {
    Found { name: String, source: TextureSource },

    NotATexture(&'static str),

    Missing,
}

pub(super) const NO_DICTIONARY: &str = "Active document has no texture dictionary";

const NO_TEXTURES: &str = "This document has no textures";

const PICK_IN_TEXTURES_SECTION: &str = "Select one of its textures, under Textures";

const PICK_UNDER_A_DRAWABLE: &str = "Select one of its textures, under Drawable ▸ Textures";

pub const FRAGMENT_ENTRY_HASH: u32 = 0;

pub fn textures_in(payload: &DocPayload, selection: Option<u64>) -> TextureListing {
    match capabilities::for_payload(payload).and_then(|row| row.textures) {
        Some(list) => list(payload, selection),
        None => TextureListing::Nothing(NO_DICTIONARY),
    }
}

pub fn texture_for_selection(payload: &DocPayload, selection: u64) -> TextureResolution {
    match capabilities::for_payload(payload).and_then(|row| row.texture_for_selection) {
        Some(resolve) => resolve(payload, selection),
        None => TextureResolution::NotATexture(NO_TEXTURES),
    }
}

pub fn texture_at<'p>(
    payload: &'p DocPayload,
    source: TextureSource,
    name: &str,
) -> Option<Texture<'p>> {
    (capabilities::for_payload(payload)?.texture_at?)(payload, source, name)
}

pub fn texture_named<'p>(
    payload: &'p DocPayload,
    selection: Option<u64>,
    name: &str,
) -> Option<Texture<'p>> {
    (capabilities::for_payload(payload)?.texture_named?)(payload, selection, name)
}

pub use super::gta6_textures::{open_pack_entry, resolved_for_gta6, sized_for_gta6};

/// The name of the one texture a document is, which the preview shows with nothing selected.
pub fn sole_texture(payload: &DocPayload) -> Option<String> {
    match payload {
        DocPayload::Dds(texture) => Some(texture.name.clone()),
        _ => None,
    }
}

/// What the picture [`document_image`] makes shows, for a document that offers one.
pub fn image_label(payload: &DocPayload) -> Option<&'static str> {
    match payload {
        DocPayload::HeightMap(_) => {
            Some("Each cell's maximum height, north at the top; black has none.")
        }
        DocPayload::WaterMap(_) => Some(
            "Each tile's first water body, north at the top: blue a river, cyan a lake, \
             yellow a pool, navy the ocean; black has none.",
        ),
        DocPayload::WorldBlanket(_) => Some(
            "Each cell's stated value as grey, white at the largest; row 0 at the top, \
             as stored, since the file states no orientation.",
        ),
        DocPayload::NavPrecalc(_) => Some(
            "Each cell's entry count as grey, white at the most, the cells laid out as \
             the square their count makes; row 0 at the top, as stored.",
        ),
        _ => None,
    }
}

/// The picture a document that is not a texture offers the texture workspace.
pub fn document_image(payload: &DocPayload) -> Option<or_io::codec::texture::RgbaImage> {
    match payload {
        DocPayload::HeightMap(map) => height_map_image(map),
        DocPayload::WaterMap(map) => water_map_image(map),
        DocPayload::WorldBlanket(blanket) => {
            grey_square(blanket.cells.iter().map(|&c| usize::from(c)).collect())
        }
        DocPayload::NavPrecalc(precalc) => {
            grey_square(precalc.cells.iter().map(|c| c.entries()).collect())
        }
        _ => None,
    }
}

/// `values` as a square of greys scaled to the largest; `None` unless their count is a square.
fn grey_square(values: Vec<usize>) -> Option<or_io::codec::texture::RgbaImage> {
    let side = values.len().isqrt();
    if side == 0 || side * side != values.len() {
        return None;
    }
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let data = values
        .iter()
        .flat_map(|&v| {
            let grey = (v * 255 / max) as u8;
            [grey, grey, grey, 0xFF]
        })
        .collect();
    Some(or_io::codec::texture::RgbaImage {
        width: side as u32,
        height: side as u32,
        data,
        is_srgb: false,
    })
}

fn water_map_image(map: &or_io::water_map::WaterMap) -> Option<or_io::codec::texture::RgbaImage> {
    use or_io::water_map::WaterMapRefType as T;

    let (cols, rows) = (map.num_tile_cols, map.num_tile_rows());
    if cols == 0 || rows == 0 {
        return None;
    }
    // Row 0 is the grid's north edge, so the rows stay in their stored order.
    let data = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (col, row)))
        .flat_map(|(col, row)| {
            let first = map.tile_refs(col, row).and_then(|refs| refs.first());
            match first.map(|r| r.kind()) {
                None => [0, 0, 0, 0xFF],
                Some(T::Ocean) => [0x20, 0x30, 0x80, 0xFF],
                Some(T::River) => [0x40, 0x80, 0xFF, 0xFF],
                Some(T::Lake) => [0x40, 0xE0, 0xE0, 0xFF],
                Some(T::Pool) => [0xFF, 0xE0, 0x40, 0xFF],
            }
        })
        .collect();
    Some(or_io::codec::texture::RgbaImage {
        width: u32::from(cols),
        height: rows as u32,
        data,
        is_srgb: false,
    })
}

fn height_map_image(
    map: &or_io::height_map::HeightMap,
) -> Option<or_io::codec::texture::RgbaImage> {
    let (cols, rows) = (usize::from(map.num_cols), usize::from(map.num_rows));
    if cols == 0 || rows == 0 {
        return None;
    }
    // Row 0 is the grid's south edge, so the rows are reversed to put north at the top.
    let data = (map.max.get(..cols * rows)?.chunks(cols).rev())
        .flatten()
        .flat_map(|&height| [height, height, height, 0xFF])
        .collect();
    Some(or_io::codec::texture::RgbaImage {
        width: u32::from(map.num_cols),
        height: u32::from(map.num_rows),
        data,
        is_srgb: false,
    })
}

/// Writes `texture` to `path`: its whole mip chain for a `.dds`, else mip `level` as a PNG.
pub fn export_texture(
    texture: &Texture<'_>,
    level: u32,
    path: &std::path::Path,
) -> Result<(), String> {
    use or_io::codec::texture::{decode_level, save_png, to_dds};

    let is_dds = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dds"));
    let written = if is_dds {
        to_dds(texture).and_then(|dds| dds.save(path))
    } else {
        decode_level(texture, level).and_then(|image| save_png(path, &image))
    };
    written.map_err(|e| format!("{}: {e}", path.display()))
}

/// The Enhanced texture behind a selection, whatever its format.
pub fn enhanced_texture_at<'p>(
    payload: &'p DocPayload,
    source: TextureSource,
    name: &str,
) -> Option<&'p gen9::GrcTextureG9> {
    let dictionary = match (payload, source) {
        (DocPayload::Resource(or_io::Asset::TextureDictGen9(dict)), TextureSource::Document) => {
            dict
        }
        (DocPayload::Resource(or_io::Asset::DrawableGen9(d)), TextureSource::Root) => {
            drawable_textures(d)?
        }
        (
            DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)),
            TextureSource::Entry { hash },
        ) => drawable_textures(rows::entry(dict, hash)?)?,
        (
            DocPayload::Resource(or_io::Asset::FragmentGen9(frag)),
            TextureSource::Entry {
                hash: FRAGMENT_ENTRY_HASH,
            },
        ) => rows::fragment_textures(frag)?,
        _ => return None,
    };
    find_in_dictionary_g9(Some(dictionary), name)
}

/// The texture `name`, lowercase, names in a Legacy dictionary.
pub fn find_in_dictionary<'a>(
    dict: Option<&'a PgDictionary<gen8::GrcTexture>>,
    name: &str,
) -> Option<&'a gen8::GrcTexture> {
    (dict?.entries.iter().flatten())
        .find(|t| t.name.to_ascii_lowercase() == name)
        .map(|t| &**t)
}

/// The dictionary a drawable of either target embeds.
pub fn drawable_textures<T: or_io::gta5::DrawableTarget>(
    drawable: &RmcDrawable<T>,
) -> Option<&PgDictionary<T::Texture>> {
    drawable
        .shader_group
        .as_deref()?
        .texture_dictionary
        .as_deref()
}

/// What the browser and the preview read of a texture and its dictionary, per target.
pub(crate) trait Lookup: or_io::gta5::DrawableTarget {
    /// The texture `name`, lowercase, names in `dict`, where the preview decodes it.
    fn named<'a>(dict: &'a PgDictionary<Self::Texture>, name: &str) -> Option<Texture<'a>>;

    /// `texture` as the preview reads it, whether or not it decodes.
    fn wrap(texture: &Self::Texture) -> Texture<'_>;

    fn name_of(texture: &Self::Texture) -> &str;

    /// Whether the preview decodes `texture`.
    fn decodes(texture: &Self::Texture) -> bool;
}

impl Lookup for Gen8 {
    fn named<'a>(dict: &'a PgDictionary<gen8::GrcTexture>, name: &str) -> Option<Texture<'a>> {
        find_in_dictionary(Some(dict), name).map(Self::wrap)
    }

    fn wrap(texture: &gen8::GrcTexture) -> Texture<'_> {
        Texture::Owned(Cow::Borrowed(texture))
    }

    fn name_of(texture: &gen8::GrcTexture) -> &str {
        &texture.name
    }

    fn decodes(_: &gen8::GrcTexture) -> bool {
        true
    }
}

impl Lookup for Gen9 {
    fn named<'a>(dict: &'a PgDictionary<gen9::GrcTextureG9>, name: &str) -> Option<Texture<'a>> {
        find_in_dictionary_g9(Some(dict), name)
            .filter(|t| decodes_g9(t))
            .map(Self::wrap)
    }

    fn wrap(texture: &gen9::GrcTextureG9) -> Texture<'_> {
        Texture::Enhanced(Cow::Borrowed(texture))
    }

    fn name_of(texture: &gen9::GrcTextureG9) -> &str {
        &texture.name
    }

    fn decodes(texture: &gen9::GrcTextureG9) -> bool {
        decodes_g9(texture)
    }
}

/// The texture named `name`, whatever its case, in an Enhanced dictionary.
pub fn find_in_dictionary_g9<'a>(
    dict: Option<&'a PgDictionary<gen9::GrcTextureG9>>,
    name: &str,
) -> Option<&'a gen9::GrcTextureG9> {
    (dict?.entries.iter().flatten())
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .map(|t| &**t)
}

/// Whether the preview decodes an Enhanced texture: its format has a Legacy counterpart.
pub fn decodes_g9(texture: &gen9::GrcTextureG9) -> bool {
    or_io::codec::texture::TextureSource::pixel_format(texture) != GrcTextureFormat::None
}

pub(crate) mod rows {
    use super::*;

    pub(super) fn selected_entry_index(hashes: &[u32], selection: Option<u64>) -> Option<usize> {
        use crate::ui::panels::selection_ids::{dict_entry_unpack_part, is_dict_entry_selection};

        let selection = selection?;
        if !is_dict_entry_selection(selection) {
            return None;
        }
        let (hash, _, _) = dict_entry_unpack_part(selection);
        hashes.iter().position(|h| *h == hash)
    }

    pub(super) fn fragment_textures<T: or_io::gta5::DrawableTarget>(
        frag: &or_io::gta5::FragType<T>,
    ) -> Option<&PgDictionary<T::Texture>> {
        drawable_textures(&frag.common_drawable.as_deref()?.base)
    }

    /// The row the browser lists for `texture`, whichever target it is of.
    pub(crate) fn row<T: Lookup>(texture: &T::Texture, selection_id: u64) -> DocumentTexture {
        let texture = T::wrap(texture);
        DocumentTexture {
            name: texture.name().to_string(),
            width: u32::from(texture.width()),
            height: u32::from(texture.height()),
            format: texture.format(),
            selection_id,
        }
    }

    /// A drawable dictionary's entry under `hash`.
    pub(super) fn entry<T: or_io::gta5::DrawableTarget>(
        dict: &PgDictionary<or_io::gta5::GtaDrawable<T>>,
        hash: u32,
    ) -> Option<&or_io::gta5::GtaDrawable<T>> {
        let at = dict.codes.iter().position(|h| *h == hash)?;
        dict.entries.get(at)?.as_deref()
    }

    pub(crate) mod list {
        use super::*;
        use crate::state::particle_target::on_particle_target;
        use crate::state::particle_textures;
        use crate::ui::panels::selection_ids::{
            dict_entry_part_id, name_hash, SECT_MASK, SECT_TEXTURE, YPT_MODEL_KIND_TEXTURE,
        };

        pub(crate) fn ytd(payload: &DocPayload, _selection: Option<u64>) -> TextureListing {
            let textures: Vec<DocumentTexture> = match payload {
                DocPayload::Resource(or_io::Asset::TextureDict(dict)) => (dict.entries.iter())
                    .flatten()
                    .map(|t| row::<Gen8>(t, name_hash(&t.name)))
                    .collect(),
                DocPayload::Resource(or_io::Asset::TextureDictGen9(dict)) => (dict.entries.iter())
                    .flatten()
                    .map(|t| row::<Gen9>(t, name_hash(&t.name)))
                    .collect(),
                _ => return TextureListing::Nothing(NO_DICTIONARY),
            };
            TextureListing::Textures {
                textures,
                source: TextureSource::Document,
            }
        }

        pub(crate) fn ypt(payload: &DocPayload, selection: Option<u64>) -> TextureListing {
            on_particle_target!(payload, T => particle_textures::list::<T>(payload, selection),
                else TextureListing::Nothing(NO_DICTIONARY))
        }

        pub(crate) fn ydr(payload: &DocPayload, _selection: Option<u64>) -> TextureListing {
            let id = |name: &str| SECT_TEXTURE | (name_hash(name) & !SECT_MASK);
            let textures: Vec<DocumentTexture> = match payload {
                DocPayload::Resource(or_io::Asset::Drawable(drawable)) => {
                    let Some(dict) = drawable_textures(drawable) else {
                        return TextureListing::Nothing("This drawable has no embedded textures");
                    };
                    (dict.entries.iter().flatten())
                        .map(|t| row::<Gen8>(t, id(&t.name)))
                        .collect()
                }
                DocPayload::Resource(or_io::Asset::DrawableGen9(drawable)) => {
                    let Some(dict) = drawable_textures(drawable) else {
                        return TextureListing::Nothing("This drawable has no embedded textures");
                    };
                    (dict.entries.iter().flatten())
                        .map(|t| row::<Gen9>(t, id(&t.name)))
                        .collect()
                }
                _ => return TextureListing::Nothing(NO_DICTIONARY),
            };
            TextureListing::Textures {
                textures,
                source: TextureSource::Root,
            }
        }

        pub(crate) fn ydd(payload: &DocPayload, selection: Option<u64>) -> TextureListing {
            let part =
                |hash: u32, i: usize| dict_entry_part_id(hash, YPT_MODEL_KIND_TEXTURE, i as u32);

            let (hash, textures): (u32, Vec<DocumentTexture>) = match payload {
                DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => {
                    let Some(index) = selected_entry_index(&dict.codes, selection) else {
                        return TextureListing::Nothing("Select one of its drawables");
                    };
                    let Some(entry) = dict.entries.get(index).and_then(Option::as_deref) else {
                        return TextureListing::Nothing("Selection is no longer valid");
                    };
                    let hash = dict.codes[index];
                    let Some(textures) = drawable_textures(entry) else {
                        return TextureListing::Nothing("This drawable has no embedded textures");
                    };
                    (
                        hash,
                        (textures.entries.iter().flatten())
                            .enumerate()
                            .map(|(i, t)| row::<Gen8>(t, part(hash, i)))
                            .collect(),
                    )
                }
                DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => {
                    let Some(index) = selected_entry_index(&dict.codes, selection) else {
                        return TextureListing::Nothing("Select one of its drawables");
                    };
                    let Some(entry) = dict.entries.get(index).and_then(Option::as_deref) else {
                        return TextureListing::Nothing("Selection is no longer valid");
                    };
                    let hash = dict.codes[index];
                    let Some(textures) = drawable_textures(entry) else {
                        return TextureListing::Nothing("This drawable has no embedded textures");
                    };
                    (
                        hash,
                        (textures.entries.iter().flatten())
                            .enumerate()
                            .map(|(i, t)| row::<Gen9>(t, part(hash, i)))
                            .collect(),
                    )
                }
                _ => return TextureListing::Nothing(NO_DICTIONARY),
            };

            TextureListing::Textures {
                textures,
                source: TextureSource::Entry { hash },
            }
        }

        pub(crate) fn yft(payload: &DocPayload, _selection: Option<u64>) -> TextureListing {
            match payload {
                DocPayload::Resource(or_io::Asset::Fragment(frag)) => fragment_rows(frag),
                DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => fragment_rows(frag),
                _ => TextureListing::Nothing(NO_DICTIONARY),
            }
        }

        fn fragment_rows<T: Lookup>(frag: &or_io::gta5::FragType<T>) -> TextureListing {
            let Some(textures) = fragment_textures(frag) else {
                return TextureListing::Nothing("This fragment has no embedded textures");
            };
            let id = |i: usize| {
                dict_entry_part_id(FRAGMENT_ENTRY_HASH, YPT_MODEL_KIND_TEXTURE, i as u32)
            };
            TextureListing::Textures {
                textures: (textures.entries.iter().flatten())
                    .enumerate()
                    .map(|(i, t)| row::<T>(t, id(i)))
                    .collect(),
                source: TextureSource::Entry {
                    hash: FRAGMENT_ENTRY_HASH,
                },
            }
        }

        pub(crate) fn dds(payload: &DocPayload, _selection: Option<u64>) -> TextureListing {
            let DocPayload::Dds(texture) = payload else {
                return TextureListing::Nothing(NO_DICTIONARY);
            };
            TextureListing::Textures {
                textures: vec![row::<Gen8>(texture, name_hash(&texture.name))],
                source: TextureSource::Document,
            }
        }

        /// A texture pack's rows, from its directory alone; the selection is the index.
        pub(crate) fn ttpf(payload: &DocPayload, _selection: Option<u64>) -> TextureListing {
            let DocPayload::Ttpf(ttpf) = payload else {
                return TextureListing::Nothing(NO_DICTIONARY);
            };
            let textures = (ttpf.pack.entries.iter().enumerate())
                .map(|(index, entry)| DocumentTexture {
                    name: ttpf.name(index),
                    width: entry.width,
                    height: entry.height,
                    format: entry
                        .format()
                        .map_or(GrcTextureFormat::None, |f| f.format()),
                    selection_id: index as u64,
                })
                .collect();
            TextureListing::Textures {
                textures,
                source: TextureSource::Document,
            }
        }
    }

    pub(crate) mod selected {
        use super::*;
        use crate::ui::panels::selection_ids::{
            dict_entry_unpack_part, name_hash, section_of, texture_name_for_selection,
            ydr_drawable_texture_name_for_selection, ypt_model_texture_name_for_selection,
            ypt_tag_of, ypt_texture_name_for_selection, ypt_unpack_model_part, SECT_TEXTURE,
            YPT_MODEL, YPT_MODEL_KIND_TEXTURE, YPT_TEXTURE,
        };

        fn found(name: String, source: TextureSource) -> TextureResolution {
            TextureResolution::Found { name, source }
        }

        pub(crate) fn ytd(payload: &DocPayload, selection: u64) -> TextureResolution {
            let name = match payload {
                DocPayload::Resource(or_io::Asset::TextureDictGen9(dict)) => (dict.entries.iter())
                    .flatten()
                    .find(|t| name_hash(&t.name) == selection)
                    .map(|t| t.name.clone()),
                _ => texture_name_for_selection(payload, selection),
            };
            match name {
                Some(name) => found(name, TextureSource::Document),
                None => TextureResolution::Missing,
            }
        }

        pub(crate) fn ypt(payload: &DocPayload, selection: u64) -> TextureResolution {
            match ypt_tag_of(selection) {
                YPT_TEXTURE => match ypt_texture_name_for_selection(payload, selection) {
                    Some(name) => found(name, TextureSource::Document),
                    None => TextureResolution::Missing,
                },
                YPT_MODEL => {
                    let (model_hash, kind, _index) = ypt_unpack_model_part(selection);
                    if kind != YPT_MODEL_KIND_TEXTURE {
                        return TextureResolution::NotATexture(PICK_IN_TEXTURES_SECTION);
                    }
                    match ypt_model_texture_name_for_selection(payload, selection) {
                        Some(name) => found(name, TextureSource::Entry { hash: model_hash }),
                        None => TextureResolution::Missing,
                    }
                }
                _ => TextureResolution::NotATexture(PICK_IN_TEXTURES_SECTION),
            }
        }

        pub(crate) fn ydr(payload: &DocPayload, selection: u64) -> TextureResolution {
            if section_of(selection) != SECT_TEXTURE {
                return TextureResolution::NotATexture(PICK_UNDER_A_DRAWABLE);
            }
            match ydr_drawable_texture_name_for_selection(payload, selection) {
                Some(name) => found(name, TextureSource::Root),
                None => TextureResolution::Missing,
            }
        }

        pub(crate) fn ydd(payload: &DocPayload, selection: u64) -> TextureResolution {
            let (hash, kind, index) = dict_entry_unpack_part(selection);
            if kind != YPT_MODEL_KIND_TEXTURE {
                return TextureResolution::NotATexture(PICK_UNDER_A_DRAWABLE);
            }
            let name = match payload {
                DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => entry(dict, hash)
                    .and_then(|d| drawable_textures(d))
                    .and_then(|td| td.entries.iter().flatten().nth(index as usize))
                    .map(|t| t.name.clone()),
                DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => entry(dict, hash)
                    .and_then(|d| drawable_textures(d))
                    .and_then(|td| td.entries.iter().flatten().nth(index as usize))
                    .map(|t| t.name.clone()),
                _ => return TextureResolution::Missing,
            };
            match name {
                Some(name) => found(name, TextureSource::Entry { hash }),
                None => TextureResolution::Missing,
            }
        }

        pub(crate) fn yft(payload: &DocPayload, selection: u64) -> TextureResolution {
            fn name_in<T: Lookup>(frag: &or_io::gta5::FragType<T>, index: u32) -> Option<String> {
                let texture = fragment_textures(frag)?.entries.iter().flatten();
                texture
                    .map(|t| T::name_of(t).to_string())
                    .nth(index as usize)
            }
            let (hash, kind, index) = dict_entry_unpack_part(selection);
            let name = match payload {
                DocPayload::Resource(or_io::Asset::Fragment(frag)) => name_in(frag, index),
                DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => name_in(frag, index),
                _ => return TextureResolution::Missing,
            };
            if kind != YPT_MODEL_KIND_TEXTURE {
                return TextureResolution::NotATexture(PICK_UNDER_A_DRAWABLE);
            }
            if hash != FRAGMENT_ENTRY_HASH {
                return TextureResolution::Missing;
            }
            match name {
                Some(name) => found(name, TextureSource::Entry { hash }),
                None => TextureResolution::Missing,
            }
        }

        /// A `.dds` is one texture, whatever row is selected.
        pub(crate) fn dds(payload: &DocPayload, _selection: u64) -> TextureResolution {
            match sole_texture(payload) {
                Some(name) => found(name, TextureSource::Document),
                None => TextureResolution::Missing,
            }
        }

        pub(crate) fn ttpf(payload: &DocPayload, selection: u64) -> TextureResolution {
            match payload {
                DocPayload::Ttpf(ttpf) if (selection as usize) < ttpf.pack.entries.len() => {
                    found(ttpf.name(selection as usize), TextureSource::Document)
                }
                _ => TextureResolution::Missing,
            }
        }
    }

    pub(crate) mod at {
        use super::*;
        use crate::state::particle_target::on_particle_target;
        use crate::state::particle_textures;

        fn found_g9<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            enhanced_texture_at(payload, source, name)
                .filter(|t| decodes_g9(t))
                .map(|t| Texture::Enhanced(Cow::Borrowed(t)))
        }

        pub(crate) fn ytd<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            if source != TextureSource::Document {
                return None;
            }
            match payload {
                DocPayload::Resource(or_io::Asset::TextureDict(dict)) => {
                    dict.get(name).map(|t| Texture::Owned(Cow::Borrowed(t)))
                }
                DocPayload::Resource(or_io::Asset::TextureDictGen9(_)) => {
                    found_g9(payload, source, name)
                }
                _ => None,
            }
        }

        pub(crate) fn ypt<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            on_particle_target!(payload, T => particle_textures::at::<T>(payload, source, name),
                else None)
        }

        pub(crate) fn ydr<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            if source != TextureSource::Root {
                return None;
            }
            match payload {
                DocPayload::Resource(or_io::Asset::Drawable(drawable)) => {
                    drawable_textures(drawable)
                        .and_then(|td| td.entries.iter().flatten().find(|t| t.name == name))
                        .map(|t| Texture::Owned(Cow::Borrowed(t)))
                }
                DocPayload::Resource(or_io::Asset::DrawableGen9(_)) => {
                    found_g9(payload, source, name)
                }
                _ => None,
            }
        }

        pub(crate) fn ydd<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            let TextureSource::Entry { hash } = source else {
                return None;
            };
            match payload {
                DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => entry(dict, hash)
                    .and_then(|d| drawable_textures(d))
                    .and_then(|td| td.entries.iter().flatten().find(|t| t.name == name))
                    .map(|t| Texture::Owned(Cow::Borrowed(t))),
                DocPayload::Resource(or_io::Asset::DrawableDictGen9(_)) => {
                    found_g9(payload, source, name)
                }
                _ => None,
            }
        }

        pub(crate) fn yft<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            fn in_fragment<'p, T: Lookup>(
                frag: &'p or_io::gta5::FragType<T>,
                name: &str,
            ) -> Option<Texture<'p>> {
                let textures = fragment_textures(frag)?;
                let found = textures
                    .entries
                    .iter()
                    .flatten()
                    .find(|t| T::name_of(t) == name);
                found.filter(|t| T::decodes(t)).map(|t| T::wrap(t))
            }
            if source
                != (TextureSource::Entry {
                    hash: FRAGMENT_ENTRY_HASH,
                })
            {
                return None;
            }
            match payload {
                DocPayload::Resource(or_io::Asset::Fragment(frag)) => in_fragment(frag, name),
                DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => in_fragment(frag, name),
                _ => None,
            }
        }

        pub(crate) fn dds<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            match (payload, source) {
                (DocPayload::Dds(t), TextureSource::Document) if t.name == name => {
                    Some(Texture::Owned(Cow::Borrowed(t)))
                }
                _ => None,
            }
        }

        /// Reads the texture from the pack's file the first time it is asked for.
        pub(crate) fn ttpf<'p>(
            payload: &'p DocPayload,
            source: TextureSource,
            name: &str,
        ) -> Option<Texture<'p>> {
            let (DocPayload::Ttpf(ttpf), TextureSource::Document) = (payload, source) else {
                return None;
            };
            let index = ttpf.index_of(name)?;
            let texture = ttpf.texture(index).ok()?;
            Some(Texture::Owned(Cow::Owned(gen8::GrcTexture::clone(
                &texture,
            ))))
        }
    }

    pub(crate) mod named {
        use super::*;
        use crate::ui::panels::selection_ids::{
            ydd_selected_entry_hash, ypt_tag_of, ypt_unpack_model_hash, YPT_MODEL,
        };

        fn embedded<'p, T: Lookup>(
            drawable: &'p RmcDrawable<T>,
            name: &str,
        ) -> Option<Texture<'p>> {
            T::named(drawable_textures(drawable)?, name)
        }

        pub(crate) fn ydr<'p>(
            payload: &'p DocPayload,
            _selection: Option<u64>,
            name: &str,
        ) -> Option<Texture<'p>> {
            match payload {
                DocPayload::Resource(or_io::Asset::Drawable(d)) => embedded(&d.base, name),
                DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => embedded(&d.base, name),
                _ => None,
            }
        }

        pub(crate) fn ydd<'p>(
            payload: &'p DocPayload,
            selection: Option<u64>,
            name: &str,
        ) -> Option<Texture<'p>> {
            let hash = ydd_selected_entry_hash(payload, selection?)?;
            match payload {
                DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => {
                    embedded(&entry(dict, hash)?.base, name)
                }
                DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => {
                    embedded(&entry(dict, hash)?.base, name)
                }
                _ => None,
            }
        }

        pub(crate) fn yft<'p>(
            payload: &'p DocPayload,
            _selection: Option<u64>,
            name: &str,
        ) -> Option<Texture<'p>> {
            match payload {
                DocPayload::Resource(or_io::Asset::Fragment(frag)) => {
                    embedded(&frag.common_drawable.as_deref()?.base, name)
                }
                DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => {
                    embedded(&frag.common_drawable.as_deref()?.base, name)
                }
                _ => None,
            }
        }

        pub(crate) fn ypt<'p>(
            payload: &'p DocPayload,
            selection: Option<u64>,
            name: &str,
        ) -> Option<Texture<'p>> {
            /// The selected model's dictionary first, then every other model's, then the list's own.
            fn in_list<'p, T: Lookup>(
                asset: &'p or_io::gta5::PtxFxList<T>,
                selected: Option<u32>,
                name: &str,
            ) -> Option<Texture<'p>> {
                selected
                    .and_then(|hash| or_particle::list::model(asset, hash))
                    .map(drawable_textures)
                    .into_iter()
                    .chain(
                        or_particle::list::models(asset)
                            .filter(|(hash, _)| Some(*hash) != selected)
                            .map(|(_, model)| drawable_textures(model)),
                    )
                    .chain(std::iter::once(asset.texture_dict.as_deref()))
                    .flatten()
                    .find_map(|dict| T::named(dict, name))
            }
            let selected = selection
                .filter(|sel| ypt_tag_of(*sel) == YPT_MODEL)
                .map(ypt_unpack_model_hash);
            match payload {
                DocPayload::Resource(or_io::Asset::Particles(asset)) => {
                    in_list(asset, selected, name)
                }
                DocPayload::Resource(or_io::Asset::ParticlesGen9(asset)) => {
                    in_list(asset, selected, name)
                }
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::selection_ids::{dict_entry_id, name_hash, SECT_MASK, SECT_TEXTURE};

    use or_io::decl::{Gen9, Open};
    use or_io::gta5::GtaDrawable;

    fn enhanced_texture(name: &str, width: u16, height: u16) -> gen9::GrcTextureG9 {
        gen9::GrcTextureG9 {
            name: name.to_string(),
            width,
            height,
            format: Open::new(gen9::PixelFormat::Bc3UnormSrgb),
            ..Default::default()
        }
    }

    /// `textures` under the codes of their names, in the order given.
    fn enhanced_dictionary_of(
        textures: Vec<gen9::GrcTextureG9>,
    ) -> PgDictionary<gen9::GrcTextureG9> {
        PgDictionary {
            codes: (textures.iter())
                .map(|t| or_io::hash::string_hash_str(&t.name))
                .collect(),
            entries: textures.into_iter().map(|t| Some(Box::new(t))).collect(),
            ..Default::default()
        }
    }

    fn enhanced_dictionary(names: &[&str]) -> PgDictionary<gen9::GrcTextureG9> {
        enhanced_dictionary_of(
            (names.iter().enumerate())
                .map(|(i, name)| enhanced_texture(name, 4 << i, 8 << i))
                .collect(),
        )
    }

    #[allow(clippy::field_reassign_with_default)]
    fn enhanced_drawable(names: &[&str]) -> GtaDrawable<Gen9> {
        let mut group = or_io::gta5::GrmShaderGroup::<Gen9>::default();
        group.texture_dictionary = Some(Box::new(enhanced_dictionary(names)));
        let mut drawable = GtaDrawable::<Gen9>::default();
        drawable.base.shader_group = Some(Box::new(group));
        drawable
    }

    fn enhanced_ytd(names: &[&str]) -> DocPayload {
        DocPayload::Resource(or_io::Asset::TextureDictGen9(enhanced_dictionary(names)))
    }

    fn listed(
        payload: &DocPayload,
        selection: Option<u64>,
    ) -> (Vec<DocumentTexture>, TextureSource) {
        match textures_in(payload, selection) {
            TextureListing::Textures { textures, source } => (textures, source),
            TextureListing::Nothing(why) => panic!("expected a listing, got {why:?}"),
        }
    }

    fn round_trip(payload: &DocPayload, selection: Option<u64>, expect: &[&str]) {
        let (textures, source) = listed(payload, selection);
        let names: Vec<&str> = textures.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, expect);

        for tile in &textures {
            let resolved = texture_for_selection(payload, tile.selection_id);
            let TextureResolution::Found { name, source: back } = resolved else {
                panic!(
                    "{}: a tile's own id should name it, got {resolved:?}",
                    tile.name
                );
            };
            assert_eq!(name, tile.name, "a tile resolved to a different texture");
            assert_eq!(back, source, "and must point at the same dictionary");

            let fetched = texture_at(payload, source, &name)
                .unwrap_or_else(|| panic!("{name}: resolved and could not be fetched"));
            assert_eq!(fetched.name(), tile.name);
            assert_eq!(
                (fetched.width() as u32, fetched.height() as u32),
                (tile.width, tile.height)
            );
            assert_eq!(fetched.format(), tile.format, "caption format drift");
        }
    }

    #[test]
    fn an_enhanced_texture_dictionary_lists_its_textures() {
        let payload = enhanced_ytd(&["diffuse", "normal"]);
        let (textures, source) = listed(&payload, None);

        assert_eq!(textures.len(), 2);
        assert_eq!(source, TextureSource::Document);
        assert_eq!((textures[0].width, textures[0].height), (4, 8));
        assert_eq!((textures[1].width, textures[1].height), (8, 16));
        assert_eq!(textures[0].format, GrcTextureFormat::DXT5);
    }

    #[test]
    fn an_enhanced_tile_resolves_back_to_its_own_texture() {
        let payload = enhanced_ytd(&["diffuse", "normal"]);
        round_trip(&payload, None, &["diffuse", "normal"]);
    }

    #[test]
    fn an_enhanced_tile_is_addressed_the_way_its_outline_row_is() {
        let payload = enhanced_ytd(&["diffuse"]);
        let (textures, _) = listed(&payload, None);
        assert_eq!(textures[0].selection_id, name_hash("diffuse"));
    }

    #[test]
    fn an_enhanced_drawables_embedded_textures_are_browsable() {
        let payload = DocPayload::Resource(or_io::Asset::DrawableGen9(enhanced_drawable(&[
            "spec", "detail",
        ])));
        round_trip(&payload, None, &["spec", "detail"]);

        let (textures, source) = listed(&payload, None);
        assert_eq!(source, TextureSource::Root);
        assert_eq!(
            textures[0].selection_id,
            SECT_TEXTURE | (name_hash("spec") & !SECT_MASK)
        );
    }

    #[test]
    fn an_enhanced_dictionary_shows_the_selected_entrys_textures() {
        let payload = DocPayload::Resource(or_io::Asset::DrawableDictGen9(PgDictionary {
            codes: vec![0x1111_1111, 0x2222_2222],
            entries: vec![
                Some(Box::new(enhanced_drawable(&["first"]))),
                Some(Box::new(enhanced_drawable(&["second"]))),
            ],
            ..Default::default()
        }));

        assert!(matches!(
            textures_in(&payload, None),
            TextureListing::Nothing(_)
        ));

        for (hash, expect) in [(0x1111_1111u32, "first"), (0x2222_2222, "second")] {
            let selection = Some(dict_entry_id(hash));
            round_trip(&payload, selection, &[expect]);
            let (_, source) = listed(&payload, selection);
            assert_eq!(source, TextureSource::Entry { hash });
        }
    }

    #[test]
    fn neither_row_pays_for_a_conversion() {
        let mut legacy = PgDictionary::<gen8::GrcTexture>::default();
        legacy.insert(gen8::GrcTexture {
            name: "diffuse".to_string(),
            format: GrcTextureFormat::DXT5.to_raw_format(),
            ..Default::default()
        });
        let legacy = DocPayload::Resource(or_io::Asset::TextureDict(legacy));
        let enhanced = enhanced_ytd(&["diffuse"]);

        assert!(matches!(
            texture_at(&legacy, TextureSource::Document, "diffuse"),
            Some(Texture::Owned(Cow::Borrowed(_)))
        ));
        assert!(matches!(
            texture_at(&enhanced, TextureSource::Document, "diffuse"),
            Some(Texture::Enhanced(Cow::Borrowed(_)))
        ));
    }

    /// The owned Legacy `.ytd` reaches the tiles and the fetch the compiled one did.
    #[test]
    fn a_legacy_dictionarys_tiles_round_trip() {
        let mut dict = PgDictionary::<gen8::GrcTexture>::default();
        for (name, width) in [("diffuse", 4u16), ("normal", 8)] {
            dict.insert(gen8::GrcTexture {
                name: name.to_string(),
                width,
                height: 4,
                format: GrcTextureFormat::DXT1.to_raw_format(),
                ..Default::default()
            });
        }
        let payload = DocPayload::Resource(or_io::Asset::TextureDict(dict));
        let (textures, _) = listed(&payload, None);
        let in_code_order: Vec<&str> = textures.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = in_code_order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, ["diffuse", "normal"]);
        round_trip(&payload, None, &in_code_order);
    }

    #[test]
    fn an_enhanced_format_with_no_legacy_equivalent_is_listed_and_not_faked() {
        let payload =
            DocPayload::Resource(or_io::Asset::TextureDictGen9(enhanced_dictionary_of(vec![
                gen9::GrcTextureG9 {
                    name: "exotic".to_string(),
                    width: 16,
                    height: 16,
                    format: Open::new(gen9::PixelFormat::Unknown),
                    ..Default::default()
                },
            ])));

        let (textures, source) = listed(&payload, None);
        assert_eq!(textures.len(), 1, "it is still shown");
        assert_eq!(textures[0].format, GrcTextureFormat::None);
        assert_eq!((textures[0].width, textures[0].height), (16, 16));

        assert!(matches!(
            texture_for_selection(&payload, textures[0].selection_id),
            TextureResolution::Found { .. }
        ));
        assert!(texture_at(&payload, source, "exotic").is_none());
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn a_legacy_drawables_textures_still_round_trip() {
        let mut dictionary = PgDictionary::<gen8::GrcTexture>::default();
        dictionary.insert(gen8::GrcTexture {
            name: "diffuse".to_string(),
            width: 2,
            height: 4,
            format: GrcTextureFormat::DXT1.to_raw_format(),
            ..Default::default()
        });
        let mut group = or_io::gta5::GrmShaderGroup::default();
        group.texture_dictionary = Some(Box::new(dictionary));
        let mut drawable = or_io::gta5::GtaDrawable::default();
        drawable.base.shader_group = Some(Box::new(group));

        round_trip(
            &DocPayload::Resource(or_io::Asset::Drawable(drawable)),
            None,
            &["diffuse"],
        );
    }
}

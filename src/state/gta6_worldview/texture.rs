//! A texture as a worker read it: the level the screen asks for and every level under it, as the
//! block rows its pack stores where a device takes them as they are, else as 8-bit RGBA.

use std::collections::HashMap;
use std::path::PathBuf;

use or_io::codec::texture::{decode_level, DxgiFormat, TextureSource};
use or_io::gta6::{Texture, TexturePack};
use wgpu::TextureFormat;

/// A texture on the CPU: its levels, the largest first, each half the last.
pub struct CpuTexture {
    pub width: u32,
    pub height: u32,
    /// `Rgba8Unorm` or `Rgba8UnormSrgb` for decoded levels; a `Bc*` format for levels of whole block rows.
    pub format: TextureFormat,
    pub levels: Vec<Vec<u8>>,
}

impl CpuTexture {
    /// The bytes of its levels.
    pub fn bytes(&self) -> u64 {
        self.levels.iter().map(|level| level.len() as u64).sum()
    }
}

/// The texture packs of the install, and the pack each key is in.
pub(super) struct Places {
    pub(super) packs: Vec<PathBuf>,
    pub(super) keys: HashMap<u64, u32>,
    /// The format code each key's texture states.
    pub(super) formats: HashMap<u64, u16>,
}

impl Places {
    /// Whether the texture of `key` states an sRGB format, as a colour map does; `false` for a key no pack holds.
    pub(super) fn is_colour(&self, key: u64) -> bool {
        let srgb = |code: &u16| DxgiFormat::from_raw((code >> 3).into()).is_srgb();
        self.formats.get(&key).is_some_and(srgb)
    }
}

/// A pack a worker opened, with the place of each key in it; `None` for one that did not open.
pub(super) type OpenPack = Option<(TexturePack, HashMap<u64, usize>)>;

/// The format a device samples the blocks of `format` as they are stored; `None` for one it decodes.
fn block_format(format: DxgiFormat) -> Option<TextureFormat> {
    Some(match format {
        DxgiFormat::Bc1Unorm => TextureFormat::Bc1RgbaUnorm,
        DxgiFormat::Bc1UnormSrgb => TextureFormat::Bc1RgbaUnormSrgb,
        DxgiFormat::Bc2Unorm => TextureFormat::Bc2RgbaUnorm,
        DxgiFormat::Bc3Unorm => TextureFormat::Bc3RgbaUnorm,
        DxgiFormat::Bc3UnormSrgb => TextureFormat::Bc3RgbaUnormSrgb,
        DxgiFormat::Bc4Unorm => TextureFormat::Bc4RUnorm,
        DxgiFormat::Bc5Unorm => TextureFormat::Bc5RgUnorm,
        DxgiFormat::Bc7Unorm => TextureFormat::Bc7RgbaUnorm,
        DxgiFormat::Bc7UnormSrgb => TextureFormat::Bc7RgbaUnormSrgb,
        _ => return None,
    })
}

/// The mip of a `width` by `height` texture a screen size of `pixels` asks for: the smallest whose
/// longer side is at least that, the top where none is.
pub(super) fn level_for(width: u32, height: u32, pixels: u32) -> u32 {
    // Each mip halves the side: as many halvings as keep it at or over `pixels`.
    (width.max(height) / pixels.max(1))
        .checked_ilog2()
        .unwrap_or(0)
}

/// `texture`, of one face, as a device takes it: its block rows as they are where a device samples
/// its format and its top level is whole blocks, else every level decoded to RGBA.
pub(super) fn cpu_texture(texture: &Texture) -> Option<CpuTexture> {
    let (width, height) = (texture.width, texture.height);
    // ⚠ A device refuses a block-compressed texture whose first level is not whole blocks.
    let whole = width.is_multiple_of(4) && height.is_multiple_of(4);
    if let Some(format) = block_format(texture.format.dxgi()).filter(|_| whole) {
        let (stored, payload) = (texture.pixel_format(), texture.payload());
        let levels: Vec<Vec<u8>> = (0..texture.levels)
            .map_while(|level| payload.get(stored.mip_level_range(width, height, level)))
            .map(<[u8]>::to_vec)
            .collect();
        return (!levels.is_empty()).then_some(CpuTexture {
            width,
            height,
            format,
            levels,
        });
    }
    let top = decode_level(texture, 0).ok()?;
    let (width, height, srgb) = (top.width, top.height, top.is_srgb);
    let mut levels = vec![top.data];
    for under in 1..texture.levels {
        let Ok(image) = decode_level(texture, under) else {
            break;
        };
        let k = levels.len() as u32;
        if (image.width, image.height) != ((width >> k).max(1), (height >> k).max(1)) {
            break;
        }
        levels.push(image.data);
    }
    Some(CpuTexture {
        width,
        height,
        format: match srgb {
            true => TextureFormat::Rgba8UnormSrgb,
            false => TextureFormat::Rgba8Unorm,
        },
        levels,
    })
}

/// The texture of the key `key` at the level a screen size of `pixels` asks for and every level
/// under it, read from the rows of its pack those levels lie in; where the rows of that level do
/// not decode, from the largest level under it whose rows do. `None` where no pack holds it or
/// none of it reads.
pub(super) fn read_texture(
    open: &mut HashMap<u32, OpenPack>,
    places: &Places,
    key: u64,
    pixels: u32,
) -> Option<CpuTexture> {
    let at = *places.keys.get(&key)?;
    let pack = open.entry(at).or_insert_with(|| {
        let pack = TexturePack::open(places.packs.get(at as usize)?).ok()?;
        let entries = (pack.entries.iter().enumerate())
            .filter(|(_, e)| e.on_disk)
            .map(|(place, e)| (e.key, place))
            .collect();
        Some((pack, entries))
    });
    let (pack, entries) = pack.as_ref()?;
    let place = *entries.get(&key)?;
    let entry = pack.entries.get(place)?;
    let level = level_for(entry.width, entry.height, pixels);
    let (_, texture) = pack.texture_from(place, level).ok()?;
    // ponytail: a cube, an array or a volume is read whole for its first face or slice.
    let texture = match texture.faces {
        1 => texture,
        _ => texture.face(0)?,
    };
    cpu_texture(&texture)
}

#[cfg(test)]
#[path = "texture_tests.rs"]
mod tests;

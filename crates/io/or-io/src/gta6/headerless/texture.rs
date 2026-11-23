//! A texture dictionary's texture: a 0x70-byte object whose name lies behind an 8-byte zero prefix,
//! and whose pixels are the packs' 4 KiB-tiled chains, slice by slice, in the physical section.

use super::super::tiling::{chain, volume_chain, Layout, Tile};
use super::super::ttpf::{Format, Texture};
use super::bytes::Bytes;
use super::{HeaderlessError, Result};

/// The texture object's size.
const OBJECT: usize = 0x70;

#[derive(Debug)]
pub struct DictionaryTexture {
    pub name: String,
    pub width: u16,
    pub height: u16,
    /// 1 for a 2D texture, the slices of an array, six per cube, or a volume's depth.
    pub depth: u16,
    /// 1 for 2D (an array when `depth` is above 1), 2 for a cube array of `depth / 6` cubes, 3 for a volume.
    pub dimension: u8,
    /// The DXGI format number.
    pub format: u8,
    pub mip_levels: u8,
    /// Where its pixels start in the physical section, and their bytes.
    pub pixels_at: u64,
    pub pixels_len: u64,
    /// Its pixels untiled, or why they are not.
    pub texture: Result<Texture>,
}

/// Whether the record at `at` is a texture object: no runtime slot, its view embedded at +0x50,
/// a name pointer, a physical pointer to its pixels and a size of 1 to 16384 each way.
pub(super) fn is_texture(v: Bytes, at: usize) -> bool {
    let size = |o| {
        v.u16("texture size", at + o)
            .is_ok_and(|s| (1..=16384).contains(&s))
    };
    v.get("texture", at, OBJECT).is_ok()
        && v.u64("texture vft", at).is_ok_and(|w| w == 0)
        && v.ptr("texture view", at + 0x28).ok().flatten() == Some(at + 0x50)
        && v.ptr("texture name", at + 0x30).is_ok_and(|p| p.is_some())
        && v.phys("texture pixels", at + 0x18).is_ok()
        && size(0x38)
        && size(0x3A)
}

pub(super) fn texture_at(v: Bytes, p: Bytes, at: usize) -> Result<DictionaryTexture> {
    object_at(v, p, at, true)
}

/// The texture object at `at`, its name read where `named`; an embedded dictionary's record at
/// +0x30 holds a key there instead, and that texture reads with an empty name.
pub(super) fn object_at(v: Bytes, p: Bytes, at: usize, named: bool) -> Result<DictionaryTexture> {
    let name = v.to("texture name", at + 0x30)?;
    let checks = [
        (
            "texture name prefix",
            name,
            0,
            match named {
                true => v.u64("texture name prefix", name)?,
                false => 0,
            },
        ),
        (
            "texture reference count",
            at + 0x46,
            1,
            v.u16("texture reference count", at + 0x46)?.into(),
        ),
        (
            "texture word at +0x4C",
            at + 0x4C,
            2,
            v.u32("texture word at +0x4C", at + 0x4C)?.into(),
        ),
        // The packs' 4 KiB tiles: the only tile mode measured.
        (
            "texture tile mode",
            at + 0x40,
            5,
            v.u8("texture tile mode", at + 0x40)?.into(),
        ),
    ];
    if let Some(&(what, at, _, found)) = checks.iter().find(|(.., want, found)| want != found) {
        return Err(HeaderlessError::Constant {
            what,
            at: at as u64,
            found,
        });
    }
    let side = |o| v.u16("texture size", at + o);
    let (width, height, depth) = (side(0x38)?, side(0x3A)?, side(0x3C)?);
    let byte = |o| v.u8("texture field", at + o);
    let (dimension, format, mip_levels) = (byte(0x3E)?, byte(0x3F)?, byte(0x42)?);
    let pixels_at = v.phys("texture pixels", at + 0x18)?;
    let count = v.u32("texture block count", at + 0x08)?;
    let stride = v.u16("texture block stride", at + 0x0C)?;
    let of = if dimension == 3 {
        Format::of_volume_dxgi
    } else {
        Format::of_dxgi
    };
    let texture = of(format)
        .ok_or(HeaderlessError::TextureFormat(format))
        .and_then(|format| {
            // A cube is a cube array: its depth counts faces, six per cube, cube after cube.
            if dimension == 2 {
                let view = v.u16("texture view dimension", at + 0x60)?;
                if view != 0x292 {
                    return Err(HeaderlessError::Constant {
                        what: "cube texture's view dimension",
                        at: (at + 0x60) as u64,
                        found: view.into(),
                    });
                }
                if depth == 0 || !depth.is_multiple_of(6) {
                    return Err(HeaderlessError::Disagree {
                        what: "cube texture's slices and the six faces of each cube",
                        first: depth.into(),
                        second: 6,
                    });
                }
            }
            if usize::from(stride) != format.element_bytes() {
                return Err(HeaderlessError::Disagree {
                    what: "texture's block stride and its format's element size",
                    first: stride.into(),
                    second: format.element_bytes() as u64,
                });
            }
            let memory = p.array("texture pixels", pixels_at, count as usize, stride.into())?;
            pixels(
                format,
                [width, height, depth.max(1)],
                dimension,
                mip_levels,
                memory,
            )
        });
    Ok(DictionaryTexture {
        name: match named {
            true => v.c_str("texture name", name + 8)?,
            false => String::new(),
        },
        width,
        height,
        depth,
        dimension,
        format,
        mip_levels,
        pixels_at: pixels_at as u64,
        pixels_len: u64::from(count) * u64::from(stride),
        texture,
    })
}

/// The pixels of a texture of `size` (width, height, depth) whose tiled slices are `memory`.
fn pixels(
    format: Format,
    size: [u16; 3],
    dimension: u8,
    mip_levels: u8,
    memory: &[u8],
) -> Result<Texture> {
    let [width, height, depth] = size.map(u32::from);
    let len = memory.len() as u64;
    let not_chain = || HeaderlessError::Disagree {
        what: "texture's pixel bytes and the tiled chains of its slices",
        first: len,
        second: depth.into(),
    };
    let volume = dimension == 3;
    let (tile, levels, unit) = if volume {
        let tile = Tile::volume(format).ok_or_else(not_chain)?;
        let sides = [width, height, depth];
        let (levels, slab) =
            volume_chain(sides, format, &tile, mip_levels.into(), len).ok_or_else(not_chain)?;
        (tile, levels, slab)
    } else {
        let tile = Tile::of(format, false);
        let unit = len / u64::from(depth);
        let mut levels = (unit * u64::from(depth) == len)
            .then(|| chain(width, height, format, &tile, mip_levels <= 1, unit))
            .flatten()
            .ok_or_else(not_chain)?;
        levels.truncate(mip_levels.max(1).into());
        (tile, levels, unit)
    };
    let layout = Layout {
        format,
        width,
        height,
        tile,
        levels,
        slices: depth.into(),
        unit,
        piece_major: false,
        stated: true,
        depth: volume.then_some(depth),
    };
    let (levels, pixels) = layout
        .read(memory, 0)
        .ok_or(HeaderlessError::TopMipInTail {
            format: format.name(),
        })?;
    Ok(Texture {
        width,
        height,
        format,
        levels,
        faces: depth,
        pixels,
    })
}

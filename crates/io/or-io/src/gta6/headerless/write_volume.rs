//! A dictionary's volume texture of one level written from its slices, each slice one z-plane,
//! into the z-slabs of tiles it reads from; nothing moves.

use super::super::tiling::{place_planes, volume_chain, Tile};
use super::bytes::sections;
use super::dictionary::entry_of;
use super::texture::{is_texture, texture_at};
use super::{HeaderlessError, Result};
use crate::codec::texture::DxgiFormat;
use crate::gta6::hash::name_hash32;

fn refuse(why: &'static str) -> HeaderlessError {
    HeaderlessError::NotInPlace(why)
}

/// The dictionary volume `name` written from `pixels`, its z-planes one after another as the
/// reader gives them; nothing is written where `pixels` are the ones the texture holds.
// A volume of several levels holds part of each level in every z-slab, which is not written.
pub fn write_volume_texture(
    virtual_section: &[u8],
    physical: &mut [u8],
    name: &str,
    pixels: &[u8],
) -> Result<()> {
    let plan = sections(virtual_section, Some(physical), |v, p| {
        let at = entry_of(v, name_hash32(name.as_bytes()))?
            .ok_or(refuse("no entry of the dictionary has the texture's name"))?;
        if !is_texture(v, at) {
            return Err(refuse("the entry of the texture's name is no texture"));
        }
        let t = texture_at(v, p, at)?;
        let read = t.texture?;
        if read.pixels == pixels {
            return Ok(None);
        }
        if t.dimension != 3 {
            return Err(refuse("the texture is no volume"));
        }
        if t.mip_levels > 1 {
            return Err(refuse(
                "a volume of several levels holds part of each level in every z-slab, and whether its levels written into every slab read back as its payload is not confirmed, so only a volume of one level is written",
            ));
        }
        if read.format.dxgi() == DxgiFormat::Bc6hSf16 {
            return Err(refuse(
                "a signed BC6H texture reads as floats, not as the blocks it holds",
            ));
        }
        if pixels.len() != read.pixels.len() {
            return Err(refuse(
                "the pixels are not the size of the texture's planes",
            ));
        }
        let sides = [read.width, read.height, u32::from(t.depth.max(1))];
        Ok(Some((
            t.pixels_at as usize,
            t.pixels_len,
            read.format,
            sides,
        )))
    })?;
    let Some((at, len, format, sides)) = plan else {
        return Ok(());
    };
    let not_slabs = || refuse("the texture's memory is no z-slabs of its level");
    let tile = Tile::volume(format).ok_or_else(not_slabs)?;
    let (levels, slab) = volume_chain(sides, format, &tile, 1, len).ok_or_else(not_slabs)?;
    let level = levels.first().ok_or_else(not_slabs)?;
    let memory = (usize::try_from(len).ok())
        .and_then(|len| at.checked_add(len))
        .and_then(|end| physical.get_mut(at..end))
        .ok_or(refuse("the texture's memory passes the physical section"))?;
    let planes = 0..sides[2] as usize;
    let size = [level.width, level.height];
    if !place_planes(memory, level.at, size, planes, pixels, &tile, slab) {
        return Err(refuse(
            "the pixels are not the size of the texture's planes",
        ));
    }
    Ok(())
}

//! A dictionary's volume texture written from its slices into the z-slabs of tiles it reads from,
//! each level's planes into every slab, every slab holding part of each level; nothing moves.

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

/// The dictionary volume `name` written from `pixels`, its slices one after another as the reader
/// gives them, slice s holding plane `s >> m` of level m; nothing is written where `pixels` are
/// the ones the texture holds.
// Level m's plane p is taken from slice `p << m`, the first slice that shows it.
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
            t.mip_levels,
        )))
    })?;
    let Some((at, len, format, sides, mips)) = plan else {
        return Ok(());
    };
    let not_slabs = || refuse("the texture's memory is no z-slabs of its levels");
    let not_planes = || refuse("the pixels are not the size of the texture's planes");
    let tile = Tile::volume(format).ok_or_else(not_slabs)?;
    let chain = volume_chain(sides, format, &tile, mips.into(), len);
    let (levels, slab) = chain.ok_or_else(not_slabs)?;
    let memory = (usize::try_from(len).ok())
        .and_then(|len| at.checked_add(len))
        .and_then(|end| physical.get_mut(at..end))
        .ok_or(refuse("the texture's memory passes the physical section"))?;
    let (element, depth) = (format.element_bytes(), sides[2] as usize);
    // One slice's bytes: a plane of each level.
    let each: usize = levels.iter().map(|l| l.width * l.height * element).sum();
    if pixels.len() != each * depth {
        return Err(not_planes());
    }
    let mut offset = 0;
    for (m, level) in levels.iter().enumerate() {
        let plane = level.width * level.height * element;
        let planes = (depth >> m).max(1);
        let mut elements = Vec::with_capacity(plane * planes);
        for p in 0..planes {
            let from = (p << m).min(depth - 1) * each + offset;
            elements.extend_from_slice(pixels.get(from..from + plane).ok_or_else(not_planes)?);
        }
        let size = [level.width, level.height];
        if !place_planes(memory, level.at, size, 0..planes, &elements, &tile, slab) {
            return Err(not_planes());
        }
        offset += plane;
    }
    Ok(())
}

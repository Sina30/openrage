//! Edits written back into a resource's sections where they move nothing: a drawable's constants
//! and texture keys where the reader read them, and a dictionary texture's levels into its tiles.

use std::collections::BTreeMap;

use super::super::tiling::{chain, Tile};
use super::super::ttpf::place;
use super::bytes::sections;
use super::dictionary::entry_of;
use super::drawable::{is_drawable_root, shaders_at, Places};
use super::texture::{is_texture, object_at};
use super::{HeaderlessError, ParameterValue, Result, Shader};
use crate::codec::texture::DxgiFormat;
use crate::container::rpf::Rpf9Role;
use crate::gta6::hash::name_hash32;

fn refuse(why: &'static str) -> HeaderlessError {
    HeaderlessError::NotInPlace(why)
}

/// `bytes` over `section` at `at`.
fn put(section: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let len = section.len() as u64;
    let to = (at.checked_add(bytes.len()))
        .and_then(|end| section.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "edited value",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    to.copy_from_slice(bytes);
    Ok(())
}

/// The drawable of a resource of `role` (its own, a fragment's, or the dictionary entry of `code`)
/// written from `shaders`: each constant's words and each texture slot's key where the reader read
/// them.
pub fn write_shaders(
    role: Rpf9Role,
    virtual_section: &mut [u8],
    code: Option<u32>,
    shaders: &[Shader],
) -> Result<()> {
    let (words, keys) = sections(virtual_section, None, |v, _| {
        let root = match (role, code) {
            (Rpf9Role::Drawable, None) => 0,
            (Rpf9Role::DrawableWithBound, None) => v.to("drawable", 0x18)?,
            (Rpf9Role::Dictionary, Some(code)) => {
                entry_of(v, code)?.ok_or(refuse("no entry of the dictionary has that code"))?
            }
            _ => return Err(refuse("the resource holds no drawable there")),
        };
        if !is_drawable_root(v, root, matches!(role, Rpf9Role::DrawableWithBound)) {
            return Err(HeaderlessError::NotADrawable { at: root as u64 });
        }
        let read = shaders_at(v, root)?;
        if read.len() != shaders.len() {
            return Err(refuse("the drawable holds another number of shaders"));
        }
        let (mut words, mut keys) = (Vec::new(), BTreeMap::new());
        for ((read, places), edited) in read.iter().zip(shaders) {
            placed(read, places, edited, &mut words, &mut keys)?;
        }
        Ok((words, keys))
    })?;
    for (at, w) in words {
        let bytes: Vec<u8> = w.iter().flat_map(|w| w.to_le_bytes()).collect();
        put(virtual_section, at, &bytes)?;
    }
    for (at, key) in keys {
        put(virtual_section, at, &key.to_le_bytes())?;
    }
    Ok(())
}

/// The words and keys `edited` writes over `read`, whose values lie at `places`; refused where
/// `edited` is laid out otherwise.
fn placed<'a>(
    read: &Shader,
    places: &Places,
    edited: &'a Shader,
    words: &mut Vec<(usize, &'a [u32])>,
    keys: &mut BTreeMap<usize, u64>,
) -> Result<()> {
    let same = |a: &ParameterValue, b: &ParameterValue| match (a, b) {
        (ParameterValue::Texture { slot: x }, ParameterValue::Texture { slot: y }) => x == y,
        (
            ParameterValue::Constant {
                buffer,
                offset,
                words,
            },
            ParameterValue::Constant {
                buffer: b,
                offset: o,
                words: w,
            },
        ) => buffer == b && offset == o && words.len() == w.len(),
        _ => false,
    };
    let parameters = read.parameters.iter().zip(&edited.parameters);
    if read.name_hash != edited.name_hash
        || read.parameters.len() != edited.parameters.len()
        || read.texture_keys.len() != edited.texture_keys.len()
        || parameters
            .clone()
            .any(|(r, e)| r.hash != e.hash || !same(&r.value, &e.value))
    {
        return Err(refuse(
            "it changes a shader's effect, its parameters or the size of a constant",
        ));
    }
    for (p, at) in edited.parameters.iter().zip(&places.constants) {
        if let (ParameterValue::Constant { words: w, .. }, Some(at)) = (&p.value, at) {
            words.push((*at, w));
        }
    }
    for (key, at) in edited.texture_keys.iter().zip(&places.keys) {
        match (key, at) {
            (Some(key), Some(at)) => {
                if *keys.entry(*at).or_insert(*key) != *key {
                    return Err(refuse(
                        "two texture slots share one texture entry and the edit gives them different keys",
                    ));
                }
            }
            (None, None) => {}
            (Some(_), None) => {
                return Err(refuse(
                    "an empty texture slot has no texture entry to hold a key",
                ))
            }
            (None, Some(_)) => {
                return Err(refuse(
                    "it empties a texture slot, whose binding a write in place cannot remove",
                ))
            }
        }
    }
    Ok(())
}

/// The dictionary texture `name` written from `pixels`, slice after slice, into its tiles.
// Each slice's levels come top first, as the reader gives them; nothing is written where `pixels`
// are the ones the texture holds.
pub fn write_texture(
    virtual_section: &[u8],
    physical: &mut [u8],
    name: &str,
    pixels: &[u8],
) -> Result<()> {
    let at = sections(virtual_section, None, |v, _| {
        let at = entry_of(v, name_hash32(name.as_bytes()))?
            .ok_or(refuse("no entry of the dictionary has the texture's name"))?;
        if !is_texture(v, at) {
            return Err(refuse("the entry of the texture's name is no texture"));
        }
        Ok(at)
    })?;
    write_object(virtual_section, physical, at, true, pixels)
}

/// The texture object at `at` written from `pixels`, as [`write_texture`] writes a dictionary's;
/// its name read where `named`, as an embedded dictionary's record holds a key there instead.
pub(super) fn write_object(
    virtual_section: &[u8],
    physical: &mut [u8],
    at: usize,
    named: bool,
    pixels: &[u8],
) -> Result<()> {
    let plan = sections(virtual_section, Some(physical), |v, p| {
        let t = object_at(v, p, at, named)?;
        let read = t.texture?;
        if read.pixels == pixels {
            return Ok(None);
        }
        // A volume's z-slabs each hold part of every level, so its slices are no chains of their own.
        if t.dimension == 3 {
            return Err(refuse(
                "only a 2D texture's, an array's or a cube's pixels are written",
            ));
        }
        if read.format.dxgi() == DxgiFormat::Bc6hSf16 {
            return Err(refuse(
                "a signed BC6H texture reads as floats, not as the blocks it holds",
            ));
        }
        if read.levels != u32::from(t.mip_levels.max(1)) {
            return Err(refuse("a level lies in a tail slot that was not measured"));
        }
        if pixels.len() != read.pixels.len() {
            return Err(refuse(
                "the pixels are not the size of the texture's levels",
            ));
        }
        let (at, len) = (t.pixels_at as usize, t.pixels_len as usize);
        Ok(Some((
            at,
            len,
            read.format,
            [read.width, read.height],
            t.mip_levels,
            usize::from(t.depth.max(1)),
        )))
    })?;
    let Some((at, len, format, [width, height], mips, slices)) = plan else {
        return Ok(());
    };
    // Each slice's chain is `unit` bytes after the one before it, as the reader reads them.
    let unit = len / slices;
    if unit == 0 || unit * slices != len {
        return Err(refuse("the texture's memory is no chain of its levels"));
    }
    let tile = Tile::of(format, false);
    let mut levels = chain(width, height, format, &tile, mips <= 1, unit as u64)
        .ok_or(refuse("the texture's memory is no chain of its levels"))?;
    levels.truncate(mips.max(1).into());
    let memory = (at.checked_add(unit * slices))
        .and_then(|end| physical.get_mut(at..end))
        .ok_or(refuse("the texture's memory passes the physical section"))?;
    let element = format.element_bytes();
    let each = pixels.len() / slices;
    for (slice, memory) in memory.chunks_exact_mut(unit).enumerate() {
        let mut from = slice * each;
        for level in &levels {
            let end = from + level.width * level.height * element;
            let elements = (pixels.get(from..end)).ok_or(refuse(
                "the pixels are not the size of the texture's levels",
            ))?;
            if !place(memory, level, elements, &tile, element) {
                return Err(refuse("a level's tiles pass the texture's memory"));
            }
            from = end;
        }
        if from != (slice + 1) * each {
            return Err(refuse(
                "the pixels are not the size of the texture's levels",
            ));
        }
    }
    Ok(())
}

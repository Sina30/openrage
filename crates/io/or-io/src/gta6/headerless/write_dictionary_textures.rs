//! A dictionary's textures as an editor holds them, written over the dictionary's sections: in
//! place where every texture keeps its shape and none is added or removed, else both sections
//! planned anew from the textures.

use std::collections::{HashMap, HashSet};

use super::write_volume::write_volume_texture;
use super::{
    read_dictionary, read_dictionary_model, write_dictionary, write_texture, DictionaryWriteError,
    Entry, HeaderlessError, ModelTexture,
};
use crate::codec::texture::GrcTextureFormat;
use crate::container::rpf::Rpf9Sections;
use crate::gta6::Format;

/// A dictionary texture as a save writes it.
#[derive(Debug, Clone)]
pub struct HeldTexture {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub format: GrcTextureFormat,
    pub srgb: bool,
    pub mips: u8,
    /// Its levels top first, slice after slice; empty for one that does not read.
    pub pixels: Vec<u8>,
}

/// Why a dictionary's textures are not written.
#[derive(Debug, thiserror::Error)]
pub enum TexturePlanError {
    #[error("{format:?} is no format a GTA VI dictionary holds a texture in")]
    Format { format: GrcTextureFormat },
    #[error("{name} does not read, so its pixels are not replaced")]
    Unread { name: String },
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Write(#[from] DictionaryWriteError),
    #[error("{name}: {source}")]
    Texture {
        name: String,
        source: HeaderlessError,
    },
}

type Planned<T> = std::result::Result<T, TexturePlanError>;

impl HeldTexture {
    /// Its levels tiled as a dictionary's texture, in place of `was` where it holds one of its name.
    pub fn tiled(&self, was: Option<&ModelTexture>) -> Planned<ModelTexture> {
        let format = Format::of_grc(self.format, self.srgb)
            .and_then(|f| u8::try_from(f.dxgi().to_raw()).ok())
            .ok_or(TexturePlanError::Format {
                format: self.format,
            })?;
        let unread = || TexturePlanError::Unread {
            name: self.name.clone(),
        };
        let (width, height) = (u32::from(self.width), u32::from(self.height));
        let (mut levels, mut from) = (Vec::new(), 0usize);
        for level in 0..u32::from(self.mips.max(1)) {
            let side = |pixels: u32| pixels.checked_shr(level).unwrap_or(0).max(1);
            let len = self.format.mip_size(side(width), side(height)) as usize;
            let end = from.checked_add(len).ok_or_else(unread)?;
            levels.push(self.pixels.get(from..end).ok_or_else(unread)?.to_vec());
            from = end;
        }
        let (width, height) = (self.width, self.height);
        let tiled = match was {
            Some(was) => was.reshaped(width, height, format, &levels),
            None => {
                let name = self.name.to_ascii_lowercase();
                ModelTexture::tiled(&name, width, height, format, &levels)
            }
        };
        Ok(tiled?)
    }
}

/// `held` written over the dictionary `sections` hold, in place or else both sections planned anew.
// In place only where every texture keeps its shape and none is added or removed.
pub fn write_dictionary_textures(sections: &mut Rpf9Sections, held: &[HeldTexture]) -> Planned<()> {
    let (v, p) = (&sections.virtual_section, &sections.physical);
    let read = read_dictionary(v, Some(p))?;
    let mut shapes = HashMap::new();
    let mut volumes = HashSet::new();
    for (_, entry) in &read.entries {
        if let Ok(Entry::Texture(t)) = entry {
            let shape = (t.texture.as_ref().ok()).map(|x| {
                let format = (x.format.format(), x.format.is_srgb());
                (x.width, x.height, format, x.levels)
            });
            let name = t.name.to_ascii_lowercase();
            if t.dimension == 3 {
                volumes.insert(name.clone());
            }
            shapes.insert(name, shape);
        }
    }
    let mut changed = Vec::new();
    for h in held.iter().filter(|h| !h.pixels.is_empty()) {
        let (width, height) = (u32::from(h.width), u32::from(h.height));
        let shape = (width, height, (h.format, h.srgb), u32::from(h.mips));
        let name = h.name.to_ascii_lowercase();
        if shapes.get(&name) == Some(&Some(shape)) {
            let (v, p) = (&sections.virtual_section, &mut sections.physical);
            let written = match volumes.contains(&name) {
                true => write_volume_texture(v, p, &h.name, &h.pixels),
                false => write_texture(v, p, &h.name, &h.pixels),
            };
            written.map_err(|source| TexturePlanError::Texture {
                name: h.name.clone(),
                source,
            })?;
        } else {
            changed.push(h);
        }
    }
    let kept: HashSet<String> = held.iter().map(|h| h.name.to_ascii_lowercase()).collect();
    let removed: Vec<&String> = shapes.keys().filter(|n| !kept.contains(*n)).collect();
    if changed.is_empty() && removed.is_empty() {
        return Ok(());
    }
    let (v, p) = (&sections.virtual_section, &sections.physical);
    let mut model = read_dictionary_model(v, p)?;
    for name in removed {
        model.remove(name);
    }
    for h in changed {
        let was = (model.textures.iter()).find(|t| t.name.eq_ignore_ascii_case(&h.name));
        let texture = h.tiled(was)?;
        model.insert(texture);
    }
    *sections = write_dictionary(&model)?;
    Ok(())
}

#[cfg(test)]
#[path = "write_dictionary_textures_tests.rs"]
mod tests;

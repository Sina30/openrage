//! What a drawable's physical section holds beside its geometries: the texture its shader group's
//! dictionary embeds, the tint palettes its group's table reaches, and the kind of the object the
//! section starts with, which is the largest of the first block and not always a vertex buffer.

use std::collections::HashSet;

use super::bytes::{sections, u32_at, Bytes};
use super::streams::buffer;
use super::texture::{object_at, DictionaryTexture};
use super::{HeaderlessError, Result};

/// Stride, kind and flags of every measured tint palette's object.
const CPU_TEXTURE: (u16, u16, u32) = (4, 8, 0x982);

/// Each kind of buffer object by its flags at +0x10 and the sub-kind at +0x0E it is measured with.
const KINDS: [(u32, u16, BufferKind); 5] = [
    (0x1601, 9, BufferKind::Vertex),
    (0x1380, 8, BufferKind::Raw),
    (0x980, 8, BufferKind::Texture),
    (0x982, 8, BufferKind::CpuTexture),
    (0x1600, 4, BufferKind::Structured),
];

/// The kind of a buffer object whose data lies in the physical section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKind {
    Vertex,
    Raw,
    Texture,
    /// A texture the CPU reads, as a tint palette is.
    CpuTexture,
    Structured,
}

#[derive(Debug)]
pub struct DrawableHead {
    /// The drawable root's name, as the drawable reader reads it.
    pub name: String,
    /// The kind of the buffer object whose data starts the physical section; `None` where none does.
    pub lead: Option<BufferKind>,
    /// Each texture of the dictionary the shader group points at from +0x08.
    pub textures: Vec<Result<EmbeddedTexture>>,
    /// Each tint palette row 0 of the table the shader group points at from +0x30 reaches.
    pub palettes: Vec<Result<TintPalette>>,
    /// The vertex buffer objects of the body that no geometry of the drawable's models points at.
    pub unlisted_vertex_buffers: usize,
}

#[derive(Debug)]
pub struct EmbeddedTexture {
    /// The key a shader's texture slot binds it by: the word at +0x08 of its record at +0x30.
    pub key: u64,
    /// Its entry of the dictionary's hash array, whose source text is not known.
    pub hash: u32,
    /// Named `hash_` and its key, the record holding no name.
    pub texture: DictionaryTexture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TintPalette {
    /// Where its words start in the physical section.
    pub at: u64,
    /// Its leading run of RGBA8 colours in byte order, each alpha 0xFF, less the zeros after it.
    pub colours: Vec<[u8; 4]>,
}

/// The head of the drawable at the root of `virtual_section`, or of its fragment's where `fragment`.
pub fn read_drawable_head(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
    fragment: bool,
) -> Result<DrawableHead> {
    sections(virtual_section, physical, |v, p| {
        let root = match fragment {
            true => v.to("drawable", 0x18)?,
            false => 0,
        };
        let group = v.to("shader group", root + 0x10)?;
        let name = v.c_str("name", v.to("name", root + 0xA8)?)?;
        let objects = buffer_objects(v);
        let listed = listed_vertex_buffers(v, root)?;
        Ok(DrawableHead {
            name,
            lead: (objects.iter())
                .find(|(_, _, data)| *data == 0)
                .map(|(_, kind, _)| *kind),
            textures: embedded_textures(v, p, group)?,
            palettes: palettes(v, p, group)?,
            unlisted_vertex_buffers: (objects.iter())
                .filter(|(at, kind, _)| *kind == BufferKind::Vertex && !listed.contains(at))
                .count(),
        })
    })
}

/// Each buffer object of the virtual section, 8-aligned: where it lies, its kind, and where its
/// data starts in the physical section.
pub(super) fn buffer_objects(v: Bytes) -> Vec<(usize, BufferKind, usize)> {
    (0..v.len().saturating_sub(0x4F))
        .step_by(8)
        .filter_map(|at| {
            let data = v.phys("buffer's data", at + 0x18).ok()?;
            let flags = v.u32("buffer's flags", at + 0x10).ok()?;
            let sub_kind = v.u16("buffer's kind", at + 0x0E).ok()?;
            (KINDS.iter())
                .find(|(f, s, _)| (*f, *s) == (flags, sub_kind))
                .map(|(.., kind)| (at, *kind, data))
        })
        .collect()
}

/// The vertex buffer objects the geometries of the models of the drawable at `root` point at from
/// +0x08.
fn listed_vertex_buffers(v: Bytes, root: usize) -> Result<HashSet<usize>> {
    let mut listed = HashSet::new();
    for l in 0..4 {
        let Some(level) = v.ptr("level", root + 0x50 + 8 * l)? else {
            continue;
        };
        let count = usize::from(v.u16("model count", level + 8)?);
        if count == 0 {
            continue;
        }
        let models = v.to("models", level)?;
        v.array("models", models, count, 8)?;
        for i in 0..count {
            let model = v.to("model", models + 8 * i)?;
            let count = usize::from(v.u16("geometry count", model + 0x10)?);
            if count == 0 {
                continue;
            }
            let geometries = v.to("geometries", model + 0x08)?;
            v.array("geometries", geometries, count, 8)?;
            for j in 0..count {
                let geometry = v.to("geometry", geometries + 8 * j)?;
                listed.extend(v.ptr("vertex buffer", geometry + 0x08).ok().flatten());
            }
        }
    }
    Ok(listed)
}

/// The textures of GTA V's dictionary layout the shader group at `group` points at from +0x08:
/// hashes at +0x20 and textures at +0x30, each a pointer and a u16 count; none where it is null.
fn embedded_textures(v: Bytes, p: Bytes, group: usize) -> Result<Vec<Result<EmbeddedTexture>>> {
    let Some(dictionary) = v.ptr("embedded texture dictionary", group + 0x08)? else {
        return Ok(Vec::new());
    };
    let hashes = v.u16("embedded dictionary's hash count", dictionary + 0x28)?;
    let textures = v.u16("embedded dictionary's texture count", dictionary + 0x38)?;
    if hashes != textures {
        return Err(HeaderlessError::Disagree {
            what: "embedded dictionary's hash and texture counts",
            first: hashes.into(),
            second: textures.into(),
        });
    }
    let count = usize::from(textures);
    if count == 0 {
        return Ok(Vec::new());
    }
    let hashes = v.to("embedded dictionary's hashes", dictionary + 0x20)?;
    let hashes = v.array("embedded dictionary's hashes", hashes, count, 4)?;
    let list = v.to("embedded textures", dictionary + 0x30)?;
    v.array("embedded textures", list, count, 8)?;
    Ok((0..count)
        .map(|k| {
            let at = v.to("embedded texture", list + 8 * k)?;
            let record = v.to("embedded texture's key record", at + 0x30)?;
            let key = v.u64("embedded texture's key", record + 0x08)?;
            let mut texture = object_at(v, p, at, false)?;
            texture.name = format!("hash_{key:016X}");
            Ok(EmbeddedTexture {
                key,
                hash: u32_at(hashes, 4 * k),
                texture,
            })
        })
        .collect())
}

/// The tint palettes row 0 of the table the shader group at `group` points at from +0x30 reaches:
/// the record at +0x00, and the one at +0x08 where that word addresses a second.
fn palettes(v: Bytes, p: Bytes, group: usize) -> Result<Vec<Result<TintPalette>>> {
    let Some(table) = v.ptr("tint palette table", group + 0x30)? else {
        return Ok(Vec::new());
    };
    let first = v.ptr("tint palette", table)?;
    // The row's word at +0x08 is another word than a palette in most bodies.
    let second = (v.ptr("tint palette", table + 0x08).ok().flatten()).filter(|&at| {
        v.u32("buffer's flags", at + 0x10)
            .is_ok_and(|f| f == CPU_TEXTURE.2)
    });
    Ok([first, second]
        .into_iter()
        .flatten()
        .map(|at| palette(v, p, at))
        .collect())
}

/// The 0x982 object at `at`: a leading run of RGBA8 words, each alpha 0xFF, then zero words.
fn palette(v: Bytes, p: Bytes, at: usize) -> Result<TintPalette> {
    let data = buffer(v, p, "tint palette", at, CPU_TEXTURE)?;
    let start = v.phys("tint palette", at + 0x18)?;
    let words = data.as_chunks::<4>().0;
    let run = words.iter().position(|w| *w == [0; 4]);
    let run = run.unwrap_or(words.len());
    let translucent = words[..run].iter().position(|w| w[3] != 0xFF);
    let after = words[run..].iter().position(|w| *w != [0; 4]);
    if let Some(k) = translucent.or(after.map(|k| run + k)) {
        return Err(HeaderlessError::Constant {
            what: "tint palette word",
            at: (start + 4 * k) as u64,
            found: u32_at(data, 4 * k).into(),
        });
    }
    Ok(TintPalette {
        at: start as u64,
        colours: words[..run].to_vec(),
    })
}

//! A texture dictionary written anew from its textures, both sections laid out by the archiver's
//! block rule, so that a texture may change size, format or mips and one may be added or removed.
//!
//! ⚠ Nothing in a dictionary states a section's size, so any texture's new size can move every
//! payload and every virtual allocation; the textures' creation order breaks ties and is carried.

use std::cmp::Reverse;

use super::super::tiling::{chain, Tile};
use super::super::ttpf::{place, Format};
use super::bytes::{sections, Bytes};
use super::texture::is_texture;
use super::{HeaderlessError, Result};
use crate::codec::texture::DxgiFormat;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};
use crate::gta6::hash::name_hash32;

/// A section's head is whole 64 KiB pieces, and a tail of the rest follows it.
const PIECE: usize = 0x1_0000;
/// The longest tail a block map states as a block apart from the head's stream.
const TAIL_MAX: usize = PIECE - 1;
/// A payload starts on a 4 KiB tile.
const PAGE: usize = 0x1000;
/// The bytes of a section a pointer reaches: its low 28 bits.
const SECTION_LIMIT: usize = 1 << 28;
/// The root's bytes, at the start of the virtual section.
const ROOT: usize = 0x40;
const RECORD: usize = 0x70;
const VIRTUAL: u64 = 0x5000_0000;
const PHYSICAL: u64 = 0x6000_0000;
/// The flags word of a texture; CPU-read data maps also set bit 1.
const FLAGS: u32 = 0x980;
const CPU_FLAGS: u32 = 0x982;
/// How many virtual plans may follow one another before the block map's size must settle.
const PLANS: usize = 4;

/// Each DXGI format a dictionary holds: its element's bytes and the pixels along its side.
const HELD: [(u8, usize, u32); 22] = [
    (10, 8, 1),
    (24, 4, 1),
    (28, 4, 1),
    (29, 4, 1),
    (34, 4, 1),
    (41, 4, 1),
    (49, 2, 1),
    (54, 2, 1),
    (56, 2, 1),
    (61, 1, 1),
    (65, 1, 1),
    (71, 8, 4),
    (72, 8, 4),
    (74, 16, 4),
    (77, 16, 4),
    (78, 16, 4),
    (80, 8, 4),
    (83, 16, 4),
    (87, 4, 1),
    (95, 16, 4),
    (98, 16, 4),
    (99, 16, 4),
];

/// A texture dictionary as its writer plans it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryModel {
    /// The textures in the order their records were created, which breaks ties of size.
    pub textures: Vec<ModelTexture>,
    /// The key and entry arrays' capacity, which follows the count once that changes.
    pub capacity: u16,
}

/// One texture of a [`DictionaryModel`]: its record's values and its tiled payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTexture {
    pub name: String,
    pub width: u16,
    pub height: u16,
    /// 1 for a 2D texture, the slices of an array, six per cube, or a volume's depth.
    pub depth: u16,
    /// 1 for 2D or an array, 2 for cubes, 3 for a volume.
    pub dimension: u8,
    /// The DXGI format number.
    pub format: u8,
    pub mip_levels: u8,
    /// The word at +0x10, whose bit 1 marks a CPU-read data map.
    pub flags: u32,
    /// The byte at +0x43, whose role is not known.
    pub byte_43: u8,
    /// The byte at +0x45, whose role is not known.
    pub byte_45: u8,
    /// Its 4 KiB-tiled chains, slice after slice.
    pub payload: Vec<u8>,
}

/// Why a dictionary is not written.
#[derive(Debug, thiserror::Error)]
pub enum DictionaryWriteError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error("a texture dictionary holds at least one texture; an empty one is laid out otherwise")]
    Empty,
    #[error("{count} textures pass the arrays' capacity of {capacity}")]
    Count { count: usize, capacity: u16 },
    #[error("the texture name {name:?} is empty, not ASCII, not lower case or holds a NUL, as no stored name is")]
    Name { name: String },
    #[error("{first:?} and {second:?} hash to one key, which a dictionary's ascending keys cannot hold twice")]
    SameKey { first: String, second: String },
    #[error("{name}: DXGI format {format} is none a texture dictionary holds")]
    Format { name: String, format: u8 },
    #[error(
        "{name}: {width}x{height}x{depth} of dimension {dimension} is no shape a dictionary holds"
    )]
    Shape {
        name: String,
        width: u16,
        height: u16,
        depth: u16,
        dimension: u8,
    },
    #[error(
        "{name}: flags {flags:#X} are neither of the two words dictionaries hold, 0x980 and 0x982"
    )]
    Flags { name: String, flags: u32 },
    #[error("{name}: its {len} payload bytes are not the {chain} bytes of its tiled chain")]
    Payload { name: String, len: u64, chain: u64 },
    #[error("{name}: {why}")]
    Pixels { name: String, why: &'static str },
    #[error(
        "the {section} section of {len} bytes passes the {SECTION_LIMIT} bytes a pointer reaches"
    )]
    TooLong { section: &'static str, len: u64 },
    #[error("the virtual section's tail comes to exactly 64 KiB after a head, which a block map cannot state apart from the head, so the plan is not written")]
    WholePieceTail,
    #[error(
        "the block map's size does not settle: each virtual plan needs another number of blocks"
    )]
    MapNotSettled,
}

type Written<T> = std::result::Result<T, DictionaryWriteError>;

impl DictionaryModel {
    /// `texture` in place of the one of its name, or added last; the capacity follows a count that changes.
    pub fn insert(&mut self, texture: ModelTexture) {
        let held = (self.textures.iter_mut()).find(|t| t.name.eq_ignore_ascii_case(&texture.name));
        match held {
            Some(held) => *held = texture,
            None => {
                self.textures.push(texture);
                self.capacity = self.count();
            }
        }
    }

    /// The texture named `name` taken out; the capacity follows the count.
    pub fn remove(&mut self, name: &str) -> Option<ModelTexture> {
        let at = (self.textures.iter()).position(|t| t.name.eq_ignore_ascii_case(name))?;
        let texture = self.textures.remove(at);
        self.capacity = self.count();
        Some(texture)
    }

    /// The count as the arrays state it; a count past 16 bits is refused when written.
    fn count(&self) -> u16 {
        u16::try_from(self.textures.len()).unwrap_or(u16::MAX)
    }
}

impl ModelTexture {
    /// A 2D texture of one slice, its `levels` top first as linear elements of DXGI `format`, tiled into its chain.
    pub fn tiled(
        name: &str,
        width: u16,
        height: u16,
        format: u8,
        levels: &[Vec<u8>],
    ) -> Written<Self> {
        Self::tiled_slices(name, width, height, 1, format, &[levels])
    }

    /// A 2D texture, an array (`dimension` 1) or cubes (`dimension` 2) of `slices`, each its levels top first as linear elements of DXGI `format`, each tiled into its chain and the chains laid slice after slice.
    pub fn tiled_slices(
        name: &str,
        width: u16,
        height: u16,
        dimension: u8,
        format: u8,
        slices: &[&[Vec<u8>]],
    ) -> Written<Self> {
        let refuse = |why| DictionaryWriteError::Pixels {
            name: name.to_string(),
            why,
        };
        let levels = slices.first().copied().unwrap_or_default();
        let mip_levels = (u8::try_from(levels.len()).ok())
            .filter(|&m| m > 0)
            .ok_or_else(|| refuse("it holds no level, or more than a record states"))?;
        if slices.iter().any(|slice| slice.len() != levels.len()) {
            return Err(refuse("its slices hold different numbers of levels"));
        }
        if dimension == 3 {
            return Err(refuse(
                "a volume's levels lie across its z-slabs, which are not tiled slice by slice",
            ));
        }
        let depth = (u16::try_from(slices.len()).ok())
            .ok_or_else(|| refuse("it holds more slices than a record states"))?;
        let mut texture = Self {
            name: name.to_string(),
            width,
            height,
            depth,
            dimension,
            format,
            mip_levels,
            flags: FLAGS,
            byte_43: 0,
            byte_45: 0,
            payload: Vec::new(),
        };
        let (element, side) = texture.element()?;
        texture.shape_holds()?;
        let tiled = (Format::of_dxgi(format))
            .filter(|f| f.dxgi() != DxgiFormat::Bc6hSf16)
            .ok_or_else(|| refuse("OpenRage tiles no new pixels of its format"))?;
        let len = texture.chain_len(element, side);
        if len > SECTION_LIMIT as u64 {
            return Err(DictionaryWriteError::TooLong {
                section: "physical",
                len,
            });
        }
        let tile = Tile::of(tiled, false);
        let lone = levels.len() <= 1;
        // Each slice is one chain, `unit` bytes after the one before it.
        let unit = len / u64::from(depth.max(1));
        let mut placed = chain(width.into(), height.into(), tiled, &tile, lone, unit)
            .ok_or_else(|| refuse("its levels are no chain of 4 KiB tiles"))?;
        placed.truncate(levels.len());
        if placed.len() != levels.len() {
            return Err(refuse("a level lies in a tail slot that was not measured"));
        }
        if element == 2 && placed.iter().any(|l| l.tail) {
            return Err(refuse("a 2-byte texture's tail slots were not measured"));
        }
        let mut payload = vec![0u8; len as usize];
        let chains = payload.chunks_exact_mut((unit as usize).max(1));
        for (memory, each) in chains.zip(slices) {
            for (level, elements) in placed.iter().zip(each.iter()) {
                if elements.len() != level.width * level.height * element
                    || !place(memory, level, elements, &tile, element)
                {
                    return Err(refuse("a level is not the size of its place in the chain"));
                }
            }
        }
        texture.payload = payload;
        texture.checked()?;
        Ok(texture)
    }

    /// The texture given another size, format or levels, tiled anew; its flags and unknown bytes carried.
    pub fn reshaped(
        &self,
        width: u16,
        height: u16,
        format: u8,
        levels: &[Vec<u8>],
    ) -> Written<Self> {
        if self.dimension != 1 || self.depth != 1 {
            return Err(DictionaryWriteError::Pixels {
                name: self.name.clone(),
                why: "only a 2D texture of one slice is tiled anew; an array's, a cube's or a volume's payload is carried",
            });
        }
        Ok(Self {
            flags: self.flags,
            byte_43: self.byte_43,
            byte_45: self.byte_45,
            ..Self::tiled(&self.name, width, height, format, levels)?
        })
    }

    /// The array or cubes given another size, format or levels, each of its `slices` tiled anew; its flags and unknown bytes carried.
    pub fn reshaped_slices(
        &self,
        width: u16,
        height: u16,
        format: u8,
        slices: &[&[Vec<u8>]],
    ) -> Written<Self> {
        let refuse = |why| DictionaryWriteError::Pixels {
            name: self.name.clone(),
            why,
        };
        if self.dimension == 3 {
            return Err(refuse(
                "a volume's levels lie across its z-slabs, so its payload is carried",
            ));
        }
        if slices.len() != usize::from(self.depth) {
            return Err(refuse("a new shape keeps the texture's count of slices"));
        }
        Ok(Self {
            flags: self.flags,
            byte_43: self.byte_43,
            byte_45: self.byte_45,
            ..Self::tiled_slices(&self.name, width, height, self.dimension, format, slices)?
        })
    }

    /// Its element's bytes and the pixels along its side, where its format is one dictionaries hold.
    fn element(&self) -> Written<(usize, u32)> {
        HELD.iter()
            .find(|held| held.0 == self.format)
            .map(|&(_, element, side)| (element, side))
            .ok_or_else(|| DictionaryWriteError::Format {
                name: self.name.clone(),
                format: self.format,
            })
    }

    fn shape_holds(&self) -> Written<()> {
        let sides = 1..=16384;
        let cube = self.dimension == 2 && !self.depth.is_multiple_of(6);
        if (1..=3).contains(&self.dimension)
            && sides.contains(&self.width)
            && sides.contains(&self.height)
            && self.depth > 0
            && !cube
        {
            return Ok(());
        }
        Err(DictionaryWriteError::Shape {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            depth: self.depth,
            dimension: self.dimension,
        })
    }

    /// Its element's bytes, once its name, format, shape, flags and payload are ones a dictionary holds.
    fn checked(&self) -> Written<usize> {
        let name = || self.name.clone();
        let stored = |b: u8| b != 0 && b.is_ascii() && !b.is_ascii_uppercase();
        if self.name.is_empty() || !self.name.bytes().all(stored) {
            return Err(DictionaryWriteError::Name { name: name() });
        }
        let (element, side) = self.element()?;
        self.shape_holds()?;
        if ![FLAGS, CPU_FLAGS].contains(&self.flags) {
            return Err(DictionaryWriteError::Flags {
                name: name(),
                flags: self.flags,
            });
        }
        let chain = self.chain_len(element, side);
        if self.payload.len() as u64 != chain {
            return Err(DictionaryWriteError::Payload {
                name: name(),
                len: self.payload.len() as u64,
                chain,
            });
        }
        Ok(element)
    }

    /// The bytes of its 4 KiB-tiled chains, of `element`-byte elements `side` pixels across.
    fn chain_len(&self, element: usize, side: u32) -> u64 {
        let along = |level: u32, pixels: u16| {
            let pixels = u32::from(pixels).checked_shr(level).unwrap_or(0).max(1);
            u64::from(pixels.div_ceil(side))
        };
        let (page, element) = (PAGE as u64, element as u64);
        let mips = u32::from(self.mip_levels.max(1));
        let depth = u64::from(self.depth);
        let mut tail = false;
        if self.dimension == 3 {
            let (tx, ty, tz) = match element {
                1 => (16, 16, 16),
                2 => (8, 16, 16),
                4 => (8, 16, 8),
                8 => (8, 8, 8),
                _ => (4, 8, 8),
            };
            let mut slab = 0;
            for m in 0..mips {
                let (w, h) = (along(m, self.width), along(m, self.height));
                let d = depth.checked_shr(m).unwrap_or(0).max(1);
                if mips > 1 && w <= tx && h <= ty && w * h * d.min(tz) * element <= page / 2 {
                    tail = true;
                } else {
                    slab += w.div_ceil(tx) * h.div_ceil(ty) * page;
                }
            }
            return depth.div_ceil(tz) * (slab + if tail { page } else { 0 });
        }
        let (tw, th) = match element {
            16 => (16, 16),
            8 => (32, 16),
            4 => (32, 32),
            2 => (64, 32),
            _ => (64, 64),
        };
        let mut slice = 0;
        for m in 0..mips {
            let (w, h) = (along(m, self.width), along(m, self.height));
            if w <= tw / 2 && h <= th {
                tail = true;
            } else {
                slice += w.div_ceil(tw) * h.div_ceil(th) * page;
            }
        }
        (slice + if tail { page } else { 0 }) * depth
    }

    /// Its 0x70-byte record at `at`, its name at `name`, its payload at `pixels` of the physical section.
    pub(super) fn record(
        &self,
        element: usize,
        at: usize,
        name: usize,
        pixels: usize,
    ) -> [u8; RECORD] {
        let mut r = [0u8; RECORD];
        let count = (self.payload.len() / element) as u32;
        put(&mut r, 0x08, &count.to_le_bytes());
        put(&mut r, 0x0C, &(element as u16).to_le_bytes());
        put(&mut r, 0x0E, &8u16.to_le_bytes());
        put(&mut r, 0x10, &self.flags.to_le_bytes());
        put(&mut r, 0x18, &(PHYSICAL + pixels as u64).to_le_bytes());
        put(&mut r, 0x28, &(VIRTUAL + (at + 0x50) as u64).to_le_bytes());
        put(&mut r, 0x30, &(VIRTUAL + name as u64).to_le_bytes());
        for (o, side) in [(0x38, self.width), (0x3A, self.height), (0x3C, self.depth)] {
            put(&mut r, o, &side.to_le_bytes());
        }
        (r[0x3E], r[0x3F], r[0x40]) = (self.dimension, self.format, 5);
        (r[0x42], r[0x43], r[0x45]) = (self.mip_levels, self.byte_43, self.byte_45);
        put(&mut r, 0x46, &1u16.to_le_bytes());
        put(&mut r, 0x4C, &2u32.to_le_bytes());
        // The view's kind: 2D, an array of 2D slices, cubes or a volume.
        let kind: u16 = match (self.dimension, self.depth > 1) {
            (1, false) => 4,
            (1, true) => 6,
            (2, _) => 9,
            _ => 10,
        };
        let view = 0x200 | kind << 4 | u16::from(self.dimension);
        put(&mut r, 0x60, &view.to_le_bytes());
        put(&mut r, 0x62, &u16::MAX.to_le_bytes());
        put(&mut r, 0x64, &u32::MAX.to_le_bytes());
        r
    }
}

fn put(to: &mut [u8], at: usize, bytes: &[u8]) {
    to[at..at + bytes.len()].copy_from_slice(bytes);
}

/// The model of the texture dictionary whose sections are `virtual_section` and `physical`.
pub fn read_dictionary_model(virtual_section: &[u8], physical: &[u8]) -> Written<DictionaryModel> {
    let refuse = |why| HeaderlessError::Layout {
        shape: "texture dictionary",
        why,
    };
    let model = sections(virtual_section, Some(physical), |v, p| {
        let codes = (v.u16("code count", 0x28)?, v.u16("code capacity", 0x2A)?);
        let entries = (v.u16("entry count", 0x38)?, v.u16("entry capacity", 0x3A)?);
        if codes != entries {
            return Err(HeaderlessError::Disagree {
                what: "dictionary's code and entry counts or capacities",
                first: codes.0.into(),
                second: entries.0.into(),
            });
        }
        let (count, capacity) = codes;
        if count == 0 || count > capacity {
            return Err(refuse("it holds no texture, or more than its capacity"));
        }
        let codes = v.array("codes", v.to("codes", 0x20)?, count.into(), 4)?;
        let entries = v.to("entries", 0x30)?;
        v.array("entries", entries, count.into(), 8)?;
        let mut records = Vec::with_capacity(count.into());
        for (i, code) in codes.as_chunks::<4>().0.iter().enumerate() {
            let at = v.to("entry", entries + 8 * i)?;
            if !is_texture(v, at) {
                return Err(refuse("an entry is no texture record"));
            }
            let texture = record_at(v, p, at)?;
            let (code, hash) = (
                u32::from_le_bytes(*code),
                name_hash32(texture.name.as_bytes()),
            );
            if code != hash {
                return Err(HeaderlessError::Disagree {
                    what: "entry's code and the hash of its name",
                    first: code.into(),
                    second: hash.into(),
                });
            }
            records.push((at, texture));
        }
        // The records lie in V in the order they were created.
        records.sort_by_key(|(at, _)| *at);
        Ok(DictionaryModel {
            textures: records.into_iter().map(|(_, t)| t).collect(),
            capacity,
        })
    })?;
    Ok(model)
}

/// The texture record at `at`, its name and its payload's bytes.
fn record_at(v: Bytes, p: Bytes, at: usize) -> Result<ModelTexture> {
    let name = v.to("texture name", at + 0x30)?;
    let name = v.c_str("texture name", name + 8)?;
    let byte = |o| v.u8("texture field", at + o);
    let side = |o| v.u16("texture size", at + o);
    let format = byte(0x3F)?;
    let stride = v.u16("texture block stride", at + 0x0C)?;
    if let Some(&(_, element, _)) = HELD.iter().find(|held| held.0 == format) {
        if usize::from(stride) != element {
            return Err(HeaderlessError::Disagree {
                what: "texture's block stride and its format's element size",
                first: stride.into(),
                second: element as u64,
            });
        }
    }
    let count = v.u32("texture block count", at + 0x08)?;
    let pixels = v.phys("texture pixels", at + 0x18)?;
    let payload = p.array("texture pixels", pixels, count as usize, stride.into())?;
    Ok(ModelTexture {
        name,
        width: side(0x38)?,
        height: side(0x3A)?,
        depth: side(0x3C)?,
        dimension: byte(0x3E)?,
        format,
        mip_levels: byte(0x42)?,
        flags: v.u32("texture flags", at + 0x10)?,
        byte_43: byte(0x43)?,
        byte_45: byte(0x45)?,
        payload: payload.to_vec(),
    })
}

/// Objects of (size, alignment), in placement order, placed from `start`: each in a head of whole
/// pieces where it fits after the head's fill, else at the tail's fill; the head grows by a piece
/// while the tail is longer than a block map states. Their offsets, the head and the tail.
fn plan(objects: &[(usize, usize)], start: usize) -> (Vec<usize>, usize, usize) {
    let bump = (objects.iter()).fold(start, |end, &(size, align)| {
        end.next_multiple_of(align) + size
    });
    let mut head = bump - bump % PIECE;
    loop {
        // A fill only grows: a gap an object skipped is never filled.
        let (mut fill, mut tail) = (start, if head == 0 { start } else { 0 });
        let mut at = Vec::with_capacity(objects.len());
        for &(size, align) in objects {
            let from = fill.next_multiple_of(align);
            if from + size <= head {
                at.push(from);
                fill = from + size;
            } else {
                let from = tail.next_multiple_of(align);
                at.push(head + from);
                tail = from + size;
            }
        }
        // Once the head holds the bump placement every object fits it, so this ends.
        if tail <= TAIL_MAX {
            return (at, head, tail);
        }
        head += PIECE;
    }
}

/// How many blocks a section of `head` and `tail` is, once a read of its length counts as many.
fn blocks(head: usize, tail: usize) -> Written<usize> {
    let planned = usize::from(head > 0) + usize::from(tail > 0);
    let len = head + tail;
    let read = usize::from(len >= PIECE) + usize::from(!len.is_multiple_of(PIECE));
    if planned != read {
        return Err(DictionaryWriteError::WholePieceTail);
    }
    Ok(planned)
}

fn limit(section: &'static str, len: usize) -> Written<()> {
    if len > SECTION_LIMIT {
        return Err(DictionaryWriteError::TooLong {
            section,
            len: len as u64,
        });
    }
    Ok(())
}

/// One allocation of the virtual section.
#[derive(Debug, Clone, Copy)]
enum Object {
    Record(usize),
    Map,
    Entries,
    Name(usize),
    Codes,
}

/// `model`'s sections: payloads largest first, the virtual allocations by size, each section in
/// a head of whole pieces and a tail under one, and the block map counting both.
pub fn write_dictionary(model: &DictionaryModel) -> Written<Rpf9Sections> {
    let textures = &model.textures;
    let count = textures.len();
    if count == 0 {
        return Err(DictionaryWriteError::Empty);
    }
    if count > usize::from(model.capacity) {
        return Err(DictionaryWriteError::Count {
            count,
            capacity: model.capacity,
        });
    }
    let elements = (textures.iter())
        .map(ModelTexture::checked)
        .collect::<Written<Vec<_>>>()?;
    let mut keys: Vec<(u32, usize)> = (textures.iter().enumerate())
        .map(|(i, t)| (name_hash32(t.name.as_bytes()), i))
        .collect();
    keys.sort_unstable();
    if let Some(pair) = keys.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(DictionaryWriteError::SameKey {
            first: textures[pair[0].1].name.clone(),
            second: textures[pair[1].1].name.clone(),
        });
    }

    // The payloads largest first, ties in creation order.
    let mut by_size: Vec<usize> = (0..count).collect();
    by_size.sort_by_key(|&i| Reverse(textures[i].payload.len()));
    let sized: Vec<(usize, usize)> = (by_size.iter())
        .map(|&i| (textures[i].payload.len(), PAGE))
        .collect();
    let (placed, head, tail) = plan(&sized, 0);
    limit("physical", head + tail)?;
    let physical_blocks = blocks(head, tail)?;
    let mut physical = vec![0u8; head + tail];
    let mut pixels = vec![0; count];
    for (&i, &at) in by_size.iter().zip(&placed) {
        pixels[i] = at;
        put(&mut physical, at, &textures[i].payload);
    }

    let capacity = usize::from(model.capacity);
    let mut slots = 1 + physical_blocks;
    for _ in 0..PLANS {
        let mut objects: Vec<(Object, usize, usize)> = (0..count)
            .map(|i| (Object::Record(i), RECORD, 16))
            .collect();
        objects.push((Object::Map, 16 + 8 * slots, 16));
        objects.push((Object::Entries, 8 * capacity, 8));
        let names = textures.iter().enumerate();
        objects.extend(names.map(|(i, t)| (Object::Name(i), 9 + t.name.len(), 8)));
        objects.push((Object::Codes, 4 * capacity, 8));
        // Stable: creation order breaks ties of rounded size.
        objects.sort_by_key(|&(_, size, _)| Reverse(size.next_multiple_of(8)));
        let sized: Vec<(usize, usize)> = objects.iter().map(|&(_, s, a)| (s, a)).collect();
        let (placed, head, tail) = plan(&sized, ROOT);
        let tail = tail.next_multiple_of(16);
        let virtual_blocks = blocks(head, tail)?;
        if virtual_blocks + physical_blocks != slots {
            slots = virtual_blocks + physical_blocks;
            continue;
        }
        limit("virtual", head + tail)?;
        let (mut records, mut names) = (vec![0; count], vec![0; count]);
        let (mut map, mut entries, mut codes) = (0, 0, 0);
        for (&(object, ..), &at) in objects.iter().zip(&placed) {
            match object {
                Object::Record(i) => records[i] = at,
                Object::Name(i) => names[i] = at,
                Object::Map => map = at,
                Object::Entries => entries = at,
                Object::Codes => codes = at,
            }
        }
        let mut v = vec![0u8; head + tail];
        put(&mut v, 0x08, &(VIRTUAL + map as u64).to_le_bytes());
        put(&mut v, 0x18, &1u32.to_le_bytes());
        for (at, array) in [(0x20, codes), (0x30, entries)] {
            put(&mut v, at, &(VIRTUAL + array as u64).to_le_bytes());
            put(&mut v, at + 8, &(count as u16).to_le_bytes());
            put(&mut v, at + 10, &model.capacity.to_le_bytes());
        }
        (v[map + 8], v[map + 9]) = (virtual_blocks as u8, physical_blocks as u8);
        for (k, &(key, i)) in keys.iter().enumerate() {
            put(&mut v, codes + 4 * k, &key.to_le_bytes());
            put(
                &mut v,
                entries + 8 * k,
                &(VIRTUAL + records[i] as u64).to_le_bytes(),
            );
        }
        for (i, t) in textures.iter().enumerate() {
            let record = t.record(elements[i], records[i], names[i], pixels[i]);
            put(&mut v, records[i], &record);
            // Eight zero bytes, then the name and its NUL.
            put(&mut v, names[i] + 8, t.name.as_bytes());
        }
        return Ok(Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: v,
            physical,
        });
    }
    Err(DictionaryWriteError::MapNotSettled)
}

#[cfg(test)]
#[path = "write_dictionary_tests.rs"]
mod tests;

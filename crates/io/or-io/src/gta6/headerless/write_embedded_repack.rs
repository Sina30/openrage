//! A drawable's embedded texture given another size, format or mips, embedded or unembedded: the
//! physical section packed anew from its buffer objects, each object's +0x18 its only referrer.
//!
//! ⚠ A 0x6 word of the virtual section that aims at an object's start and is no object's +0x18
//! refuses the re-pack: it would be a referrer left aiming where the object no longer lies.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::super::ttpf::Format;
use super::bytes::{sections, u64_at, Bytes};
use super::head::{buffer_objects, BufferKind};
use super::write_dictionary::ModelTexture;
use super::write_material::based;
use super::{HeaderlessError, Result};
use crate::container::rpf::{grow_section, Rpf9Form, Rpf9GrowError, Rpf9Section, Rpf9Sections};

const VIRTUAL: u64 = 0x5000_0000;
const PHYSICAL: u64 = 0x6000_0000;
/// The offset a pointer word holds below its section nibble.
const TARGET: u64 = 0x0FFF_FFFF;
/// A section's head is whole 64 KiB pieces, and a tail of the rest follows it.
const PIECE: usize = 0x1_0000;
/// The bytes of a section a pointer reaches: its low 28 bits.
const SECTION_LIMIT: usize = 1 << 28;
const OBJECT: usize = 0x70;
const DICTIONARY: usize = 0x40;
/// A key record: its header `{6, word}`, then its entry `{the header tagged 5, the key}`.
const RECORD: usize = 0x20;
const RECORD_KIND: u64 = 6;
const RECORD_TAG: u64 = 5;
/// A scan lists a stored block without its trailing zeros, so an object may end this far past
/// the physical section it read.
const SHORT_TAIL: usize = 16;

/// Why an embedded texture is not written.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Grow(#[from] Rpf9GrowError),
    #[error("the embedded texture is not written: {0}")]
    Refused(&'static str),
    #[error("the virtual section's word at {at:#X} aims at the physical object at {target:#X} and is not that object's +0x18, a referrer the re-pack does not know")]
    Referrer { at: u64, target: u64 },
    #[error("two buffer objects start at {target:#X} of the physical section, which the re-pack would part")]
    Shared { target: u64 },
    #[error("the physical section would pass the {SECTION_LIMIT} bytes a pointer reaches")]
    TooLong,
}

type Written<T> = std::result::Result<T, EmbedError>;

fn refuse(why: &'static str) -> EmbedError {
    EmbedError::Refused(why)
}

/// The shader group of the drawable at the root, or of its fragment's where `fragment`, and the
/// dictionary it points at from +0x08.
fn group_of(v: Bytes, fragment: bool) -> Result<(usize, Option<usize>)> {
    let root = match fragment {
        true => v.to("drawable", 0x18)?,
        false => 0,
    };
    let group = v.to("shader group", root + 0x10)?;
    Ok((group, v.ptr("embedded texture dictionary", group + 0x08)?))
}

/// The texture object of `key` the group's dictionary holds, the dictionary and its texture count.
fn embedded_at(v: Bytes, fragment: bool, key: u64) -> Result<Option<(usize, usize, usize)>> {
    let (_, Some(dictionary)) = group_of(v, fragment)? else {
        return Ok(None);
    };
    let count = usize::from(v.u16("embedded dictionary's texture count", dictionary + 0x38)?);
    if count == 0 {
        return Ok(None);
    }
    let list = v.to("embedded textures", dictionary + 0x30)?;
    v.array("embedded textures", list, count, 8)?;
    for k in 0..count {
        let at = v.to("embedded texture", list + 8 * k)?;
        let record = v.to("embedded texture's key record", at + 0x30)?;
        if v.u64("embedded texture's key", record + 0x08)? == key {
            return Ok(Some((at, dictionary, count)));
        }
    }
    Ok(None)
}

/// `bytes` over `v` at `at`.
fn put(v: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let len = v.len() as u64;
    let to = (at.checked_add(bytes.len()))
        .and_then(|end| v.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "embedded texture's record",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    to.copy_from_slice(bytes);
    Ok(())
}

fn pointer(at: usize) -> [u8; 8] {
    (VIRTUAL + at as u64).to_le_bytes()
}

/// The element bytes of `texture`, a 2D texture of one slice whose payload is whole elements.
fn element_of(texture: &ModelTexture) -> Written<usize> {
    if texture.dimension != 1 || texture.depth != 1 {
        return Err(refuse(
            "only a 2D texture of one slice is embedded: no drawable embeds another, so its form is not measured",
        ));
    }
    let element = (Format::of_dxgi(texture.format).map(Format::element_bytes))
        .ok_or(HeaderlessError::TextureFormat(texture.format))?;
    let count = texture.payload.len() / element;
    if count == 0 || count * element != texture.payload.len() || u32::try_from(count).is_err() {
        return Err(refuse(
            "the texture's payload is no whole number of its elements",
        ));
    }
    Ok(element)
}

/// The texture of `key` the drawable, or its fragment's where `fragment`, embeds, given the size, format, mips and payload of `texture`.
// The physical section is packed anew; the object keeps its flags, its key record and its
// dictionary's word.
pub fn reshape_embedded_texture(
    sections_of: &mut Rpf9Sections,
    fragment: bool,
    key: u64,
    texture: &ModelTexture,
) -> Written<()> {
    let element = element_of(texture)?;
    let (at, shape) = sections(&sections_of.virtual_section, None, |v, _| {
        let Some((at, ..)) = embedded_at(v, fragment, key)? else {
            return Ok(None);
        };
        let shape = (
            v.u8("texture dimension", at + 0x3E)?,
            v.u16("texture depth", at + 0x3C)?,
        );
        Ok(Some((at, shape)))
    })?
    .ok_or(refuse("the drawable embeds no texture of that key"))?;
    if shape != (1, 1) {
        return Err(refuse(
            "the embedded texture is no 2D texture of one slice, whose form was not measured",
        ));
    }
    let mut s = sections_of.clone();
    let v = &mut s.virtual_section;
    let count = (texture.payload.len() / element) as u32;
    put(v, at + 0x08, &count.to_le_bytes())?;
    put(v, at + 0x0C, &(element as u16).to_le_bytes())?;
    put(v, at + 0x38, &texture.width.to_le_bytes())?;
    put(v, at + 0x3A, &texture.height.to_le_bytes())?;
    put(v, at + 0x3F, &[texture.format])?;
    put(v, at + 0x42, &[texture.mip_levels])?;
    repack(&mut s, &[(at, &texture.payload)], None)?;
    *sections_of = s;
    Ok(())
}

/// The texture of `key` taken out of the drawable's shader group, its dictionary left in the empty form.
// Its object is cleared and the physical section packed anew without its pixels. The slots keep
// the key, which a pack must then hold; the record and arrays stay as bytes nothing names.
pub fn unembed_texture(sections_of: &mut Rpf9Sections, fragment: bool, key: u64) -> Written<()> {
    let (at, dictionary, count) = sections(&sections_of.virtual_section, None, |v, _| {
        embedded_at(v, fragment, key)
    })?
    .ok_or(refuse("the drawable embeds no texture of that key"))?;
    if count != 1 {
        return Err(refuse(
            "the dictionary holds several textures, a form no drawable was measured with",
        ));
    }
    let mut s = sections_of.clone();
    let mut empty = [0u8; DICTIONARY];
    empty[0x18] = 1;
    put(&mut s.virtual_section, dictionary, &empty)?;
    put(&mut s.virtual_section, at, &[0; OBJECT])?;
    repack(&mut s, &[], None)?;
    *sections_of = s;
    Ok(())
}

/// `texture` embedded under `key` in the drawable's shader group, its key record and the dictionary's hash stating `word`.
// The dictionary is the group's empty one, or a new one at the virtual section's end; the
// physical section is packed anew.
pub fn embed_texture(
    sections_of: &mut Rpf9Sections,
    fragment: bool,
    key: u64,
    word: u32,
    texture: &ModelTexture,
) -> Written<()> {
    let element = element_of(texture)?;
    let held = sections(&sections_of.virtual_section, None, |v, _| {
        let (_, dictionary) = group_of(v, fragment)?;
        let Some(d) = dictionary else {
            return Ok(None);
        };
        let hashes = v.u16("embedded dictionary's hash count", d + 0x28)?;
        let textures = v.u16("embedded dictionary's texture count", d + 0x38)?;
        Ok(Some(hashes != 0 || textures != 0))
    })?;
    if held == Some(true) {
        return Err(refuse(
            "the shader group's dictionary already holds a texture, and none of two was measured",
        ));
    }
    let new_dictionary = held.is_none();
    let len = OBJECT + RECORD + 16 + 16 + if new_dictionary { DICTIONARY } else { 0 };
    let mut s = sections_of.clone();
    let pointers = based(&s);
    let end = s.virtual_section.len();
    let base = grow_section(&mut s, &pointers, Rpf9Section::Virtual, end, len as isize)?;
    // The map's slots may have grown before the group, so it is found again.
    let (group, dictionary) = sections(&s.virtual_section, None, |v, _| group_of(v, fragment))?;
    let (object, header) = (base, base + OBJECT);
    let entry = header + 0x10;
    let (hashes, list) = (header + RECORD, header + RECORD + 16);
    let dictionary = dictionary.unwrap_or(list + 16);
    // Where the pixels go is the re-pack's; until then the object aims past every other.
    let unplaced = s.physical.len();
    let v = &mut s.virtual_section;
    put(v, object, &texture.record(element, object, entry, unplaced))?;
    put(v, header, &RECORD_KIND.to_le_bytes())?;
    put(v, header + 8, &u64::from(word).to_le_bytes())?;
    put(
        v,
        entry,
        &(VIRTUAL + header as u64 + RECORD_TAG).to_le_bytes(),
    )?;
    put(v, entry + 8, &key.to_le_bytes())?;
    put(v, hashes, &word.to_le_bytes())?;
    put(v, list, &pointer(object))?;
    if new_dictionary {
        put(v, dictionary + 0x18, &1u32.to_le_bytes())?;
        put(v, group + 0x08, &pointer(dictionary))?;
    }
    for (at, array) in [(0x20, hashes), (0x30, list)] {
        put(v, dictionary + at, &pointer(array))?;
        put(v, dictionary + at + 8, &1u16.to_le_bytes())?;
        put(v, dictionary + at + 10, &1u16.to_le_bytes())?;
    }
    repack(&mut s, &[(object, &texture.payload)], Some(object))?;
    *sections_of = s;
    Ok(())
}

/// The 32-bit name hash a key record of the virtual section states for `key`.
// A record is a 16-aligned header `{6, word}` whose entry after it holds the key behind a pointer
// to the header tagged 5.
pub fn key_word(virtual_section: &[u8], key: u64) -> Option<u32> {
    let v = virtual_section;
    (0..v.len().saturating_sub(RECORD - 1))
        .step_by(16)
        .find_map(|header| {
            let word = |o: usize| u64_at(v, header + o);
            let tagged = VIRTUAL + header as u64 + RECORD_TAG;
            (word(0) == RECORD_KIND && word(0x10) == tagged && word(0x18) == key)
                .then(|| word(0x08) as u32)
        })
}

/// A buffer object: where it lies, its kind, where its data starts and its bytes.
struct Object {
    at: usize,
    kind: BufferKind,
    start: usize,
    len: usize,
}

impl Object {
    fn align(&self) -> usize {
        match self.kind {
            BufferKind::Texture => 0x1000,
            BufferKind::CpuTexture => 0x100,
            _ => 16,
        }
    }
}

fn objects(s: &Rpf9Sections) -> Result<Vec<Object>> {
    sections(&s.virtual_section, None, |v, _| {
        (buffer_objects(v).into_iter())
            .map(|(at, kind, start)| {
                let count = v.u32("buffer's element count", at + 0x08)? as usize;
                let stride = usize::from(v.u16("buffer's stride", at + 0x0C)?);
                let len = (count.checked_mul(stride))
                    .filter(|&len| len <= SECTION_LIMIT)
                    .ok_or(HeaderlessError::PastEnd {
                        what: "buffer's data",
                        at: start as u64,
                        section: "physical",
                        len: s.physical.len() as u64,
                    })?;
                Ok(Object {
                    at,
                    kind,
                    start,
                    len,
                })
            })
            .collect::<Result<Vec<_>>>()
    })
}

/// Refuses two objects of one start, and a 0x6 word aiming at an object's start that is not that
/// object's +0x18; `fresh`, made by the edit, has no start of its own yet.
fn check_referrers(v: &[u8], objects: &[Object], fresh: Option<usize>) -> Written<()> {
    let mut starts = HashMap::new();
    for o in objects.iter().filter(|o| Some(o.at) != fresh) {
        if starts.insert(o.start, o.at).is_some() {
            return Err(EmbedError::Shared {
                target: o.start as u64,
            });
        }
    }
    for at in (0..v.len().saturating_sub(7)).step_by(8) {
        let word = u64_at(v, at);
        if word >> 32 != 0 || word >> 28 != PHYSICAL >> 28 {
            continue;
        }
        let target = (word & TARGET) as usize;
        if starts
            .get(&target)
            .is_some_and(|&object| object + 0x18 != at)
        {
            return Err(EmbedError::Referrer {
                at: at as u64,
                target: target as u64,
            });
        }
    }
    Ok(())
}

/// The head of whole pieces and the tail of the rest a section of `len` bytes is cut into.
fn cut(len: usize) -> Vec<usize> {
    [len - len % PIECE, len % PIECE]
        .into_iter()
        .filter(|n| *n > 0)
        .collect()
}

/// Each object's start, in `order`, first fit into the blocks of a section of `len` bytes, each
/// aligned and none across a block; `None` where one does not fit.
fn first_fit(objects: &[Object], order: &[usize], len: usize) -> Option<Vec<usize>> {
    let blocks = cut(len);
    let mut starts = Vec::with_capacity(blocks.len());
    let mut next = 0;
    for b in &blocks {
        starts.push(next);
        next += b;
    }
    let mut fills = starts.clone();
    let mut at = vec![0; objects.len()];
    for &i in order {
        let o = &objects[i];
        let k = (0..blocks.len())
            .find(|&k| fills[k].next_multiple_of(o.align()) + o.len <= starts[k] + blocks[k])?;
        at[i] = fills[k].next_multiple_of(o.align());
        fills[k] = at[i] + o.len;
    }
    Some(at)
}

/// Each object's new start and the section's length: first-fit decreasing by size rounded to 16,
/// ties by where the object lies, into the shortest length of whole 16 bytes whose cut holds them.
fn pack(objects: &[Object]) -> Written<(Vec<usize>, usize)> {
    let mut order: Vec<usize> = (0..objects.len()).collect();
    order.sort_by_key(|&i| (Reverse(objects[i].len.next_multiple_of(16)), objects[i].at));
    let mut len = (objects.iter()).fold(0usize, |sum, o| {
        sum.saturating_add(o.len.next_multiple_of(16))
    });
    loop {
        if len > SECTION_LIMIT {
            return Err(EmbedError::TooLong);
        }
        if let Some(at) = first_fit(objects, &order, len) {
            return Ok((at, len));
        }
        len += 16;
    }
}

/// The physical section packed anew from the buffer objects: an object of `data` takes the bytes
/// given, every other its own; each object's +0x18 aims at its new start.
fn repack(s: &mut Rpf9Sections, data: &[(usize, &[u8])], fresh: Option<usize>) -> Written<()> {
    if s.form != Rpf9Form::BlockMapped {
        return Err(refuse("a stored RSC8 has no physical section to pack"));
    }
    let objects = objects(s)?;
    check_referrers(&s.virtual_section, &objects, fresh)?;
    let (placed, len) = pack(&objects)?;
    let mut physical = vec![0u8; len];
    for (o, &to) in objects.iter().zip(&placed) {
        let given = data.iter().find(|(at, _)| *at == o.at).map(|(_, b)| *b);
        let bytes = match given {
            Some(bytes) if bytes.len() == o.len => bytes,
            Some(_) => {
                return Err(refuse(
                    "the pixels are not the bytes the texture object states",
                ))
            }
            None if Some(o.at) == fresh => return Err(refuse("a new object has no bytes")),
            None => {
                let end = o.start.checked_add(o.len).ok_or(EmbedError::TooLong)?;
                if o.start > s.physical.len() || end > s.physical.len() + SHORT_TAIL {
                    return Err(HeaderlessError::PastEnd {
                        what: "buffer's data",
                        at: o.start as u64,
                        section: "physical",
                        len: s.physical.len() as u64,
                    }
                    .into());
                }
                &s.physical[o.start..end.min(s.physical.len())]
            }
        };
        physical[to..to + bytes.len()].copy_from_slice(bytes);
        let word = (PHYSICAL + to as u64).to_le_bytes();
        put(&mut s.virtual_section, o.at + 0x18, &word)?;
    }
    fit_map(s, physical)
}

/// The offset of the block map the virtual section points at from 0x08.
fn map_of(v: &[u8]) -> Written<usize> {
    let none =
        || refuse("the virtual section's word at 0x08 is no pointer to a block map inside it");
    let word = v.get(8..16).map(|b| u64_at(b, 0)).ok_or_else(none)?;
    let map = (word & TARGET) as usize;
    match word >> 28 == VIRTUAL >> 28 && map + 16 <= v.len() {
        true => Ok(map),
        false => Err(none()),
    }
}

/// How many blocks each section is cut into once written.
fn counts(s: &Rpf9Sections) -> (usize, usize) {
    let blocks = |len: usize| cut(len.next_multiple_of(16)).len();
    (blocks(s.virtual_section.len()), blocks(s.physical.len()))
}

/// `physical` as the section, the block map's slots grown where its blocks need more and its
/// counts set.
fn fit_map(s: &mut Rpf9Sections, physical: Vec<u8>) -> Written<()> {
    let map = map_of(&s.virtual_section)?;
    let room = usize::from(s.virtual_section[map + 8]) + usize::from(s.virtual_section[map + 9]);
    s.physical = physical;
    let (virtual_blocks, physical_blocks) = counts(s);
    if virtual_blocks + physical_blocks > room {
        // 16 zero bytes after the slots, and the grower gives the map the slots it still lacks.
        // An odd number of slots ends 8 bytes short of the boundary the grower inserts at.
        let pointers = based(s);
        let at = (map + 16 + 8 * room).next_multiple_of(16);
        grow_section(s, &pointers, Rpf9Section::Virtual, at, 16)?;
    }
    let (virtual_blocks, physical_blocks) = counts(s);
    let map = map_of(&s.virtual_section)?;
    s.virtual_section[map + 8] = virtual_blocks as u8;
    s.virtual_section[map + 9] = physical_blocks as u8;
    Ok(())
}

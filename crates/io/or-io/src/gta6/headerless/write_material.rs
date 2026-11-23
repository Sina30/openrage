//! A drawable's materials written where an edit changes a shader's effect, or adds or removes a
//! parameter or a texture slot: such a shader's parameter infos and parameter block are laid out
//! anew at the end of the virtual section, which grows by the resource rule, and its old ones stay
//! as bytes nothing names.
//!
//! ⚠ Every based word of the virtual section is taken for a pointer: in drawables, fragments and
//! dictionaries no word of that shape that a reader types is data.

use super::bytes::{sections, u64_at, Bytes};
use super::dictionary::entry_of;
use super::drawable::{drawable_at, is_drawable_root, shaders_at, Drawable};
use super::write::write_shaders;
use super::{HeaderlessError, ParameterValue, Result, Shader};
use crate::container::rpf::{grow_section, Rpf9GrowError, Rpf9Role, Rpf9Section, Rpf9Sections};

/// The base of a virtual-section pointer.
const VIRTUAL: u64 = 0x5000_0000;
/// The offset a pointer word holds below its section nibble.
const TARGET: u64 = 0x0FFF_FFFF;
/// A packed word's 6-bit index names a constant's buffer.
const BUFFERS: usize = 1 << 6;
/// A packed word's 14-bit offset.
const OFFSET_LIMIT: usize = 1 << 14;
/// A packed word's 10-bit size in 4-byte words.
const UNITS_LIMIT: usize = 1 << 10;
/// Shader +0x3A states the parameter block's length through the texture table, plus this.
const LENGTH_EXTRA: usize = 32;
/// A binding's template: its texture flags word at +0x10, and the words at +0x38 and +0x40.
const BINDING_FLAGS: u64 = 0x980;
const BINDING_TAIL: [u64; 2] = [0x0001_0001_0000_0000, 0x0001_0000_0001_00FF];
/// A key record's header states this kind, and its entry points at the header with this tag.
const RECORD_KIND: u64 = 6;
const RECORD_TAG: usize = 5;
/// A key record (its header, then the entry a binding points at) and the binding after it.
const RECORD_LEN: usize = 0x20;
const BOUND_LEN: usize = RECORD_LEN + 0x50;

/// Why a drawable's materials are not written.
#[derive(Debug, thiserror::Error)]
pub enum MaterialLayoutError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Grow(#[from] Rpf9GrowError),
    #[error("shader {shader} is not laid out anew: {why}")]
    Layout { shader: usize, why: &'static str },
    #[error(
        "shader {shader}'s texture key hash_{key:016X} is not bound to a new slot: no name or bound \
         entry states this texture's 32-bit name hash"
    )]
    UnnamedKey { shader: usize, key: u64 },
    #[error("the written drawable does not read back as edited: {0}")]
    ReadBack(&'static str),
}

/// Whether `shader` can be laid out (textures first in their own slots, each constant stated by a packed word, in its rows and over no other); why not.
pub fn shader_layout_holds(shader: &Shader) -> std::result::Result<(), &'static str> {
    let slots = shader.texture_keys.len();
    if shader.parameters.len() > usize::from(u8::MAX) || slots > usize::from(u8::MAX) {
        return Err("a shader holds at most 255 parameters and 255 texture slots");
    }
    let mut constants = Vec::new();
    for (k, p) in shader.parameters.iter().enumerate() {
        match &p.value {
            ParameterValue::Texture { slot } => {
                if usize::from(*slot) != k || k >= slots {
                    return Err("a texture record is not in its own slot, or follows a constant");
                }
            }
            ParameterValue::Constant {
                buffer,
                offset,
                words,
            } => {
                let (buffer, start) = (usize::from(*buffer), usize::from(*offset));
                if k < slots {
                    return Err("a constant comes before a texture slot's record");
                }
                if buffer >= BUFFERS {
                    return Err("a constant's buffer is past the 64 a packed word names");
                }
                if !start.is_multiple_of(4) || start >= OFFSET_LIMIT {
                    return Err("a constant's offset is not a 4-byte word below 16 KiB");
                }
                if words.is_empty() || words.len() >= UNITS_LIMIT {
                    return Err("a constant holds from 1 to 1,023 words");
                }
                let end = start + 4 * words.len();
                if end - start <= 16 && start / 16 != (end - 1) / 16 {
                    return Err("a constant of 16 bytes or less crosses a 16-byte row");
                }
                constants.push((buffer, start, end));
            }
        }
    }
    if constants.len() + slots != shader.parameters.len() {
        return Err("its texture records and its texture slots differ in number");
    }
    constants.sort_unstable();
    if constants
        .windows(2)
        .any(|w| w[0].0 == w[1].0 && w[1].1 < w[0].2)
    {
        return Err("two constants of one buffer overlap");
    }
    if constants.is_empty() && slots == 0 {
        return Err(
            "a shader of neither a constant nor a texture has no parameter block, whose pointer is not known then",
        );
    }
    Ok(())
}

/// The drawable `role` and `code` name written from `shaders`, a shader laid out otherwise than read given new infos and a new block at the virtual section's end.
// The drawable is its own, a fragment's, or the dictionary entry of `code`; every value is then
// written where it lies, as `write_shaders` writes it.
pub fn write_materials(
    role: Rpf9Role,
    sections: &mut Rpf9Sections,
    code: Option<u32>,
    shaders: &[Shader],
) -> std::result::Result<(), MaterialLayoutError> {
    write_materials_with(role, sections, code, shaders, &|_| None)
}

/// [`write_materials`], a key bound to a new slot taking the 32-bit name hash `word_of` gives, else the one a key record of the body states.
pub fn write_materials_with(
    role: Rpf9Role,
    sections: &mut Rpf9Sections,
    code: Option<u32>,
    shaders: &[Shader],
    word_of: &dyn Fn(u64) -> Option<u32>,
) -> std::result::Result<(), MaterialLayoutError> {
    let (root, own_bound, held) = read_held(&sections.virtual_section, role, code)?;
    let count = |held: &[Held]| match held.len() == shaders.len() {
        true => Ok(()),
        false => Err(HeaderlessError::NotInPlace(
            "the drawable holds another number of shaders",
        )),
    };
    count(&held)?;
    let mut plans = Vec::new();
    for (index, (h, edited)) in held.iter().zip(shaders).enumerate() {
        if !moves(&h.shader, edited) {
            continue;
        }
        let refuse = |w| MaterialLayoutError::Layout {
            shader: index,
            why: w,
        };
        shader_layout_holds(edited).map_err(refuse)?;
        let mut p = plan(index, h, edited).map_err(refuse)?;
        p.words = (p.bound.iter())
            .map(|&(_, key)| {
                (word_of(key).or_else(|| stated_word(&sections.virtual_section, &held, key)))
                    .ok_or(MaterialLayoutError::UnnamedKey { shader: index, key })
            })
            .collect::<std::result::Result<_, _>>()?;
        plans.push(p);
    }
    // Values written in place touch only the virtual section, so only a moved layout copies both.
    if plans.is_empty() {
        let mut v = sections.virtual_section.clone();
        name_hashes(&mut v, &held, shaders)?;
        write_shaders(role, &mut v, code, shaders)?;
        rekey(&sections.virtual_section, &mut v, role, code, word_of)?;
        sections.virtual_section = v;
        return Ok(());
    }
    let mut grown = sections.clone();
    let before = shape(&drawn(&grown, root, own_bound)?);
    let total: usize = plans.iter().map(|p| p.len).sum();
    let pointers = based(&grown);
    let at = grown.virtual_section.len();
    let mut base = grow_section(
        &mut grown,
        &pointers,
        Rpf9Section::Virtual,
        at,
        total as isize,
    )?;
    // The map's slots may have grown before the shaders, so they are found again.
    let (root, own_bound, held) = read_held(&grown.virtual_section, role, code)?;
    count(&held)?;
    for p in &plans {
        let (h, edited) = (&held[p.index], &shaders[p.index]);
        lay_out(&mut grown.virtual_section, p, h, edited, base)?;
        base += p.len;
    }
    name_hashes(&mut grown.virtual_section, &held, shaders)?;
    write_shaders(role, &mut grown.virtual_section, code, shaders)?;
    rekey(
        &sections.virtual_section,
        &mut grown.virtual_section,
        role,
        code,
        word_of,
    )?;
    let after = drawn(&grown, root, own_bound)?;
    if after.shaders != shaders {
        return Err(MaterialLayoutError::ReadBack(
            "its shaders differ from the edit",
        ));
    }
    if shape(&after) != before {
        return Err(MaterialLayoutError::ReadBack(
            "its name, box, levels or geometries differ from the body's",
        ));
    }
    *sections = grown;
    Ok(())
}

/// A shader as read: where it lies, what it holds, its infos' header and each slot's binding.
struct Held {
    at: usize,
    shader: Shader,
    header: [u8; 8],
    bindings: Vec<u64>,
}

/// The new infos and block of shader `index`: their buffer count and sizes, each new texture slot's
/// old slot (`None` binds no texture, or a binding of its own), each slot bound anew with its key
/// and its key's 32-bit name hash, and the bytes they take.
struct Plan {
    index: usize,
    buffers: usize,
    sizes: Vec<usize>,
    from: Vec<Option<usize>>,
    bound: Vec<(usize, u64)>,
    words: Vec<u32>,
    len: usize,
}

/// Whether `edited` lays out otherwise than `read`: an effect alone is written in place.
fn moves(read: &Shader, edited: &Shader) -> bool {
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
    read.parameters.len() != edited.parameters.len()
        || read.texture_keys.len() != edited.texture_keys.len()
        || (read.parameters.iter().zip(&edited.parameters))
            .any(|(r, e)| r.hash != e.hash || !same(&r.value, &e.value))
        || (read.texture_keys.iter().zip(&edited.texture_keys))
            .any(|(r, e)| r.is_some() != e.is_some())
}

fn plan(index: usize, h: &Held, edited: &Shader) -> std::result::Result<Plan, &'static str> {
    let mut buffers = usize::from(h.header[0]);
    let mut ends: Vec<usize> = Vec::new();
    for p in &edited.parameters {
        if let ParameterValue::Constant {
            buffer,
            offset,
            words,
        } = &p.value
        {
            let b = usize::from(*buffer);
            buffers = buffers.max(b + 1);
            ends.resize(ends.len().max(b + 1), 0);
            ends[b] = ends[b].max(usize::from(*offset) + 4 * words.len());
        }
    }
    ends.resize(buffers, 0);
    let sizes: Vec<usize> = ends.iter().map(|e| e.next_multiple_of(16)).collect();
    // A kept slot keeps its binding: the old slot of the same hash, each taken once.
    let old: Vec<u32> = (h.shader.parameters.iter())
        .take(h.shader.texture_keys.len())
        .map(|p| p.hash)
        .collect();
    let mut taken = vec![false; old.len()];
    let mut from = Vec::with_capacity(edited.texture_keys.len());
    let mut bound_anew = Vec::new();
    for (slot, (p, key)) in edited
        .parameters
        .iter()
        .zip(&edited.texture_keys)
        .enumerate()
    {
        let found = (0..old.len()).find(|&k| !taken[k] && old[k] == p.hash);
        if let Some(k) = found {
            taken[k] = true;
        }
        let bound = found.filter(|&k| h.bindings.get(k).is_some_and(|w| *w != 0));
        match (key, bound) {
            (None, _) => from.push(None),
            (Some(_), Some(k)) => from.push(Some(k)),
            // One binding and one key record per slot, never shared.
            (Some(key), None) => {
                from.push(None);
                bound_anew.push((slot, *key));
            }
        }
    }
    let block = block_len(buffers, &sizes, edited.texture_keys.len());
    if block + LENGTH_EXTRA > usize::from(u16::MAX) {
        return Err("its parameter block passes the 16 bits shader +0x3A states its length in");
    }
    let len = infos_len(edited) + block.next_multiple_of(16) + BOUND_LEN * bound_anew.len();
    Ok(Plan {
        index,
        buffers,
        sizes,
        from,
        bound: bound_anew,
        words: Vec::new(),
        len,
    })
}

fn word_at(v: &[u8], at: usize) -> Option<u64> {
    Some(u64_at(v.get(at..at.checked_add(8)?)?, 0))
}

/// The entry the binding word `binding` points at and the header of its key record, where the
/// entry's tagged pointer names a record of kind 6.
fn record_of(v: &[u8], binding: u64) -> Option<(usize, usize)> {
    let target = |at: usize| {
        let w = word_at(v, at)?;
        (w >> 32 == 0 && w >> 28 == VIRTUAL >> 28).then_some((w & TARGET) as usize)
    };
    let binding = (binding >> 28 == VIRTUAL >> 28).then_some((binding & TARGET) as usize)?;
    let entry = target(binding.checked_add(0x30)?)?;
    let tagged = target(entry)?;
    let header = tagged.checked_sub(RECORD_TAG)?;
    (tagged % 16 == RECORD_TAG && word_at(v, header)? == RECORD_KIND).then_some((entry, header))
}

/// The 32-bit name hash a key record of the body states for `key`, found through a binding one of `held` holds.
fn stated_word(v: &[u8], held: &[Held], key: u64) -> Option<u32> {
    (held.iter().flat_map(|h| &h.bindings)).find_map(|&binding| {
        let (entry, header) = record_of(v, binding)?;
        (word_at(v, entry.checked_add(8)?)? == key)
            .then(|| word_at(v, header + 8).map(|w| w as u32))
            .flatten()
    })
}

/// Each key a slot of the drawable of `role` and `code` binds with the 32-bit name hash its key record states.
pub fn stated_words(virtual_section: &[u8], role: Rpf9Role, code: Option<u32>) -> Vec<(u64, u32)> {
    let v = virtual_section;
    let Ok((_, _, held)) = read_held(v, role, code) else {
        return Vec::new();
    };
    (held.iter().flat_map(|h| &h.bindings))
        .filter_map(|&binding| {
            let (entry, header) = record_of(v, binding)?;
            Some((
                word_at(v, entry.checked_add(8)?)?,
                word_at(v, header + 8)? as u32,
            ))
        })
        .collect()
}

/// `written`, the drawable of `role` and `code` written over `read`, with the word of each key
/// record `read` holds whose entry now holds another key set to that key's 32-bit name hash.
fn rekey(
    read: &[u8],
    written: &mut [u8],
    role: Rpf9Role,
    code: Option<u32>,
    word_of: &dyn Fn(u64) -> Option<u32>,
) -> std::result::Result<(), MaterialLayoutError> {
    let (_, _, before) = read_held(read, role, code)?;
    let (_, _, after) = read_held(written, role, code)?;
    let mut words = Vec::new();
    for (shader, h) in after.iter().enumerate() {
        for &binding in &h.bindings {
            let Some((entry, header)) = record_of(written, binding) else {
                continue;
            };
            let (Some(key), Some(was)) = (word_at(written, entry + 8), word_at(read, entry + 8))
            else {
                continue;
            };
            if key != was {
                let word = (word_of(key).or_else(|| stated_word(read, &before, key)))
                    .ok_or(MaterialLayoutError::UnnamedKey { shader, key })?;
                words.push((header + 8, u64::from(word)));
            }
        }
    }
    for (at, word) in words {
        put(written, at, &word.to_le_bytes())?;
    }
    Ok(())
}

fn infos_len(s: &Shader) -> usize {
    (8 + 8 * s.parameters.len()).next_multiple_of(16)
}

/// The buffers' pointers, the buffers and the texture table.
fn block_len(buffers: usize, sizes: &[usize], slots: usize) -> usize {
    8 * buffers + sizes.iter().sum::<usize>() + 8 * slots
}

/// `bytes` over `v` at `at`.
fn put(v: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let len = v.len() as u64;
    let to = (at.checked_add(bytes.len()))
        .and_then(|end| v.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "laid-out material",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    to.copy_from_slice(bytes);
    Ok(())
}

/// `edited`'s infos then its block at `base`, and the shader at `h.at` pointed at them.
fn lay_out(v: &mut [u8], p: &Plan, h: &Held, edited: &Shader, base: usize) -> Result<()> {
    let slots = edited.texture_keys.len();
    let infos = infos_len(edited);
    let mut b = vec![0u8; p.len];
    b[..8].copy_from_slice(&h.header);
    // Counts below 256 each, as `shader_layout_holds` and `plan` keep them.
    b[0] = p.buffers as u8;
    b[1] = slots as u8;
    b[4] = edited.parameters.len() as u8;
    let block = infos;
    let mut starts = Vec::with_capacity(p.buffers);
    let mut next = block + 8 * p.buffers;
    for (k, size) in p.sizes.iter().enumerate() {
        let word = VIRTUAL + (base + next) as u64;
        b[block + 8 * k..block + 8 * k + 8].copy_from_slice(&word.to_le_bytes());
        starts.push(next);
        next += size;
    }
    let table = next;
    for (k, param) in edited.parameters.iter().enumerate() {
        let packed = match &param.value {
            ParameterValue::Texture { slot } => 4 * u32::from(*slot),
            ParameterValue::Constant {
                buffer,
                offset,
                words,
            } => {
                let at = starts[usize::from(*buffer)] + usize::from(*offset);
                for (i, w) in words.iter().enumerate() {
                    b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&w.to_le_bytes());
                }
                3 | u32::from(*buffer) << 2 | u32::from(*offset) << 8 | (words.len() as u32) << 22
            }
        };
        b[8 + 8 * k..12 + 8 * k].copy_from_slice(&param.hash.to_le_bytes());
        b[12 + 8 * k..16 + 8 * k].copy_from_slice(&packed.to_le_bytes());
    }
    for (k, from) in p.from.iter().enumerate() {
        let word = from.and_then(|j| h.bindings.get(j).copied()).unwrap_or(0);
        b[table + 8 * k..table + 8 * k + 8].copy_from_slice(&word.to_le_bytes());
    }
    // Each key record, `{6, word}` then its entry `{tagged header, key}`, then its binding.
    let first = p.len - BOUND_LEN * p.bound.len();
    for (j, (&(slot, key), &word)) in p.bound.iter().zip(&p.words).enumerate() {
        let header = first + BOUND_LEN * j;
        let (entry, binding) = (header + 0x10, header + RECORD_LEN);
        let at = |o: usize| VIRTUAL + (base + o) as u64;
        let words = [
            (header, RECORD_KIND),
            (header + 8, u64::from(word)),
            (entry, at(header) + RECORD_TAG as u64),
            (entry + 8, key),
            (binding + 0x10, BINDING_FLAGS),
            (binding + 0x30, at(entry)),
            (binding + 0x38, BINDING_TAIL[0]),
            (binding + 0x40, BINDING_TAIL[1]),
            (table + 8 * slot, at(binding)),
        ];
        for (o, w) in words {
            b[o..o + 8].copy_from_slice(&w.to_le_bytes());
        }
    }
    put(v, base, &b)?;
    let pointer = |o: usize| (VIRTUAL + (base + o) as u64).to_le_bytes();
    put(v, h.at + 0x08, &pointer(block))?;
    let table_word = match slots {
        0 => [0; 8],
        _ => pointer(table),
    };
    put(v, h.at + 0x10, &table_word)?;
    put(v, h.at + 0x20, &pointer(0))?;
    let length = block_len(p.buffers, &p.sizes, slots) + LENGTH_EXTRA;
    put(v, h.at + 0x3A, &(length as u16).to_le_bytes())
}

/// Each shader's effect where the edit changes it.
fn name_hashes(v: &mut [u8], held: &[Held], shaders: &[Shader]) -> Result<()> {
    for (h, edited) in held.iter().zip(shaders) {
        if h.shader.name_hash != edited.name_hash {
            put(v, h.at, &edited.name_hash.to_le_bytes())?;
        }
    }
    Ok(())
}

/// The root of the drawable `role` and `code` name, and whether its word at 0xC8 is a bound.
fn root_of(v: Bytes, role: Rpf9Role, code: Option<u32>) -> Result<(usize, bool)> {
    let refuse = HeaderlessError::NotInPlace;
    let (root, own_bound) = match (role, code) {
        (Rpf9Role::Drawable, None) => (0, true),
        (Rpf9Role::DrawableWithBound, None) => (v.to("drawable", 0x18)?, false),
        (Rpf9Role::Dictionary, Some(code)) => (
            entry_of(v, code)?.ok_or(refuse("no entry of the dictionary has that code"))?,
            true,
        ),
        _ => return Err(refuse("the resource holds no drawable there")),
    };
    if !is_drawable_root(v, root, !own_bound) {
        return Err(HeaderlessError::NotADrawable { at: root as u64 });
    }
    Ok((root, own_bound))
}

fn read_held(v: &[u8], role: Rpf9Role, code: Option<u32>) -> Result<(usize, bool, Vec<Held>)> {
    sections(v, None, |v, _| {
        let (root, own_bound) = root_of(v, role, code)?;
        let read = shaders_at(v, root)?;
        let list = match read.len() {
            0 => 0,
            _ => v.to("shaders", v.to("shader group", root + 0x10)? + 0x10)?,
        };
        let held = (read.into_iter().enumerate())
            .map(|(i, (shader, _))| {
                let at = v.to("shader", list + 8 * i)?;
                let infos = v.to("parameter infos", at + 0x20)?;
                let mut header = [0u8; 8];
                header.copy_from_slice(v.get("parameter infos", infos, 8)?);
                let slots = shader.texture_keys.len();
                let bindings = match slots {
                    0 => Vec::new(),
                    _ => {
                        let table = v.to("texture bindings", at + 0x10)?;
                        (0..slots)
                            .map(|k| v.u64("texture binding", table + 8 * k))
                            .collect::<Result<_>>()?
                    }
                };
                Ok(Held {
                    at,
                    shader,
                    header,
                    bindings,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((root, own_bound, held))
    })
}

fn drawn(s: &Rpf9Sections, root: usize, own_bound: bool) -> Result<Drawable> {
    sections(&s.virtual_section, Some(&s.physical), |v, p| {
        drawable_at(v, p, root, own_bound)
    })
}

/// What a layout edit leaves as it was: the name, box and distances, and each level's geometries
/// by their shader, vertex count and triangle count.
type Shape = (String, [u32; 10], Vec<Vec<Option<(u16, u32, usize)>>>);

fn shape(d: &Drawable) -> Shape {
    let mut floats = [0u32; 10];
    for (to, x) in floats
        .iter_mut()
        .zip(d.box_min.iter().chain(&d.box_max).chain(&d.lod_distances))
    {
        *to = x.to_bits();
    }
    let levels = (d.levels.iter())
        .map(|level| {
            (level.iter().flatten())
                .flat_map(|m| &m.geometries)
                .map(|g| {
                    g.as_ref()
                        .ok()
                        .map(|g| (g.shader, g.vertex_count, g.triangles.len()))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    (d.name.clone(), floats, levels)
}

/// Every 8-aligned word of the virtual section whose high half is zero and whose low half
/// addresses a byte of the virtual (0x5) or the physical (0x6) section.
pub(super) fn based(s: &Rpf9Sections) -> Vec<usize> {
    let v = &s.virtual_section;
    (0..v.len().saturating_sub(7))
        .step_by(8)
        .filter(|&o| {
            let word = u64_at(v, o);
            let target = (word & TARGET) as usize;
            word >> 32 == 0
                && match word >> 28 {
                    5 => target < v.len(),
                    6 => target < s.physical.len(),
                    _ => false,
                }
        })
        .collect()
}

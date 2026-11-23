//! Small headerless resources read field by field: the descriptor that names a keyed blob by its
//! key, the asset index of up to five group hash tables, a buffer of four neighbours an element,
//! and the offset table of a body with no block map.

use std::collections::HashSet;

use super::bytes::{sections, u32_at, u64_at, Bytes};
use super::streams::{buffer, VERTEX_BUFFER};
use super::{HeaderlessError, Result};

/// The control byte of an empty slot.
const EMPTY: u8 = 0x80;
/// Table 0's slot: a key, two words, and the pointer to the asset's value record.
const ASSET_SLOT: usize = 0x20;
/// The slot of tables 1 to 4: a key, a u32 count and capacity, and the pointer to a list of keys.
const GROUP_SLOT: usize = 0x18;
/// The root's length; the descriptor's stream and the asset index's table 0 start here.
const ROOT: usize = 0x60;

const DESCRIPTOR: &str = "keyed-blob descriptor";
const INDEX: &str = "asset index";
const NEIGHBOURS: &str = "neighbour table";
const OFFSETS: &str = "offset table";

fn refuse<T>(shape: &'static str, why: &'static str) -> Result<T> {
    Err(HeaderlessError::Layout { shape, why })
}

/// The root that names, by its key at 0x48, the keyed blob after it that states the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedBlobDescriptor {
    /// The key the paired keyed blob states at its 0x20.
    pub key: u64,
    /// The words at 0x30 to 0x44, whose units are not known; 0x3C rounds 0x38 up to 16.
    pub words: [u32; 6],
    /// The u16 pair at 0x50: 7, then 7 or 8.
    pub pair: [u16; 2],
    /// The stream at 0x60, u32 words whose grammar is not known.
    pub stream: Vec<u8>,
}

pub fn read_keyed_blob_descriptor(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
) -> Result<KeyedBlobDescriptor> {
    sections(virtual_section, physical, |v, p| {
        let no = |why| refuse(DESCRIPTOR, why);
        if p.len() > 0 {
            return no("it holds a physical section, which no measured descriptor has");
        }
        let head = v.get("descriptor root", 0, ROOT)?;
        let len = u32_at(head, 0x10);
        if len == 0 || len != u32_at(head, 0x14) {
            return no("its stream's length at 0x10 is zero or differs from its capacity at 0x14");
        }
        if u64_at(head, 0x18) != 0x5000_0000 + ROOT as u64 {
            return no("its stream pointer at 0x18 does not address 0x60");
        }
        if head[0x20..0x30]
            .iter()
            .chain(&head[0x54..])
            .any(|b| *b != 0)
        {
            return no("its bytes at 0x20 to 0x2F or 0x54 to 0x5F are not zero");
        }
        let words: [u32; 6] = std::array::from_fn(|k| u32_at(head, 0x30 + 4 * k));
        let up = |w: u32| u64::from(w).next_multiple_of(16);
        if u64::from(words[3]) != up(words[2]) {
            return no("its word at 0x3C is not the word at 0x38 rounded up to 16");
        }
        if !words[5].is_multiple_of(16) || words[5] < words[4] {
            return no("its word at 0x44 is not a multiple of 16 at or above the word at 0x40");
        }
        let key = u64_at(head, 0x48);
        if key == 0 {
            return no("its key at 0x48 is zero");
        }
        let pair = [0x50usize, 0x52].map(|at| u16::from_le_bytes([head[at], head[at + 1]]));
        if pair[0] != 7 || !matches!(pair[1], 7 | 8) || (pair[1] == 8 && len != 76) {
            return no("its u16 pair at 0x50 is not 7 and 7, or 7 and 8 over a 76-byte stream");
        }
        let map = v.to("block map", 0x08)?;
        if map as u64 != (ROOT as u64 + u64::from(len)).next_multiple_of(16) {
            return no("its block map does not follow its stream at the next 16-byte boundary");
        }
        let blocks = v.get("block map", map, 0x20)?;
        if blocks[8] != 1
            || blocks[9] != 0
            || v.len() != map + 0x20
            || blocks[0x10..].iter().any(|b| *b != 0)
        {
            return no("its block map is not one virtual block, no physical, then 16 zero bytes");
        }
        let stream = v.array("stream", ROOT, len as usize, 1)?.to_vec();
        Ok(KeyedBlobDescriptor {
            key,
            words,
            pair,
            stream,
        })
    })
}

/// Up to five group hash tables over 64-bit keys: table 0 of value records, 1 to 4 of its keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetIndex {
    /// Table 0's keys in slot order, each with where its value record lies; no record is read.
    pub assets: Vec<(u64, u64)>,
    /// Tables 1 to 4: each key in slot order and the table-0 keys it lists; empty where unused.
    pub groups: [Vec<(u64, Vec<u64>)>; 4],
}

pub fn read_asset_index(virtual_section: &[u8], physical: Option<&[u8]>) -> Result<AssetIndex> {
    sections(virtual_section, physical, |v, p| {
        let no = |why| refuse(INDEX, why);
        if p.len() > 0 {
            return no("it holds a physical section, which no measured asset index has");
        }
        let map = v.to("block map", 0x08)?;
        let blocks = v.get("block map", map, 0x10)?;
        if blocks[8] == 0 || blocks[9] != 0 {
            return no("its block map counts no virtual block, or a physical one");
        }
        if v.u64("table 0", 0x18)? != 0x5000_0000 + ROOT as u64 {
            return no("its table 0 does not lie at 0x60");
        }
        let Some((slots, span)) = table(v, 0x10, ASSET_SLOT)? else {
            return no("its table 0 is unused");
        };
        let mut spans = vec![span];
        let mut assets = Vec::with_capacity(slots.len());
        for slot in slots {
            let record = v.to("value record", slot + 0x18)?;
            assets.push((v.u64("asset key", slot)?, record as u64));
        }
        let keys: HashSet<u64> = assets.iter().map(|(k, _)| *k).collect();
        let mut groups: [Vec<(u64, Vec<u64>)>; 4] = Default::default();
        for (t, out) in groups.iter_mut().enumerate() {
            let Some((slots, span)) = table(v, 0x20 + 0x10 * t, GROUP_SLOT)? else {
                continue;
            };
            spans.push(span);
            for slot in slots {
                let count = v.u32("list count", slot + 0x08)?;
                if count == 0 || count != v.u32("list capacity", slot + 0x0C)? {
                    return no("a list's count is zero or differs from its capacity");
                }
                let list = v.array(
                    "key list",
                    v.to("key list", slot + 0x10)?,
                    count as usize,
                    8,
                )?;
                let listed: Vec<u64> = (list.as_chunks::<8>().0.iter())
                    .map(|w| u64::from_le_bytes(*w))
                    .collect();
                if !listed.iter().all(|k| keys.contains(k)) {
                    return no("a list names a key that table 0 does not hold");
                }
                out.push((v.u64("group key", slot)?, listed));
            }
        }
        let mut seen = HashSet::with_capacity(assets.len());
        for (_, record) in &assets {
            let at = *record as usize;
            let inside = spans
                .iter()
                .any(|(start, end)| (*start..*end).contains(&at))
                || (map..map + 0x10).contains(&at);
            if !at.is_multiple_of(16) || inside || !seen.insert(at) {
                return no("a value record is unaligned, in a table or the block map, or shared");
            }
        }
        Ok(AssetIndex { assets, groups })
    })
}

/// The least power of two, at least 2, at which `keys` fill no more than 14 slots a group.
fn group_count(keys: u32) -> u64 {
    let mut groups = 2u64;
    while 14 * groups < u64::from(keys) {
        groups *= 2;
    }
    groups
}

/// A table's full slots by offset, and the span it covers.
type Table = (Vec<usize>, (usize, usize));

/// The full slots, by offset, of the table whose group count, key count and pointer lie at
/// `header`, and the span the table covers; `None` for an unused table.
fn table(v: Bytes, header: usize, slot: usize) -> Result<Option<Table>> {
    let no = |why| refuse(INDEX, why);
    let (groups, keys) = (
        v.u32("group count", header)?,
        v.u32("key count", header + 4)?,
    );
    if groups == 0 {
        if keys != 0 || v.u64("table", header + 8)? != 0 {
            return no("an unused table states keys or a pointer");
        }
        return Ok(None);
    }
    if u64::from(groups) != group_count(keys) {
        return no("its group count is not the least power of two, at least 2, of 14 keys a group");
    }
    let stride = 16 + 16 * slot;
    let at = v.to("table", header + 8)?;
    let groups = groups as usize;
    let bytes = v.array("table", at, groups, stride)?;
    let control = |g: usize| &bytes[stride * g..stride * g + 16];
    // How many groups in a row before each group hold no empty slot, the table read as a ring.
    let room: Vec<bool> = (0..groups).map(|g| control(g).contains(&EMPTY)).collect();
    let mut full_before = vec![groups; groups];
    if let Some(start) = room.iter().position(|r| *r) {
        let mut run = 0;
        for i in 1..=groups {
            let g = (start + i) % groups;
            full_before[g] = run;
            run = if room[g] { 0 } else { run + 1 };
        }
    }
    let mut full = Vec::with_capacity(keys as usize);
    for (g, &before) in full_before.iter().enumerate() {
        for (s, tag) in control(g).iter().enumerate() {
            let off = stride * g + 16 + slot * s;
            let held = &bytes[off..off + slot];
            if *tag == EMPTY {
                if held.iter().any(|b| *b != 0) {
                    return no("an empty slot is not zero");
                }
                continue;
            }
            let key = u64_at(held, 0);
            if u64::from(*tag) != key & 0x7F {
                return no("a full slot's control byte is not its key's low seven bits");
            }
            let home = ((key >> 7) & (groups as u64 - 1)) as usize;
            if (g + groups - home) % groups > before {
                return no("a key lies past a group with room after its home group");
            }
            full.push(at + off);
        }
    }
    if full.len() != keys as usize {
        return no("a table holds another number of keys than it states");
    }
    Ok(Some((full, (at, at + bytes.len()))))
}

/// A raw buffer of elements of four u16 indices, each of another element; whose mesh is not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighbourTable {
    pub elements: Vec<[u16; 4]>,
}

pub fn read_neighbour_table(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
) -> Result<NeighbourTable> {
    sections(virtual_section, physical, |v, p| {
        let no = |why| refuse(NEIGHBOURS, why);
        let raw = v.to("raw buffer", 0x10)?;
        let declaration = v.u64("vertex declaration", raw + 0x40)?;
        if declaration != 0 {
            return Err(HeaderlessError::Constant {
                what: "neighbour table's vertex declaration",
                at: (raw + 0x40) as u64,
                found: declaration,
            });
        }
        let words = v.u32("buffer's count", raw + 0x08)?;
        if !words.is_multiple_of(2) {
            return no("its buffer holds an odd number of words: not whole elements");
        }
        if p.len() as u64 != (4 * u64::from(words)).next_multiple_of(16) {
            return no("its physical section is not its buffer's words rounded up to 16 bytes");
        }
        let data = buffer(v, p, "neighbours", raw, VERTEX_BUFFER)?;
        let elements: Vec<[u16; 4]> = (data.as_chunks::<8>().0.iter())
            .map(|e| std::array::from_fn(|k| u16::from_le_bytes([e[2 * k], e[2 * k + 1]])))
            .collect();
        let count = elements.len();
        for (i, e) in elements.iter().enumerate() {
            if let Some(n) = e.iter().find(|n| usize::from(**n) >= count) {
                return Err(HeaderlessError::IndexPastCount {
                    what: "neighbour index",
                    index: (*n).into(),
                    count: count as u64,
                });
            }
            if e.iter().any(|n| usize::from(*n) == i) {
                return no("an element names itself among its neighbours");
            }
        }
        Ok(NeighbourTable { elements })
    })
}

/// A body with no block map: a count at 0x14, then that many item offsets from 0x28.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetTable {
    /// Each item's offset in the body: 16-aligned and ascending, the first just past the offsets.
    pub offsets: Vec<u64>,
    /// The body's length, to which the last item runs.
    pub len: u64,
}

pub fn read_offset_table(body: &[u8]) -> Result<OffsetTable> {
    sections(body, None, |v, _| {
        let no = |why| refuse(OFFSETS, why);
        let head = v.get("offset table head", 0, 0x28)?;
        if head[..0x14].iter().chain(&head[0x18..]).any(|b| *b != 0) {
            return no("its head holds a word other than the count at 0x14");
        }
        let count = u32_at(head, 0x14) as usize;
        if count == 0 {
            return no("its count at 0x14 is zero");
        }
        let table = v.array("item offsets", 0x28, count, 8)?;
        let offsets: Vec<u64> = (table.as_chunks::<8>().0.iter())
            .map(|w| u64::from_le_bytes(*w))
            .collect();
        let len = v.len() as u64;
        let first = 0x28 + 8 * count as u64;
        if offsets.first() != Some(&first)
            || offsets.windows(2).any(|w| w[1] <= w[0])
            || offsets.iter().any(|o| !o.is_multiple_of(16))
            || offsets.last().is_none_or(|last| *last >= len)
        {
            return no("its offsets do not ascend, 16-aligned, from the end of their table");
        }
        Ok(OffsetTable { offsets, len })
    })
}

#[cfg(test)]
#[path = "small_tests.rs"]
mod tests;

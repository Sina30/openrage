//! What GTA VI audio game data items link to and how the measured ones are laid out: the Sounds
//! item header, the sound and speech items laid out so far, an audio configuration body's names
//! from its name pool, and the audio container a sound's bank stands for.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use super::audio_data::{AudioData, AudioDataError, AudioDataKind, AudioItem};

#[path = "audio_tables.rs"]
mod tables;

pub use tables::{SlotRole, SlotValue, TableField, TableItem, TableValue};

/// Byte +1 of a Sounds item that carries no Sounds header, as GTA V fills an unset word.
pub const NO_HEADER: u8 = 0xAA;

/// The size of the field sub-mask bit `j` of field-mask bit `i < 4` states; 0 where never measured.
const SUB_SIZES: [[usize; 8]; 4] = [
    [4, 4, 0, 4, 2, 2, 2, 2],
    [2, 2, 2, 2, 2, 1, 2, 1],
    [2; 8],
    [2, 2, 4, 2, 1, 0, 0, 0],
];

/// The size of the field mask bit `bit >= 8` states, where it was measured.
fn field_size(bit: u32) -> Option<usize> {
    Some(match bit {
        8 => 8,
        9 | 20 | 21 | 26 | 32 | 37 | 41..=44 | 46..=51 | 53 | 55 | 57 | 60..=63 => 4,
        31 | 34 | 38..=40 => 1,
        10..=36 => 2,
        _ => return None,
    })
}

/// What a Sounds item states after its class byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundsHeader {
    /// Byte +1 is [`NO_HEADER`].
    Absent,
    /// A field whose size was never measured, so where the header ends is not known.
    Unmeasured,
    Read {
        /// Byte +1: bit k set means byte k of `mask` is stated; not derivable from `mask`.
        presence: u8,
        mask: u64,
        /// The item offset of the first byte after the header's fields.
        end: usize,
    },
}

/// The Sounds header of `item`, its class byte first; `None` where it runs past the item.
pub fn sounds_header(item: &[u8]) -> Option<SoundsHeader> {
    let presence = *item.get(1)?;
    if presence == NO_HEADER {
        return Some(SoundsHeader::Absent);
    }
    let mut at = 2;
    let mut mask = 0u64;
    for k in 0..8 {
        if presence >> k & 1 != 0 {
            mask |= u64::from(*item.get(at)?) << (8 * k);
            at += 1;
        }
    }
    let mut fields = 0usize;
    let mut unmeasured = false;
    for i in (0..8).filter(|i| mask >> i & 1 != 0) {
        let sub = *item.get(at)?;
        at += 1;
        for j in (0..8).filter(|j| sub >> j & 1 != 0) {
            match SUB_SIZES.get(i).map_or(0, |sizes| sizes[j]) {
                0 => unmeasured = true,
                size => fields += size,
            }
        }
    }
    for bit in (8..64).filter(|bit| mask >> bit & 1 != 0) {
        match field_size(bit) {
            Some(size) => fields += size,
            None => unmeasured = true,
        }
    }
    if unmeasured {
        return Some(SoundsHeader::Unmeasured);
    }
    let end = at + fields;
    (end <= item.len()).then_some(SoundsHeader::Read {
        presence,
        mask,
        end,
    })
}

/// An item laid out from the bytes; a row is a row of the body's reference rows.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemLayout {
    /// Sounds class 12: a wave container's hash and the wave's, then the rows it refers to.
    SimpleSound {
        mask: u64,
        bank: u32,
        wave: u32,
        rows: Vec<u32>,
    },
    /// Sounds class 8: the sounds that play its container's channels.
    StreamingSound {
        mask: u64,
        /// Inferred from GTA V's StreamingSound.
        duration: u32,
        /// The class of its children: 12 in every item measured.
        child_class: u8,
        rows: Vec<u32>,
    },
    /// Sounds class 6: two rows or none around a group slot or none.
    Class6 {
        mask: u64,
        first_row: Option<u32>,
        group_slot: Option<u32>,
        second_row: Option<u32>,
    },
    Speech(SpeechForm),
    /// A GameObjects, Music, DynamicMixer, Categories or ModularSynth item, by its class's table.
    Table(TableItem),
}

/// A speech item's form, told apart by its length and first bytes, in the order they are tried.
#[derive(Debug, Clone, PartialEq)]
pub enum SpeechForm {
    /// u8 2, then a wave container's hash; one item of a body states 8 more bytes.
    Bank { bank: u32, rest: Option<[u8; 8]> },
    /// u8 n, u16[n].
    Shorts(Vec<u16>),
    /// u8 n, {u32, u32, u32}[n].
    Triples(Vec<[u32; 3]>),
    /// u32 n, {u32, u16, u16}[n], u32.
    Rows {
        rows: Vec<(u32, u16, u16)>,
        last: u32,
    },
    /// u32, u32, u8 n, {u32, u32, u32}[n].
    HeadedTriples { head: [u32; 2], rows: Vec<[u32; 3]> },
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// The layout of `item` of `data`, where one is measured and its bytes to the next item hold to it.
pub fn layout(data: &AudioData, item: &AudioItem) -> Option<ItemLayout> {
    let start = item.offset as usize;
    let bytes = data
        .data
        .get(start..start.checked_add(item.span as usize)?)?;
    // A field table lists body offsets, 8 past data offsets.
    let listed = |table: &[u32], at: usize| {
        u32::try_from(start + at + 8).is_ok_and(|f| table.binary_search(&f).is_ok())
    };
    let reference = |at: usize| listed(&data.reference_fields, at);
    let group = |at: usize| listed(&data.group_fields, at);
    match data.kind {
        AudioDataKind::Speech => {
            speech(bytes, listed(&data.wave_fields, 1)).map(ItemLayout::Speech)
        }
        AudioDataKind::Sounds => sound(bytes, item, reference, group),
        _ => tables::table_item(data, start, bytes).map(ItemLayout::Table),
    }
}

fn sound(
    bytes: &[u8],
    item: &AudioItem,
    reference: impl Fn(usize) -> bool,
    group: impl Fn(usize) -> bool,
) -> Option<ItemLayout> {
    let SoundsHeader::Read { mask, end: h, .. } = sounds_header(bytes)? else {
        return None;
    };
    let rows = |from: usize, n: usize| -> Option<Vec<u32>> {
        (0..n)
            .map(|i| {
                let at = from + 4 * i;
                u32_at(bytes, at).filter(|_| reference(at))
            })
            .collect()
    };
    // A word that is no reference field holds -1.
    let row_or_none = |at: usize| match u32_at(bytes, at)? {
        u32::MAX => Some(None),
        row => reference(at).then_some(Some(row)),
    };
    match bytes[0] {
        12 => {
            let bank_first = item.waves.first().map(|w| w.at as usize) == Some(h);
            if !bank_first || bytes.get(h + 8..h + 10)? != [0, 0] {
                return None;
            }
            Some(ItemLayout::SimpleSound {
                mask,
                bank: u32_at(bytes, h)?,
                wave: u32_at(bytes, h + 4)?,
                rows: rows(h + 11, usize::from(*bytes.get(h + 10)?))?,
            })
        }
        8 => Some(ItemLayout::StreamingSound {
            mask,
            duration: u32_at(bytes, h)?,
            child_class: *bytes.get(h + 4)?,
            rows: rows(h + 6, usize::from(*bytes.get(h + 5)?))?,
        }),
        6 if bytes.len() == h + 12 && group(h + 4) => Some(ItemLayout::Class6 {
            mask,
            first_row: row_or_none(h)?,
            group_slot: Some(u32_at(bytes, h + 4)?).filter(|slot| *slot != u32::MAX),
            second_row: row_or_none(h + 8)?,
        }),
        _ => None,
    }
}

/// The form of a speech item of `bytes`; `bank_at_1` says a wave field starts at +1.
fn speech(bytes: &[u8], bank_at_1: bool) -> Option<SpeechForm> {
    let len = bytes.len();
    let n = usize::from(*bytes.first()?);
    if bank_at_1 {
        if bytes[0] != 2 || (len != 5 && len != 13) {
            return None;
        }
        let rest = bytes.get(5..13).and_then(|r| <[u8; 8]>::try_from(r).ok());
        return Some(SpeechForm::Bank {
            bank: u32_at(bytes, 1)?,
            rest,
        });
    }
    let triples = |from: usize, n: usize| -> Option<Vec<[u32; 3]>> {
        (0..n)
            .map(|i| {
                let at = from + 12 * i;
                Some([
                    u32_at(bytes, at)?,
                    u32_at(bytes, at + 4)?,
                    u32_at(bytes, at + 8)?,
                ])
            })
            .collect()
    };
    if len == 1 + 2 * n {
        return (0..n)
            .map(|i| u16_at(bytes, 1 + 2 * i))
            .collect::<Option<_>>()
            .map(SpeechForm::Shorts);
    }
    if len == 1 + 12 * n {
        return triples(1, n).map(SpeechForm::Triples);
    }
    if len >= 8 {
        let count = u32_at(bytes, 0)? as usize;
        if count.checked_mul(8).and_then(|b| b.checked_add(8)) == Some(len) {
            let rows = (0..count)
                .map(|i| {
                    let at = 4 + 8 * i;
                    Some((
                        u32_at(bytes, at)?,
                        u16_at(bytes, at + 4)?,
                        u16_at(bytes, at + 6)?,
                    ))
                })
                .collect::<Option<_>>()?;
            return Some(SpeechForm::Rows {
                rows,
                last: u32_at(bytes, len - 4)?,
            });
        }
    }
    if len >= 9 {
        let count = usize::from(bytes[8]);
        if len == 9 + 12 * count {
            return Some(SpeechForm::HeadedTriples {
                head: [u32_at(bytes, 0)?, u32_at(bytes, 4)?],
                rows: triples(9, count)?,
            });
        }
    }
    None
}

fn bad(why: String) -> AudioDataError {
    AudioDataError::Malformed(why)
}

/// An audio configuration body's item names by key, read from `pool` where each item's +1 points.
// The names are NUL-terminated; refused unless every item's name hashes to its key.
pub fn config_names(
    data: &AudioData,
    pool: &[u8],
) -> Result<BTreeMap<u32, String>, AudioDataError> {
    if data.kind != AudioDataKind::AudioConfig {
        return Err(bad(
            "only audio configuration items name a pool entry".into()
        ));
    }
    let mut names = BTreeMap::new();
    let mut used = BTreeSet::new();
    for &(key, offset) in &data.index {
        let o = offset as usize;
        let b = (data.data.get(o + 1..o + 4)).ok_or_else(|| {
            bad(format!(
                "the item at {offset:#X} ends before its pool offset"
            ))
        })?;
        let at = u32::from_le_bytes([b[0], b[1], b[2], 0]) as usize;
        if !used.insert(at) {
            return Err(bad(format!("two items name the pool entry at {at}")));
        }
        let starts = at < pool.len() && (at == 0 || pool[at - 1] == 0);
        let name = (pool.get(at..))
            .filter(|_| starts)
            .and_then(|rest| rest.iter().position(|b| *b == 0).map(|end| &rest[..end]))
            .filter(|name| name.is_ascii())
            .ok_or_else(|| {
                bad(format!(
                    "the pool offset {at} of key {key:#010X} starts no name"
                ))
            })?;
        if super::hash::name_hash32(name) != key {
            return Err(bad(format!(
                "the pool name at {at} does not hash to its item's key {key:#010X}"
            )));
        }
        names.insert(key, String::from_utf8_lossy(name).into_owned());
    }
    Ok(names)
}

/// The audio containers holding each stream id a body's waves name, with their counts and digests.
// A digest of a container's stream ids tells copies of one container from different ones.
#[derive(Debug, Clone, Default)]
pub struct WaveIndex {
    holders: BTreeMap<u32, Vec<u64>>,
    containers: BTreeMap<u64, (usize, u64)>,
}

/// The audio container a bank stands for, or why none is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankLink {
    /// The container that holds every found wave of the bank: the only one, any of identical
    /// copies, or the one with the fewest streams.
    Container(u64),
    /// Containers that hold every found wave, differ, and have equally few streams.
    Tied(Vec<u64>),
    /// Some waves are found, but no one container holds all of them.
    Split,
    /// No container holds any of its waves.
    Absent,
}

impl WaveIndex {
    /// Adds the container at `offset` whose stream table states `words`, keeping the ids of `wanted`.
    pub fn add(&mut self, offset: u64, words: &[u32], wanted: &BTreeSet<u32>) {
        let mut ids: Vec<u32> = words.iter().map(|w| w & 0x1FFF_FFFF).collect();
        ids.sort_unstable();
        let mut digest = DefaultHasher::new();
        ids.hash(&mut digest);
        self.containers.insert(offset, (ids.len(), digest.finish()));
        for id in ids.iter().filter(|id| wanted.contains(id)) {
            self.holders.entry(*id).or_default().push(offset);
        }
    }

    /// Whether a container added holds the stream `id`.
    pub fn holds(&self, id: u32) -> bool {
        self.holders.contains_key(&id)
    }

    /// The container a bank whose waves are the stream ids `streams` stands for.
    pub fn bank(&self, streams: &BTreeSet<u32>) -> BankLink {
        let mut found = streams.iter().filter_map(|s| self.holders.get(s));
        let Some(first) = found.next() else {
            return BankLink::Absent;
        };
        let mut candidates = first.clone();
        for holders in found {
            candidates.retain(|c| holders.contains(c));
        }
        candidates.sort_unstable();
        candidates.dedup();
        let facts = |c: &u64| self.containers.get(c).copied().unwrap_or_default();
        let Some(&one) = candidates.first() else {
            return BankLink::Split;
        };
        if candidates.iter().all(|c| facts(c) == facts(&one)) {
            return BankLink::Container(one);
        }
        let fewest = candidates.iter().map(|c| facts(c).0).min().unwrap_or(0);
        let least: Vec<u64> = (candidates.into_iter())
            .filter(|c| facts(c).0 == fewest)
            .collect();
        match least.as_slice() {
            [one] => BankLink::Container(*one),
            _ => BankLink::Tied(least),
        }
    }
}

#[cfg(test)]
#[path = "audio_links_tests.rs"]
mod tests;

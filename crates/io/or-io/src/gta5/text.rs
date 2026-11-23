//! `.gxt2`: a text table, string hashes to UTF-8 strings.
//!
//! A header, a directory of entries sorted by hash, one more entry whose hash
//! is the magic and whose offset is where the strings end, then the strings,
//! each NUL-terminated at the offset its entry names.

use std::mem::size_of;

use crate::decl::*;
use crate::error::Error;
use crate::string_hash_str;

/// `GXT2`, little-endian.
pub const GXT2_MAGIC: u32 = 0x4758_5432;

/// What a table starts with; its directory follows.
#[record(size = 0x08)]
pub struct CGxt2Header {
    pub magic: u32,
    count: u32,
}

/// One directory entry: a string's hash, and its offset from the header.
#[record(size = 0x08)]
pub struct CGxt2Entry {
    pub hash: Hash32,
    offset: u32,
}

const HEADER: usize = size_of::<Raw<CGxt2Header>>();
const ENTRY: usize = size_of::<Raw<CGxt2Entry>>();

/// One string of a table: its key's hash and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gxt2Entry {
    pub hash: u32,
    pub value: String,
}

/// A `.gxt2` opened as an owned value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gxt2 {
    /// Sorted by hash, as opening and `insert` keep them; a save sorts them again.
    pub entries: Vec<Gxt2Entry>,
    origin: Origin,
}

impl Heap for Gxt2 {
    fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Gxt2Entry>()
            + self
                .entries
                .iter()
                .map(|e| e.value.capacity())
                .sum::<usize>()
    }
}

impl Gxt2 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, hash: u32) -> Option<&str> {
        self.entries
            .binary_search_by(|e| e.hash.cmp(&hash))
            .ok()
            .map(|i| self.entries[i].value.as_str())
    }

    pub fn get_by_key(&self, key: &str) -> Option<&str> {
        self.get(string_hash_str(key))
    }

    /// Sets the string `hash` names, returning the one it replaced.
    pub fn insert(&mut self, hash: u32, value: impl Into<String>) -> Option<String> {
        match self.entries.binary_search_by(|e| e.hash.cmp(&hash)) {
            Ok(i) => Some(std::mem::replace(&mut self.entries[i].value, value.into())),
            Err(i) => {
                self.entries.insert(
                    i,
                    Gxt2Entry {
                        hash,
                        value: value.into(),
                    },
                );
                None
            }
        }
    }

    pub fn insert_key(&mut self, key: &str, value: impl Into<String>) -> Option<String> {
        self.insert(string_hash_str(key), value)
    }
}

impl HasOrigin for Gxt2 {
    fn origin(&self) -> &Origin {
        &self.origin
    }

    fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }
}

impl File for Gxt2 {
    const EXTENSION: &'static str = "gxt2";

    fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        let header = Raw::<CGxt2Header>::read_from(data).ok_or(Error::DataTooSmall {
            expected: HEADER,
            actual: data.len(),
        })?;
        if header.magic != GXT2_MAGIC {
            return Err(Error::InvalidData {
                context: format!(
                    "not a GXT2 file: expected magic {GXT2_MAGIC:#010X} ('GXT2'), found {:#010X}",
                    header.magic
                ),
            });
        }
        let count = header.count as usize;
        let strings_start = count
            .checked_add(1)
            .and_then(|n| n.checked_mul(ENTRY))
            .and_then(|n| n.checked_add(HEADER))
            .ok_or_else(|| Error::InvalidData {
                context: format!("GXT2 entry count {count} overflows"),
            })?;
        if data.len() < strings_start {
            return Err(Error::UnexpectedEof {
                offset: HEADER as u64,
                needed: strings_start,
                available: data.len(),
            });
        }
        let mut entries = Vec::with_capacity(count);
        for at in (HEADER..).step_by(ENTRY).take(count) {
            let entry = Raw::<CGxt2Entry>::read_from(&data[at..]).ok_or(Error::UnexpectedEof {
                offset: at as u64,
                needed: ENTRY,
                available: data.len() - at,
            })?;
            entries.push(Gxt2Entry {
                hash: entry.hash.get(),
                value: read_c_string(data, entry.offset as usize)?,
            });
        }
        entries.sort_by_key(|e| e.hash);
        Ok(Self {
            entries,
            origin: Origin::default(),
        })
    }

    /// Refused when two strings share a hash or a string holds a NUL, since the game would find only one.
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut entries: Vec<&Gxt2Entry> = self.entries.iter().collect();
        entries.sort_by_key(|e| e.hash);
        if let Some(pair) = entries.windows(2).find(|p| p[0].hash == p[1].hash) {
            return Err(Error::InvalidData {
                context: format!(
                    "two GXT2 strings share the hash {:#010X}, so the game would find only one",
                    pair[0].hash
                ),
            });
        }
        if let Some(e) = entries.iter().find(|e| e.value.contains('\0')) {
            return Err(Error::InvalidData {
                context: format!(
                    "the GXT2 string {:#010X} holds a NUL, so the game would read it cut short",
                    e.hash
                ),
            });
        }
        let strings_start = HEADER + (entries.len() + 1) * ENTRY;
        let end = strings_start + entries.iter().map(|e| e.value.len() + 1).sum::<usize>();
        let too_large = || Error::DataTooLarge {
            size: end,
            max: u32::MAX as usize,
        };
        let count = u32::try_from(entries.len()).map_err(|_| too_large())?;
        u32::try_from(end).map_err(|_| too_large())?;

        let mut out = Vec::with_capacity(end);
        let mut header = <Raw<CGxt2Header> as Plain>::zeroed();
        header.magic = GXT2_MAGIC;
        header.count = count;
        out.extend_from_slice(header.as_bytes());
        let mut offset = strings_start;
        // The last entry is the end of the strings, under the magic as its hash.
        let directory = entries
            .iter()
            .map(|e| (e.hash, e.value.len() + 1))
            .chain([(GXT2_MAGIC, 0)]);
        for (hash, len) in directory {
            let mut entry = <Raw<CGxt2Entry> as Plain>::zeroed();
            entry.hash = Hash32::from_raw(hash);
            entry.offset = offset as u32;
            out.extend_from_slice(entry.as_bytes());
            offset += len;
        }
        for e in &entries {
            out.extend_from_slice(e.value.as_bytes());
            out.push(0);
        }
        debug_assert_eq!(out.len(), end);
        Ok(out)
    }
}

fn read_c_string(data: &[u8], offset: usize) -> Result<String, Error> {
    let rest = data.get(offset..).ok_or(Error::PointerOutOfBounds {
        offset: offset as u64,
        size: data.len(),
    })?;
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(table: &Gxt2) -> Gxt2 {
        Gxt2::from_bytes(&table.to_bytes().expect("write")).expect("read")
    }

    #[test]
    fn empty_roundtrips() {
        assert!(roundtrip(&Gxt2::new()).is_empty());
    }

    #[test]
    fn keys_roundtrip_and_lookup() {
        let mut table = Gxt2::new();
        table.insert_key("PM_PAUSE_MAP", "Map");
        table.insert_key("CELL_EMPTY", "");
        table.insert_key("GREETING", "Hello, Los Santos");

        let back = roundtrip(&table);
        assert_eq!(back.len(), 3);
        assert_eq!(back.get_by_key("PM_PAUSE_MAP"), Some("Map"));
        assert_eq!(back.get_by_key("CELL_EMPTY"), Some(""));
        assert_eq!(back.get_by_key("GREETING"), Some("Hello, Los Santos"));
        assert_eq!(back.get_by_key("MISSING"), None);
    }

    #[test]
    fn directory_is_hash_sorted() {
        let mut table = Gxt2::new();
        table.entries = [3, 1, 2]
            .map(|hash| Gxt2Entry {
                hash,
                value: hash.to_string(),
            })
            .into();
        let bytes = table.to_bytes().expect("write");
        let hashes: Vec<u32> = (0..3)
            .map(|i| {
                Raw::<CGxt2Entry>::read_from(&bytes[HEADER + i * ENTRY..])
                    .unwrap()
                    .hash
                    .get()
            })
            .collect();
        assert_eq!(hashes, [1, 2, 3]);
    }

    #[test]
    fn utf8_values_survive() {
        let mut table = Gxt2::new();
        table.insert_key("ACCENT", "Peña — café ¡Hola!");
        assert_eq!(
            roundtrip(&table).get_by_key("ACCENT"),
            Some("Peña — café ¡Hola!")
        );
    }

    #[test]
    fn insert_replaces_existing() {
        let mut table = Gxt2::new();
        assert_eq!(table.insert_key("K", "first"), None);
        assert_eq!(table.insert_key("K", "second").as_deref(), Some("first"));
        assert_eq!(table.len(), 1);
        assert_eq!(table.get_by_key("K"), Some("second"));
    }

    #[test]
    fn a_table_the_game_would_misread_is_refused_at_save() {
        let mut shared = Gxt2::new();
        shared.entries = vec![
            Gxt2Entry {
                hash: 7,
                value: "a".into(),
            },
            Gxt2Entry {
                hash: 7,
                value: "b".into(),
            },
        ];
        assert!(shared.to_bytes().is_err(), "two strings under one hash");
        let mut nul = Gxt2::new();
        nul.insert(7, "a\0b");
        assert!(nul.to_bytes().is_err(), "a NUL inside a string");
    }
}

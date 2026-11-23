//! The table the word at 0x50 of a GTA VI headerless RBM header points at: a u32 count, then that
//! many distinct 16-byte entries that no other bytes of the virtual section repeat. What object an
//! entry names is not known, so the table is carried as read and never edited.

use std::collections::HashSet;

/// The header's word that points at the table.
const WORD: usize = 0x50;
const ENTRY: usize = 16;

/// What stops the table of the word at 0x50 from being read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdTableError {
    #[error(
        "the word at 0x50 is {0:#X}, which is neither zero nor a pointer into the virtual section"
    )]
    Pointer(u64),
    #[error(
        "the table the word at 0x50 points at ({at:#X}) runs past the {len}-byte virtual section"
    )]
    Past { at: usize, len: usize },
    #[error("the table the word at 0x50 points at states no entries")]
    Empty,
    #[error("the table the word at 0x50 points at holds {distinct} distinct entries of {count}, where every one stored holds each once")]
    Repeated { distinct: usize, count: usize },
    #[error("an entry of the table the word at 0x50 points at also stands at {0:#X} of the virtual section, so moving the table could leave a reference to it behind")]
    Elsewhere(usize),
}

/// The table at the word at 0x50: where it starts and its entries, in stored order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdTable {
    /// Its offset in the virtual section.
    pub at: usize,
    pub entries: Vec<[u8; ENTRY]>,
}

impl IdTable {
    /// Whether every entry has a version 1 or 4 UUID's version and RFC 4122 variant.
    // ⚠ About 1 in 32 random 16-byte values have them too, so this is said of the whole table.
    pub fn reads_as_uuids(&self) -> bool {
        self.entries.iter().all(is_uuid)
    }
}

/// Whether `e`, in GUID byte order, states version 1 or 4 in byte 7 and the variant `10` in byte 8.
pub fn is_uuid(e: &[u8; ENTRY]) -> bool {
    matches!(e[7] >> 4, 1 | 4) && e[8] >> 6 == 0b10
}

/// `e` as UUID text, its first three fields little-endian as GUID byte order stores them.
pub fn uuid_text(e: &[u8; ENTRY]) -> String {
    let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
    format!(
        "{:08x}-{:04x}-{:04x}-{}-{}",
        u32::from_le_bytes([e[0], e[1], e[2], e[3]]),
        u16::from_le_bytes([e[4], e[5]]),
        u16::from_le_bytes([e[6], e[7]]),
        hex(&e[8..10]),
        hex(&e[10..16])
    )
}

/// The table the word at 0x50 of `virtual_section` points at, `None` where the word is zero.
pub fn id_table(virtual_section: &[u8]) -> Result<Option<IdTable>, IdTableError> {
    let len = virtual_section.len();
    let word = (virtual_section.get(WORD..WORD + 8))
        .and_then(|b| b.try_into().ok())
        .map_or(0, u64::from_le_bytes);
    if word == 0 {
        return Ok(None);
    }
    if word >> 28 != 5 {
        return Err(IdTableError::Pointer(word));
    }
    let at = (word & 0x0FFF_FFFF) as usize;
    let count = (virtual_section.get(at..at.saturating_add(4)))
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .ok_or(IdTableError::Past { at, len })?;
    if count == 0 {
        return Err(IdTableError::Empty);
    }
    let end = (count.checked_mul(ENTRY))
        .and_then(|n| n.checked_add(at + 4))
        .filter(|&end| end <= len)
        .ok_or(IdTableError::Past { at, len })?;
    let entries: Vec<[u8; ENTRY]> = virtual_section[at + 4..end].as_chunks::<ENTRY>().0.to_vec();
    let distinct: HashSet<[u8; ENTRY]> = entries.iter().copied().collect();
    if distinct.len() != count {
        return Err(IdTableError::Repeated {
            distinct: distinct.len(),
            count,
        });
    }
    // A sorted list of each entry's first eight bytes keeps the scan to one lookup per offset.
    let mut heads: Vec<[u8; 8]> = (entries.iter())
        .filter_map(|e| e.first_chunk::<8>().copied())
        .collect();
    heads.sort_unstable();
    for (i, window) in virtual_section.windows(ENTRY).enumerate() {
        if (at..end).contains(&i) {
            continue;
        }
        let (Ok(window), Some(head)) =
            (<&[u8; ENTRY]>::try_from(window), window.first_chunk::<8>())
        else {
            continue;
        };
        if heads.binary_search(head).is_ok() && distinct.contains(window) {
            return Err(IdTableError::Elsewhere(i));
        }
    }
    Ok(Some(IdTable { at, entries }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIRTUAL: u64 = 0x5000_0000;

    /// A version 4 UUID with the RFC variant, in GUID byte order, made distinct by `n`.
    fn uuid(n: u8) -> [u8; ENTRY] {
        let mut e = [n; ENTRY];
        e[7] = 0x4A;
        e[8] = 0x9B;
        e
    }

    /// A virtual section of 0x100 bytes whose word at 0x50 points at `entries` at 0xA4.
    fn section(entries: &[[u8; ENTRY]]) -> Vec<u8> {
        let mut v = vec![0u8; 0xA4];
        v[WORD..WORD + 8].copy_from_slice(&(VIRTUAL + 0xA4).to_le_bytes());
        v.extend((entries.len() as u32).to_le_bytes());
        v.extend(entries.iter().flatten());
        v.resize(v.len().max(0x100), 0);
        v
    }

    #[test]
    fn the_table_reads_its_entries_and_a_zero_word_points_at_none() {
        let entries = [uuid(1), uuid(2), uuid(3)];
        let table = id_table(&section(&entries))
            .expect("it reads")
            .expect("a table");
        assert_eq!(table.at, 0xA4);
        assert_eq!(table.entries, entries);
        assert!(table.reads_as_uuids());
        assert_eq!(id_table(&[0u8; 0x60]), Ok(None));
    }

    #[test]
    fn one_entry_without_a_uuids_version_makes_the_whole_table_bytes() {
        let mut other = uuid(9);
        other[7] = 0x2A;
        assert!(!is_uuid(&other));
        let table = id_table(&section(&[uuid(1), other]))
            .expect("it reads")
            .expect("a table");
        assert!(!table.reads_as_uuids());
    }

    #[test]
    fn an_entry_reads_as_guid_ordered_uuid_text() {
        let e = [
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x46, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ];
        assert!(is_uuid(&e));
        assert_eq!(uuid_text(&e), "00112233-4455-4677-8899-aabbccddeeff");
    }

    #[test]
    fn a_table_that_breaks_a_rule_is_refused_by_it() {
        let mut v = section(&[uuid(1), uuid(2)]);
        v[WORD..WORD + 8].copy_from_slice(&0x6000_0000u64.to_le_bytes());
        assert_eq!(id_table(&v), Err(IdTableError::Pointer(0x6000_0000)));

        assert_eq!(id_table(&section(&[])), Err(IdTableError::Empty));

        assert_eq!(
            id_table(&section(&[uuid(1), uuid(1)])),
            Err(IdTableError::Repeated {
                distinct: 1,
                count: 2
            })
        );

        let mut v = section(&[uuid(1), uuid(2)]);
        v[0xA4..0xA8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            id_table(&v),
            Err(IdTableError::Past {
                at: 0xA4,
                len: v.len()
            })
        );

        let mut v = section(&[uuid(1), uuid(2)]);
        v[0xE0..0xF0].copy_from_slice(&uuid(2));
        assert_eq!(id_table(&v), Err(IdTableError::Elsewhere(0xE0)));
    }
}

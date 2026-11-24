//! The entries a decrypted table of a GTA VI archive lists, by name. An entry is named by the
//! 64-bit hash of its path inside the archive: with its extension for a plain file, without one
//! for a resource or a nested archive.
//!
//! ⚠ A key snapshot deciphers a top-level archive's table alone: a nested archive's own table and
//! a ciphered entry's bytes stay closed.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use or_io::container::rpf::{GtaViKeys, Rpf9Group, Rpf9Toc};
use or_io::gta6::hash::name_hash64;

/// The column of an entry's place: its offset and kind, its stored length and its stated one.
const PLACE: u64 = 0x33F7_0DFA_F2C5_0989;
/// The column of the 64-bit hash of an entry's path.
const WORD: u64 = 0x73BC_B72D_CE74_F11F;
/// The column of the 32-bit hash of a file's or a resource's path.
const NAME: u64 = 0xB707_6B62_45D3_5FB5;
/// The column of the cipher an entry's bytes are under.
const CIPHER: u64 = 0x2860_30A3_58A7_89FB;
/// The column that counts the 64 KiB chunks a deflated entry inflates to.
const CHUNKS: u64 = 0xE0C1_2D7A_5D48_84C5;
/// The unit an entry's offset and stored length are counted in.
const SLOT: u64 = 16;
/// A table longer than this is not read.
const TABLE_LIMIT: u64 = 64 << 20;

/// A nested archive, by the kind byte of its place.
pub const KIND_ARCHIVE: u8 = 0x00;
/// A resource.
pub const KIND_RESOURCE: u8 = 0x0B;
/// A plain file.
pub const KIND_FILE: u8 = 0xFF;

/// Why a decrypted table gives no entries.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TableError {
    #[error("the table has no column {0:#018X}")]
    NoColumn(u64),
    #[error("the column {column:#018X} has rows of {found} bytes, where every measured table has {expected}")]
    Width {
        column: u64,
        found: u16,
        expected: u16,
    },
    #[error("the table's index runs past its {0} bytes")]
    IndexPastEnd(usize),
    #[error("entry {entry} names row {row} of the column {column:#018X}, which has {rows}")]
    Row {
        entry: u32,
        column: u64,
        row: u32,
        rows: u32,
    },
}

/// One entry of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The 64-bit hash of its path.
    pub word: u64,
    /// The 32-bit hash of its path, which a nested archive does not state.
    pub name: Option<u32>,
    /// Where its bytes start in the archive.
    pub offset: u64,
    /// Its length as stored, a whole number of 16-byte slots.
    pub stored: u64,
    /// The length it states: its inflated length, or its own where it is stored as it is.
    pub stated: u64,
    /// [`KIND_ARCHIVE`], [`KIND_RESOURCE`] or [`KIND_FILE`].
    pub kind: u8,
    /// Whether its bytes are under a cipher of their own.
    pub ciphered: bool,
    /// The 64 KiB chunks it inflates to; 0 where it is not deflated.
    pub chunks: u16,
}

/// The entries of one archive, sorted by the hash of their path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    entries: Vec<Entry>,
}

fn round16(x: usize) -> usize {
    (x + 15) & !15
}

fn le<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at + N)?.try_into().ok()
}

/// One column of a table: its rows, and where each entry's index into it lies.
struct Column<'t> {
    hash: u64,
    rows: &'t [u8],
    width: usize,
    count: u32,
    /// Where entry 0's index lies, and how far apart two entries' are.
    index: (usize, usize),
}

impl<'t> Column<'t> {
    /// The row entry `entry` has in this column; `None` where it has none.
    fn row(&self, bytes: &[u8], entry: u32, wide: bool) -> Result<Option<&'t [u8]>, TableError> {
        let at = self.index.0 + self.index.1 * entry as usize;
        let past = TableError::IndexPastEnd(bytes.len());
        let (row, none) = match wide {
            true => (
                u32::from(u16::from_le_bytes(le(bytes, at).ok_or(past)?)),
                0xFFFF,
            ),
            false => (u32::from(*bytes.get(at).ok_or(past)?), 0xFF),
        };
        if row == none {
            return Ok(None);
        }
        let from = self.width * row as usize;
        match self.rows.get(from..from + self.width) {
            Some(bytes) if row < self.count => Ok(Some(bytes)),
            _ => Err(TableError::Row {
                entry,
                column: self.hash,
                row,
                rows: self.count,
            }),
        }
    }
}

/// The row `entry` has in `column`; `None` where the table has no such column or the entry no row.
fn row_of<'t>(
    column: Option<&Column<'t>>,
    bytes: &[u8],
    entry: u32,
    wide: bool,
) -> Result<Option<&'t [u8]>, TableError> {
    match column {
        Some(column) => column.row(bytes, entry, wide),
        None => Ok(None),
    }
}

impl Table {
    /// The entries the decrypted table `toc` lists; an entry that states no place or no name is left out.
    // The index is one row an entry: an index into each column of the main region, in the columns'
    // order, after the column descriptors; and one into each external column, at the split.
    pub fn of(toc: &Rpf9Toc) -> Result<Self, TableError> {
        let bytes = toc.bytes.as_slice();
        let past = || TableError::IndexPastEnd(bytes.len());
        let records = usize::from(u16::from_le_bytes(le(bytes, 0x1A).ok_or_else(past)?));
        let metadata = u32::from_le_bytes(le(bytes, 0x1C).ok_or_else(past)?) as usize;
        let split = u32::from_le_bytes(le(bytes, 0x08).ok_or_else(past)?) as usize;
        let wide = match bytes.get(0x2C) {
            Some(1) => false,
            Some(2) => true,
            _ => return Err(past()),
        };
        let width = if wide { 2 } else { 1 };
        let main = round16(0x40 + round16(8 * records) + metadata) + 24 * toc.groups.len();
        let externals = toc.groups.iter().filter(|g| g.external).count();
        let mains = toc.groups.len() - externals;
        let (mut next_main, mut next_external) = (0, 0);
        let mut columns = Vec::with_capacity(toc.groups.len());
        for group in &toc.groups {
            let Rpf9Group {
                name_hash,
                count,
                width: row_width,
                external,
                data,
            } = group;
            let index = match *external {
                true => {
                    next_external += 1;
                    (split + width * (next_external - 1), width * externals)
                }
                false => {
                    next_main += 1;
                    (main + width * (next_main - 1), width * mains)
                }
            };
            columns.push(Column {
                hash: *name_hash,
                rows: bytes.get(data.clone()).ok_or_else(past)?,
                width: usize::from(*row_width),
                count: *count,
                index,
            });
        }
        let column = |hash: u64, expected: u16, needed: bool| match columns
            .iter()
            .find(|c| c.hash == hash)
        {
            Some(c) if c.width != usize::from(expected) => Err(TableError::Width {
                column: hash,
                found: c.width as u16,
                expected,
            }),
            None if needed => Err(TableError::NoColumn(hash)),
            found => Ok(found),
        };
        let place = column(PLACE, 24, true)?;
        let word = column(WORD, 8, true)?;
        let name = column(NAME, 4, false)?;
        let cipher = column(CIPHER, 64, false)?;
        let chunks = column(CHUNKS, 8, false)?;
        let row = |c, entry| row_of(c, bytes, entry, wide);
        let mut entries = Vec::with_capacity(toc.entry_count as usize);
        for entry in 0..toc.entry_count {
            let (Some(place), Some(word)) = (row(place, entry)?, row(word, entry)?) else {
                continue;
            };
            let at = |k: usize| u64::from_le_bytes(le(place, 8 * k).unwrap_or_default());
            let first = at(0);
            entries.push(Entry {
                word: u64::from_le_bytes(le(word, 0).unwrap_or_default()),
                name: row(name, entry)?.and_then(|n| le(n, 0).map(u32::from_le_bytes)),
                offset: (first & 0xFF_FFFF_FFFF) * SLOT,
                stored: at(1).saturating_mul(SLOT),
                stated: at(2),
                kind: (first >> 40) as u8,
                ciphered: row(cipher, entry)?.is_some(),
                chunks: (row(chunks, entry)?)
                    .and_then(|c| le(c, 4).map(u16::from_le_bytes))
                    .unwrap_or(0),
            });
        }
        Ok(Self::of_entries(entries))
    }

    /// A table of `entries`, given in any order.
    pub fn of_entries(mut entries: Vec<Entry>) -> Self {
        entries.sort_unstable_by_key(|e| (e.word, e.offset));
        Self { entries }
    }

    /// The table of the archive at `archive`, deciphered with `keys`.
    pub fn open(archive: &Path, keys: &GtaViKeys) -> io::Result<Self> {
        let mut file = File::open(archive)?;
        let mut head = [0u8; 8];
        file.read_exact(&mut head)?;
        let end = u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
        if !(64..=TABLE_LIMIT).contains(&end) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the archive states a table of {end} bytes"),
            ));
        }
        let mut bytes = head.to_vec();
        bytes.resize(end as usize, 0);
        file.read_exact(&mut bytes[8..])?;
        let toc = Rpf9Toc::decrypt(&bytes, keys).map_err(io::Error::other)?;
        Self::of(&toc).map_err(io::Error::other)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry whose path hashes to `word`.
    pub fn by_word(&self, word: u64) -> Option<&Entry> {
        let at = self.entries.partition_point(|e| e.word < word);
        self.entries.get(at).filter(|e| e.word == word)
    }

    /// The entry of the path `path` inside the archive, in any case: with its extension for a plain file, without one for a resource or a nested archive.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        self.by_word(name_hash64(path.as_bytes()))
    }

    /// The entry a registered file names: its path with no device, as a resource or a nested archive is named and then as a plain file is.
    pub fn find_registered(&self, filename: &str) -> Option<&Entry> {
        let path = entry_path(filename);
        let (bare, extension) = split_extension(path);
        (!extension.is_empty())
            .then(|| self.find(bare))
            .flatten()
            .or_else(|| self.find(path))
    }
}

/// A registered file's path inside its archive: what follows its device or its path macro.
pub fn entry_path(filename: &str) -> &str {
    if let Some(rest) = filename.strip_prefix("$(") {
        return rest.split_once(")/").map_or(filename, |(_, path)| path);
    }
    filename.split_once(":/").map_or(filename, |(_, path)| path)
}

/// A path without the extension of its last component, and that extension.
pub fn split_extension(path: &str) -> (&str, &str) {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    match leaf.rsplit_once('.') {
        Some((_, extension)) => (&path[..path.len() - extension.len() - 1], extension),
        None => (path, ""),
    }
}

/// Where the entry of the path `path` starts in the top-level archive at `archive`; `None` with no key snapshot, a table that does not decipher, or no such entry.
pub fn entry_offset(archive: &Path, path: &str) -> Option<u64> {
    let keys = GtaViKeys::for_install(crate::config::config_dir().as_deref())
        .inspect_err(|error| tracing::info!(%error, "the GTA VI key snapshot is not read"))
        .ok()??;
    let table = Table::open(archive, &keys)
        .inspect_err(|error| tracing::info!(path = %archive.display(), %error, "no table read"))
        .ok()?;
    table.find(path).map(|entry| entry.offset)
}

#[cfg(test)]
#[path = "gta6_mount_table_tests.rs"]
mod tests;

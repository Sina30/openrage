//! The RPF9 archive explorer: an entry listing over a decrypted table of contents.
//!
//! [`Rpf9Toc`] deciphers an RPF9's metadata and column index; this joins the columns into one row per
//! entry. The table is a column store: after the group directory comes one entry-major index array
//! (`entries x non-external-columns x index_width`), and a second array at the split word for the
//! external columns. Each index points into a column's deduplicated rows, or holds the all-ones
//! sentinel (0xFF / 0xFFFF) — a value at or past the column's row count — when the entry has no row
//! there. So entry `e`'s name64, name32, entry row and content-cipher record are read through that
//! index, not by pairing row `e` of one column with row `e` of another: the columns are deduplicated
//! and share no row order, which is why the old row-order pairing listed nothing for the four archives
//! whose name64 column is shorter than their entry count.
//!
//! Two entry-row shapes exist (a table may hold both; an entry has a row in exactly one). The 24-byte
//! row carries start, stored size, real size and a class byte; the 20-byte row, universal in the
//! nested tables, carries only a proven start. A column whose hash this reader does not know is left
//! undecoded rather than read as a row of a guessed shape, so an entry lists what the known columns
//! state and nothing invented.
//!
//! A [`NameResolver`] maps an entry's name64 back to its path, through the recovered-names vocabulary
//! and, as a shortcut, the per-build name tables; nested archive mounting lives in
//! [`super::gta6_mount`].

use std::collections::HashMap;
use std::path::Path;

use byteorder::{ByteOrder, LE};

use super::rpf9_toc::{GtaViKeys, Rpf9Group, Rpf9Toc};
use crate::error::Result;
use crate::gta6::names::Vocabulary;

/// The name64 column: 8-byte records, the Murmur3 x64_128 low-64 hash of each entry's lower-case
/// path, the engine's key for it.
pub const GROUP_NAME64: u64 = 0x73BC_B72D_CE74_F11F;
/// The name32 column: 4-byte records, the joaat of the same path.
pub const GROUP_NAME32: u64 = 0xB707_6B62_45D3_5FB5;
/// The 24-byte entry row: three u64 holding the start (16-byte units, with a class byte above bit 40),
/// the stored size (16-byte units, rounded up) and the real (inflated) size.
pub const GROUP_ENTRY_ROW: u64 = 0x33F7_0DFA_F2C5_0989;
/// The 20-byte entry row, the second row shape, universal in the nested tables: `[x, 3, size,
/// start/16, 0xB]`. Only the start at +12 is proven; the size word at +8 is not yet decoded.
pub const GROUP_ENTRY_ROW_20: u64 = 0xBA18_0AFB_232D_1B4B;
/// The content-cipher column: 64-byte records whose head is `{u32 keyId, u16 cipherType, byte
/// iv[16]}`. An entry with such a row has an enciphered body under a cipher this reader does not open.
pub const GROUP_CIPHER: u64 = 0x2860_30A3_58A7_89FB;

/// A content-cipher record: the key id, cipher type and IV an enciphered entry's body is under. The
/// body itself is not opened here; this marks the entry and carries what the record states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gta6Cipher {
    pub key_id: u32,
    pub cipher_type: u16,
    pub iv: [u8; 16],
}

/// One entry of an RPF9 archive, joined from the index columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gta6Entry {
    /// The entry's number, `0..entry_count`. Unlike a column row, it is the archive's own order.
    pub index: u32,
    /// The Murmur3 x64_128 low-64 hash of the entry's lower-case path; `0` for the few entries with
    /// no name64 row.
    pub name64: u64,
    /// The joaat of the same path, where the archive carries a name32 column for the entry.
    pub name32: Option<u32>,
    /// The recovered path, when a [`NameResolver`] has it; the hash alone otherwise.
    pub name: Option<String>,
    /// The entry's start, in bytes, relative to this archive's own header (absolute for a top-level
    /// archive); `None` when the entry has no entry row.
    pub start: Option<u64>,
    /// The stored (on-disk) size in bytes, rounded up to 16; `None` for a 20-byte row, whose size is
    /// not yet decoded, or no row.
    pub stored: Option<u64>,
    /// The real (inflated) size in bytes; `None` for a 20-byte row or no row.
    pub real: Option<u64>,
    /// The class byte of the 24-byte row (`0x00`, `0x0B`, `0xFF`, `0x0E` seen); its meaning is not
    /// measured. `None` for a 20-byte row or no row.
    pub class: Option<u8>,
    /// The content-cipher record, when the entry's body is enciphered.
    pub cipher: Option<Gta6Cipher>,
}

impl Gta6Entry {
    /// The entry's real (inflated) size as a `u32`, for the listing views; `None` when it has no
    /// entry row or the size does not fit a `u32`. The full value is [`Self::real`].
    pub fn size(&self) -> Option<u32> {
        self.real.and_then(|r| u32::try_from(r).ok())
    }

    /// The entry's recovered path, or `hash_{name64:016x}` where it never resolved — the fallback a
    /// listing shows, as the engine falls back when its own vocabulary misses.
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("hash_{:016x}", self.name64),
        }
    }
}

/// An opened RPF9 archive's listing: its decrypted table of contents and the entries read from it.
#[derive(Debug, Clone)]
pub struct Gta6Archive {
    /// The decrypted table of contents: the header bytes, the key ids, the groups.
    pub toc: Rpf9Toc,
    /// The entries, one per `entry_count`, in the archive's own order.
    pub entries: Vec<Gta6Entry>,
}

impl Gta6Archive {
    /// Open the RPF9 archive at the start of `archive`, deciphering its table of contents with
    /// `keys`. `archive` need only hold the header and table of contents.
    pub fn open(archive: &[u8], keys: &GtaViKeys) -> Result<Self> {
        let toc = Rpf9Toc::decrypt(archive, keys)?;
        let entries = read_entries(&toc);
        Ok(Self { toc, entries })
    }

    /// Fill in each entry's name from `resolver`, where it has the hash. Entries it does not know
    /// keep `None` and show as their hash.
    pub fn resolve_names(&mut self, resolver: &NameResolver) {
        for entry in &mut self.entries {
            if entry.name.is_none() {
                entry.name = resolver.lookup(entry.name64).map(str::to_owned);
            }
        }
    }

    /// The archive's index groups, in order.
    pub fn groups(&self) -> &[Rpf9Group] {
        &self.toc.groups
    }

    /// The columns this reader does not decode into an entry row, by hash and width: the index key,
    /// the per-resource external columns and any column set a future row shape would need. A table
    /// whose entries lack a known entry-row shape shows it here, so a census or a test can name what
    /// was not read rather than silently listing those entries without a start.
    pub fn unknown_columns(&self) -> Vec<(u64, u16)> {
        self.toc
            .groups
            .iter()
            .filter(|g| {
                !matches!(
                    (g.name_hash, g.width),
                    (GROUP_NAME64, 8)
                        | (GROUP_NAME32, 4)
                        | (GROUP_ENTRY_ROW, 24)
                        | (GROUP_ENTRY_ROW_20, 20)
                        | (GROUP_CIPHER, 64)
                )
            })
            .map(|g| (g.name_hash, g.width))
            .collect()
    }
}

fn round16(x: usize) -> usize {
    (x + 15) & !15
}

/// One column of the decrypted index, with the entry-major index-array parameters that locate the
/// row of a given entry. Reading is bounds-checked: a wrong offset yields `None`, never a panic.
struct Column {
    /// The column's records, `[data_start, data_start + count*width)` within the TOC bytes.
    data_start: usize,
    width: usize,
    count: usize,
    /// The start of this column's kind's index array (the main array, or the split for an external
    /// column).
    index_base: usize,
    /// Bytes an entry occupies in that index array (`columns_of_this_kind * index_width`).
    index_stride: usize,
    /// This column's position among its kind, in records.
    slot: usize,
    index_width: usize,
}

impl Column {
    /// Entry `e`'s record in this column, through the index array; `None` when the entry has no row
    /// (the sentinel, a value at or past the row count) or any offset falls outside the TOC bytes.
    fn row<'a>(&self, bytes: &'a [u8], e: usize) -> Option<&'a [u8]> {
        let at = self.index_base + e * self.index_stride + self.slot * self.index_width;
        let raw = bytes.get(at..at + self.index_width)?;
        let row = match self.index_width {
            1 => raw[0] as usize,
            _ => LE::read_u16(raw) as usize,
        };
        if row >= self.count {
            return None;
        }
        let o = self.data_start + row * self.width;
        bytes.get(o..o + self.width)
    }
}

/// The columns this reader knows, located for the archive's index arrays.
#[derive(Default)]
struct KnownColumns {
    name64: Option<Column>,
    name32: Option<Column>,
    row24: Option<Column>,
    row20: Option<Column>,
    cipher: Option<Column>,
}

/// Build one row per entry by joining the known columns through the entry-major index arrays. An
/// archive whose index layout is unreadable (an entry count of zero, an index width other than 1 or
/// 2) lists nothing rather than guessing.
fn read_entries(toc: &Rpf9Toc) -> Vec<Gta6Entry> {
    let bytes = &toc.bytes;
    // The header fields the index arrays are placed by; all lie in the first 0x40 bytes the decrypt
    // guaranteed present.
    let read_u16 = |at: usize| bytes.get(at..at + 2).map(LE::read_u16);
    let read_u32 = |at: usize| bytes.get(at..at + 4).map(LE::read_u32);
    let (Some(split), Some(n), Some(m), Some(group_count), Some(external_count), Some(entries)) = (
        read_u32(0x08),
        read_u16(0x1A),
        read_u32(0x1C),
        read_u16(0x20),
        read_u16(0x22),
        read_u32(0x24),
    ) else {
        return Vec::new();
    };
    let index_width = bytes.get(0x2C).copied().unwrap_or(0) as usize;
    let (entries, group_count, external_count) = (
        entries as usize,
        group_count as usize,
        external_count as usize,
    );
    if entries == 0 || !(1..=2).contains(&index_width) || external_count > group_count {
        return Vec::new();
    }

    let metadata_start = 0x40 + round16(8 * n as usize);
    let groups_start = round16(metadata_start + m as usize);
    let main_base = groups_start + 24 * group_count;
    let main_stride = (group_count - external_count) * index_width;
    let ext_base = split as usize;
    let ext_stride = external_count * index_width;

    // Each group's position among its kind, in directory order, gives its slot in the index array.
    let mut columns = KnownColumns::default();
    let (mut main_slot, mut ext_slot) = (0usize, 0usize);
    for group in &toc.groups {
        let slot = if group.external { ext_slot } else { main_slot };
        let (index_base, index_stride) = if group.external {
            (ext_base, ext_stride)
        } else {
            (main_base, main_stride)
        };
        let column = Column {
            data_start: group.data.start,
            width: group.width as usize,
            count: group.count as usize,
            index_base,
            index_stride,
            slot,
            index_width,
        };
        match (group.name_hash, group.width) {
            (GROUP_NAME64, 8) => columns.name64 = Some(column),
            (GROUP_NAME32, 4) => columns.name32 = Some(column),
            (GROUP_ENTRY_ROW, 24) => columns.row24 = Some(column),
            (GROUP_ENTRY_ROW_20, 20) => columns.row20 = Some(column),
            (GROUP_CIPHER, 64) => columns.cipher = Some(column),
            _ => {}
        }
        if group.external {
            ext_slot += 1;
        } else {
            main_slot += 1;
        }
    }

    (0..entries)
        .map(|e| join_entry(bytes, &columns, e))
        .collect()
}

/// Join entry `e`'s columns into one row.
fn join_entry(bytes: &[u8], columns: &KnownColumns, e: usize) -> Gta6Entry {
    let name64 = columns
        .name64
        .as_ref()
        .and_then(|c| c.row(bytes, e))
        .map(LE::read_u64)
        .unwrap_or(0);
    let name32 = columns
        .name32
        .as_ref()
        .and_then(|c| c.row(bytes, e))
        .map(LE::read_u32);
    let cipher = columns
        .cipher
        .as_ref()
        .and_then(|c| c.row(bytes, e))
        .map(read_cipher);

    // An entry has a row in exactly one of the two entry-row shapes; the 24-byte row is read in full,
    // the 20-byte row for its proven start alone.
    let mut start = None;
    let mut stored = None;
    let mut real = None;
    let mut class = None;
    if let Some(row) = columns.row24.as_ref().and_then(|c| c.row(bytes, e)) {
        let a = LE::read_u64(&row[0..]);
        let b = LE::read_u64(&row[8..]);
        let c = LE::read_u64(&row[16..]);
        start = Some((a & ((1u64 << 40) - 1)) * 16);
        class = Some(((a >> 40) & 0xFF) as u8);
        // A corrupt row can state a stored size that overflows 16-byte units; a malformed file must
        // not panic under overflow checks, so an unrepresentable size reads as absent.
        stored = b.checked_mul(16);
        real = Some(c);
    } else if let Some(row) = columns.row20.as_ref().and_then(|c| c.row(bytes, e)) {
        let start_units = u64::from(LE::read_u32(&row[12..]));
        start = Some(start_units * 16);
    }

    Gta6Entry {
        index: e as u32,
        name64,
        name32,
        name: None,
        start,
        stored,
        real,
        class,
        cipher,
    }
}

/// Read a content-cipher record from its 64-byte row.
fn read_cipher(row: &[u8]) -> Gta6Cipher {
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&row[6..22]);
    Gta6Cipher {
        key_id: LE::read_u32(&row[0..]),
        cipher_type: LE::read_u16(&row[4..]),
        iv,
    }
}

/// A map from an entry's name64 to its path.
///
/// Names are game-derived and never committed; a session supplies them at run time. The primary
/// source is the recovered-names [`Vocabulary`], which hashes each name with `name_hash64` as it
/// loads, so it resolves any build and reaches the nested tables. The per-build name tables
/// (`toc-entries-resolved.tsv`, `resolved-names.tsv`) stay an optional shortcut. With neither, the
/// resolver names nothing and every entry shows as its hash.
#[derive(Debug, Default, Clone)]
pub struct NameResolver {
    vocabulary: Vocabulary,
    shortcut: HashMap<u64, String>,
}

impl NameResolver {
    /// An empty resolver, which names nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// A resolver over `vocabulary`, the recovered-names corpus hashed by `name_hash64` at load.
    pub fn from_vocabulary(vocabulary: Vocabulary) -> Self {
        Self {
            vocabulary,
            shortcut: HashMap::new(),
        }
    }

    /// A resolver over just the `vocabulary.txt` under `dir`, with no shortcut tables. The shortcut
    /// tables are derived from one build's own tables and so resolve by construction; a caller that
    /// must measure resolution against the corpus alone uses this.
    pub fn load_vocabulary(dir: &Path) -> Self {
        Self::from_vocabulary(read_vocabulary(&dir.join("vocabulary.txt")))
    }

    /// A resolver over `dir`: the recovered-names `vocabulary.txt`, plus the per-build name tables as
    /// a shortcut where present. Any file that is absent is passed over, so a partial corpus works.
    pub fn load(dir: &Path) -> Self {
        let mut resolver = Self::load_vocabulary(dir);
        if let Ok(text) = std::fs::read_to_string(dir.join("toc-entries-resolved.tsv")) {
            resolver.add_toc_entries_tsv(&text);
        }
        if let Ok(text) = std::fs::read_to_string(dir.join("resolved-names.tsv")) {
            resolver.add_hash_name_tsv(&text);
        }
        resolver
    }

    /// Fold `other` into this resolver's vocabulary, so names a caller harvested elsewhere (its own
    /// config-dir vocabulary) resolve alongside the ones loaded from a folder.
    pub fn merge_vocabulary(&mut self, other: &Vocabulary) {
        self.vocabulary.merge(other);
    }

    /// Whether the resolver knows no names.
    pub fn is_empty(&self) -> bool {
        self.vocabulary.is_empty() && self.shortcut.is_empty()
    }

    /// How many names the resolver holds across both sources.
    pub fn len(&self) -> usize {
        self.vocabulary.len() + self.shortcut.len()
    }

    /// The path whose name64 is `hash`: the shortcut first, then the vocabulary.
    pub fn lookup(&self, hash: u64) -> Option<&str> {
        if let Some(name) = self.shortcut.get(&hash) {
            return Some(name);
        }
        self.vocabulary.name64(hash)
    }

    /// The path whose name32 is `hash`, from the vocabulary; `None` where two held names share it.
    pub fn lookup_name32(&self, hash: u32) -> Option<&str> {
        self.vocabulary.name32(hash)
    }

    /// How many of `entries` the resolver names, and how many there are: a coverage report.
    pub fn coverage(&self, entries: &[Gta6Entry]) -> (usize, usize) {
        let resolved = entries
            .iter()
            .filter(|e| self.lookup(e.name64).is_some())
            .count();
        (resolved, entries.len())
    }

    /// Record a `hash -> name` shortcut, keeping the first name seen for a hash.
    pub fn insert(&mut self, hash: u64, name: impl Into<String>) {
        self.shortcut.entry(hash).or_insert_with(|| name.into());
    }

    /// Add the entries of a `hash<TAB>name` table, as `resolved-names.tsv` holds them: the first
    /// field a `0x…` hash (8 or 16 hex digits), the second the path. Lines that do not parse are
    /// skipped, so a header row or a comment does no harm.
    pub fn add_hash_name_tsv(&mut self, text: &str) {
        for line in text.lines() {
            let mut fields = line.split('\t');
            let (Some(hash), Some(name)) = (fields.next(), fields.next()) else {
                continue;
            };
            if let (Some(hash), false) = (parse_hash(hash), name.is_empty()) {
                self.insert(hash, name);
            }
        }
    }

    /// Add the entries of the per-archive name table (`toc-entries-resolved.tsv`): a header row,
    /// then `archive<TAB>entry<TAB>name64<TAB>name32<TAB>name<TAB>source`. The name64 (column 3) is
    /// mapped to the name (column 5) when both are present; other rows are skipped.
    pub fn add_toc_entries_tsv(&mut self, text: &str) {
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let [_, _, name64, _, name, ..] = fields.as_slice() else {
                continue;
            };
            if let (Some(hash), false) = (parse_hash(name64), name.is_empty()) {
                self.insert(hash, *name);
            }
        }
    }
}

/// The vocabulary at `file`: the kept `openrage gta6 names 1` format where it is one, else a plain
/// list of one name per line (the shape the owner's recovered-names corpora take), each hashed with
/// `name_hash64` as [`Vocabulary::add`] takes it. Blank lines and `//` comments are skipped. A file
/// that is present but yields no name is warned about, since it then resolves nothing.
fn read_vocabulary(file: &Path) -> Vocabulary {
    let kept = Vocabulary::load(file);
    if !kept.is_empty() {
        return kept;
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return kept; // Absent or unreadable: an empty vocabulary, silently.
    };
    let mut vocabulary = Vocabulary::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        vocabulary.add(line, None);
    }
    if vocabulary.is_empty() {
        tracing::warn!(file = %file.display(), "names vocabulary held no usable names");
    }
    vocabulary
}

/// A `0x…` hash of 1 to 16 hex digits as a `u64`; `None` for anything else.
fn parse_hash(field: &str) -> Option<u64> {
    let digits = field
        .strip_prefix("0x")
        .or_else(|| field.strip_prefix("0X"))?;
    if !matches!(digits.len(), 1..=16) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
#[path = "gta6_archive_tests.rs"]
mod tests;

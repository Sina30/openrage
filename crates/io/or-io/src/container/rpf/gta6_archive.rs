//! The RPF9 archive explorer: a listing view over a decrypted table of contents.
//!
//! [`Rpf9Toc`] deciphers an RPF9's metadata and index; this turns that into an entry listing an
//! editor or a CLI can walk. Each entry carries its name64 — the Murmur3 x64_128 low-64 hash of its
//! lower-case path, which the engine keys the archive by — and its raw descriptor record. A
//! [`NameResolver`] maps a hash back to a path when a recovered-names corpus is present.
//!
//! ⚠ Only the index-level structure is established: the groups, the per-entry name64 column, and the
//! descriptor record's bytes. The descriptor's field *meanings* (compressed/uncompressed size, the
//! on-disk offset) are not recovered — the content itself is encrypted under a cipher type this
//! reader does not open — so offsets are not decoded and nested archives are found by their plaintext
//! `9FPR` header, not by a descriptor offset. A column's row count is at most the header's entry
//! count, because the engine deduplicates columns; the per-entry index that joins columns of unequal
//! length is not decoded, so this lists the name64 column's own rows (see [`read_entries`]).

use std::collections::HashMap;
use std::ops::Range;

use byteorder::{ByteOrder, LE};

use super::rpf9::{Rpf9Header, RPF9_HEADER_SIZE, RPF9_MAGIC};
use super::rpf9_toc::{GtaViKeys, Rpf9Group, Rpf9Toc};
use crate::error::Result;

/// The index group whose 8-byte records are each entry's name64 (Murmur3 x64_128 low 64 of the
/// lower-case path). Every archive measured carries it, one row per entry.
pub const GROUP_NAME64: u64 = 0x73BC_B72D_CE74_F11F;
/// The index group whose 24-byte records are each entry's descriptor. Its field layout beyond the
/// size magnitude at [`DESCRIPTOR_SIZE_AT`] is not established.
pub const GROUP_DESCRIPTOR: u64 = 0x33F7_0DFA_F2C5_0989;
/// The byte offset, within a descriptor record, of the u32 whose magnitude tracks the entry's size
/// (976 for a ~1 KiB config, tens of millions for a map blob). Named `size` provisionally: the unit
/// and whether a second field is a compressed size are not confirmed.
pub const DESCRIPTOR_SIZE_AT: usize = 16;

/// The most content bytes [`Gta6Archive::nested`] scans for a nested header. A larger archive (the
/// world and audio packs run to tens of gigabytes) is left unscanned rather than read whole.
pub const NESTED_SCAN_CAP: usize = 256 * 1024 * 1024;

/// One entry of an RPF9 archive, as the decrypted index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gta6Entry {
    /// The entry's row position in the name64 column. ⚠ Not the archive's entry number: the column
    /// is deduplicated, so a row maps back to an entry only through the per-entry index, which this
    /// reader does not decode.
    pub index: u32,
    /// The Murmur3 x64_128 low-64 hash of the entry's lower-case path; the engine's key for it.
    pub name64: u64,
    /// The recovered path, when a [`NameResolver`] has it; the hash alone otherwise.
    pub name: Option<String>,
    /// The entry's raw descriptor record; empty when the archive lists no descriptor column.
    pub descriptor: Vec<u8>,
}

impl Gta6Entry {
    /// The size the descriptor states, or `None` when there is no descriptor record. The field is
    /// the one at [`DESCRIPTOR_SIZE_AT`]; see that constant for why it is provisional.
    pub fn size(&self) -> Option<u32> {
        self.descriptor
            .get(DESCRIPTOR_SIZE_AT..DESCRIPTOR_SIZE_AT + 4)
            .map(LE::read_u32)
    }
}

/// Nested archives found inside an archive, each with its byte offset into the file.
pub type NestedArchives = Vec<(usize, Gta6Archive)>;

/// An opened RPF9 archive's listing: its decrypted table of contents and the entries read from it.
#[derive(Debug, Clone)]
pub struct Gta6Archive {
    /// The decrypted table of contents: the header bytes, the key ids, the groups.
    pub toc: Rpf9Toc,
    /// The entries, in name64-column order; empty when the index lists no name64 column.
    pub entries: Vec<Gta6Entry>,
}

impl Gta6Archive {
    /// Open the RPF9 archive at the start of `archive`, deciphering its table of contents with
    /// `keys`. `archive` need only hold the header and table of contents; the content after it is
    /// read only by [`Self::nested`].
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

    /// Where the table of contents ends and the content begins.
    pub fn content_start(&self) -> usize {
        self.toc.bytes.len()
    }

    /// Nested RPF9 archives held inside `archive`, each opened with `keys` and paired with its byte
    /// offset. `archive` is the whole file. The content is encrypted, but a nested archive's 64-byte
    /// `9FPR` header is in the clear, so this scans the content region for that header on the engine's
    /// 16-byte grid and keeps each hit whose header parses and whose table of contents deciphers —
    /// so a byte pattern that is not a real header is dropped, not mistaken for one.
    ///
    /// Returns `Ok(None)` when the content region is larger than [`NESTED_SCAN_CAP`], which says the
    /// archive was too large to scan rather than that it holds no nested archive.
    //
    // ponytail: bounded header scan. The descriptor's on-disk offset is not recovered, so there is no
    // table to jump to; a confirmed offset column would replace the scan with a direct read.
    pub fn nested(&self, archive: &[u8], keys: &GtaViKeys) -> Result<Option<NestedArchives>> {
        let start = self.content_start();
        let Some(region) = archive.get(start..) else {
            return Ok(Some(Vec::new()));
        };
        if region.len() > NESTED_SCAN_CAP {
            return Ok(None);
        }
        let mut found = Vec::new();
        let mut at = start;
        while at + RPF9_HEADER_SIZE <= archive.len() {
            if LE::read_u32(&archive[at..]) == RPF9_MAGIC
                && Rpf9Header::parse(&archive[at..]).is_ok()
            {
                if let Ok(child) = Gta6Archive::open(&archive[at..], keys) {
                    found.push((at, child));
                }
            }
            at += 16;
        }
        Ok(Some(found))
    }
}

/// Read the entries from a decrypted table of contents. The name64 column lists one row per distinct
/// entry name; its row count is at most the header's `entry_count`, because the engine deduplicates a
/// column, and it — not `entry_count` — is how many rows the listing holds. A descriptor record is
/// attached to a row by position only when the descriptor column has the same number of rows as the
/// name64 column, so the two are co-indexed; a descriptor column of another length is left unattached
/// rather than mis-joined, since the per-entry index that would join columns of unequal length is not
/// decoded (see the module header). An archive whose name64 column is absent or malformed lists no
/// entries rather than guessing.
fn read_entries(toc: &Rpf9Toc) -> Vec<Gta6Entry> {
    let Some((names, name_count)) = column(toc, GROUP_NAME64, 8) else {
        return Vec::new();
    };
    let descriptors = column(toc, GROUP_DESCRIPTOR, 24)
        .filter(|&(_, count)| count == name_count)
        .map(|(range, _)| range);
    (0..name_count)
        .map(|i| {
            let name64 = LE::read_u64(&toc.bytes[names.start + i * 8..]);
            let descriptor = descriptors
                .as_ref()
                .map(|d| {
                    let at = d.start + i * 24;
                    toc.bytes[at..at + 24].to_vec()
                })
                .unwrap_or_default();
            Gta6Entry {
                index: i as u32,
                name64,
                name: None,
                descriptor,
            }
        })
        .collect()
}

/// The byte range and row count of the group `name_hash`, when it is present with `width`-byte
/// records whose range fits the decrypted bytes. The count is the column's own — at most the header's
/// entry count — and is not assumed equal to it, because the engine deduplicates columns.
fn column(toc: &Rpf9Toc, name_hash: u64, width: u16) -> Option<(Range<usize>, usize)> {
    let group = toc.groups.iter().find(|g| g.name_hash == name_hash)?;
    if group.width != width {
        return None;
    }
    let count = group.count as usize;
    let need = count.checked_mul(width as usize)?;
    (group.data.end <= toc.bytes.len() && group.data.end - group.data.start >= need)
        .then(|| (group.data.clone(), count))
}

/// A map from a name hash to the path that hashes to it, read from a recovered-names corpus.
///
/// The corpus is game-derived and never committed; a session supplies it at runtime, and when none
/// is present the map is empty and every entry shows as its hash. Both the 64-bit name64 keys of the
/// per-archive listing and the 32-bit `joaat` keys of the parser-name tables live in the one map, so
/// a lookup by either width works.
#[derive(Debug, Default, Clone)]
pub struct NameResolver {
    by_hash: HashMap<u64, String>,
}

impl NameResolver {
    /// An empty resolver, which names nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the resolver knows no names.
    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// How many hashes the resolver maps.
    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    /// The path that hashes to `hash`, when the corpus has it.
    pub fn lookup(&self, hash: u64) -> Option<&str> {
        self.by_hash.get(&hash).map(String::as_str)
    }

    /// Record that `name` hashes to `hash`, keeping the first name seen for a hash.
    pub fn insert(&mut self, hash: u64, name: impl Into<String>) {
        self.by_hash.entry(hash).or_insert_with(|| name.into());
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
    /// mapped to the name (column 5) when both are present; other rows are skipped. This is the only
    /// corpus keyed by the archives' own name64, so it is what resolves an entry's hash.
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
mod tests {
    use super::*;

    #[test]
    fn parse_hash_reads_both_widths_and_refuses_junk() {
        assert_eq!(parse_hash("0x12FD3D26"), Some(0x12FD3D26));
        assert_eq!(
            parse_hash("0x092DF78629F7833A"),
            Some(0x092D_F786_29F7_833A)
        );
        assert_eq!(parse_hash("12FD3D26"), None); // no prefix
        assert_eq!(parse_hash("0xZZ"), None);
        assert_eq!(parse_hash("anim"), None);
        assert_eq!(parse_hash("0x"), None);
    }

    #[test]
    fn hash_name_tsv_maps_the_hash_to_the_path() {
        let mut r = NameResolver::new();
        r.add_hash_name_tsv("0x12FD3D26\tconfig.dat\n0x0019CF52\tmovies/foo.bk2\n");
        assert_eq!(r.lookup(0x12FD3D26), Some("config.dat"));
        assert_eq!(r.lookup(0x0019CF52), Some("movies/foo.bk2"));
        assert_eq!(r.lookup(0xDEAD), None);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn toc_entries_tsv_maps_name64_to_the_path_and_skips_the_header() {
        let mut r = NameResolver::new();
        let text = "archive\tentry\tname64\tname32\tname\tsource\n\
                    appdata0\t0\t0x092DF78629F7833A\t0x12FD3D26\tconfig.dat\tstrings\n\
                    textures\t6\t0x45AF281108824644\t\t\t\n";
        r.add_toc_entries_tsv(text);
        // The named row resolves by its name64; the empty-name row is skipped.
        assert_eq!(r.lookup(0x092D_F786_29F7_833A), Some("config.dat"));
        assert_eq!(r.lookup(0x45AF_2811_0882_4644), None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn insert_keeps_the_first_name_for_a_hash() {
        let mut r = NameResolver::new();
        r.insert(1, "first");
        r.insert(1, "second");
        assert_eq!(r.lookup(1), Some("first"));
    }

    /// A synthetic TOC whose name64 column is shorter than `entry_count`: the listing holds the
    /// column's own rows, and an empty resolver leaves every name unresolved.
    #[test]
    fn a_short_name_column_lists_its_rows_and_an_empty_resolver_resolves_none() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x1111_2222_3333_4444u64.to_le_bytes());
        bytes.extend_from_slice(&0x5555_6666_7777_8888u64.to_le_bytes());
        let toc = Rpf9Toc {
            bytes,
            key_id: 0,
            registry_id: 0,
            // The header claims three entries; the column deduplicates to two rows.
            entry_count: 3,
            groups: vec![Rpf9Group {
                name_hash: GROUP_NAME64,
                count: 2,
                width: 8,
                external: false,
                data: 0..16,
            }],
        };
        let mut archive = Gta6Archive {
            entries: read_entries(&toc),
            toc,
        };
        assert_eq!(
            archive.entries.len(),
            2,
            "lists the column's rows, not entry_count"
        );
        assert_eq!(archive.entries[0].name64, 0x1111_2222_3333_4444);
        assert!(archive.entries.iter().all(|e| e.descriptor.is_empty()));
        archive.resolve_names(&NameResolver::new());
        assert!(archive.entries.iter().all(|e| e.name.is_none()));
    }
}

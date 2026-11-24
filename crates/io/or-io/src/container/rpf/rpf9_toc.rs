//! RPF9's table of contents (GTA VI): a white-box table cipher chained like CBC, its key material
//! read from a snapshot the user's own executable yields. The 64-byte header is plaintext (see
//! [`Rpf9Header`]); the metadata table and the index after it are deciphered here so an RPF9 archive
//! lists its entries at the index level. File-content decryption, filename recovery, nested archives
//! and non-index cipher types stay out of this.
//!
//! ⚠ The transform and its constants — table sizes, the header offsets, the CBC and white-box math,
//! the registry-id formula — are the algorithm, committed per the owner's "truth from the
//! executable" ruling. The key bytes are not: the snapshot that holds them (`gtavi_magic.dat`, or a
//! path [`GTA6_SNAPSHOT_ENV`] names) is game-derived and never committed, and nothing here bundles it.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};

use byteorder::{BigEndian, ByteOrder, LE};

use super::gta6_keygate;
use super::rpf9::Rpf9Header;
use crate::error::{IoError, Result};

/// The environment variable that names a user's GTA VI key snapshot.
pub const GTA6_SNAPSHOT_ENV: &str = "OPENRAGE_GTA6_KEYS";
/// The file a user's GTA VI snapshot is looked up under, beside the keys cache.
pub const GTA6_SNAPSHOT_FILE_NAME: &str = "gtavi_magic.dat";
/// The environment variable that names the GTA VI executable, which unmasks a masked snapshot
/// (`ORGTAVIM`). A plaintext snapshot needs no executable; only a masked one, which may be committed
/// because it is inert without the user's own executable (D18, [`gta6_keygate`]).
pub const GTA6_EXE_ENV: &str = "OPENRAGE_GTA6_EXE";

/// `ORGTAVIK`: the snapshot container's magic.
const SNAPSHOT_MAGIC: &[u8; 8] = b"ORGTAVIK";
/// The container format version this loader reads.
const SNAPSHOT_VERSION: u32 = 1;
/// The mode tag `'BC'`: the TOC cipher runs in CBC.
const SNAPSHOT_MODE_CBC: u32 = 0x4342;
/// The marker each per-archive key blob opens with.
const BLOB_MAGIC: [u8; 4] = [0x8e, 0x9b, 0x3b, 0x4e];
/// A key blob: the marker, 60 round-key words, then sixteen 256-byte output maps.
const BLOB_LEN: usize = 4 + WORDS * 4 + MAPS * MAP_LEN;
/// Round-key words in a blob; the first four key the input, the rest the thirteen rounds.
const WORDS: usize = 60;
/// Byte substitution tables (input and, per blob, output), one per block byte.
const MAPS: usize = 16;
const MAP_LEN: usize = 256;
/// The shared input byte tables, 16 of 256 bytes.
const INPUT_TABLES_LEN: usize = MAPS * MAP_LEN;
/// The shared round word tables, 16 of 256 `u32`.
const ROUND_TABLES_LEN: usize = MAPS * MAP_LEN * 4;
/// Rounds of the T-box transform between the input and output substitutions.
const ROUNDS: usize = 13;

/// The registry id the bootstrap pass over the metadata table uses, the same for every archive.
const BOOTSTRAP_REGISTRY_ID: u64 = 0x1000_0001_0200_0000;
/// The index cipher type this reader opens; others are not supported.
const INDEX_CIPHER_TYPE: u16 = 16;

/// A per-archive white-box key: the round-key words and the output byte maps.
struct KeyBlob {
    words: [u32; WORDS],
    output: Box<[[u8; MAP_LEN]; MAPS]>,
}

/// A GTA VI key snapshot: the shared cipher tables, the bootstrap IV, and a key per registry id.
///
/// The parsed container is the plaintext `ORGTAVIK` snapshot. A snapshot may also be committed masked
/// (`ORGTAVIM`), inert until [`from_file`](Self::from_file) unmasks it with the executable
/// [`GTA6_EXE_ENV`] names (D18, [`gta6_keygate`]). The exe SHA-256 it records says which build it was
/// taken from.
pub struct GtaViKeys {
    exe_sha256: [u8; 32],
    bootstrap_iv: [u8; 16],
    input_tables: Box<[[u8; MAP_LEN]; MAPS]>,
    round_tables: Box<[[u32; MAP_LEN]; MAPS]>,
    blobs: BTreeMap<u64, KeyBlob>,
    snapshot_file: Option<PathBuf>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let slice = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or_else(|| snapshot_short(self.at + n, self.bytes.len()))?;
        self.at += n;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(LE::read_u32(self.take(4)?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(LE::read_u64(self.take(8)?))
    }
}

fn snapshot_short(want: usize, have: usize) -> IoError {
    IoError::Invalid {
        what: "a GTA VI key snapshot".into(),
        why: format!("it ends at {have} bytes, before the {want} the format states"),
    }
}

fn snapshot_bad(why: impl Into<String>) -> IoError {
    IoError::Invalid {
        what: "a GTA VI key snapshot".into(),
        why: why.into(),
    }
}

/// The plaintext of `bytes` when it is a masked (`ORGTAVIM`) snapshot, decoded with the executable
/// [`GTA6_EXE_ENV`] names; `None` when `bytes` is already plaintext, so the caller parses it as read.
fn unmask_if_needed(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    unmask_with(bytes, std::env::var_os(GTA6_EXE_ENV).as_deref())
}

/// The plaintext of `bytes` when it is a masked (`ORGTAVIM`) snapshot, decoded with the executable at
/// `exe_path`; `None` when `bytes` is already plaintext. A masked snapshot with no executable to key
/// it is refused rather than parsed as noise. Split out from [`unmask_if_needed`] so the masked path
/// is testable without the environment.
fn unmask_with(bytes: &[u8], exe_path: Option<&OsStr>) -> Result<Option<Vec<u8>>> {
    if !gta6_keygate::is_masked(bytes) {
        return Ok(None);
    }
    let exe_path = exe_path.ok_or_else(|| {
        snapshot_bad(format!(
            "it is masked and needs the GTA VI executable to open; set {GTA6_EXE_ENV} to its path"
        ))
    })?;
    let exe = std::fs::read(exe_path).map_err(|source| IoError::FileReadFailed {
        path: PathBuf::from(exe_path),
        source,
    })?;
    gta6_keygate::decode_masked(bytes, &exe).map(Some)
}

impl GtaViKeys {
    /// Parse an `ORGTAVIK` snapshot container; refused, never panicking, on any malformed field.
    pub fn parse(snapshot: &[u8]) -> Result<Self> {
        let mut c = Cursor {
            bytes: snapshot,
            at: 0,
        };
        if gta6_keygate::is_masked(snapshot) {
            return Err(snapshot_bad(
                "it is a masked (`ORGTAVIM`) snapshot; open it with `from_file` and the GTA VI \
                 executable, which parse does not have",
            ));
        }
        if c.take(8)? != SNAPSHOT_MAGIC {
            return Err(snapshot_bad("the magic is not `ORGTAVIK`"));
        }
        let version = c.u32()?;
        if version != SNAPSHOT_VERSION {
            return Err(snapshot_bad(format!(
                "version {version}, where this reader knows {SNAPSHOT_VERSION}"
            )));
        }
        let mode = c.u32()?;
        if mode != SNAPSHOT_MODE_CBC {
            return Err(snapshot_bad(format!(
                "mode tag {mode:#06X}, not the CBC tag {SNAPSHOT_MODE_CBC:#06X}"
            )));
        }
        let mut exe_sha256 = [0u8; 32];
        exe_sha256.copy_from_slice(c.take(32)?);
        let mut bootstrap_iv = [0u8; 16];
        bootstrap_iv.copy_from_slice(c.take(16)?);

        let input_len = c.u32()? as usize;
        if input_len != INPUT_TABLES_LEN {
            return Err(snapshot_bad(format!(
                "input tables are {input_len} bytes, not {INPUT_TABLES_LEN}"
            )));
        }
        let input_tables = read_byte_tables(c.take(INPUT_TABLES_LEN)?);

        let round_len = c.u32()? as usize;
        if round_len != ROUND_TABLES_LEN {
            return Err(snapshot_bad(format!(
                "round tables are {round_len} bytes, not {ROUND_TABLES_LEN}"
            )));
        }
        let round_tables = read_word_tables(c.take(ROUND_TABLES_LEN)?);

        let count = c.u32()? as usize;
        let mut blobs = BTreeMap::new();
        for _ in 0..count {
            let registry_id = c.u64()?;
            let blob_len = c.u32()? as usize;
            if blob_len != BLOB_LEN {
                return Err(snapshot_bad(format!(
                    "a key blob is {blob_len} bytes, not {BLOB_LEN}"
                )));
            }
            let blob = c.take(BLOB_LEN)?;
            if blob[..4] != BLOB_MAGIC {
                return Err(snapshot_bad(format!(
                    "the key for {registry_id:#018X} does not open with its marker"
                )));
            }
            let mut words = [0u32; WORDS];
            for (i, word) in words.iter_mut().enumerate() {
                *word = LE::read_u32(&blob[4 + i * 4..]);
            }
            blobs.insert(
                registry_id,
                KeyBlob {
                    words,
                    output: read_byte_tables(&blob[4 + WORDS * 4..]),
                },
            );
        }

        Ok(Self {
            exe_sha256,
            bootstrap_iv,
            input_tables,
            round_tables,
            blobs,
            snapshot_file: None,
        })
    }

    /// Parse the snapshot at `path`, unmasking it first when it is a masked (`ORGTAVIM`) container:
    /// the executable [`GTA6_EXE_ENV`] names keys the unmasking, and a masked snapshot with no such
    /// executable is refused, since nothing else can open it.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| IoError::FileReadFailed {
            path: path.to_path_buf(),
            source,
        })?;
        let plain = unmask_if_needed(&bytes)?;
        let mut keys = Self::parse(plain.as_deref().unwrap_or(&bytes))?;
        keys.snapshot_file = Some(path.to_path_buf());
        Ok(keys)
    }

    /// The snapshot the user supplies — [`GTA6_SNAPSHOT_ENV`], else [`GTA6_SNAPSHOT_FILE_NAME`]
    /// beside the keys cache in `config_dir` — or `None` when neither is present. Nothing is
    /// bundled, so `None` means the TOC cannot be read and the archive is left encrypted.
    pub fn for_install(config_dir: Option<&Path>) -> Result<Option<Self>> {
        let path = std::env::var_os(GTA6_SNAPSHOT_ENV)
            .map(PathBuf::from)
            .or_else(|| config_dir.map(|dir| dir.join(GTA6_SNAPSHOT_FILE_NAME)))
            .filter(|p| p.is_file());
        path.map(Self::from_file).transpose()
    }

    /// The SHA-256 of the executable this snapshot was taken from.
    pub fn exe_sha256(&self) -> &[u8; 32] {
        &self.exe_sha256
    }

    /// The snapshot file these keys came from, when loaded from one.
    pub fn snapshot_file(&self) -> Option<&Path> {
        self.snapshot_file.as_deref()
    }

    fn blob(&self, registry_id: u64) -> Result<&KeyBlob> {
        self.blobs
            .get(&registry_id)
            .ok_or(IoError::RpfEncryptionUnsupported {
                // The registry id is 64-bit; its low word is what identifies the key.
                key_id: registry_id as u32,
            })
    }

    /// Transform one 16-byte block under `blob`: the input substitution, thirteen T-box rounds, then
    /// the output substitution. This is the cipher's block primitive, used for decryption under CBC.
    fn block(&self, data: &[u8; 16], blob: &KeyBlob) -> [u8; 16] {
        let input = &self.input_tables;
        let mut encoded = [0u8; 16];
        for (i, slot) in encoded.iter_mut().enumerate() {
            *slot = input[i][data[i] as usize];
        }
        let w = &blob.words;
        let mut state = [0u32; 4];
        for (i, lane) in state.iter_mut().enumerate() {
            *lane = BigEndian::read_u32(&encoded[i * 4..]) ^ w[i];
        }
        let t = &self.round_tables;
        for r in 1..=ROUNDS {
            let [a, b, c, d] = state;
            state = [
                t[0][(a >> 24) as usize]
                    ^ t[13][((d >> 16) & 0xFF) as usize]
                    ^ t[10][((c >> 8) & 0xFF) as usize]
                    ^ t[7][(b & 0xFF) as usize]
                    ^ w[4 * r],
                t[1][((a >> 16) & 0xFF) as usize]
                    ^ t[14][((d >> 8) & 0xFF) as usize]
                    ^ t[4][(b >> 24) as usize]
                    ^ t[11][(c & 0xFF) as usize]
                    ^ w[4 * r + 1],
                t[5][((b >> 16) & 0xFF) as usize]
                    ^ t[2][((a >> 8) & 0xFF) as usize]
                    ^ t[8][(c >> 24) as usize]
                    ^ t[15][(d & 0xFF) as usize]
                    ^ w[4 * r + 2],
                t[9][((c >> 16) & 0xFF) as usize]
                    ^ t[6][((b >> 8) & 0xFF) as usize]
                    ^ t[12][(d >> 24) as usize]
                    ^ t[3][(a & 0xFF) as usize]
                    ^ w[4 * r + 3],
            ];
        }
        let [a, b, c, d] = state;
        let indices = [
            a >> 24,
            d >> 16,
            c >> 8,
            b,
            b >> 24,
            a >> 16,
            d >> 8,
            c,
            c >> 24,
            b >> 16,
            a >> 8,
            d,
            d >> 24,
            c >> 16,
            b >> 8,
            a,
        ];
        let out_maps = &blob.output;
        let mut out = [0u8; 16];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = out_maps[i][(indices[i] & 0xFF) as usize];
        }
        out
    }

    /// Decrypt `data` in place under `registry_id`, CBC-chained from `iv`: each block is run through
    /// [`Self::block`], XORed with the previous ciphertext (the IV for the first), and the chaining
    /// value advances to the block just read. `data` must be a whole number of 16-byte blocks.
    fn cbc_decrypt(&self, registry_id: u64, iv: &[u8; 16], data: &mut [u8]) -> Result<()> {
        if !data.len().is_multiple_of(16) {
            return Err(IoError::Invalid {
                what: "an RPF9 cipher region".into(),
                why: format!("it is {} bytes, not a whole number of blocks", data.len()),
            });
        }
        let blob = self.blob(registry_id)?;
        let mut prev = *iv;
        for chunk in data.chunks_mut(16) {
            let cipher: [u8; 16] = chunk.try_into().expect("chunks_mut(16) yields 16 bytes");
            let plain = self.block(&cipher, blob);
            for (out, (p, pv)) in chunk.iter_mut().zip(plain.iter().zip(prev.iter())) {
                *out = p ^ pv;
            }
            prev = cipher;
        }
        Ok(())
    }
}

fn read_byte_tables(bytes: &[u8]) -> Box<[[u8; MAP_LEN]; MAPS]> {
    let mut tables = Box::new([[0u8; MAP_LEN]; MAPS]);
    for (i, table) in tables.iter_mut().enumerate() {
        table.copy_from_slice(&bytes[i * MAP_LEN..(i + 1) * MAP_LEN]);
    }
    tables
}

fn read_word_tables(bytes: &[u8]) -> Box<[[u32; MAP_LEN]; MAPS]> {
    let mut tables = Box::new([[0u32; MAP_LEN]; MAPS]);
    for (i, table) in tables.iter_mut().enumerate() {
        for (j, word) in table.iter_mut().enumerate() {
            *word = LE::read_u32(&bytes[(i * MAP_LEN + j) * 4..]);
        }
    }
    tables
}

/// One group of the decrypted index: a column of `count` fixed-width records keyed by a name hash.
///
/// The index is column-oriented. A non-external group's records lie in the main region, an external
/// group's in the region after the split; `data` is the records' byte range within [`Rpf9Toc::bytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rpf9Group {
    /// The engine's name hash for what this column holds.
    pub name_hash: u64,
    /// Records in the column, at most the archive's entry count.
    pub count: u32,
    /// Bytes in one record.
    pub width: u16,
    /// Whether the column lies in the external region after the split.
    pub external: bool,
    /// The records' byte range within the decrypted TOC.
    pub data: Range<usize>,
}

/// An RPF9 archive's decrypted table of contents.
#[derive(Debug, Clone)]
pub struct Rpf9Toc {
    /// The plaintext header and the decrypted metadata and index, `[0, toc_end)` as the file holds it.
    pub bytes: Vec<u8>,
    /// The archive's index key id, from its metadata.
    pub key_id: u32,
    /// The registry id the index key id selects.
    pub registry_id: u64,
    /// Entries the archive holds; a nested archive counts as one.
    pub entry_count: u32,
    /// The index's groups, in order.
    pub groups: Vec<Rpf9Group>,
}

impl Rpf9Toc {
    /// Decrypt the table of contents of the RPF9 archive at the start of `archive` with `keys`.
    ///
    /// The header is parsed and the three cipher regions are deciphered in the order the engine
    /// reads them: the metadata table under the bootstrap key, then the two index regions under the
    /// key the metadata names. The result holds the decrypted bytes and the parsed index groups.
    pub fn decrypt(archive: &[u8], keys: &GtaViKeys) -> Result<Self> {
        let header = Rpf9Header::parse(archive)?;
        // Fields the probe reads at these offsets; the header reuses them under its own names.
        let n = u64::from(u16::from_le_bytes([header.raw[0x1A], header.raw[0x1B]]));
        let m = u64::from(header.part_c_offset);
        let e = u64::from(header.part_b_offset);
        let split = u64::from(header.index_offset);
        let toc_end = u64::from(header.toc_end);

        let metadata_start = 0x40 + round16(8 * n);
        let bootstrap_end = metadata_start
            .checked_add(m)
            .and_then(|v| v.checked_sub(e))
            .ok_or_else(|| toc_bad("the metadata size and its exclusion do not form a range"))?;
        if !(0x40 <= metadata_start && metadata_start <= bootstrap_end && bootstrap_end <= split) {
            return Err(toc_bad("the metadata region is out of order"));
        }
        let toc_len = toc_end as usize;
        if split > toc_end || archive.len() < toc_len {
            return Err(toc_bad("the table of contents runs past the file"));
        }

        let mut bytes = archive[..toc_len].to_vec();

        // 1. The metadata table, under the bootstrap key and IV.
        keys.cbc_decrypt(
            BOOTSTRAP_REGISTRY_ID,
            &keys.bootstrap_iv,
            &mut bytes[0x40..bootstrap_end as usize],
        )?;

        // 2. The one type-1 descriptor names the index key id, its cipher type and its IV.
        let (key_id, iv) = index_key(&bytes, n, metadata_start, m)?;
        // 3. The index registry id the key id selects.
        let registry_id = (u64::from(key_id) << 32) | 0x0200_0000;

        // 4 and 5. The two index regions, both from that same IV.
        keys.cbc_decrypt(
            registry_id,
            &iv,
            &mut bytes[bootstrap_end as usize..split as usize],
        )?;
        keys.cbc_decrypt(registry_id, &iv, &mut bytes[split as usize..toc_len])?;

        let (entry_count, groups) = parse_groups(&bytes, metadata_start, m)?;
        Ok(Self {
            bytes,
            key_id,
            registry_id,
            entry_count,
            groups,
        })
    }
}

fn round16(x: u64) -> u64 {
    (x + 15) & !15
}

fn round8(x: u64) -> u64 {
    (x + 7) & !7
}

fn toc_bad(why: impl Into<String>) -> IoError {
    IoError::Invalid {
        what: "an RPF9 table of contents".into(),
        why: why.into(),
    }
}

/// The index key id and IV from the metadata table's one type-1 descriptor.
fn index_key(
    bytes: &[u8],
    n: u64,
    metadata_start: u64,
    metadata_size: u64,
) -> Result<(u32, [u8; 16])> {
    let mut found: Option<u64> = None;
    for i in 0..n {
        let at = (0x40 + 8 * i) as usize;
        let kind = LE::read_u16(&bytes[at..]);
        let off = u64::from(LE::read_u32(&bytes[at + 4..]));
        if kind == 1 {
            if found.is_some() {
                return Err(toc_bad("more than one index cipher descriptor"));
            }
            if off >= metadata_size {
                return Err(toc_bad("the index descriptor points past the metadata"));
            }
            found = Some(metadata_start + off);
        }
    }
    let at = found.ok_or_else(|| toc_bad("no index cipher descriptor"))? as usize;
    if bytes.len() < at + 22 {
        return Err(toc_bad("the index descriptor runs past the metadata"));
    }
    let key_id = LE::read_u32(&bytes[at..]);
    let cipher_type = LE::read_u16(&bytes[at + 4..]);
    if cipher_type != INDEX_CIPHER_TYPE {
        return Err(IoError::RpfEncryptionUnsupported { key_id });
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&bytes[at + 6..at + 22]);
    Ok((key_id, iv))
}

/// Parse and bounds-check the decrypted index groups; a group that breaks a measured invariant is
/// refused, so a wrong key — which leaves noise here — does not read as a valid listing.
fn parse_groups(
    bytes: &[u8],
    metadata_start: u64,
    metadata_size: u64,
) -> Result<(u32, Vec<Rpf9Group>)> {
    let toc_end = u64::from(LE::read_u32(&bytes[0x04..]));
    let split = u64::from(LE::read_u32(&bytes[0x08..]));
    let group_count = u64::from(LE::read_u16(&bytes[0x20..]));
    let external_count = u64::from(LE::read_u16(&bytes[0x22..]));
    let entries = u64::from(LE::read_u32(&bytes[0x24..]));
    let index_width = u64::from(bytes[0x2C]);
    if !(1..=2).contains(&index_width) || external_count > group_count || group_count > 64 {
        return Err(toc_bad("the group layout is unsupported"));
    }
    let groups_start = round16(metadata_start + metadata_size);
    let main_start = round8(
        groups_start + 24 * group_count + (group_count - external_count) * entries * index_width,
    );
    let external_start = round8(split + external_count * entries * index_width);

    let mut groups = Vec::with_capacity(group_count as usize);
    let mut seen_external = 0u64;
    for i in 0..group_count {
        let at = (groups_start + 24 * i) as usize;
        if bytes.len() < at + 24 {
            return Err(toc_bad("a group record runs past the table"));
        }
        let name_hash = LE::read_u64(&bytes[at..]);
        let count = u64::from(LE::read_u32(&bytes[at + 8..]));
        let offset = u64::from(LE::read_u32(&bytes[at + 12..]));
        let fmt = LE::read_u32(&bytes[at + 16..]);
        let padding = LE::read_u32(&bytes[at + 20..]);
        let width = u64::from(fmt & 0xFFFF);
        let external_flag = fmt >> 16;
        let (region_start, region_end) = if external_flag == 1 {
            (external_start, toc_end)
        } else {
            (main_start, split)
        };
        let record_end = region_start + offset + count * width;
        if !(width > 0 && count <= entries && external_flag <= 1 && padding == 0)
            || record_end > region_end
        {
            return Err(toc_bad(format!("group {i} breaks the measured layout")));
        }
        seen_external += external_flag as u64;
        let start = (region_start + offset) as usize;
        groups.push(Rpf9Group {
            name_hash,
            count: count as u32,
            width: width as u16,
            external: external_flag == 1,
            data: start..record_end as usize,
        });
    }
    if seen_external != external_count {
        return Err(toc_bad("the external group count does not match"));
    }
    Ok((entries as u32, groups))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `ORGTAVIK` container with distinct, checkable tables and one key blob.
    fn snapshot() -> (Vec<u8>, u64) {
        let registry_id = 0x1122_3344_5566_7788u64;
        let mut out = Vec::new();
        out.extend_from_slice(SNAPSHOT_MAGIC);
        out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        out.extend_from_slice(&SNAPSHOT_MODE_CBC.to_le_bytes());
        out.extend_from_slice(&[0xAB; 32]); // exe sha256
        out.extend_from_slice(&[0xCD; 16]); // bootstrap iv
        out.extend_from_slice(&(INPUT_TABLES_LEN as u32).to_le_bytes());
        out.extend((0..INPUT_TABLES_LEN).map(|i| i as u8));
        out.extend_from_slice(&(ROUND_TABLES_LEN as u32).to_le_bytes());
        out.extend((0..ROUND_TABLES_LEN).map(|i| i as u8));
        out.extend_from_slice(&1u32.to_le_bytes()); // one descriptor
        out.extend_from_slice(&registry_id.to_le_bytes());
        out.extend_from_slice(&(BLOB_LEN as u32).to_le_bytes());
        out.extend_from_slice(&BLOB_MAGIC);
        out.extend((0..WORDS as u32).flat_map(|w| w.to_le_bytes()));
        out.extend((0..(MAPS * MAP_LEN)).map(|i| (i * 3) as u8));
        (out, registry_id)
    }

    #[test]
    fn the_snapshot_parses_its_tables_and_keys() {
        let (bytes, registry_id) = snapshot();
        let keys = GtaViKeys::parse(&bytes).expect("a valid snapshot");
        assert_eq!(keys.exe_sha256(), &[0xAB; 32]);
        assert_eq!(keys.bootstrap_iv, [0xCD; 16]);
        assert_eq!(keys.input_tables[0][5], 5);
        assert_eq!(keys.input_tables[1][0], MAP_LEN as u8);
        assert_eq!(keys.round_tables[0][1], u32::from_le_bytes([4, 5, 6, 7]));
        let blob = keys.blob(registry_id).expect("the key is registered");
        assert_eq!(blob.words[1], 1);
        assert_eq!(blob.output[0][2], 6);
        // A block run is deterministic and never panics on indexing.
        let a = keys.block(&[0; 16], blob);
        let b = keys.block(&[0; 16], blob);
        assert_eq!(a, b);
    }

    #[test]
    fn cbc_is_block_then_xor_with_the_previous_cipher() {
        let (bytes, registry_id) = snapshot();
        let keys = GtaViKeys::parse(&bytes).expect("a valid snapshot");
        let blob = keys.blob(registry_id).unwrap();
        let iv = [0x5Au8; 16];
        let mut data = (0u8..32).collect::<Vec<_>>();
        let first: [u8; 16] = data[..16].try_into().unwrap();
        let second: [u8; 16] = data[16..].try_into().unwrap();
        keys.cbc_decrypt(registry_id, &iv, &mut data).unwrap();
        let mut want0 = keys.block(&first, blob);
        for (w, v) in want0.iter_mut().zip(iv.iter()) {
            *w ^= v;
        }
        let mut want1 = keys.block(&second, blob);
        for (w, v) in want1.iter_mut().zip(first.iter()) {
            *w ^= v;
        }
        assert_eq!(&data[..16], &want0);
        assert_eq!(&data[16..], &want1);
    }

    #[test]
    fn a_region_that_is_not_whole_blocks_is_refused() {
        let (bytes, registry_id) = snapshot();
        let keys = GtaViKeys::parse(&bytes).unwrap();
        let mut data = [0u8; 17];
        assert!(matches!(
            keys.cbc_decrypt(registry_id, &[0; 16], &mut data),
            Err(IoError::Invalid { .. })
        ));
    }

    #[test]
    fn a_truncated_snapshot_is_refused_not_panicked() {
        let (bytes, _) = snapshot();
        for cut in [4, 20, 100, bytes.len() - 1] {
            assert!(matches!(
                GtaViKeys::parse(&bytes[..cut]),
                Err(IoError::Invalid { .. })
            ));
        }
    }

    #[test]
    fn a_missing_key_is_unsupported_not_a_panic() {
        let (bytes, _) = snapshot();
        let keys = GtaViKeys::parse(&bytes).unwrap();
        assert!(matches!(
            keys.cbc_decrypt(0xDEAD_BEEF_0000_0000, &[0; 16], &mut [0u8; 16]),
            Err(IoError::RpfEncryptionUnsupported { .. })
        ));
    }

    #[test]
    fn a_plaintext_snapshot_passes_through_unmasking_untouched() {
        // A plaintext `ORGTAVIK` container is not masked, so no executable is read and the bytes are
        // used as read.
        let (bytes, _) = snapshot();
        assert!(matches!(unmask_with(&bytes, None), Ok(None)));
    }

    #[test]
    fn a_masked_snapshot_opens_with_its_executable_and_refuses_otherwise() {
        let (plain, _) = snapshot();
        let masked = gta6_keygate::encode_masked(&plain, b"the-keying-executable");
        assert!(gta6_keygate::is_masked(&masked));

        // The executable is read from a path, so write the keying one and a wrong one to a temp dir.
        let dir = std::env::temp_dir().join(format!("openrage-gta6-unmask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the temp dir");
        let right = dir.join("right.exe");
        let wrong = dir.join("wrong.exe");
        std::fs::write(&right, b"the-keying-executable").expect("write the keying exe");
        std::fs::write(&wrong, b"a-different-executable").expect("write the wrong exe");

        // Masked with no executable is refused rather than parsed as noise.
        assert!(unmask_with(&masked, None).is_err());
        // A wrong executable is refused, not returned as garbage.
        assert!(unmask_with(&masked, Some(wrong.as_os_str())).is_err());
        // The keying executable recovers the exact plaintext, which then parses.
        let recovered = unmask_with(&masked, Some(right.as_os_str()))
            .expect("unmask succeeds")
            .expect("a masked snapshot yields plaintext");
        assert_eq!(recovered, plain);
        assert!(GtaViKeys::parse(&recovered).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

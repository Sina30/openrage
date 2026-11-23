//! RDR2's RPF8 cipher: a table-driven block cipher chained like CBC, keyed by the id a table or an
//! entry names, with its tables decoded from a snapshot that the game's AES key unlocks.
//!
//! ⚠ The snapshot is masked and encrypted under the game's AES key, so it decodes only with the key
//! from the user's own `RDR2.exe`. A snapshot of the user's (`OPENRAGE_RDR2_SNAPSHOT`, or
//! `rdr2_magic.dat` beside the keys cache) comes first, else the one `bundled-keys` embeds.
//! `the_key_snapshot_is_in_the_tree_once` holds the tree to that one embed.

use std::path::Path;
use std::sync::Arc;

use super::extractor::aes_key_from_executable;
use super::keys::PC_AES_KEY_LEN;
use super::magic::unmask_and_inflate;
use super::provider::{CryptoTarget, RpfCryptoProvider, TocDecryptOutcome};
use crate::container::rpf::header::RpfEncryption;
use crate::error::{IoError, Result};

/// The RDR2 key snapshot, inert until the user's executable supplies the AES key.
#[cfg(feature = "bundled-keys")]
pub const BUNDLED_SNAPSHOT: &[u8] = include_bytes!("../../../../data/rdr2_magic.dat");

/// The file a user's RDR2 snapshot is looked up under, beside the keys cache.
pub const RDR2_SNAPSHOT_FILE_NAME: &str = "rdr2_magic.dat";
/// The environment variable that names a user's RDR2 snapshot.
pub const RDR2_SNAPSHOT_ENV: &str = "OPENRAGE_RDR2_SNAPSHOT";

const KEY_SLOTS: usize = 164;
const KEY_ROUNDS: usize = 18;
const CIPHER_ROUNDS: usize = 17;
/// The slot a key id of `0xC0` selects.
const LAST_KEY_ID: u32 = 0xC0;
const ONES: u64 = 0x0101_0101_0101_0101;

const ROUND_LOOKUP: usize = 4096;
const ROUND_MASKS: usize = 256;
const ROUND_BYTES: usize = (ROUND_LOOKUP + ROUND_MASKS) * 8 + 16 * 4;
const KEYS_BYTES: usize = KEY_SLOTS * KEY_ROUNDS * 2 * 8 + 16;
const TABLES_BYTES: usize = 16 * 256 * 8 + CIPHER_ROUNDS * ROUND_BYTES + 16 * 8 * 8 + 16 * 256 + 16;

type RoundKeys = [[u64; 2]; KEY_ROUNDS];

struct Round {
    lookup: Vec<u64>,
    masks: Vec<u64>,
    xor: [u32; 16],
}

/// What a snapshot holds: a key per id, the chaining start, and the tables every key shares.
pub struct Rdr2Keys {
    keys: Vec<RoundKeys>,
    iv: [u8; 16],
    init: Vec<u64>,
    rounds: Vec<Round>,
    end_masks: Vec<u64>,
    end_tables: Vec<u8>,
    end_xor: [u8; 16],
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.bytes[self.at..self.at + 8].try_into().unwrap());
        self.at += 8;
        v
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn u64s(&mut self, n: usize) -> Vec<u64> {
        (0..n).map(|_| self.u64()).collect()
    }
}

impl Rdr2Keys {
    /// Decode a snapshot with the AES key from the user's executable.
    pub fn decode(snapshot: &[u8], aes_key: &[u8; PC_AES_KEY_LEN]) -> Result<Self> {
        let inflated = unmask_and_inflate(snapshot, aes_key)?;
        if inflated.len() < KEYS_BYTES + TABLES_BYTES {
            return Err(IoError::KeyExtractionFailed(format!(
                "RDR2 snapshot inflated to {} bytes; it holds {}",
                inflated.len(),
                KEYS_BYTES + TABLES_BYTES
            )));
        }
        let mut c = Cursor {
            bytes: &inflated,
            at: 0,
        };
        let keys = (0..KEY_SLOTS)
            .map(|_| {
                let mut round_keys = [[0u64; 2]; KEY_ROUNDS];
                for pair in &mut round_keys {
                    *pair = [c.u64(), c.u64()];
                }
                round_keys
            })
            .collect();
        let mut iv = [0u8; 16];
        iv[..8].copy_from_slice(&c.u64().to_le_bytes());
        iv[8..].copy_from_slice(&c.u64().to_le_bytes());

        let init = c.u64s(16 * 256);
        let mut rounds = Vec::with_capacity(CIPHER_ROUNDS);
        for _ in 0..CIPHER_ROUNDS {
            let lookup = c.u64s(ROUND_LOOKUP);
            let mut masks = Vec::with_capacity(ROUND_MASKS);
            let mut xor = [0u32; 16];
            for slot in &mut xor {
                masks.extend(c.u64s(16));
                *slot = c.u32();
                if *slot > 0xFFF {
                    return Err(IoError::KeyExtractionFailed(
                        "RDR2 snapshot: a round xor is out of range, so the AES key is not the game's".into(),
                    ));
                }
            }
            rounds.push(Round { lookup, masks, xor });
        }
        let end_masks = c.u64s(16 * 8);
        let end_tables = inflated[c.at..c.at + 16 * 256].to_vec();
        c.at += 16 * 256;
        let end_xor: [u8; 16] = inflated[c.at..c.at + 16].try_into().unwrap();
        Ok(Self {
            keys,
            iv,
            init,
            rounds,
            end_masks,
            end_tables,
            end_xor,
        })
    }

    /// The slot a key id selects, or none for an id the game has no cipher for.
    fn slot(key_id: u32) -> Option<usize> {
        match key_id {
            0..=162 => Some(key_id as usize),
            LAST_KEY_ID => Some(163),
            _ => None,
        }
    }

    /// Whether the snapshot holds a cipher for this key id.
    pub fn has_key(&self, key_id: u32) -> bool {
        Self::slot(key_id).is_some()
    }

    /// Decipher whole blocks of `data` in place, chained from the start; a tail under sixteen bytes is left.
    pub fn decrypt(&self, key_id: u32, data: &mut [u8]) -> Result<()> {
        let mut chain = Chain::new(self, self.key_for(key_id)?);
        chain.update(data);
        Ok(())
    }

    /// Decipher an entry body: only the ranges its `config` byte names, in the unit the game reads it by.
    pub fn decrypt_body(&self, key_id: u32, config: u8, chunk: u64, data: &mut [u8]) -> Result<()> {
        let mut chain = Chain::new(self, self.key_for(key_id)?);
        Strided::new(config, data.len() as i64, chunk as i64).run(&mut chain, data);
        Ok(())
    }

    fn key_for(&self, key_id: u32) -> Result<&RoundKeys> {
        Self::slot(key_id)
            .map(|slot| &self.keys[slot])
            .ok_or(IoError::RpfEncryptionUnsupported { key_id })
    }
}

/// One key's cipher with the chaining value it has reached.
struct Chain<'a> {
    set: &'a Rdr2Keys,
    keys: &'a RoundKeys,
    iv: [u8; 16],
}

impl<'a> Chain<'a> {
    fn new(set: &'a Rdr2Keys, keys: &'a RoundKeys) -> Self {
        Self {
            set,
            keys,
            iv: set.iv,
        }
    }

    fn update(&mut self, data: &mut [u8]) {
        for block in data.as_chunks_mut::<16>().0 {
            let sealed = *block;
            self.decrypt_block(block);
            self.iv = sealed;
        }
    }

    fn decrypt_block(&self, data: &mut [u8; 16]) {
        let set = self.set;
        let (mut r0, mut r1) = (0u64, 0u64);
        for j in 0..8 {
            r0 ^= set.init[j * 256 + usize::from(data[j])];
            r1 ^= set.init[(j + 8) * 256 + usize::from(data[j + 8])];
        }
        data[..8].copy_from_slice(&r0.to_le_bytes());
        data[8..].copy_from_slice(&r1.to_le_bytes());

        for i in 0..=16 {
            let select: u32 = if !(2..16).contains(&i) {
                0xFF00
            } else {
                0x39C6
            };
            let t = splat(data);
            self.round(data, &t, select, i);
        }

        let t = splat(data);
        for i in 0..16 {
            let input = &t[if i < 8 { 0 } else { 8 }..][..8];
            let masks = &set.end_masks[i * 8..][..8];
            let mut v = 0u64;
            for m in 0..8 {
                v ^= masks[m] & input[m];
            }
            v ^= v >> 1;
            v ^= v >> 2;
            v ^= v >> 4;
            v &= ONES;
            v |= v >> 7;
            v |= v >> 14;
            v |= v >> 28;
            data[i] = set.end_tables[i * 256 + usize::from(v as u8 ^ set.end_xor[i])] ^ self.iv[i];
        }
    }

    fn round(&self, data: &mut [u8; 16], t: &[u64; 16], select: u32, index: usize) {
        let round = &self.set.rounds[index];
        let kp = &self.keys[index + 1];
        for (half, &key) in kp.iter().enumerate() {
            let off = half * 8;
            let s = select >> off;
            let mut r = key;
            for i in 0..8 {
                let io = i + off;
                let input = &t[(((s >> i) & 1) as usize) * 8..][..8];
                let masks = &round.masks[io * 16..][..16];
                let (mut lower, mut upper) = (0u64, 0u64);
                for m in 0..8 {
                    lower ^= masks[8 + m] & input[m];
                    upper ^= masks[m] & input[m];
                }
                lower ^= lower >> 1;
                lower ^= lower >> 2;
                lower ^= lower >> 4;
                lower &= ONES;
                lower |= lower >> 7;
                lower |= lower >> 14;
                lower |= lower >> 28;

                upper ^= upper << 1;
                upper ^= upper >> 2;
                upper ^= upper >> 4;
                upper &= ONES << 1;
                upper |= upper << 7;
                upper |= upper >> 14;

                r ^= round.lookup
                    [((lower & 0xFF) ^ (upper & 0xF00) ^ u64::from(round.xor[io])) as usize];
            }
            data[off..off + 8].copy_from_slice(&r.to_le_bytes());
        }
    }
}

/// Each byte of a block, spread across a word; the high half of the block comes first within its half.
fn splat(data: &[u8; 16]) -> [u64; 16] {
    let mut t = [0u64; 16];
    for i in 0..8 {
        t[i] = ONES * u64::from(data[7 - i]);
        t[8 + i] = ONES * u64::from(data[15 - i]);
    }
    t
}

/// Which ranges of a body the cipher covers, as the game's own reader walks them.
struct Strided {
    size: i64,
    chunk: i64,
    head: i64,
    block_length: i64,
    block_stride: i64,
    /// Where the walk over the input has got to.
    at: i64,
    /// Where the next range may start, so a range is never ciphered twice.
    floor: i64,
}

impl Strided {
    fn new(config: u8, size: i64, chunk: i64) -> Self {
        let head_config = config & 0b11;
        let head = if head_config != 0 {
            0x400 << (head_config * 2)
        } else {
            0
        };
        let length_config = (config >> 2) & 0b111;
        let (block_length, block_stride) = if length_config != 0 {
            (0x400 << length_config, ((i64::from(config >> 5)) + 1) << 16)
        } else {
            (0, 0)
        };
        Self {
            size,
            chunk,
            head,
            block_length,
            block_stride,
            at: 0,
            floor: 0,
        }
    }

    fn run(&mut self, chain: &mut Chain<'_>, data: &mut [u8]) {
        let tail = self.size - 0x400;
        self.range(chain, data, 0, self.head);
        if self.head >= tail {
            return;
        }
        if self.block_length != 0 || self.block_stride != 0 {
            // ⚠ The game's reader ciphers a different range when its read unit equals the stride
            // and the file straddles it; this mirrors that, and a few files still decode wrongly.
            if self.block_stride == self.chunk
                && tail < self.block_stride
                && self.block_stride < self.size
            {
                self.floor = self.block_stride;
            } else {
                let start = if self.floor != 0 {
                    self.block_stride
                } else {
                    0
                };
                let last = tail - self.block_length;
                let mut block = start;
                while block <= last {
                    if !self.range(chain, data, block, block + self.block_length) {
                        return;
                    }
                    block += self.block_stride;
                }
            }
        }
        self.range(chain, data, tail, self.size);
    }

    /// Advance the walk to `start` without ciphering, then through `end` ciphering.
    fn range(&mut self, chain: &mut Chain<'_>, data: &mut [u8], start: i64, end: i64) -> bool {
        let start = start.max(self.floor);
        let end = end.min(self.size);
        if start >= end {
            return false;
        }
        if self.at < end {
            if self.at < start && !self.advance(chain, data, start, false) {
                return false;
            }
            if !self.advance(chain, data, end, true) {
                return false;
            }
            self.floor = end;
        }
        true
    }

    fn advance(&mut self, chain: &mut Chain<'_>, data: &mut [u8], to: i64, cipher: bool) -> bool {
        let n = to - self.at;
        if n > 0 {
            let from = self.at as usize;
            let end = (from + n as usize).min(data.len());
            if cipher {
                chain.update(&mut data[from..end]);
            }
            self.at += n;
        }
        self.at == to
    }
}

/// Decrypts RDR2's RPF8 tables and bodies; encrypting is not offered.
#[derive(Clone)]
pub struct Rdr2CryptoProvider {
    keys: Arc<Rdr2Keys>,
    snapshot_file: Option<std::path::PathBuf>,
    /// Key ids archives asked for that the snapshot has no cipher for.
    missed: Arc<std::sync::Mutex<std::collections::BTreeSet<u32>>>,
}

impl Rdr2CryptoProvider {
    pub fn new(keys: Arc<Rdr2Keys>) -> Self {
        Self {
            keys,
            snapshot_file: None,
            missed: Arc::default(),
        }
    }

    /// The user's snapshot these tables came from, or none when they are the bundled ones.
    pub fn snapshot_file(&self) -> Option<&Path> {
        self.snapshot_file.as_deref()
    }

    fn note_missing(&self, key_id: u32) {
        if let Ok(mut set) = self.missed.lock() {
            set.insert(key_id);
        }
    }

    fn holds(&self, key_id: u32) -> bool {
        let held = self.keys.has_key(key_id);
        if !held {
            self.note_missing(key_id);
        }
        held
    }

    /// The tables this provider deciphers with.
    pub fn keys(&self) -> &Rdr2Keys {
        &self.keys
    }

    /// The provider for an install: its executable supplies the AES key and `snapshot` the tables.
    pub fn from_files(exe: impl AsRef<Path>, snapshot: impl AsRef<Path>) -> Result<Self> {
        let aes = aes_key_from_executable(exe)?;
        let path = snapshot.as_ref();
        let bytes = std::fs::read(path).map_err(|e| IoError::FileReadFailed {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut provider = Self::new(Arc::new(Rdr2Keys::decode(&bytes, &aes)?));
        provider.snapshot_file = Some(path.to_path_buf());
        Ok(provider)
    }

    /// The provider for an install, from the snapshot the user supplies (named by
    /// [`RDR2_SNAPSHOT_ENV`], or beside the keys cache in `config_dir`), else the bundled one.
    pub fn for_install(exe: impl AsRef<Path>, config_dir: Option<&Path>) -> Result<Self> {
        let user = std::env::var_os(RDR2_SNAPSHOT_ENV)
            .map(std::path::PathBuf::from)
            .or_else(|| config_dir.map(|dir| dir.join(RDR2_SNAPSHOT_FILE_NAME)))
            .filter(|p| p.is_file());
        if let Some(user) = user {
            return Self::from_files(exe, user);
        }
        #[cfg(feature = "bundled-keys")]
        {
            let aes = aes_key_from_executable(exe)?;
            Ok(Self::new(Arc::new(Rdr2Keys::decode(
                BUNDLED_SNAPSHOT,
                &aes,
            )?)))
        }
        #[cfg(not(feature = "bundled-keys"))]
        {
            let _ = exe;
            Err(IoError::KeyExtractionFailed(format!(
                "this build bundles no RDR2 snapshot: set {RDR2_SNAPSHOT_ENV} or place \
                 {RDR2_SNAPSHOT_FILE_NAME} beside the keys cache"
            )))
        }
    }
}

impl RpfCryptoProvider for Rdr2CryptoProvider {
    fn missing_keys(&self) -> Vec<u32> {
        self.missed
            .lock()
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    fn supports(&self, encryption: RpfEncryption) -> bool {
        match encryption {
            RpfEncryption::None | RpfEncryption::Open => true,
            RpfEncryption::Tagged(tag) => self.holds(tag & 0xFF),
            _ => false,
        }
    }

    fn decrypt(
        &self,
        encryption: RpfEncryption,
        target: CryptoTarget<'_>,
        buffer: &mut [u8],
    ) -> Result<()> {
        let result = match (encryption, target) {
            (RpfEncryption::None | RpfEncryption::Open, _) => Ok(()),
            (RpfEncryption::Tagged(tag), CryptoTarget::Toc { .. }) => {
                self.keys.decrypt(tag & 0xFF, buffer)
            }
            (RpfEncryption::Tagged(tag), CryptoTarget::Body { chunk, .. }) => self
                .keys
                .decrypt_body(tag & 0xFF, (tag >> 8) as u8, chunk, buffer),
            (other, _) => Err(IoError::RpfEncryptionUnsupported {
                key_id: other.to_raw(),
            }),
        };
        if let Err(IoError::RpfEncryptionUnsupported { key_id }) = &result {
            self.note_missing(*key_id);
        }
        result
    }

    // ⚠ A tagged table's names blob is not under the table's cipher: deciphered with it, it is noise
    // in every archive measured, so it is kept as stored.
    fn decrypt_names(
        &self,
        _encryption: RpfEncryption,
        _target: CryptoTarget<'_>,
        _buffer: &mut [u8],
        _toc_outcome: TocDecryptOutcome,
    ) -> Result<()> {
        Ok(())
    }

    fn encrypt(
        &self,
        encryption: RpfEncryption,
        _target: CryptoTarget<'_>,
        _buffer: &mut [u8],
    ) -> Result<()> {
        match encryption {
            RpfEncryption::None | RpfEncryption::Open => Ok(()),
            other => Err(IoError::RpfEncryptionUnsupported {
                key_id: other.to_raw(),
            }),
        }
    }
}

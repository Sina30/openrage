pub const PC_AES_KEY_LEN: usize = 32;

pub const NG_KEY_LEN: usize = 0x110;

pub const NG_KEY_COUNT: usize = 101;

pub const NG_DECRYPT_TABLE_BYTES: usize = 1024;

pub const NG_DECRYPT_TABLE_ROUNDS: usize = 17;
pub const NG_DECRYPT_TABLE_LANES: usize = 16;
pub const NG_DECRYPT_TABLE_ENTRIES: usize = 256;
pub const NG_DECRYPT_TABLE_TOTAL: usize = NG_DECRYPT_TABLE_ROUNDS * NG_DECRYPT_TABLE_LANES;

pub const GTA5_HASH_LUT_LEN: usize = 256;

/// Words in the XXTEA key an obfuscated `.awc` is scrambled with.
pub const AWC_KEY_WORDS: usize = 4;

/// Decoded 32-bit lane of an NG decrypt sub-table (256 entries).
pub type NgDecryptTable = [u32; NG_DECRYPT_TABLE_ENTRIES];

pub type NgDecryptTableRound = [NgDecryptTable; NG_DECRYPT_TABLE_LANES];

// NG encrypt state — derived from the decrypt tables via RandomGauss::Solve

pub type NgEncryptTable = [u32; NG_DECRYPT_TABLE_ENTRIES];

pub type NgEncryptTableRound = [NgEncryptTable; NG_DECRYPT_TABLE_LANES];

pub struct NgEncryptLut {
    pub lut0: Box<[[u8; 256]; 256]>,
    pub lut1: Box<[[u8; 256]; 256]>,
    pub indices: Box<[u8; 65536]>,
}

impl Clone for NgEncryptLut {
    fn clone(&self) -> Self {
        let mut out = Self::zeroed();
        out.lut0.copy_from_slice(self.lut0.as_ref());
        out.lut1.copy_from_slice(self.lut1.as_ref());
        out.indices.copy_from_slice(self.indices.as_ref());
        out
    }
}

impl std::fmt::Debug for NgEncryptLut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NgEncryptLut")
            .field("lut0", &"[u8; 65536]")
            .field("lut1", &"[u8; 65536]")
            .field("indices", &"[u8; 65536]")
            .finish()
    }
}

impl NgEncryptLut {
    pub fn zeroed() -> Self {
        Self {
            lut0: boxed_filled([0u8; 256]),
            lut1: boxed_filled([0u8; 256]),
            indices: boxed_filled(0u8),
        }
    }

    #[inline]
    pub fn lookup(&self, value: u32) -> u8 {
        let h16 = ((value & 0xFFFF_0000) >> 16) as usize;
        let l8 = ((value & 0x0000_FF00) >> 8) as usize;
        let l0 = (value & 0x0000_00FF) as usize;
        self.lut0[self.lut1[self.indices[h16] as usize][l8] as usize][l0]
    }
}

pub type NgEncryptLutRound = [NgEncryptLut; NG_DECRYPT_TABLE_LANES];

/// A heap array of `N` copies of `fill`, never built on the stack.
pub(super) fn boxed_filled<T: Clone, const N: usize>(fill: T) -> Box<[T; N]> {
    let Ok(array) = Box::<[T; N]>::try_from(vec![fill; N].into_boxed_slice()) else {
        unreachable!("a vector of N elements is N long");
    };
    array
}

// Aggregate NG encrypt state derived from `Gta5Keys::ng_decrypt_tables`.
#[derive(Debug)]
pub struct Gta5NgEncryptState {
    pub tables: [Option<Box<NgEncryptTableRound>>; NG_DECRYPT_TABLE_ROUNDS],
    pub luts: [Option<Box<NgEncryptLutRound>>; NG_DECRYPT_TABLE_ROUNDS],
}

impl Default for Gta5NgEncryptState {
    fn default() -> Self {
        Self {
            tables: std::array::from_fn(|_| None),
            luts: std::array::from_fn(|_| None),
        }
    }
}

impl Gta5NgEncryptState {
    pub fn populated_table_rounds(&self) -> usize {
        self.tables.iter().filter(|t| t.is_some()).count()
    }
    pub fn populated_lut_rounds(&self) -> usize {
        self.luts.iter().filter(|t| t.is_some()).count()
    }
    pub fn is_complete(&self) -> bool {
        self.populated_table_rounds() == 3 && self.populated_lut_rounds() == 14
    }
}

// Stage of NG encrypt-state derivation, surfaced to the UI.
#[derive(Debug, Clone)]
pub struct EncryptDeriveProgress {
    pub stage: &'static str,
    pub found: usize,
    pub total: usize,
}

pub struct Gta5Keys {
    pub aes_key: [u8; PC_AES_KEY_LEN],
    pub ng_keys: Vec<[u8; NG_KEY_LEN]>,
    pub ng_decrypt_tables: Vec<NgDecryptTableRound>,
    pub lut: [u8; GTA5_HASH_LUT_LEN],
    // XXTEA key for an obfuscated `.awc`. Zero when neither the executable
    // nor a snapshot supplied it, so `has_awc` is the thing to ask.
    pub awc_key: [u32; AWC_KEY_WORDS],
    // Which of the five classes above came from the user's executable and
    // which from a snapshot. The NG keys and decrypt tables have no
    // executable source in either edition's shipped executable.
    pub provenance: super::provenance::KeyProvenance,
    // NG encrypt state, derived lazily from the decrypt tables.
    pub(crate) ng_encrypt_state: std::sync::OnceLock<std::sync::Arc<Gta5NgEncryptState>>,
}

impl Clone for Gta5Keys {
    fn clone(&self) -> Self {
        let lock = std::sync::OnceLock::new();
        if let Some(state) = self.ng_encrypt_state.get() {
            let _ = lock.set(std::sync::Arc::clone(state));
        }
        Self {
            aes_key: self.aes_key,
            ng_keys: self.ng_keys.clone(),
            ng_decrypt_tables: self.ng_decrypt_tables.clone(),
            lut: self.lut,
            awc_key: self.awc_key,
            provenance: self.provenance,
            ng_encrypt_state: lock,
        }
    }
}

impl std::fmt::Debug for Gta5Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gta5Keys")
            .field("aes_key", &"[redacted; 32 bytes]")
            .field(
                "ng_keys",
                &format_args!("[redacted; {}]", self.ng_keys.len()),
            )
            .field(
                "ng_decrypt_tables",
                &format_args!("[redacted; {} rounds]", self.ng_decrypt_tables.len()),
            )
            .field("lut", &"[redacted; 256 bytes]")
            .field("awc_key", &"[redacted; 4 words]")
            .field("provenance", &self.provenance)
            .field(
                "ng_encrypt_state",
                &format_args!(
                    "{}",
                    if self.has_encrypt_state() {
                        "derived"
                    } else {
                        "not-derived"
                    }
                ),
            )
            .finish()
    }
}

impl Gta5Keys {
    pub fn empty() -> Self {
        Self {
            aes_key: [0u8; PC_AES_KEY_LEN],
            ng_keys: Vec::new(),
            ng_decrypt_tables: Vec::new(),
            lut: [0u8; GTA5_HASH_LUT_LEN],
            awc_key: [0u32; AWC_KEY_WORDS],
            provenance: super::provenance::KeyProvenance::default(),
            ng_encrypt_state: std::sync::OnceLock::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.has_aes() && self.has_ng()
    }

    // `true` if AES decrypt is usable (key + LUT recovered).
    pub fn has_aes(&self) -> bool {
        self.aes_key.iter().any(|b| *b != 0) && self.lut.iter().any(|b| *b != 0)
    }

    // `true` if NG decrypt is usable (101 key blocks and 17 decrypt-table
    // rounds).
    pub fn has_ng(&self) -> bool {
        self.ng_keys.len() == NG_KEY_COUNT
            && self.ng_decrypt_tables.len() == NG_DECRYPT_TABLE_ROUNDS
    }

    // `true` if an obfuscated `.awc` can be unscrambled. Deliberately not
    // part of `is_complete`: archive access does not need it and an AES-only
    // key set is a usable one.
    pub fn has_awc(&self) -> bool {
        self.awc_key.iter().any(|w| *w != 0)
    }

    pub fn has_encrypt_state(&self) -> bool {
        self.ng_encrypt_state.get().is_some()
    }

    pub fn encrypt_state(&self) -> Option<std::sync::Arc<Gta5NgEncryptState>> {
        self.ng_encrypt_state.get().cloned()
    }

    pub fn install_encrypt_state(&self, state: std::sync::Arc<Gta5NgEncryptState>) {
        let _ = self.ng_encrypt_state.set(state);
    }

    // Derive (or fetch from cache) the NG encrypt state.
    pub fn ensure_encrypt_state(
        &self,
        progress: &dyn Fn(EncryptDeriveProgress),
    ) -> crate::error::Result<std::sync::Arc<Gta5NgEncryptState>> {
        if let Some(state) = self.ng_encrypt_state.get() {
            return Ok(std::sync::Arc::clone(state));
        }
        if !self.has_ng() {
            return Err(crate::error::IoError::RpfCryptoFailed {
                mode: "NG",
                message: "cannot derive NG encrypt state without NG decrypt tables; \
                     re-extract keys with full key recovery"
                    .into(),
            });
        }
        let state = std::sync::Arc::new(super::ng_encrypt_state::derive_encrypt_state(
            &self.ng_decrypt_tables,
            progress,
        )?);
        let _ = self.ng_encrypt_state.set(std::sync::Arc::clone(&state));
        Ok(self
            .ng_encrypt_state
            .get()
            .cloned()
            .expect("OnceLock populated"))
    }

    // The NG key block for an entry of `name` and `length`.
    pub fn ng_key_for(&self, name: &str, length: u32) -> Option<&[u8; NG_KEY_LEN]> {
        let idx = self.ng_key_index_for(name, length);
        self.ng_keys.get(idx)
    }

    pub fn ng_key_at(&self, idx: usize) -> Option<&[u8; NG_KEY_LEN]> {
        self.ng_keys.get(idx)
    }

    pub fn ng_key_count(&self) -> usize {
        self.ng_keys.len()
    }

    pub fn ng_key_index_for(&self, name: &str, length: u32) -> usize {
        let hash = gta5_hash(&self.lut, name);
        let idx = hash
            .wrapping_add(length)
            .wrapping_add(NG_KEY_COUNT as u32 - 40)
            % 0x65;
        idx as usize
    }

    // A decrypt sub-table's bytes as the little-endian words the NG decrypt
    // routine uses.
    pub fn decode_decrypt_subtable(buf: &[u8; NG_DECRYPT_TABLE_BYTES]) -> NgDecryptTable {
        let mut out = [0u32; NG_DECRYPT_TABLE_ENTRIES];
        for (i, dst) in out.iter_mut().enumerate() {
            let off = i * 4;
            *dst = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
        }
        out
    }
}

// `GTA5Hash::CalculateHash` reproduced bit-exactly; picks the NG key block.
pub fn gta5_hash(lut: &[u8; GTA5_HASH_LUT_LEN], text: &str) -> u32 {
    let mut result: u32 = 0;
    for byte in text.bytes() {
        let temp: u32 = 1025u32.wrapping_mul((lut[byte as usize] as u32).wrapping_add(result));
        result = (temp >> 6) ^ temp;
    }
    32769u32.wrapping_mul(((9u32.wrapping_mul(result)) >> 11) ^ (9u32.wrapping_mul(result)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ng_key_index_matches_codewalker_formula() {
        let mut k = Gta5Keys::empty();
        for (i, b) in k.lut.iter_mut().enumerate() {
            *b = i as u8;
        }
        let name = "test.rpf";
        let length: u32 = 1024;
        let computed = k.ng_key_index_for(name, length);
        let expected = (gta5_hash(&k.lut, name)
            .wrapping_add(length)
            .wrapping_add(NG_KEY_COUNT as u32 - 40))
            % 0x65;
        assert_eq!(computed, expected as usize);
        assert!(computed < 101);
    }

    #[test]
    fn debug_redacts_secret_material() {
        let k = Gta5Keys::empty();
        let s = format!("{:?}", k);
        assert!(s.contains("redacted"));
        assert!(!s.contains("0x"));
    }
}

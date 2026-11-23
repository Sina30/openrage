// On-disk cache for the derived NG encrypt state.

use std::io::{Read, Write};
use std::path::Path;

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{IoError, Result};

use super::keys::{
    boxed_filled, Gta5NgEncryptState, NgDecryptTableRound, NgEncryptLut, NgEncryptLutRound,
    NgEncryptTableRound, NG_DECRYPT_TABLE_LANES, NG_DECRYPT_TABLE_ROUNDS,
};

pub const ENCRYPT_CACHE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NgEncryptLutBytes {
    lut0: Vec<u8>,
    lut1: Vec<u8>,
    indices: Vec<u8>,
}

impl NgEncryptLutBytes {
    fn from_lut(lut: &NgEncryptLut) -> Self {
        let mut lut0 = Vec::with_capacity(65_536);
        for row in lut.lut0.iter() {
            lut0.extend_from_slice(row);
        }
        let mut lut1 = Vec::with_capacity(65_536);
        for row in lut.lut1.iter() {
            lut1.extend_from_slice(row);
        }
        let indices = lut.indices.to_vec();
        Self {
            lut0,
            lut1,
            indices,
        }
    }

    fn into_lut(self) -> Result<NgEncryptLut> {
        if self.lut0.len() != 65_536 {
            return Err(invalid(format!("lut0 len {} != 65536", self.lut0.len())));
        }
        if self.lut1.len() != 65_536 {
            return Err(invalid(format!("lut1 len {} != 65536", self.lut1.len())));
        }
        if self.indices.len() != 65_536 {
            return Err(invalid(format!(
                "indices len {} != 65536",
                self.indices.len()
            )));
        }
        let mut out = NgEncryptLut::zeroed();
        for (i, row) in out.lut0.iter_mut().enumerate() {
            row.copy_from_slice(&self.lut0[i * 256..(i + 1) * 256]);
        }
        for (i, row) in out.lut1.iter_mut().enumerate() {
            row.copy_from_slice(&self.lut1[i * 256..(i + 1) * 256]);
        }
        out.indices.copy_from_slice(&self.indices);
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptCache {
    pub schema: u32,
    pub decrypt_tables_sha1_hex: String,
    pub tables: Vec<Option<Vec<Vec<u32>>>>,
    luts: Vec<Option<Vec<NgEncryptLutBytes>>>,
}

impl EncryptCache {
    pub fn fingerprint_decrypt_tables(decrypt_tables: &[NgDecryptTableRound]) -> String {
        let mut hasher = Sha1::new();
        for round in decrypt_tables {
            for lane in round.iter() {
                for entry in lane.iter() {
                    hasher.update(entry.to_le_bytes());
                }
            }
        }
        hex(&hasher.finalize())
    }

    pub fn build(decrypt_tables: &[NgDecryptTableRound], state: &Gta5NgEncryptState) -> Self {
        let tables: Vec<Option<Vec<Vec<u32>>>> = state
            .tables
            .iter()
            .map(|opt| {
                opt.as_ref()
                    .map(|round| round.iter().map(|lane| lane.to_vec()).collect())
            })
            .collect();
        let luts: Vec<Option<Vec<NgEncryptLutBytes>>> = state
            .luts
            .iter()
            .map(|opt| {
                opt.as_ref().map(|round| {
                    round
                        .iter()
                        .map(NgEncryptLutBytes::from_lut)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        Self {
            schema: ENCRYPT_CACHE_SCHEMA,
            decrypt_tables_sha1_hex: Self::fingerprint_decrypt_tables(decrypt_tables),
            tables,
            luts,
        }
    }

    pub fn into_state(self) -> Result<Gta5NgEncryptState> {
        if self.schema != ENCRYPT_CACHE_SCHEMA {
            return Err(invalid(format!(
                "schema {} unsupported (current {})",
                self.schema, ENCRYPT_CACHE_SCHEMA
            )));
        }
        if self.tables.len() != NG_DECRYPT_TABLE_ROUNDS {
            return Err(invalid(format!(
                "tables rounds {} != {}",
                self.tables.len(),
                NG_DECRYPT_TABLE_ROUNDS
            )));
        }
        if self.luts.len() != NG_DECRYPT_TABLE_ROUNDS {
            return Err(invalid(format!(
                "luts rounds {} != {}",
                self.luts.len(),
                NG_DECRYPT_TABLE_ROUNDS
            )));
        }

        let mut state = Gta5NgEncryptState::default();
        for (round_idx, opt) in self.tables.into_iter().enumerate() {
            if let Some(round) = opt {
                if round.len() != NG_DECRYPT_TABLE_LANES {
                    return Err(invalid(format!(
                        "tables[{}] lanes {} != {}",
                        round_idx,
                        round.len(),
                        NG_DECRYPT_TABLE_LANES
                    )));
                }
                let mut group: Box<NgEncryptTableRound> = boxed_filled([0u32; 256]);
                for (lane_idx, lane) in round.iter().enumerate() {
                    if lane.len() != 256 {
                        return Err(invalid(format!(
                            "tables[{}][{}] entries {} != 256",
                            round_idx,
                            lane_idx,
                            lane.len()
                        )));
                    }
                    group[lane_idx].copy_from_slice(lane);
                }
                state.tables[round_idx] = Some(group);
            }
        }
        for (round_idx, opt) in self.luts.into_iter().enumerate() {
            if let Some(round) = opt {
                if round.len() != NG_DECRYPT_TABLE_LANES {
                    return Err(invalid(format!(
                        "luts[{}] lanes {} != {}",
                        round_idx,
                        round.len(),
                        NG_DECRYPT_TABLE_LANES
                    )));
                }
                let mut group: Box<NgEncryptLutRound> =
                    Box::new(std::array::from_fn(|_| NgEncryptLut::zeroed()));
                for (lane_idx, lane) in round.into_iter().enumerate() {
                    group[lane_idx] = lane.into_lut()?;
                }
                state.luts[round_idx] = Some(group);
            }
        }
        Ok(state)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let raw = bincode::serialize(self).map_err(|e| invalid(format!("encode: {e}")))?;
        let mut compressor = DeflateEncoder::new(Vec::new(), Compression::default());
        compressor
            .write_all(&raw)
            .map_err(|e| invalid(format!("deflate: {e}")))?;
        compressor
            .finish()
            .map_err(|e| invalid(format!("deflate finish: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut decompressor = DeflateDecoder::new(bytes);
        let mut raw = Vec::new();
        decompressor
            .read_to_end(&mut raw)
            .map_err(|e| invalid(format!("inflate: {e}")))?;
        bincode::deserialize::<Self>(&raw).map_err(|e| invalid(format!("decode: {e}")))
    }

    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| IoError::FileWriteFailed {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let tmp = path.with_extension("bin.tmp");
        let bytes = self.encode()?;
        std::fs::write(&tmp, &bytes).map_err(|e| IoError::FileWriteFailed {
            path: tmp.clone(),
            source: e,
        })?;
        #[cfg(target_os = "windows")]
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| IoError::FileWriteFailed {
                path: path.to_path_buf(),
                source: e,
            })?;
        }
        std::fs::rename(&tmp, path).map_err(|e| IoError::FileWriteFailed {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(())
    }

    pub fn read_from<P: AsRef<Path>>(path: P) -> Result<Option<Self>> {
        let path = path.as_ref();
        match std::fs::read(path) {
            Ok(bytes) => Self::decode(&bytes).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(IoError::FileReadFailed {
                path: path.to_path_buf(),
                source: e,
            }),
        }
    }

    pub fn matches(&self, decrypt_tables: &[NgDecryptTableRound]) -> bool {
        self.decrypt_tables_sha1_hex == Self::fingerprint_decrypt_tables(decrypt_tables)
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn invalid(msg: String) -> IoError {
    IoError::KeysCacheInvalid(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_state() -> (Vec<NgDecryptTableRound>, Gta5NgEncryptState) {
        let mut decrypt_tables = Vec::new();
        for round in 0..17u32 {
            let mut group = [[0u32; 256]; 16];
            for (lane_idx, lane) in group.iter_mut().enumerate() {
                for (i, slot) in lane.iter_mut().enumerate() {
                    *slot = (round << 24) ^ ((lane_idx as u32) << 8) ^ (i as u32);
                }
            }
            decrypt_tables.push(group);
        }

        let mut state = Gta5NgEncryptState::default();
        for round in [0usize, 1, 16] {
            let mut group: Box<NgEncryptTableRound> = boxed_filled([0u32; 256]);
            for (lane_idx, lane) in group.iter_mut().enumerate() {
                for (i, slot) in lane.iter_mut().enumerate() {
                    *slot = ((round as u32) << 16) ^ ((lane_idx as u32) << 8) ^ (i as u32);
                }
            }
            state.tables[round] = Some(group);
        }
        for (round, slot) in state.luts.iter_mut().enumerate().take(16).skip(2) {
            let mut group: Box<NgEncryptLutRound> =
                Box::new(std::array::from_fn(|_| NgEncryptLut::zeroed()));
            for (lane_idx, lut) in group.iter_mut().enumerate() {
                lut.indices[0] = ((round + lane_idx) & 0xFF) as u8;
                lut.lut0[0][0] = lane_idx as u8;
                lut.lut1[0][0] = (round + 1) as u8;
            }
            *slot = Some(group);
        }
        (decrypt_tables, state)
    }

    #[test]
    fn fingerprint_changes_with_tables() {
        let mut a = vec![[[0u32; 256]; 16]; 17];
        let mut b = a.clone();
        b[0][0][0] = 1;
        let fa = EncryptCache::fingerprint_decrypt_tables(&a);
        let fb = EncryptCache::fingerprint_decrypt_tables(&b);
        assert_ne!(fa, fb);
        a[0][0][0] = 1;
        assert_eq!(EncryptCache::fingerprint_decrypt_tables(&a), fb);
    }

    #[test]
    fn roundtrip_through_disk() {
        let (decrypt, state) = synthetic_state();
        let cache = EncryptCache::build(&decrypt, &state);
        let bytes = cache.encode().expect("encode");
        let decoded = EncryptCache::decode(&bytes).expect("decode");
        assert_eq!(decoded.schema, ENCRYPT_CACHE_SCHEMA);
        assert!(decoded.matches(&decrypt));
        let restored = decoded.into_state().expect("into_state");
        for round in [0usize, 1, 16] {
            let original = state.tables[round].as_ref().unwrap();
            let recovered = restored.tables[round].as_ref().unwrap();
            for lane in 0..16 {
                assert_eq!(original[lane], recovered[lane]);
            }
        }
        for round in 2..=15usize {
            let original = state.luts[round].as_ref().unwrap();
            let recovered = restored.luts[round].as_ref().unwrap();
            for lane in 0..16 {
                assert_eq!(original[lane].indices[0], recovered[lane].indices[0]);
                assert_eq!(original[lane].lut0[0][0], recovered[lane].lut0[0][0]);
                assert_eq!(original[lane].lut1[0][0], recovered[lane].lut1[0][0]);
            }
        }
    }

    #[test]
    fn schema_mismatch_rejected() {
        let (decrypt, state) = synthetic_state();
        let mut cache = EncryptCache::build(&decrypt, &state);
        cache.schema = ENCRYPT_CACHE_SCHEMA + 100;
        let err = cache.into_state().expect_err("schema mismatch");
        assert!(matches!(err, IoError::KeysCacheInvalid(_)));
    }
}

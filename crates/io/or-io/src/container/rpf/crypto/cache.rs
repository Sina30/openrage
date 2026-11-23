//! Versioned on-disk cache for the GTA V keys, keyed by the executable's SHA-1
//! fingerprint so an upgraded executable forces re-extraction.
//!
//! Extraction is a sliding-window SHA-1 scan of the whole executable, which
//! every process that wants keys would otherwise pay for.
//!
//! # Schema
//!
//! bincode, in this order:
//!
//! ```text
//! schema:            u32                   // KEYS_CACHE_SCHEMA
//! gta5_path:         PathBuf
//! gta5_sha1_hex:     String
//! gta5_size:         u64
//! extracted_at:      i64                   // unix timestamp, seconds
//! aes_key:           Vec<u8>               (32 bytes)
//! lut:               Vec<u8>               (256 bytes)
//! ng_keys:           Vec<Vec<u8>>          (101 entries of 272 bytes, or none)
//! ng_decrypt_tables: Vec<Vec<Vec<u32>>>    (17 rounds * 16 lanes * 256 u32, or none)
//! provenance:        KeyProvenance
//! awc_key:           Vec<u32>              (4 words, or none)
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{IoError, Result};

use super::extractor::ExeFingerprint;
use super::provenance::KeyOrigin;

use super::keys::{
    Gta5Keys, AWC_KEY_WORDS, GTA5_HASH_LUT_LEN, NG_DECRYPT_TABLE_ENTRIES, NG_DECRYPT_TABLE_LANES,
    NG_DECRYPT_TABLE_ROUNDS, NG_KEY_COUNT, NG_KEY_LEN, PC_AES_KEY_LEN,
};

/// 4 marks a cache written by a build that consults the bundled key snapshot (D18).
pub const KEYS_CACHE_SCHEMA: u32 = 4;
// ⚠ Schema 3's layout is schema 4's; only what wrote it differs.
const OLDEST_READABLE_SCHEMA: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeysCacheMetadata {
    pub schema: u32,
    pub gta5_path: PathBuf,
    pub gta5_sha1_hex: String,
    pub gta5_size: u64,
    pub extracted_at: i64,
}

impl KeysCacheMetadata {
    pub fn extracted_at_display(&self) -> String {
        let secs = self.extracted_at.max(0) as u64;
        let days = secs / 86_400;
        let rem = secs % 86_400;
        let h = rem / 3600;
        let m = (rem % 3600) / 60;
        let s = rem % 60;
        let z = days as i64 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", y, mo, d, h, m, s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeysCache {
    pub metadata: KeysCacheMetadata,
    payload: KeysPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeysPayload {
    aes_key: Vec<u8>,
    lut: Vec<u8>,
    ng_keys: Vec<Vec<u8>>,
    ng_decrypt_tables: Vec<Vec<Vec<u32>>>,
    provenance: super::provenance::KeyProvenance,
    awc_key: Vec<u32>,
}

impl KeysCache {
    pub fn build(gta5_path: PathBuf, fingerprint: ExeFingerprint, keys: &Gta5Keys) -> Self {
        let extracted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let payload = KeysPayload {
            aes_key: keys.aes_key.to_vec(),
            lut: keys.lut.to_vec(),
            ng_keys: keys.ng_keys.iter().map(|k| k.to_vec()).collect(),
            ng_decrypt_tables: keys
                .ng_decrypt_tables
                .iter()
                .map(|round| round.iter().map(|lane| lane.to_vec()).collect())
                .collect(),
            provenance: keys.provenance,
            awc_key: keys.awc_key.to_vec(),
        };
        Self {
            metadata: KeysCacheMetadata {
                schema: KEYS_CACHE_SCHEMA,
                gta5_path,
                gta5_sha1_hex: fingerprint.sha1_hex,
                gta5_size: fingerprint.size,
                extracted_at,
            },
            payload,
        }
    }

    /// Whether a build that could not reach the bundled snapshot wrote this cache without NG keys.
    pub fn predates_the_bundled_snapshot(&self) -> bool {
        self.metadata.schema < KEYS_CACHE_SCHEMA && self.payload.ng_keys.is_empty()
    }

    pub fn into_keys(self) -> Result<Gta5Keys> {
        if !(OLDEST_READABLE_SCHEMA..=KEYS_CACHE_SCHEMA).contains(&self.metadata.schema) {
            return Err(IoError::KeysCacheInvalid(format!(
                "schema {} is not supported (current: {}); re-extract from your game \
                 executable",
                self.metadata.schema, KEYS_CACHE_SCHEMA
            )));
        }

        let mut keys = Gta5Keys::empty();
        keys.provenance = self.payload.provenance;

        if self.payload.aes_key.len() != PC_AES_KEY_LEN {
            return Err(IoError::KeysCacheInvalid(format!(
                "aes_key length {} != {}",
                self.payload.aes_key.len(),
                PC_AES_KEY_LEN
            )));
        }
        keys.aes_key.copy_from_slice(&self.payload.aes_key);

        if self.payload.lut.len() != GTA5_HASH_LUT_LEN {
            return Err(IoError::KeysCacheInvalid(format!(
                "lut length {} != {}",
                self.payload.lut.len(),
                GTA5_HASH_LUT_LEN
            )));
        }
        keys.lut.copy_from_slice(&self.payload.lut);

        // A cache an older build wrote in AES-only mode carries no AWC key
        // at all, which is a state rather than damage.
        match self.payload.awc_key.len() {
            0 => {}
            AWC_KEY_WORDS => keys.awc_key.copy_from_slice(&self.payload.awc_key),
            n => {
                return Err(IoError::KeysCacheInvalid(format!(
                    "awc_key length {n} is neither 0 nor {AWC_KEY_WORDS}"
                )))
            }
        }
        if keys.has_awc() != (keys.provenance.awc_key != KeyOrigin::Absent) {
            return Err(IoError::KeysCacheInvalid(format!(
                "provenance claims the AWC key {} but the payload {} it",
                if keys.provenance.awc_key == KeyOrigin::Absent {
                    "is absent"
                } else {
                    "is present"
                },
                if keys.has_awc() { "carries" } else { "omits" },
            )));
        }

        let ng_keys_present = !self.payload.ng_keys.is_empty();
        let ng_tables_present = !self.payload.ng_decrypt_tables.is_empty();
        if ng_keys_present != ng_tables_present {
            return Err(IoError::KeysCacheInvalid(format!(
                "ng_keys ({}) and ng_decrypt_tables ({}) disagree on presence; \
                 expected both empty (AES-only cache) or both fully populated",
                self.payload.ng_keys.len(),
                self.payload.ng_decrypt_tables.len(),
            )));
        }

        if ng_keys_present {
            if self.payload.ng_keys.len() != NG_KEY_COUNT {
                return Err(IoError::KeysCacheInvalid(format!(
                    "ng_keys count {} != {}",
                    self.payload.ng_keys.len(),
                    NG_KEY_COUNT
                )));
            }
            for (i, k) in self.payload.ng_keys.iter().enumerate() {
                if k.len() != NG_KEY_LEN {
                    return Err(IoError::KeysCacheInvalid(format!(
                        "ng_keys[{}] length {} != {}",
                        i,
                        k.len(),
                        NG_KEY_LEN
                    )));
                }
                let mut arr = [0u8; NG_KEY_LEN];
                arr.copy_from_slice(k);
                keys.ng_keys.push(arr);
            }

            if self.payload.ng_decrypt_tables.len() != NG_DECRYPT_TABLE_ROUNDS {
                return Err(IoError::KeysCacheInvalid(format!(
                    "ng_decrypt_tables rounds {} != {}",
                    self.payload.ng_decrypt_tables.len(),
                    NG_DECRYPT_TABLE_ROUNDS
                )));
            }
            for (round_idx, round) in self.payload.ng_decrypt_tables.iter().enumerate() {
                if round.len() != NG_DECRYPT_TABLE_LANES {
                    return Err(IoError::KeysCacheInvalid(format!(
                        "ng_decrypt_tables[{}] lanes {} != {}",
                        round_idx,
                        round.len(),
                        NG_DECRYPT_TABLE_LANES
                    )));
                }
                let mut group = [[0u32; 256]; NG_DECRYPT_TABLE_LANES];
                for (lane_idx, lane) in round.iter().enumerate() {
                    if lane.len() != NG_DECRYPT_TABLE_ENTRIES {
                        return Err(IoError::KeysCacheInvalid(format!(
                            "ng_decrypt_tables[{}][{}] entries {} != {}",
                            round_idx,
                            lane_idx,
                            lane.len(),
                            NG_DECRYPT_TABLE_ENTRIES
                        )));
                    }
                    group[lane_idx].copy_from_slice(lane);
                }
                keys.ng_decrypt_tables.push(group);
            }
        }

        // A cache must make AES usable; NG material is optional.
        if !keys.has_aes() {
            return Err(IoError::KeysCacheInvalid(
                "AES key or LUT missing/zero".into(),
            ));
        }

        let claims_ng = keys.provenance.ng_keys != KeyOrigin::Absent
            || keys.provenance.ng_decrypt_tables != KeyOrigin::Absent;
        if claims_ng != ng_keys_present {
            return Err(IoError::KeysCacheInvalid(format!(
                "provenance claims NG material {} but the payload {} it",
                if claims_ng { "is present" } else { "is absent" },
                if ng_keys_present { "carries" } else { "omits" },
            )));
        }

        Ok(keys)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| IoError::KeysCacheInvalid(format!("encode: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize::<Self>(bytes)
            .map_err(|e| IoError::KeysCacheInvalid(format!("decode: {e}")))
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_keys() -> Gta5Keys {
        let mut k = Gta5Keys::empty();
        k.aes_key = [0xAB; 32];
        for (i, b) in k.lut.iter_mut().enumerate() {
            *b = i as u8;
        }
        k.ng_keys = (0..NG_KEY_COUNT).map(|i| [i as u8; NG_KEY_LEN]).collect();
        k.ng_decrypt_tables = (0..NG_DECRYPT_TABLE_ROUNDS)
            .map(|round| {
                let mut group = [[0u32; 256]; NG_DECRYPT_TABLE_LANES];
                for (lane_idx, lane) in group.iter_mut().enumerate().take(NG_DECRYPT_TABLE_LANES) {
                    for (entry, slot) in lane.iter_mut().enumerate() {
                        *slot = ((round as u32) << 24) ^ ((lane_idx as u32) << 8) ^ entry as u32;
                    }
                }
                group
            })
            .collect();
        k.provenance = super::super::provenance::KeyProvenance::all_from_executable();
        k
    }

    #[test]
    fn roundtrip_through_bincode() {
        let keys = synthetic_keys();
        let cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "deadbeef".repeat(5),
                size: 12345,
            },
            &keys,
        );
        let bytes = cache.encode().expect("encode");
        let decoded = KeysCache::decode(&bytes).expect("decode");
        assert_eq!(decoded.metadata.schema, KEYS_CACHE_SCHEMA);
        assert_eq!(decoded.metadata.gta5_size, 12345);
        let restored = decoded.into_keys().expect("into_keys");
        assert_eq!(restored.aes_key, keys.aes_key);
        assert_eq!(restored.lut, keys.lut);
        assert_eq!(restored.ng_keys.len(), NG_KEY_COUNT);
        for (a, b) in restored.ng_keys.iter().zip(keys.ng_keys.iter()) {
            assert_eq!(a, b);
        }
        for (round_a, round_b) in restored
            .ng_decrypt_tables
            .iter()
            .zip(keys.ng_decrypt_tables.iter())
        {
            for (lane_a, lane_b) in round_a.iter().zip(round_b.iter()) {
                assert_eq!(lane_a, lane_b);
            }
        }
    }

    #[test]
    fn aes_only_cache_roundtrips() {
        let mut keys = synthetic_keys();
        keys.ng_keys.clear();
        keys.ng_decrypt_tables.clear();
        keys.provenance.ng_keys = KeyOrigin::Absent;
        keys.provenance.ng_decrypt_tables = KeyOrigin::Absent;

        let cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "abc".repeat(20),
                size: 49_221_752,
            },
            &keys,
        );
        let bytes = cache.encode().expect("encode");
        let decoded = KeysCache::decode(&bytes).expect("decode");
        let restored = decoded.into_keys().expect("AES-only into_keys");
        assert!(restored.has_aes());
        assert!(!restored.has_ng());
        assert!(restored.ng_keys.is_empty());
        assert!(restored.ng_decrypt_tables.is_empty());
    }

    #[test]
    fn partial_ng_cache_is_rejected() {
        let keys = synthetic_keys();
        let mut cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "abc".into(),
                size: 1,
            },
            &keys,
        );
        cache.payload.ng_decrypt_tables.clear();
        let err = cache
            .into_keys()
            .expect_err("partial NG should be rejected");
        let msg = format!("{err}");
        assert!(msg.contains("disagree on presence"), "msg: {msg}");
    }

    #[test]
    fn provenance_survives_the_cache() {
        use super::super::provenance::{KeyProvenance, SnapshotAgreement};

        let mut keys = synthetic_keys();
        keys.awc_key = [0x0123_4567; AWC_KEY_WORDS];
        keys.provenance = KeyProvenance {
            aes_key: KeyOrigin::Executable,
            lut: KeyOrigin::Executable,
            ng_keys: KeyOrigin::EmbeddedSnapshot,
            ng_decrypt_tables: KeyOrigin::EmbeddedSnapshot,
            awc_key: KeyOrigin::EmbeddedSnapshot,
            snapshot_agreement: SnapshotAgreement::Agrees,
        };
        let cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "a".repeat(40),
                size: 47_467_128,
            },
            &keys,
        );
        let restored = KeysCache::decode(&cache.encode().expect("encode"))
            .expect("decode")
            .into_keys()
            .expect("into_keys");
        assert_eq!(restored.provenance, keys.provenance);
        assert_eq!(restored.awc_key, keys.awc_key);
    }

    /// ⚠ **A key set without an AWC key is a state rather than damage**, and
    /// a cache that reported one anyway would hand the `.awc` reader four zero
    /// words and produce plausible noise instead of a refusal.
    #[test]
    fn an_awc_key_survives_the_cache_and_its_absence_is_a_state() {
        let mut keys = synthetic_keys();
        keys.awc_key = [0xA1B2_C3D4, 1, 2, 3];
        keys.provenance.awc_key = KeyOrigin::SnapshotFile;
        let restored = KeysCache::decode(
            &KeysCache::build(
                PathBuf::from("/games/gta5.exe"),
                ExeFingerprint {
                    sha1_hex: "c".repeat(40),
                    size: 1,
                },
                &keys,
            )
            .encode()
            .expect("encode"),
        )
        .expect("decode")
        .into_keys()
        .expect("into_keys");
        assert!(restored.has_awc());
        assert_eq!(restored.awc_key, keys.awc_key);

        let without = synthetic_keys();
        assert!(!without.has_awc());
        assert_eq!(without.provenance.awc_key, KeyOrigin::Absent);
        let restored = KeysCache::decode(
            &KeysCache::build(
                PathBuf::from("/games/gta5.exe"),
                ExeFingerprint {
                    sha1_hex: "d".repeat(40),
                    size: 1,
                },
                &without,
            )
            .encode()
            .expect("encode"),
        )
        .expect("decode")
        .into_keys()
        .expect("into_keys");
        assert!(!restored.has_awc());
    }

    /// A payload and a provenance that disagree about the AWC key is a
    /// refusal for the same reason the NG pair is.
    #[test]
    fn a_cache_claiming_an_awc_key_it_does_not_carry_is_rejected() {
        let mut keys = synthetic_keys();
        keys.provenance.awc_key = KeyOrigin::SnapshotFile;
        let err = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "e".repeat(40),
                size: 1,
            },
            &keys,
        )
        .into_keys()
        .expect_err("a claimed key that is not there");
        assert!(format!("{err}").contains("AWC key"), "{err}");
    }

    #[test]
    fn a_cache_whose_provenance_disagrees_with_its_payload_is_rejected() {
        let mut keys = synthetic_keys();
        keys.ng_keys.clear();
        keys.ng_decrypt_tables.clear();
        // The payload now carries no NG material, but the provenance still
        // claims the executable supplied some.
        let cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "b".repeat(40),
                size: 1,
            },
            &keys,
        );
        let err = cache.into_keys().expect_err("incoherent provenance");
        assert!(
            format!("{err}").contains("provenance claims NG material"),
            "{err}"
        );
    }

    #[test]
    fn schema_mismatch_is_rejected() {
        let keys = synthetic_keys();
        let mut cache = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            ExeFingerprint {
                sha1_hex: "abc".into(),
                size: 1,
            },
            &keys,
        );
        cache.metadata.schema = KEYS_CACHE_SCHEMA + 999;
        let err = cache.into_keys().expect_err("schema mismatch");
        assert!(matches!(err, IoError::KeysCacheInvalid(_)));
    }

    #[test]
    fn only_a_cache_from_before_the_bundled_snapshot_without_ng_keys_predates_it() {
        let fingerprint = || ExeFingerprint {
            sha1_hex: "abc".into(),
            size: 1,
        };
        let mut aes_only = synthetic_keys();
        aes_only.ng_keys.clear();
        aes_only.ng_decrypt_tables.clear();
        aes_only.provenance.ng_keys = KeyOrigin::Absent;
        aes_only.provenance.ng_decrypt_tables = KeyOrigin::Absent;

        let mut old = KeysCache::build(PathBuf::from("/games/gta5.exe"), fingerprint(), &aes_only);
        assert!(!old.predates_the_bundled_snapshot(), "this build wrote it");
        old.metadata.schema = OLDEST_READABLE_SCHEMA;
        assert!(old.predates_the_bundled_snapshot());
        assert!(old.into_keys().is_ok(), "schema 3 is still read");

        let mut complete = KeysCache::build(
            PathBuf::from("/games/gta5.exe"),
            fingerprint(),
            &synthetic_keys(),
        );
        complete.metadata.schema = OLDEST_READABLE_SCHEMA;
        assert!(!complete.predates_the_bundled_snapshot());
    }

    #[test]
    fn read_missing_returns_none() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("does_not_exist.bin");
        let result = KeysCache::read_from(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn extracted_at_display_smoke() {
        let m = KeysCacheMetadata {
            schema: 1,
            gta5_path: PathBuf::from("/x"),
            gta5_sha1_hex: "abc".into(),
            gta5_size: 1,
            extracted_at: 1_700_000_000,
        };
        let s = m.extracted_at_display();
        assert!(s.starts_with("2023-"));
        assert!(s.ends_with("UTC"));
    }
}

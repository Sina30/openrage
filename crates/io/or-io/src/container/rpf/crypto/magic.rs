//! Decode a key snapshot in CodeWalker's `magic.dat` format: the one
//! `bundled-keys` embeds (D18), or one the user supplies.
//!
//! ⚠ The snapshot is masked with a stream seeded from the game's AES key and
//! encrypted under it, so it decodes only with the key extracted from the
//! user's own executable. `the_key_snapshot_is_in_the_tree_once` holds the
//! tree to this one copy, embedded only here.

use std::path::Path;

use flate2::read::DeflateDecoder;
use std::io::Read;

use crate::error::{IoError, Result};

use super::aes;
use super::dotnet_random::DotNetRandom;
use super::keys::{
    Gta5Keys, NgDecryptTableRound, GTA5_HASH_LUT_LEN, NG_DECRYPT_TABLE_BYTES,
    NG_DECRYPT_TABLE_LANES, NG_DECRYPT_TABLE_ROUNDS, NG_KEY_COUNT, NG_KEY_LEN, PC_AES_KEY_LEN,
};

/// CodeWalker's key snapshot, inert until the user's executable supplies the AES key.
#[cfg(feature = "bundled-keys")]
pub const BUNDLED_SNAPSHOT: &[u8] = include_bytes!("../../../../data/magic.dat");

const MAGIC_NG_KEYS_BYTES: usize = NG_KEY_COUNT * NG_KEY_LEN;
const MAGIC_NG_TABLES_BYTES: usize =
    NG_DECRYPT_TABLE_ROUNDS * NG_DECRYPT_TABLE_LANES * NG_DECRYPT_TABLE_BYTES;
const MAGIC_LUT_BYTES: usize = GTA5_HASH_LUT_LEN;
const MAGIC_AWC_BYTES: usize = 16;
pub const MAGIC_INFLATED_SIZE: usize =
    MAGIC_NG_KEYS_BYTES + MAGIC_NG_TABLES_BYTES + MAGIC_LUT_BYTES + MAGIC_AWC_BYTES;

// `unmask_key` is the AES key from the user's executable: the blob is masked
// with a stream seeded from it and AES-encrypted under it.
pub fn decode_magic_blob(magic: &[u8], unmask_key: &[u8; PC_AES_KEY_LEN]) -> Result<Gta5Keys> {
    let inflated = unmask_and_inflate(magic, unmask_key)?;
    if inflated.len() < MAGIC_INFLATED_SIZE {
        return Err(IoError::KeyExtractionFailed(format!(
            "magic.dat inflated payload too small: {} < {}",
            inflated.len(),
            MAGIC_INFLATED_SIZE
        )));
    }
    keys_from_inflated(&inflated, unmask_key)
}

/// The payload a snapshot holds once its mask, its AES layer and its deflate are taken off.
pub(super) fn unmask_and_inflate(
    magic: &[u8],
    unmask_key: &[u8; PC_AES_KEY_LEN],
) -> Result<Vec<u8>> {
    let seed = jenk_hash_bytes(unmask_key) as i32;
    let mut rng = DotNetRandom::new(seed);

    let dbl = magic.len();
    let mut rb1 = vec![0u8; dbl];
    let mut rb2 = vec![0u8; dbl];
    let mut rb3 = vec![0u8; dbl];
    let mut rb4 = vec![0u8; dbl];
    rng.next_bytes(&mut rb1);
    rng.next_bytes(&mut rb2);
    rng.next_bytes(&mut rb3);
    rng.next_bytes(&mut rb4);

    let mut db = vec![0u8; dbl];
    for i in 0..dbl {
        db[i] = magic[i]
            .wrapping_sub(rb1[i])
            .wrapping_sub(rb2[i])
            .wrapping_sub(rb3[i])
            .wrapping_sub(rb4[i]);
    }

    aes::decrypt_in_place(unmask_key, &mut db, 1);

    let mut inflated: Vec<u8> = Vec::with_capacity(MAGIC_INFLATED_SIZE);
    let mut decoder = DeflateDecoder::new(&db[..]);
    decoder
        .read_to_end(&mut inflated)
        .map_err(|e| IoError::KeyExtractionFailed(format!("magic deflate: {e}")))?;
    Ok(inflated)
}

fn keys_from_inflated(inflated: &[u8], unmask_key: &[u8; PC_AES_KEY_LEN]) -> Result<Gta5Keys> {
    let mut keys = Gta5Keys::empty();
    keys.aes_key.copy_from_slice(unmask_key);

    let ng_keys_slice = &inflated[..MAGIC_NG_KEYS_BYTES];
    keys.ng_keys.reserve(NG_KEY_COUNT);
    for i in 0..NG_KEY_COUNT {
        let off = i * NG_KEY_LEN;
        let mut arr = [0u8; NG_KEY_LEN];
        arr.copy_from_slice(&ng_keys_slice[off..off + NG_KEY_LEN]);
        keys.ng_keys.push(arr);
    }

    // NG decrypt tables: 17 rounds × 16 lanes × 1024 bytes each.
    let tables_slice = &inflated[MAGIC_NG_KEYS_BYTES..MAGIC_NG_KEYS_BYTES + MAGIC_NG_TABLES_BYTES];
    keys.ng_decrypt_tables.reserve(NG_DECRYPT_TABLE_ROUNDS);
    let mut cursor = 0usize;
    for _round in 0..NG_DECRYPT_TABLE_ROUNDS {
        let mut group: NgDecryptTableRound = [[0u32; 256]; NG_DECRYPT_TABLE_LANES];
        for lane in &mut group {
            let buf: &[u8; NG_DECRYPT_TABLE_BYTES] = tables_slice
                [cursor..cursor + NG_DECRYPT_TABLE_BYTES]
                .try_into()
                .unwrap();
            *lane = Gta5Keys::decode_decrypt_subtable(buf);
            cursor += NG_DECRYPT_TABLE_BYTES;
        }
        keys.ng_decrypt_tables.push(group);
    }

    let lut_off = MAGIC_NG_KEYS_BYTES + MAGIC_NG_TABLES_BYTES;
    keys.lut
        .copy_from_slice(&inflated[lut_off..lut_off + MAGIC_LUT_BYTES]);

    let awc_off = lut_off + MAGIC_LUT_BYTES;
    for (i, word) in keys.awc_key.iter_mut().enumerate() {
        let at = awc_off + i * 4;
        *word = u32::from_le_bytes(inflated[at..at + 4].try_into().unwrap());
    }

    if !keys.is_complete() {
        return Err(IoError::KeyExtractionFailed(
            "magic.dat decode produced an incomplete key set".into(),
        ));
    }
    Ok(keys)
}

pub fn decode_magic_file<P: AsRef<Path>>(
    magic_path: P,
    unmask_key: &[u8; PC_AES_KEY_LEN],
) -> Result<Gta5Keys> {
    let path = magic_path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| IoError::FileReadFailed {
        path: path.to_path_buf(),
        source: e,
    })?;
    decode_magic_blob(&bytes, unmask_key)
}

pub(super) fn jenk_hash_bytes(data: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for b in data {
        h = h.wrapping_add(*b as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h = h.wrapping_add(h << 15);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jenk_hash_matches_codewalker() {
        let key: Vec<u8> = (0u8..32).map(|i| i.wrapping_mul(7)).collect();
        let h = jenk_hash_bytes(&key);
        assert_eq!(h, 0xC797_C59E);
    }

    #[test]
    fn decode_rejects_garbage_blob() {
        let key = [0xAAu8; PC_AES_KEY_LEN];
        let blob = vec![0u8; 1024];
        let err = decode_magic_blob(&blob, &key).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("deflate") || msg.contains("magic"));
    }
}

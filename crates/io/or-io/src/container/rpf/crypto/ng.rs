// NG block-level encrypt + decrypt primitives for retail RPF7 archives.

use crate::error::{IoError, Result};

use super::keys::{
    Gta5NgEncryptState, NgDecryptTable, NgDecryptTableRound, NgEncryptLutRound,
    NgEncryptTableRound, NG_KEY_LEN,
};

// Decrypt one 16-byte NG block under `key`, the block
// [`super::keys::Gta5Keys::ng_key_for`] picks.
pub fn decrypt_block(block: &mut [u8; 16], key: &[u8; NG_KEY_LEN], tables: &[NgDecryptTableRound]) {
    let sub_keys = subkeys(key);

    round_a(block, &sub_keys[0], &tables[0]);
    round_a(block, &sub_keys[1], &tables[1]);
    for k in 2..=15 {
        round_b(block, &sub_keys[k], &tables[k]);
    }
    round_a(block, &sub_keys[16], &tables[16]);
}

pub fn decrypt_in_place(buffer: &mut [u8], key: &[u8; NG_KEY_LEN], tables: &[NgDecryptTableRound]) {
    for chunk in buffer.as_chunks_mut::<16>().0.iter_mut() {
        let block: &mut [u8; 16] = chunk;
        decrypt_block(block, key, tables);
    }
}

// Encrypt one 16-byte NG block. Inverse of [`decrypt_block`].
pub fn encrypt_block(
    block: &mut [u8; 16],
    key: &[u8; NG_KEY_LEN],
    state: &Gta5NgEncryptState,
) -> Result<()> {
    let sub_keys = subkeys(key);

    let table16 = state.tables[16]
        .as_deref()
        .ok_or_else(|| missing("tables[16]"))?;
    encrypt_round_a(block, &sub_keys[16], table16);
    for k in (2..=15).rev() {
        let lut = state.luts[k].as_deref().ok_or_else(|| missing("luts[k]"))?;
        encrypt_round_b_lut(block, &sub_keys[k], lut);
    }
    let table1 = state.tables[1]
        .as_deref()
        .ok_or_else(|| missing("tables[1]"))?;
    encrypt_round_a(block, &sub_keys[1], table1);
    let table0 = state.tables[0]
        .as_deref()
        .ok_or_else(|| missing("tables[0]"))?;
    encrypt_round_a(block, &sub_keys[0], table0);
    Ok(())
}

pub fn encrypt_in_place(
    buffer: &mut [u8],
    key: &[u8; NG_KEY_LEN],
    state: &Gta5NgEncryptState,
) -> Result<()> {
    for chunk in buffer.as_chunks_mut::<16>().0.iter_mut() {
        let block: &mut [u8; 16] = chunk;
        encrypt_block(block, key, state)?;
    }
    Ok(())
}

fn missing(slot: &'static str) -> IoError {
    IoError::RpfCryptoFailed {
        mode: "NG",
        message: format!(
            "encrypt state slot `{slot}` not derived; \
             call Gta5Keys::ensure_encrypt_state first"
        ),
    }
}

fn subkeys(key: &[u8; NG_KEY_LEN]) -> [[u32; 4]; 17] {
    let mut out = [[0u32; 4]; 17];
    for (i, dst) in out.iter_mut().enumerate() {
        for (j, word) in dst.iter_mut().enumerate() {
            let off = (i * 4 + j) * 4;
            *word = u32::from_le_bytes([key[off], key[off + 1], key[off + 2], key[off + 3]]);
        }
    }
    out
}

#[inline]
fn lane(table: &[NgDecryptTable; 16], idx: usize, byte: u8) -> u32 {
    table[idx][byte as usize]
}

fn round_a(block: &mut [u8; 16], key: &[u32; 4], table: &[NgDecryptTable; 16]) {
    let x1 = lane(table, 0, block[0])
        ^ lane(table, 1, block[1])
        ^ lane(table, 2, block[2])
        ^ lane(table, 3, block[3])
        ^ key[0];
    let x2 = lane(table, 4, block[4])
        ^ lane(table, 5, block[5])
        ^ lane(table, 6, block[6])
        ^ lane(table, 7, block[7])
        ^ key[1];
    let x3 = lane(table, 8, block[8])
        ^ lane(table, 9, block[9])
        ^ lane(table, 10, block[10])
        ^ lane(table, 11, block[11])
        ^ key[2];
    let x4 = lane(table, 12, block[12])
        ^ lane(table, 13, block[13])
        ^ lane(table, 14, block[14])
        ^ lane(table, 15, block[15])
        ^ key[3];

    block[0..4].copy_from_slice(&x1.to_le_bytes());
    block[4..8].copy_from_slice(&x2.to_le_bytes());
    block[8..12].copy_from_slice(&x3.to_le_bytes());
    block[12..16].copy_from_slice(&x4.to_le_bytes());
}

fn round_b(block: &mut [u8; 16], key: &[u32; 4], table: &[NgDecryptTable; 16]) {
    let x1 = lane(table, 0, block[0])
        ^ lane(table, 7, block[7])
        ^ lane(table, 10, block[10])
        ^ lane(table, 13, block[13])
        ^ key[0];
    let x2 = lane(table, 1, block[1])
        ^ lane(table, 4, block[4])
        ^ lane(table, 11, block[11])
        ^ lane(table, 14, block[14])
        ^ key[1];
    let x3 = lane(table, 2, block[2])
        ^ lane(table, 5, block[5])
        ^ lane(table, 8, block[8])
        ^ lane(table, 15, block[15])
        ^ key[2];
    let x4 = lane(table, 3, block[3])
        ^ lane(table, 6, block[6])
        ^ lane(table, 9, block[9])
        ^ lane(table, 12, block[12])
        ^ key[3];

    block[0..4].copy_from_slice(&x1.to_le_bytes());
    block[4..8].copy_from_slice(&x2.to_le_bytes());
    block[8..12].copy_from_slice(&x3.to_le_bytes());
    block[12..16].copy_from_slice(&x4.to_le_bytes());
}

pub(super) fn encrypt_round_a(block: &mut [u8; 16], key: &[u32; 4], table: &NgEncryptTableRound) {
    let key_bytes: [u8; 16] = {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&key[0].to_le_bytes());
        b[4..8].copy_from_slice(&key[1].to_le_bytes());
        b[8..12].copy_from_slice(&key[2].to_le_bytes());
        b[12..16].copy_from_slice(&key[3].to_le_bytes());
        b
    };

    let x1 = table[0][(block[0] ^ key_bytes[0]) as usize]
        ^ table[1][(block[1] ^ key_bytes[1]) as usize]
        ^ table[2][(block[2] ^ key_bytes[2]) as usize]
        ^ table[3][(block[3] ^ key_bytes[3]) as usize];
    let x2 = table[4][(block[4] ^ key_bytes[4]) as usize]
        ^ table[5][(block[5] ^ key_bytes[5]) as usize]
        ^ table[6][(block[6] ^ key_bytes[6]) as usize]
        ^ table[7][(block[7] ^ key_bytes[7]) as usize];
    let x3 = table[8][(block[8] ^ key_bytes[8]) as usize]
        ^ table[9][(block[9] ^ key_bytes[9]) as usize]
        ^ table[10][(block[10] ^ key_bytes[10]) as usize]
        ^ table[11][(block[11] ^ key_bytes[11]) as usize];
    let x4 = table[12][(block[12] ^ key_bytes[12]) as usize]
        ^ table[13][(block[13] ^ key_bytes[13]) as usize]
        ^ table[14][(block[14] ^ key_bytes[14]) as usize]
        ^ table[15][(block[15] ^ key_bytes[15]) as usize];

    block[0..4].copy_from_slice(&x1.to_le_bytes());
    block[4..8].copy_from_slice(&x2.to_le_bytes());
    block[8..12].copy_from_slice(&x3.to_le_bytes());
    block[12..16].copy_from_slice(&x4.to_le_bytes());
}

pub(super) fn encrypt_round_b_lut(block: &mut [u8; 16], key: &[u32; 4], lut: &NgEncryptLutRound) {
    let key_bytes: [u8; 16] = {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&key[0].to_le_bytes());
        b[4..8].copy_from_slice(&key[1].to_le_bytes());
        b[8..12].copy_from_slice(&key[2].to_le_bytes());
        b[12..16].copy_from_slice(&key[3].to_le_bytes());
        b
    };
    let mut data = *block;
    for y in 0..16 {
        data[y] ^= key_bytes[y];
    }
    let q1 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let q2 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let q3 = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let q4 = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    block[0] = lut[0].lookup(q1);
    block[1] = lut[1].lookup(q2);
    block[2] = lut[2].lookup(q3);
    block[3] = lut[3].lookup(q4);
    block[4] = lut[4].lookup(q2);
    block[5] = lut[5].lookup(q3);
    block[6] = lut[6].lookup(q4);
    block[7] = lut[7].lookup(q1);
    block[8] = lut[8].lookup(q3);
    block[9] = lut[9].lookup(q4);
    block[10] = lut[10].lookup(q1);
    block[11] = lut[11].lookup(q2);
    block[12] = lut[12].lookup(q4);
    block[13] = lut[13].lookup(q1);
    block[14] = lut[14].lookup(q2);
    block[15] = lut[15].lookup(q3);
}

pub(super) fn round_a_for_solver(
    block: &mut [u8; 16],
    key: &[u32; 4],
    table: &NgDecryptTableRound,
) {
    round_a(block, key, table);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_tables() -> Vec<NgDecryptTableRound> {
        let mut tables: Vec<NgDecryptTableRound> = Vec::with_capacity(17);
        for round in 0..17u32 {
            let mut group = [[0u32; 256]; 16];
            for (lane_idx, lane) in group.iter_mut().enumerate() {
                for (i, slot) in lane.iter_mut().enumerate() {
                    *slot = (round << 24) ^ ((lane_idx as u32) << 8) ^ (i as u32);
                }
            }
            tables.push(group);
        }
        tables
    }

    #[test]
    fn decrypt_block_does_something() {
        let key = [0xABu8; NG_KEY_LEN];
        let tables = dummy_tables();
        let mut block_a = [0u8; 16];
        let mut block_b = [0u8; 16];
        decrypt_block(&mut block_a, &key, &tables);
        decrypt_block(&mut block_b, &key, &tables);
        assert_eq!(block_a, block_b, "determinism");
        assert_ne!(block_a, [0u8; 16], "non-passthrough");
    }

    #[test]
    fn decrypt_in_place_skips_partial_trailing_block() {
        let key = [0xABu8; NG_KEY_LEN];
        let tables = dummy_tables();
        let mut buf = vec![0u8; 31];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        let original_tail = buf[16..].to_vec();
        decrypt_in_place(&mut buf, &key, &tables);
        assert_eq!(buf[16..], original_tail, "trailing 15 bytes untouched");
    }

    #[test]
    fn encrypt_in_place_skips_partial_trailing_block() {
        let state = Gta5NgEncryptState::default();
        let key = [0xABu8; NG_KEY_LEN];
        let mut buf = vec![0u8; 31];
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let original_tail = buf[16..].to_vec();
        let _ = encrypt_in_place(&mut buf, &key, &state);
        assert_eq!(buf[16..], original_tail, "trailing 15 bytes untouched");
    }
}

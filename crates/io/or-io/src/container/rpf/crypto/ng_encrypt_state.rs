// Derive the NG encrypt state (`PC_NG_ENCRYPT_TABLES` + `PC_NG_ENCRYPT_LUTs`)
// from the decrypt tables.

#[cfg(feature = "rayon")]
use rayon::prelude::*;
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

use crate::error::Result;

use super::keys::{
    boxed_filled, EncryptDeriveProgress, Gta5NgEncryptState, NgDecryptTableRound, NgEncryptLut,
    NgEncryptLutRound, NgEncryptTableRound, NG_DECRYPT_TABLE_ROUNDS,
};
use super::{ng, spread};

const TABLE_ROUNDS: [usize; 3] = [0, 1, 16];

const LUT_ROUNDS: std::ops::RangeInclusive<usize> = 2..=15;

pub fn derive_encrypt_state(
    decrypt_tables: &[NgDecryptTableRound],
    progress: &dyn Fn(EncryptDeriveProgress),
) -> Result<Gta5NgEncryptState> {
    if decrypt_tables.len() != NG_DECRYPT_TABLE_ROUNDS {
        return Err(crate::error::IoError::RpfCryptoFailed {
            mode: "NG",
            message: format!(
                "expected {} decrypt-table rounds, got {}",
                NG_DECRYPT_TABLE_ROUNDS,
                decrypt_tables.len()
            ),
        });
    }

    let mut state = Gta5NgEncryptState::default();
    let total = NG_DECRYPT_TABLE_ROUNDS;
    let mut completed = 0usize;

    progress(EncryptDeriveProgress {
        stage: "Deriving NG encrypt state",
        found: 0,
        total,
    });

    for round in TABLE_ROUNDS {
        progress(EncryptDeriveProgress {
            stage: "Solving NG encrypt tables",
            found: completed,
            total,
        });
        state.tables[round] = Some(solve_round(&decrypt_tables[round]));
        completed += 1;
    }

    for round in LUT_ROUNDS {
        progress(EncryptDeriveProgress {
            stage: "Building NG encrypt LUTs",
            found: completed,
            total,
        });
        state.luts[round] = Some(Box::new(build_luts_round(&decrypt_tables[round])));
        completed += 1;
    }

    progress(EncryptDeriveProgress {
        stage: "NG encrypt state ready",
        found: total,
        total,
    });
    Ok(state)
}

#[derive(Clone)]
struct GaussRow {
    a: [u64; 16],
    b: bool,
}

impl GaussRow {
    fn new() -> Self {
        Self {
            a: [0; 16],
            b: false,
        }
    }
    #[inline]
    fn get_a(&self, idx: usize) -> bool {
        (self.a[idx / 64] >> (idx % 64)) & 1 != 0
    }
    #[inline]
    fn set_a(&mut self, idx: usize) {
        self.a[idx / 64] |= 1u64 << (idx % 64);
    }
    #[inline]
    fn xor_with(&mut self, other: &GaussRow) {
        for i in 0..16 {
            self.a[i] ^= other.a[i];
        }
        self.b ^= other.b;
    }
}

fn solve_one_bit(
    table: &NgDecryptTableRound,
    in_bytes: [usize; 4],
    out_byte: usize,
    out_bit: u8,
) -> Vec<bool> {
    let no_key = [0u32; 4];
    let mut rng = XorShiftRng::seeded();

    let mut pivots: Vec<GaussRow> = Vec::with_capacity(1024);

    let mut first = GaussRow::new();
    first.set_a(0);
    first.b = true;
    pivots.push(first);

    let mut buf_encrypted = [0u8; 16];
    for pivot_idx in 1..1024usize {
        loop {
            rng.next_bytes(&mut buf_encrypted);

            let mut buf_decrypted = buf_encrypted;
            ng::round_a_for_solver(&mut buf_decrypted, &no_key, table);

            let mut row = GaussRow::new();
            row.set_a(buf_decrypted[in_bytes[0]] as usize);
            row.set_a(256 + buf_decrypted[in_bytes[1]] as usize);
            row.set_a(512 + buf_decrypted[in_bytes[2]] as usize);
            row.set_a(768 + buf_decrypted[in_bytes[3]] as usize);
            if (buf_encrypted[out_byte] & (1 << out_bit)) != 0 {
                row.b = true;
            }

            if pivot_idx == 0x2ff {
                row = GaussRow::new();
                row.set_a(0x2ff);
                row.b = true;
            }
            if pivot_idx == 0x3ff {
                row = GaussRow::new();
                row.set_a(0x3ff);
                row.b = true;
            }

            for (k, pivot) in pivots.iter().enumerate().take(pivot_idx) {
                if row.get_a(k) {
                    row.xor_with(pivot);
                }
            }

            if row.get_a(pivot_idx) {
                pivots.push(row);
                break;
            }
        }
    }

    let mut result = vec![false; 1024];
    for j in (0..1024usize).rev() {
        let val = pivots[j].b;
        result[j] = val;
        if val {
            for pivot in pivots.iter_mut().take(j) {
                if pivot.get_a(j) {
                    pivot.b ^= true;
                }
            }
        }
    }
    result
}

/// Solve a full encrypt round (16 lanes × 256 entries). Parallelises
/// across 128 output bits. The 16 KiB result is allocated on the heap
/// to avoid passing a large array by value (which would push 16 KiB
/// onto every caller's stack frame in debug mode).
fn solve_round(table: &NgDecryptTableRound) -> Box<NgEncryptTableRound> {
    let bit_results: Vec<(usize, Vec<bool>)> = spread(0..128usize)
        .map(|bit_idx| {
            let out_byte = bit_idx / 8;
            let uint_idx = out_byte / 4;
            let in_bytes = [
                4 * uint_idx,
                4 * uint_idx + 1,
                4 * uint_idx + 2,
                4 * uint_idx + 3,
            ];
            let out_bit = (bit_idx % 8) as u8;
            let result = solve_one_bit(table, in_bytes, out_byte, out_bit);
            (bit_idx, result)
        })
        .collect();

    let mut out: Box<NgEncryptTableRound> = boxed_filled([0u32; 256]);
    for (bit_idx, bit_result) in bit_results {
        let out_byte = bit_idx / 8;
        let uint_idx = out_byte / 4;
        let in_bytes = [
            4 * uint_idx,
            4 * uint_idx + 1,
            4 * uint_idx + 2,
            4 * uint_idx + 3,
        ];
        let z = (bit_idx % 32) as u32;
        let bit_mask = 1u32 << z;
        for i in 0..256 {
            if bit_result[i] {
                out[in_bytes[0]][i] |= bit_mask;
            }
            if bit_result[256 + i] {
                out[in_bytes[1]][i] |= bit_mask;
            }
            if bit_result[512 + i] {
                out[in_bytes[2]][i] |= bit_mask;
            }
            if bit_result[768 + i] {
                out[in_bytes[3]][i] |= bit_mask;
            }
        }
    }
    out
}

type SharedLanes<const N: usize> = [Box<[AtomicU8; N]>; 16];

fn zeroed_shared_lanes<const N: usize>() -> SharedLanes<N> {
    std::array::from_fn(|_| {
        let zeroed: Box<[AtomicU8]> = (0..N).map(|_| AtomicU8::new(0)).collect();
        let Ok(lane) = Box::<[AtomicU8; N]>::try_from(zeroed) else {
            unreachable!("N elements were collected");
        };
        lane
    })
}

fn into_plain_bytes<const N: usize>(shared: SharedLanes<N>) -> Vec<Vec<u8>> {
    shared
        .into_iter()
        .map(|lane| {
            let lane: Box<[AtomicU8]> = lane;
            lane.into_vec()
                .into_iter()
                .map(AtomicU8::into_inner)
                .collect()
        })
        .collect()
}

fn build_luts_round(decrypt_table: &NgDecryptTableRound) -> NgEncryptLutRound {
    let temp = zeroed_shared_lanes::<65_536>();
    let temp_luts = zeroed_shared_lanes::<16_777_216>();

    build_luts_round_inner(decrypt_table, &temp, &temp_luts);
    let temp = into_plain_bytes(temp);
    let temp_luts = into_plain_bytes(temp_luts);

    let mut result: NgEncryptLutRound = std::array::from_fn(|_| NgEncryptLut::zeroed());

    for i in 0..16 {
        let lut0 = &mut result[i].lut0;
        for block_idx in 0..256usize {
            let mut xl = [0u8; 256];
            xl.copy_from_slice(&temp[i][256 * block_idx..256 * block_idx + 256]);
            (*lut0)[xl[0] as usize] = xl;
        }

        let lut1 = &mut result[i].lut1;
        for block_idx in 0..256usize {
            let mut xl = [0u8; 256];
            xl.copy_from_slice(&temp_luts[i][256 * block_idx..256 * block_idx + 256]);
            (*lut1)[xl[0] as usize] = xl;
        }

        let indices = &mut result[i].indices;
        for block_idx in 0..65_536usize {
            (*indices)[block_idx] = temp_luts[i][256 * block_idx];
        }
    }

    result
}

// Threads split on `b3` and nothing keeps two of them off one slot, so the
// tables are atomic bytes: plain stores through shared pointers would be a data
// race. A relaxed store is the same instruction and the last one still wins.
fn build_luts_round_inner(
    table: &NgDecryptTableRound,
    temp: &SharedLanes<65_536>,
    temp_luts: &SharedLanes<16_777_216>,
) {
    let t = table;

    spread(0..256u32).for_each(|b3| {
        for b2 in 0..256u32 {
            for b1 in 0..256u32 {
                for b0 in 0..256u32 {
                    let x1 = t[0][b0 as usize]
                        ^ t[7][b1 as usize]
                        ^ t[10][b2 as usize]
                        ^ t[13][b3 as usize];
                    let x2 = t[1][b0 as usize]
                        ^ t[4][b1 as usize]
                        ^ t[11][b2 as usize]
                        ^ t[14][b3 as usize];
                    let x3 = t[2][b0 as usize]
                        ^ t[5][b1 as usize]
                        ^ t[8][b2 as usize]
                        ^ t[15][b3 as usize];
                    let x4 = t[3][b0 as usize]
                        ^ t[6][b1 as usize]
                        ^ t[9][b2 as usize]
                        ^ t[12][b3 as usize];

                    let b0u = b0 as u8;
                    let b1u = b1 as u8;
                    let b2u = b2 as u8;
                    let b3u = b3 as u8;

                    if x1 < 65536 {
                        temp[0][x1 as usize].store(b0u, Relaxed);
                        temp[7][x1 as usize].store(b1u, Relaxed);
                        temp[10][x1 as usize].store(b2u, Relaxed);
                        temp[13][x1 as usize].store(b3u, Relaxed);
                    }
                    if x2 < 65536 {
                        temp[1][x2 as usize].store(b0u, Relaxed);
                        temp[4][x2 as usize].store(b1u, Relaxed);
                        temp[11][x2 as usize].store(b2u, Relaxed);
                        temp[14][x2 as usize].store(b3u, Relaxed);
                    }
                    if x3 < 65536 {
                        temp[2][x3 as usize].store(b0u, Relaxed);
                        temp[5][x3 as usize].store(b1u, Relaxed);
                        temp[8][x3 as usize].store(b2u, Relaxed);
                        temp[15][x3 as usize].store(b3u, Relaxed);
                    }
                    if x4 < 65536 {
                        temp[3][x4 as usize].store(b0u, Relaxed);
                        temp[6][x4 as usize].store(b1u, Relaxed);
                        temp[9][x4 as usize].store(b2u, Relaxed);
                        temp[12][x4 as usize].store(b3u, Relaxed);
                    }

                    if (x1 & 0xFF) == 0 {
                        let off = (x1 >> 8) as usize;
                        temp_luts[0][off].store(b0u, Relaxed);
                        temp_luts[7][off].store(b1u, Relaxed);
                        temp_luts[10][off].store(b2u, Relaxed);
                        temp_luts[13][off].store(b3u, Relaxed);
                    }
                    if (x2 & 0xFF) == 0 {
                        let off = (x2 >> 8) as usize;
                        temp_luts[1][off].store(b0u, Relaxed);
                        temp_luts[4][off].store(b1u, Relaxed);
                        temp_luts[11][off].store(b2u, Relaxed);
                        temp_luts[14][off].store(b3u, Relaxed);
                    }
                    if (x3 & 0xFF) == 0 {
                        let off = (x3 >> 8) as usize;
                        temp_luts[2][off].store(b0u, Relaxed);
                        temp_luts[5][off].store(b1u, Relaxed);
                        temp_luts[8][off].store(b2u, Relaxed);
                        temp_luts[15][off].store(b3u, Relaxed);
                    }
                    if (x4 & 0xFF) == 0 {
                        let off = (x4 >> 8) as usize;
                        temp_luts[3][off].store(b0u, Relaxed);
                        temp_luts[6][off].store(b1u, Relaxed);
                        temp_luts[9][off].store(b2u, Relaxed);
                        temp_luts[12][off].store(b3u, Relaxed);
                    }
                }
            }
        }
    });
}

struct XorShiftRng {
    state: u64,
}

impl XorShiftRng {
    fn seeded() -> Self {
        let stack_local = 0u8;
        let addr = (&stack_local as *const u8) as u64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut state = addr ^ now ^ 0x9E37_79B9_7F4A_7C15u64;
        if state == 0 {
            state = 1;
        }
        Self { state }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_bytes(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i + 8 <= buf.len() {
            let word = self.next_u64().to_le_bytes();
            buf[i..i + 8].copy_from_slice(&word);
            i += 8;
        }
        if i < buf.len() {
            let word = self.next_u64().to_le_bytes();
            for (j, slot) in buf[i..].iter_mut().enumerate() {
                *slot = word[j];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn solve_round_trivial_tables() {
        let mut tables: NgDecryptTableRound = [[0u32; 256]; 16];
        for (lane_idx, lane) in tables.iter_mut().enumerate() {
            for (i, slot) in lane.iter_mut().enumerate() {
                *slot = ((lane_idx as u32) << 24) ^ ((i as u32) << 16) ^ ((i as u32) << 8);
            }
        }
        let encrypt_round = solve_round(&tables);
        let nonzero: usize = encrypt_round
            .iter()
            .flat_map(|lane| lane.iter())
            .filter(|v| **v != 0)
            .count();
        assert!(
            nonzero > 0,
            "solve_round must produce some non-zero encrypt entries"
        );
    }

    #[test]
    fn xorshift_smoke() {
        let mut rng = XorShiftRng::seeded();
        let mut buf = [0u8; 32];
        rng.next_bytes(&mut buf);
        let zeros = buf.iter().filter(|b| **b == 0).count();
        assert!(zeros < 16, "xorshift produced too many zeros: {:?}", buf);
    }
}

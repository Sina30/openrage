// bcpack-rs by Ahmet Dedeler (https://github.com/Ahmet-Dedeler, https://ahmetdedeler.com)
// Source: https://github.com/Ahmet-Dedeler/bcpack-rs
// SPDX-License-Identifier: MIT OR Apache-2.0
// If you copy, vendor or port this file, keep this header and credit bcpack-rs in your README.
//
// Origin: bcpack-rs, `src/bc1.rs` (`assemble` and the two symbol maps), commit
// 5bf5cc56ed56f1ababf1f9592300bd182ff90bd8, taken under its MIT licence. A reference to a preset
// slot with no known value, and a lane 1 that is not zero, are refused here.
//! BC1 blocks from the 12 lanes of a BC1 body, one symbol of each lane for each block.
//!
//! Lane 0 says which earlier block predicts the block's endpoints, lanes 2 to 7 hold the
//! differences of R0 G0 B0 R1 G1 B1 from it, and lanes 8 to 11 name the indices of the block's
//! four 2 by 2 quadrants.

use super::bc1_tables::{PRESET_ENDPOINTS, QUADRANT_INDICES};
use super::BcPackError;

/// Bits of the endpoint fields R0 G0 B0 R1 G1 B1, which lanes 2 to 7 carry.
const FIELD_BITS: [u32; 6] = [5, 6, 5, 5, 6, 5];
/// The pixel at the top left of the quadrants of lanes 8 to 11.
const QUADRANTS: [(u32, u32); 4] = [(0, 0), (2, 0), (0, 2), (2, 2)];

/// How many blocks back lane 0's symbol points: 0 for no reference, the symbol read as a signed zigzag, modulo 256.
pub(super) fn distance(symbol: u16) -> usize {
    let symbol = usize::from(symbol) & 0xFF;
    if symbol & 1 == 0 {
        symbol >> 1
    } else {
        255 - (symbol >> 1)
    }
}

/// A difference from its symbol: 0, 1, 2, 3, 4 are 0, -1, 1, -2, 2.
pub(super) fn difference(symbol: u16) -> i32 {
    let symbol = i32::from(symbol);
    if symbol & 1 != 0 {
        -((symbol + 1) >> 1)
    } else {
        symbol >> 1
    }
}

/// The 32-bit index word of a block whose quadrants carry `symbols`: pixel (x, y) at bit 2 * (4y + x).
pub(super) fn index_word(symbols: [u16; 4]) -> u32 {
    let mut word = 0;
    for (symbol, (left, top)) in symbols.into_iter().zip(QUADRANTS) {
        let indices = QUADRANT_INDICES[usize::from(symbol) & 0xFF];
        for (k, index) in indices.into_iter().enumerate() {
            let (x, y) = (left + k as u32 % 2, top + k as u32 / 2);
            word |= u32::from(index & 3) << (2 * (4 * y + x));
        }
    }
    word
}

/// The blocks of `lanes`, 8 bytes each: the two colours, then the index word.
pub(super) fn blocks(lanes: &[Vec<u16>]) -> Result<Vec<u8>, BcPackError> {
    let count = lanes[0].len();
    let mut fields: Vec<[u8; 6]> = Vec::with_capacity(count);
    let mut out = Vec::with_capacity(8 * count);
    for block in 0..count {
        if lanes[1][block] != 0 {
            let symbol = lanes[1][block];
            return Err(BcPackError::Lane1 { block, symbol });
        }
        let back = distance(lanes[0][block]);
        let predicted = if back == 0 {
            [0; 6]
        } else if back <= block {
            fields[block - back]
        } else {
            // A reference to before the first block reads a fixed table, the same for every body.
            // A distance is at most 255, so the slot is 1 or more: slot 0 is never read.
            let slot = 256 + block - back;
            PRESET_ENDPOINTS[slot].ok_or(BcPackError::PresetNotKnown { block, slot })?
        };
        let mut field = [0u8; 6];
        for (k, value) in field.iter_mut().enumerate() {
            let mask = (1i32 << FIELD_BITS[k]) - 1;
            *value = ((i32::from(predicted[k]) + difference(lanes[2 + k][block])) & mask) as u8;
        }
        fields.push(field);
        let colour = |f: &[u8]| u16::from(f[0]) << 11 | u16::from(f[1]) << 5 | u16::from(f[2]);
        out.extend(colour(&field[..3]).to_le_bytes());
        out.extend(colour(&field[3..]).to_le_bytes());
        let symbols = std::array::from_fn(|quadrant| lanes[8 + quadrant][block]);
        out.extend(index_word(symbols).to_le_bytes());
    }
    Ok(out)
}

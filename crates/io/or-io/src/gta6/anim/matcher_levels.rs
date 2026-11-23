//! A matcher database's weights, and a class-3 database's frames in blocks and its levels of
//! bounds over them.

use super::{s, zero, MatcherLevel, BLOCK_LEN, LEVEL_LEN};
use crate::gta6::anim::Section;

/// Class 3's pair count, each frame's stored dimensions, and its levels.
type Blocks = (u16, Vec<Vec<u16>>, Vec<MatcherLevel>);

/// Eight f32 weights.
const WEIGHTS_LEN: usize = 32;
/// Frames a block holds, and lanes a level's node holds.
const LANES: usize = 8;
/// The only level word seen.
const LEVEL_WORD: u32 = 16;

/// Where `h` falls in the order of f16 values, -0 below +0.
fn rank(h: u16) -> u16 {
    match h & 0x8000 {
        0 => h | 0x8000,
        _ => !h,
    }
}

fn signed_zero(h: u16) -> bool {
    h & 0x7FFF == 0
}

/// The sixteen halves of a 32-byte element.
fn halves(bytes: &[u8]) -> [u16; 16] {
    std::array::from_fn(|h| u16::from_le_bytes([bytes[2 * h], bytes[2 * h + 1]]))
}

impl Section<'_> {
    /// The weights at 0xB0: `ceil(dims / 8)` elements of eight f32, positive within `dims`, zero
    /// past them, summing to 1.
    pub(super) fn matcher_weights(&self, d: usize, dims: u64) -> Result<Vec<f32>, String> {
        let (first, count) = self.matcher_slot("matcher weights", d + 0xB0, WEIGHTS_LEN)?;
        if count as u64 != dims.div_ceil(8) {
            return Err(format!(
                "its database holds {count} elements of weights at 0xB0, not ceil({dims} / 8)"
            ));
        }
        let bytes = (self.bytes("matcher weights", first, count * WEIGHTS_LEN)).map_err(s)?;
        let mut all = Vec::with_capacity(bytes.len() / 4);
        for w in bytes.as_chunks::<4>().0 {
            all.push(f32::from_le_bytes(*w));
        }
        // `count` is ceil(dims / 8), so the elements hold `dims` weights.
        let (weights, past) = all.split_at(dims as usize);
        let sum: f64 = weights.iter().map(|w| f64::from(*w)).sum();
        if !weights.iter().all(|w| w.is_finite() && *w > 0.0)
            || past.iter().any(|w| *w != 0.0)
            || (sum - 1.0).abs() >= 1e-3
        {
            return Err(format!(
                "its database's weights at 0xB0 are not positive within its {dims} dimensions, zero past them and summing to 1"
            ));
        }
        Ok(weights.to_vec())
    }

    /// Class 3's word at 0x12C, its frames' stored dimensions from the blocks at 0xE8, and its
    /// levels at `levels_at` with their `words`, every bound re-derived from the frames.
    pub(super) fn matcher_blocks(
        &self,
        d: usize,
        frames: u32,
        dims: usize,
        levels_at: usize,
        words: &[u32],
    ) -> Result<Blocks, String> {
        let pairs = self.u16("matcher database", d + 0x12C).map_err(s)?;
        let p = usize::from(pairs);
        if p != dims.div_ceil(2) {
            return Err(format!(
                "its word at 0x12C is {pairs}, not ceil({dims} / 2), the pairs of its stored dimensions"
            ));
        }
        if self.u32("matcher database", d + 0x130).map_err(s)? != 1
            || !zero(self.bytes("matcher database", d + 0x134, 0x2C).map_err(s)?)
        {
            return Err("its class-3 database's word at 0x130 is not 1 or a byte from 0x134 to 0x160 is not zero".into());
        }
        let f = frames as usize;
        if f == 0 {
            return Err("its class-3 database holds no frame".into());
        }
        let groups = f.div_ceil(LANES);
        let (blocks_at, count) =
            self.matcher_slot("matcher class-3 blocks", d + 0xE8, BLOCK_LEN)?;
        if groups.checked_mul(p) != Some(count) {
            return Err(format!(
                "its {count} class-3 blocks are not ceil({frames} / 8) groups of {p}"
            ));
        }
        let bytes =
            (self.bytes("matcher class-3 blocks", blocks_at, count * BLOCK_LEN)).map_err(s)?;
        // Stored dimension k of frame f, the frames padded to whole blocks.
        let mut by_dimension = vec![vec![0u16; LANES * groups]; 2 * p];
        for (i, block) in bytes.as_chunks::<BLOCK_LEN>().0.iter().enumerate() {
            let (g, q) = (i / p, i % p);
            let block = halves(block);
            let (even, odd) = block.split_at(LANES);
            for (lane, (a, b)) in even.iter().zip(odd).enumerate() {
                by_dimension[2 * q][LANES * g + lane] = *a;
                by_dimension[2 * q + 1][LANES * g + lane] = *b;
            }
        }
        for (k, dimension) in by_dimension.iter().enumerate() {
            let holds = if k < dims {
                dimension[f..].iter().all(|h| *h == dimension[f - 1])
            } else {
                dimension.iter().all(|h| signed_zero(*h))
            };
            if !holds {
                return Err(format!(
                    "its stored dimension {k} does not repeat frame {} in the frames that pad its last block, or is not zero past its {dims} dimensions",
                    f - 1
                ));
            }
        }
        let mut stored = Vec::with_capacity(f);
        for frame in 0..f {
            stored.push(by_dimension[..dims].iter().map(|k| k[frame]).collect());
        }
        if words.iter().any(|w| *w != LEVEL_WORD) {
            return Err(format!(
                "its level words are {words:?}, where only {LEVEL_WORD} is known"
            ));
        }
        let mut levels = Vec::with_capacity(words.len());
        for l in 0..words.len() {
            let width = (words[l..].iter())
                .try_fold(1u32, |w, word| w.checked_mul(*word))
                .ok_or_else(|| {
                    format!("its level {l} gives a lane more frames than a count holds")
                })?;
            let level = self.matcher_level(levels_at + LEVEL_LEN * l, l, width, p, f)?;
            for (i, (lo, hi)) in level.minima.iter().zip(&level.maxima).enumerate() {
                let (node, q) = (i / p, i % p);
                for lane in 0..LANES {
                    let (first, end) = lane_frames(node * LANES + lane, width, frames);
                    for k in 0..2 {
                        let dimension = 2 * q + k;
                        let (low, high) = (lo[LANES * k + lane], hi[LANES * k + lane]);
                        let holds = match by_dimension[..dims].get(dimension) {
                            Some(values) => {
                                let run = values[first..end].iter().copied();
                                run.clone().min_by_key(|h| rank(*h)) == Some(low)
                                    && run.max_by_key(|h| rank(*h)) == Some(high)
                            }
                            None => signed_zero(low) && signed_zero(high),
                        };
                        if !holds {
                            return Err(format!(
                                "its level {l}, node {node}, lane {lane} does not bound stored dimension {dimension} by the least and greatest of frames {first}..{end}"
                            ));
                        }
                    }
                }
            }
            levels.push(level);
        }
        Ok((pairs, stored, levels))
    }

    /// The level whose two heads are at `at`: `ceil(frames / (8 * width))` nodes of `p`
    /// elements in each.
    fn matcher_level(
        &self,
        at: usize,
        l: usize,
        width: u32,
        p: usize,
        frames: usize,
    ) -> Result<MatcherLevel, String> {
        let nodes = (frames as u64).div_ceil(LANES as u64 * u64::from(width)) as usize;
        let (minima_at, lows) = self.matcher_slot("matcher level minima", at, BLOCK_LEN)?;
        let (maxima_at, highs) = self.matcher_slot("matcher level maxima", at + 16, BLOCK_LEN)?;
        if lows != highs || nodes.checked_mul(p) != Some(lows) {
            return Err(format!(
                "its level {l} holds {lows} and {highs} elements, not {nodes} nodes of {p}"
            ));
        }
        let read = |first: usize| -> Result<Vec<[u16; 16]>, String> {
            let bytes = (self.bytes("matcher level", first, lows * BLOCK_LEN)).map_err(s)?;
            Ok(bytes
                .as_chunks::<BLOCK_LEN>()
                .0
                .iter()
                .map(|b| halves(b))
                .collect())
        };
        Ok(MatcherLevel {
            lane_frames: width,
            minima: read(minima_at)?,
            maxima: read(maxima_at)?,
        })
    }
}

/// The frames `first..end` lane `lane` of a level of `width` covers; a lane past the last
/// frame covers what the last lane holding a frame does.
fn lane_frames(lane: usize, width: u32, frames: u32) -> (usize, usize) {
    let (width, frames) = (u64::from(width), u64::from(frames));
    let last = (frames - 1) / width * width;
    let first = (lane as u64 * width).min(last);
    (first as usize, (first + width).min(frames) as usize)
}

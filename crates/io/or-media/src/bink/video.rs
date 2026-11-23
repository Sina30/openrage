//! The Bink 1 video decoder: one packet in, one planar 4:2:0 picture out.
//!
//! Origin: FFmpeg n7.0 (commit 083443d67cb159ce469e5d902346b8d0c2cd1c93), libavcodec, the file
//! bink.c, Copyright (c) 2009 Konstantin Shishkov and (C) 2011 Peter Ross, LGPL-2.1-or-later.
//! Ported to safe Rust with the reference's order of reads and its integer arithmetic kept, so a
//! picture matches it bit for bit; the revisions `f`, `g`, `h`, `i` and `k` are decoded, and the
//! old revision `b` is refused. Listed in LICENSE-third-party.md.

use std::sync::OnceLock;

use super::{dsp, tables};
use crate::bits::BitReader;
use crate::yuv::{Plane, Yuv420};
use crate::MediaError;

/// The video flag that says the picture carries an alpha plane.
const FLAG_ALPHA: u32 = 0x0010_0000;

/// The largest picture the container accepts.
const MAX_WIDTH: u32 = 7680;
const MAX_HEIGHT: u32 = 4800;

const SKIP_BLOCK: u8 = 0;
const SCALED_BLOCK: u8 = 1;
const MOTION_BLOCK: u8 = 2;
const RUN_BLOCK: u8 = 3;
const RESIDUE_BLOCK: u8 = 4;
const INTRA_BLOCK: u8 = 5;
const FILL_BLOCK: u8 = 6;
const INTER_BLOCK: u8 = 7;
const PATTERN_BLOCK: u8 = 8;
const RAW_BLOCK: u8 = 9;

// The nine streams of values a plane's rows are decoded from.
const BLOCK_TYPES: usize = 0;
const SUB_BLOCK_TYPES: usize = 1;
const COLORS: usize = 2;
const PATTERN: usize = 3;
const X_OFF: usize = 4;
const Y_OFF: usize = 5;
const INTRA_DC: usize = 6;
const INTER_DC: usize = 7;
const RUN: usize = 8;
const BUNDLES: usize = 9;

/// Bits the first DC value of a bundle takes.
const DC_START_BITS: u32 = 11;

/// What a run-length block type stands for: runs of the last type.
const RLE_LENS: [usize; 4] = [4, 8, 12, 32];

fn bad(what: &str) -> MediaError {
    MediaError::Decode(format!("this Bink 1 picture is corrupt: {what}"))
}

/// A Huffman code over four bits: the code table the tree number names.
struct Vlc {
    /// For each `bits`-bit prefix, the leaf it starts and the length of its code.
    table: Vec<(u8, u8)>,
    bits: u32,
}

impl Vlc {
    fn new(lens: &[u8; 16], codes: &[u8; 16]) -> Self {
        let bits = u32::from(lens.iter().copied().max().unwrap_or(0));
        let mut table = vec![(0u8, 0u8); 1 << bits];
        for (leaf, (&len, &code)) in lens.iter().zip(codes).enumerate() {
            for filler in 0..1usize << (bits - u32::from(len)) {
                table[usize::from(code) | filler << len] = (leaf as u8, len);
            }
        }
        Self { table, bits }
    }

    fn get(&self, gb: &mut BitReader) -> u8 {
        let (leaf, len) = self.table[gb.peek(self.bits) as usize];
        gb.skip(usize::from(len));
        leaf
    }
}

fn vlcs() -> &'static [Vlc; 16] {
    static TREES: OnceLock<[Vlc; 16]> = OnceLock::new();
    TREES.get_or_init(|| {
        std::array::from_fn(|i| Vlc::new(&tables::TREE_LENS[i], &tables::TREE_BITS[i]))
    })
}

/// A tree as a stream states it: which code table, and which symbol each leaf stands for.
#[derive(Clone, Copy, Default)]
struct Tree {
    vlc: usize,
    syms: [u8; 16],
}

impl Tree {
    fn get(&self, gb: &mut BitReader) -> u8 {
        self.syms[usize::from(vlcs()[self.vlc].get(gb))]
    }
}

/// Merges two lists of `size` leaves, `src[..size]` and `src[size..2 * size]`, by bits read.
fn merge(gb: &mut BitReader, dst: &mut [u8], src: &[u8], size: usize) {
    let (mut a, mut b) = (0, size);
    let (mut left_a, mut left_b) = (size, size);
    let mut at = 0;
    loop {
        if gb.get1() == 0 {
            dst[at] = src[a];
            a += 1;
            left_a -= 1;
        } else {
            dst[at] = src[b];
            b += 1;
            left_b -= 1;
        }
        at += 1;
        if left_a == 0 || left_b == 0 {
            break;
        }
    }
    for _ in 0..left_a {
        dst[at] = src[a];
        a += 1;
        at += 1;
    }
    for _ in 0..left_b {
        dst[at] = src[b];
        b += 1;
        at += 1;
    }
}

fn read_tree(gb: &mut BitReader) -> Result<Tree, MediaError> {
    if gb.bits_left() < 4 {
        return Err(bad("a tree runs past the end"));
    }
    let vlc = gb.get(4) as usize;
    let mut syms: [u8; 16] = std::array::from_fn(|i| i as u8);
    if vlc == 0 {
        return Ok(Tree { vlc, syms });
    }
    if gb.get1() != 0 {
        let mut len = gb.get(3) as usize;
        let mut taken = [false; 16];
        for sym in syms.iter_mut().take(len + 1) {
            *sym = gb.get(4) as u8;
            taken[usize::from(*sym)] = true;
        }
        for (leaf, _) in taken.iter().enumerate().filter(|(_, &t)| !t) {
            if len >= 15 {
                break;
            }
            len += 1;
            syms[len] = leaf as u8;
        }
    } else {
        let rounds = gb.get(2) as usize;
        let mut from: [u8; 16] = std::array::from_fn(|i| i as u8);
        let mut to = [0u8; 16];
        for round in 0..=rounds {
            let size = 1 << round;
            for t in (0..16).step_by(size * 2) {
                merge(gb, &mut to[t..t + 2 * size], &from[t..t + 2 * size], size);
            }
            std::mem::swap(&mut from, &mut to);
        }
        syms = from;
    }
    Ok(Tree { vlc, syms })
}

/// One stream of decoded values, and how far it is decoded and consumed.
struct Bundle {
    /// Bits of the count that heads each batch.
    len: u32,
    tree: Tree,
    data: Vec<u8>,
    /// Where the next decoded value goes; `None` once the stream is finished for the plane.
    cur_dec: Option<usize>,
    /// Where the next value is read from.
    cur_ptr: usize,
}

impl Bundle {
    fn reset(&mut self) {
        self.cur_dec = Some(0);
        self.cur_ptr = 0;
    }

    /// The count of values in the next batch, or `None` when there is none to decode.
    fn begin(&mut self, gb: &mut BitReader) -> Option<usize> {
        let dec = self.cur_dec?;
        if dec > self.cur_ptr {
            return None;
        }
        let count = gb.get(self.len) as usize;
        if count == 0 {
            self.cur_dec = None;
            return None;
        }
        Some(count)
    }

    /// The end of a batch of `count` values from `dec`, or an error if it overruns the buffer.
    fn end_of(&self, dec: usize, count: usize, what: &str) -> Result<usize, MediaError> {
        let end = dec + count;
        if end > self.data.len() {
            return Err(bad(what));
        }
        Ok(end)
    }

    fn fill(&mut self, from: usize, to: usize, value: u8) {
        self.data[from..to].fill(value);
    }
}

fn log2(x: usize) -> u32 {
    usize::BITS - 1 - x.leading_zeros()
}

/// A plane's bytes, padded to whole 16×16 blocks so a scaled block never leaves the buffer.
struct PlaneBuf {
    data: Vec<u8>,
    stride: usize,
}

impl PlaneBuf {
    fn new(blocks_wide: usize, blocks_high: usize) -> Self {
        let stride = (blocks_wide * 8).next_multiple_of(16);
        let rows = (blocks_high * 8).next_multiple_of(16);
        Self {
            data: vec![0; stride * rows],
            stride,
        }
    }

    fn view(&self, width: usize, height: usize) -> Plane<'_> {
        Plane {
            data: &self.data,
            stride: self.stride,
            width,
            height,
        }
    }
}

/// Every plane of one picture: Y, U, V and, when the movie has one, alpha.
struct Frame {
    planes: Vec<PlaneBuf>,
}

impl Frame {
    fn new(width: usize, height: usize, alpha: bool) -> Self {
        let luma = PlaneBuf::new(width.div_ceil(8), height.div_ceil(8));
        let chroma = || PlaneBuf::new((width + 15) >> 4, (height + 15) >> 4);
        let mut planes = vec![luma, chroma(), chroma()];
        if alpha {
            planes.push(PlaneBuf::new(width.div_ceil(8), height.div_ceil(8)));
        }
        Self { planes }
    }
}

/// What decoding a plane reads and writes besides the bits: the nine bundles and the colour trees.
struct Coder {
    version: u8,
    width: usize,
    height: usize,
    bundles: [Bundle; BUNDLES],
    col_high: [Tree; 16],
    col_lastval: usize,
}

impl Coder {
    fn new(version: u8, width: usize, height: usize) -> Self {
        let blocks = width.div_ceil(8) * height.div_ceil(8);
        let bundle = || Bundle {
            len: 0,
            tree: Tree::default(),
            data: vec![0; blocks * 64],
            cur_dec: Some(0),
            cur_ptr: 0,
        };
        Self {
            version,
            width,
            height,
            bundles: std::array::from_fn(|_| bundle()),
            col_high: [Tree::default(); 16],
            col_lastval: 0,
        }
    }

    fn init_lengths(&mut self, width: usize, bw: usize) {
        let width = width.next_multiple_of(8);
        let len = |x: usize| log2(x + 511) + 1;
        self.bundles[BLOCK_TYPES].len = len(width >> 3);
        self.bundles[SUB_BLOCK_TYPES].len = len(width >> 4);
        self.bundles[COLORS].len = len(bw * 64);
        for b in [INTRA_DC, INTER_DC, X_OFF, Y_OFF] {
            self.bundles[b].len = len(width >> 3);
        }
        self.bundles[PATTERN].len = len(bw << 3);
        self.bundles[RUN].len = len(bw * 48);
    }

    fn read_bundle(&mut self, gb: &mut BitReader, n: usize) -> Result<(), MediaError> {
        if n == COLORS {
            for tree in &mut self.col_high {
                *tree = read_tree(gb)?;
            }
            self.col_lastval = 0;
        }
        if n != INTRA_DC && n != INTER_DC {
            self.bundles[n].tree = read_tree(gb)?;
        }
        self.bundles[n].reset();
        Ok(())
    }

    fn read_runs(&mut self, gb: &mut BitReader) -> Result<(), MediaError> {
        let b = &mut self.bundles[RUN];
        let Some(count) = b.begin(gb) else {
            return Ok(());
        };
        let dec = b.cur_dec.unwrap_or(0);
        let end = b.end_of(dec, count, "a run value is out of bounds")?;
        if gb.bits_left() < 1 {
            return Err(bad("the run values run past the end"));
        }
        if gb.get1() != 0 {
            let v = gb.get(4) as u8;
            b.fill(dec, end, v);
        } else {
            for at in dec..end {
                b.data[at] = b.tree.get(gb);
            }
        }
        b.cur_dec = Some(end);
        Ok(())
    }

    fn read_motion_values(&mut self, gb: &mut BitReader, n: usize) -> Result<(), MediaError> {
        let b = &mut self.bundles[n];
        let Some(count) = b.begin(gb) else {
            return Ok(());
        };
        let dec = b.cur_dec.unwrap_or(0);
        let end = b.end_of(dec, count, "there are too many motion values")?;
        if gb.bits_left() < 1 {
            return Err(bad("the motion values run past the end"));
        }
        let signed = |v: u8, gb: &mut BitReader| -> u8 {
            if v == 0 {
                return 0;
            }
            let sign = -(gb.get1() as i32);
            ((i32::from(v) ^ sign) - sign) as u8
        };
        if gb.get1() != 0 {
            let v = gb.get(4) as u8;
            let v = signed(v, gb);
            b.fill(dec, end, v);
        } else {
            for at in dec..end {
                let v = b.tree.get(gb);
                b.data[at] = signed(v, gb);
            }
        }
        b.cur_dec = Some(end);
        Ok(())
    }

    fn read_block_types(&mut self, gb: &mut BitReader, n: usize) -> Result<(), MediaError> {
        let version = self.version;
        let b = &mut self.bundles[n];
        let Some(mut count) = b.begin(gb) else {
            return Ok(());
        };
        if version == b'k' {
            count ^= 0xBB;
            if count == 0 {
                b.cur_dec = None;
                return Ok(());
            }
        }
        let dec = b.cur_dec.unwrap_or(0);
        let end = b.end_of(dec, count, "there are too many block types")?;
        if gb.bits_left() < 1 {
            return Err(bad("the block types run past the end"));
        }
        let mut at = dec;
        if gb.get1() != 0 {
            let v = gb.get(4) as u8;
            b.fill(dec, end, v);
            at = end;
        } else {
            let mut last = 0u8;
            while at < end {
                let v = b.tree.get(gb);
                if v < 12 {
                    last = v;
                    b.data[at] = v;
                    at += 1;
                } else {
                    let run = RLE_LENS[usize::from(v - 12)];
                    if end - at < run {
                        return Err(bad("a run of block types overruns its batch"));
                    }
                    b.fill(at, at + run, last);
                    at += run;
                }
            }
        }
        b.cur_dec = Some(at);
        Ok(())
    }

    fn read_patterns(&mut self, gb: &mut BitReader) -> Result<(), MediaError> {
        let b = &mut self.bundles[PATTERN];
        let Some(count) = b.begin(gb) else {
            return Ok(());
        };
        let dec = b.cur_dec.unwrap_or(0);
        let end = b.end_of(dec, count, "there are too many patterns")?;
        for at in dec..end {
            if gb.bits_left() < 2 {
                return Err(bad("the patterns run past the end"));
            }
            let low = b.tree.get(gb);
            b.data[at] = low | b.tree.get(gb) << 4;
        }
        b.cur_dec = Some(end);
        Ok(())
    }

    fn read_colors(&mut self, gb: &mut BitReader) -> Result<(), MediaError> {
        let version = self.version;
        let b = &mut self.bundles[COLORS];
        let Some(count) = b.begin(gb) else {
            return Ok(());
        };
        let dec = b.cur_dec.unwrap_or(0);
        let end = b.end_of(dec, count, "there are too many colours")?;
        if gb.bits_left() < 1 {
            return Err(bad("the colours run past the end"));
        }
        let col_high = &self.col_high;
        let last = &mut self.col_lastval;
        let mut next = |b: &Bundle, gb: &mut BitReader| -> u8 {
            *last = usize::from(col_high[*last].get(gb));
            let mut v = (*last << 4) as i32 | i32::from(b.tree.get(gb));
            if version < b'i' {
                let sign = i32::from(v as i8) >> 7;
                v = ((v & 0x7F) ^ sign) - sign;
                v += 0x80;
            }
            v as u8
        };
        if gb.get1() != 0 {
            let v = next(b, gb);
            b.fill(dec, end, v);
        } else {
            for at in dec..end {
                if gb.bits_left() < 2 {
                    return Err(bad("the colours run past the end"));
                }
                b.data[at] = next(b, gb);
            }
        }
        b.cur_dec = Some(end);
        Ok(())
    }

    fn read_dcs(&mut self, gb: &mut BitReader, n: usize, has_sign: bool) -> Result<(), MediaError> {
        let b = &mut self.bundles[n];
        let Some(mut len) = b.begin(gb) else {
            return Ok(());
        };
        let start_bits = DC_START_BITS - u32::from(has_sign);
        if gb.bits_left() < i64::from(start_bits) {
            return Err(bad("the DC values run past the end"));
        }
        let mut dst = b.cur_dec.unwrap_or(0);
        let cap = b.data.len();
        let room = |dst: usize| cap.saturating_sub(dst) / 2;
        let mut v = gb.get(start_bits) as i32;
        if v != 0 && has_sign {
            let sign = -(gb.get1() as i32);
            v = (v ^ sign) - sign;
        }
        if room(dst) < 1 {
            return Err(bad("the DC values overrun their buffer"));
        }
        put_dc(b, &mut dst, v);
        len -= 1;
        let mut i = 0;
        while i < len {
            let len2 = (len - i).min(8);
            if room(dst) < len2 {
                return Err(bad("the DC values overrun their buffer"));
            }
            let bsize = gb.get(4);
            if bsize != 0 {
                for _ in 0..len2 {
                    let mut v2 = gb.get(bsize) as i32;
                    if v2 != 0 {
                        let sign = -(gb.get1() as i32);
                        v2 = (v2 ^ sign) - sign;
                    }
                    v += v2;
                    put_dc(b, &mut dst, v);
                    if !(-32768..=32767).contains(&v) {
                        return Err(bad("a DC value is out of range"));
                    }
                }
            } else {
                for _ in 0..len2 {
                    put_dc(b, &mut dst, v);
                }
            }
            i += 8;
        }
        b.cur_dec = Some(dst);
        Ok(())
    }

    fn value_u8(&mut self, n: usize) -> u8 {
        let b = &mut self.bundles[n];
        let v = b.data.get(b.cur_ptr).copied().unwrap_or(0);
        b.cur_ptr += 1;
        v
    }

    fn value_i8(&mut self, n: usize) -> i32 {
        i32::from(self.value_u8(n) as i8)
    }

    fn value_i16(&mut self, n: usize) -> i32 {
        let b = &mut self.bundles[n];
        let at = b.cur_ptr;
        b.cur_ptr += 2;
        match b.data.get(at..at + 2) {
            Some(two) => i32::from(i16::from_le_bytes([two[0], two[1]])),
            None => 0,
        }
    }

    /// The next 64 colours as a raw 8x8 block.
    fn raw_block(&mut self) -> [u8; 64] {
        let b = &mut self.bundles[COLORS];
        let mut block = [0u8; 64];
        if let Some(src) = b.data.get(b.cur_ptr..b.cur_ptr + 64) {
            block.copy_from_slice(src);
        }
        b.cur_ptr += 64;
        block
    }
}

/// Reads the coefficients of an intra or inter DCT block into `block`, in scan order.
///
/// Returns how many it read and the quantiser to unscale them with.
fn read_dct_coeffs(
    gb: &mut BitReader,
    block: &mut [i32; 64],
    coef_idx: &mut [usize; 64],
) -> Result<(usize, usize), MediaError> {
    let scan = &tables::SCAN;
    let mut coef_list = [0usize; 128];
    let mut mode_list = [0u8; 128];
    let (mut list_start, mut list_end) = (64usize, 64usize);
    let mut coef_count = 0;
    if gb.bits_left() < 4 {
        return Err(bad("a DCT block runs past the end"));
    }
    for (c, m) in [(4, 0), (24, 0), (44, 0), (1, 3), (2, 3), (3, 3)] {
        coef_list[list_end] = c;
        mode_list[list_end] = m;
        list_end += 1;
    }
    let mut bits = gb.get(4) as i32 - 1;
    while bits >= 0 {
        let mut list_pos = list_start;
        while list_pos < list_end {
            if (mode_list[list_pos] | u8::from(coef_list[list_pos] != 0)) == 0 || gb.get1() == 0 {
                list_pos += 1;
                continue;
            }
            let mut ccoef = coef_list[list_pos];
            let mode = mode_list[list_pos];
            match mode {
                0 | 2 => {
                    if mode == 0 {
                        coef_list[list_pos] = ccoef + 4;
                        mode_list[list_pos] = 1;
                    } else {
                        coef_list[list_pos] = 0;
                        mode_list[list_pos] = 0;
                        list_pos += 1;
                    }
                    for _ in 0..4 {
                        if gb.get1() != 0 {
                            list_start -= 1;
                            coef_list[list_start] = ccoef;
                            mode_list[list_start] = 3;
                        } else {
                            block[usize::from(scan[ccoef])] = read_coef(gb, bits);
                            coef_idx[coef_count] = ccoef;
                            coef_count += 1;
                        }
                        ccoef += 1;
                    }
                }
                1 => {
                    mode_list[list_pos] = 2;
                    for _ in 0..3 {
                        ccoef += 4;
                        coef_list[list_end] = ccoef;
                        mode_list[list_end] = 2;
                        list_end += 1;
                    }
                }
                _ => {
                    block[usize::from(scan[ccoef])] = read_coef(gb, bits);
                    coef_idx[coef_count] = ccoef;
                    coef_count += 1;
                    coef_list[list_pos] = 0;
                    mode_list[list_pos] = 0;
                    list_pos += 1;
                }
            }
        }
        bits -= 1;
    }
    Ok((coef_count, gb.get(4) as usize))
}

/// One coefficient at bit plane `bits`: its magnitude's lower bits, then its sign.
fn read_coef(gb: &mut BitReader, bits: i32) -> i32 {
    if bits == 0 {
        return 1 - ((gb.get1() as i32) << 1);
    }
    let t = (gb.get(bits as u32) | 1 << bits) as i32;
    let sign = -(gb.get1() as i32);
    (t ^ sign) - sign
}

fn unquantize(block: &mut [i32; 64], quant: &[i32; 64], coef_count: usize, coef_idx: &[usize; 64]) {
    let scan = &tables::SCAN;
    let scale = |v: i32, q: i32| ((v as u32).wrapping_mul(q as u32) as i32) >> 11;
    block[0] = scale(block[0], quant[0]);
    for &idx in &coef_idx[..coef_count] {
        let at = usize::from(scan[idx]);
        block[at] = scale(block[at], quant[idx]);
    }
}

/// Reads the residue of a block after motion, as at most `masks_count` bit planes of changes.
fn read_residue(gb: &mut BitReader, block: &mut [i16; 64], mut masks_count: i32) {
    let scan = &tables::SCAN;
    let mut coef_list = [0usize; 128];
    let mut mode_list = [0u8; 128];
    let (mut list_start, mut list_end) = (64usize, 64usize);
    let mut nz = [0usize; 64];
    let mut nz_count = 0;
    for (c, m) in [(4, 0), (24, 0), (44, 0), (0, 2)] {
        coef_list[list_end] = c;
        mode_list[list_end] = m;
        list_end += 1;
    }
    let mut mask = 1i32 << gb.get(3);
    while mask != 0 {
        for &at in &nz[..nz_count] {
            if gb.get1() == 0 {
                continue;
            }
            let v = i32::from(block[at]);
            block[at] = if v < 0 { v - mask } else { v + mask } as i16;
            masks_count -= 1;
            if masks_count < 0 {
                return;
            }
        }
        let mut list_pos = list_start;
        while list_pos < list_end {
            if (coef_list[list_pos] | usize::from(mode_list[list_pos])) == 0 || gb.get1() == 0 {
                list_pos += 1;
                continue;
            }
            let mut ccoef = coef_list[list_pos];
            let mode = mode_list[list_pos];
            match mode {
                0 | 2 => {
                    if mode == 0 {
                        coef_list[list_pos] = ccoef + 4;
                        mode_list[list_pos] = 1;
                    } else {
                        coef_list[list_pos] = 0;
                        mode_list[list_pos] = 0;
                        list_pos += 1;
                    }
                    for _ in 0..4 {
                        if gb.get1() != 0 {
                            list_start -= 1;
                            coef_list[list_start] = ccoef;
                            mode_list[list_start] = 3;
                        } else {
                            let at = usize::from(scan[ccoef]);
                            nz[nz_count] = at;
                            nz_count += 1;
                            let sign = -(gb.get1() as i32);
                            block[at] = ((mask ^ sign) - sign) as i16;
                            masks_count -= 1;
                            if masks_count < 0 {
                                return;
                            }
                        }
                        ccoef += 1;
                    }
                }
                1 => {
                    mode_list[list_pos] = 2;
                    for _ in 0..3 {
                        ccoef += 4;
                        coef_list[list_end] = ccoef;
                        mode_list[list_end] = 2;
                        list_end += 1;
                    }
                }
                _ => {
                    let at = usize::from(scan[ccoef]);
                    nz[nz_count] = at;
                    nz_count += 1;
                    let sign = -(gb.get1() as i32);
                    block[at] = ((mask ^ sign) - sign) as i16;
                    coef_list[list_pos] = 0;
                    mode_list[list_pos] = 0;
                    list_pos += 1;
                    masks_count -= 1;
                    if masks_count < 0 {
                        return;
                    }
                }
            }
        }
        mask >>= 1;
    }
}

fn copy8(dst: &mut [u8], to: usize, src: &[u8], from: usize, stride: usize) {
    for r in 0..8 {
        dst[to + r * stride..][..8].copy_from_slice(&src[from + r * stride..][..8]);
    }
}

fn fill_block(dst: &mut [u8], at: usize, stride: usize, size: usize, v: u8) {
    for r in 0..size {
        dst[at + r * stride..][..size].fill(v);
    }
}

impl Coder {
    /// Copies the 8x8 block a decoded motion vector points at in the previous picture.
    fn put_pixels(
        &mut self,
        dst: &mut PlaneBuf,
        prev: &PlaneBuf,
        at: usize,
        ref_end: usize,
    ) -> Result<(), MediaError> {
        let xoff = self.value_i8(X_OFF);
        let yoff = self.value_i8(Y_OFF);
        let from = at as isize + xoff as isize + yoff as isize * dst.stride as isize;
        if from < 0 || from as usize > ref_end {
            return Err(bad(&format!(
                "a motion vector ({xoff}, {yoff}) leaves the picture"
            )));
        }
        copy8(&mut dst.data, at, &prev.data, from as usize, dst.stride);
        Ok(())
    }

    /// The pixels of a run block or run sub-block, in the scan order a pattern names.
    fn runs(
        &mut self,
        gb: &mut BitReader,
        mut put: impl FnMut(usize, u8),
    ) -> Result<(), MediaError> {
        if gb.bits_left() < 4 {
            return Err(bad("a run block runs past the end"));
        }
        let scan = &tables::PATTERNS[gb.get(4) as usize];
        let mut i = 0usize;
        let mut at = 0usize;
        loop {
            let run = usize::from(self.value_u8(RUN)) + 1;
            i += run;
            if i > 64 {
                return Err(bad("a run goes out of bounds"));
            }
            if gb.get1() != 0 {
                let v = self.value_u8(COLORS);
                for _ in 0..run {
                    put(usize::from(scan[at]), v);
                    at += 1;
                }
            } else {
                for _ in 0..run {
                    let v = self.value_u8(COLORS);
                    put(usize::from(scan[at]), v);
                    at += 1;
                }
            }
            if i >= 63 {
                break;
            }
        }
        if i == 63 {
            let v = self.value_u8(COLORS);
            put(usize::from(scan[at]), v);
        }
        Ok(())
    }

    fn intra_block(
        &mut self,
        gb: &mut BitReader,
        put: impl FnOnce(&[i32; 64]),
    ) -> Result<(), MediaError> {
        let mut block = [0i32; 64];
        let mut coef_idx = [0usize; 64];
        block[0] = self.value_i16(INTRA_DC);
        let (count, q) = read_dct_coeffs(gb, &mut block, &mut coef_idx)?;
        unquantize(&mut block, &tables::INTRA_QUANT[q], count, &coef_idx);
        put(&block);
        Ok(())
    }

    fn decode_plane(
        &mut self,
        gb: &mut BitReader,
        dst: &mut PlaneBuf,
        prev: &PlaneBuf,
        is_chroma: bool,
    ) -> Result<(), MediaError> {
        let shift = u32::from(is_chroma);
        let (bw, bh) = if is_chroma {
            ((self.width + 15) >> 4, (self.height + 15) >> 4)
        } else {
            (self.width.div_ceil(8), self.height.div_ceil(8))
        };
        let width = self.width >> shift;
        let height = self.height >> shift;
        let stride = dst.stride;

        if self.version == b'k' && gb.get1() != 0 {
            let fill = gb.get(8) as u8;
            for row in 0..height {
                dst.data[row * stride..][..width].fill(fill);
            }
            gb.align32();
            return Ok(());
        }

        self.init_lengths(width.max(8), bw);
        for n in 0..BUNDLES {
            self.read_bundle(gb, n)?;
        }
        let ref_end = (bw - 1 + prev.stride * (bh - 1)) * 8;

        for by in 0..bh {
            self.read_block_types(gb, BLOCK_TYPES)?;
            self.read_block_types(gb, SUB_BLOCK_TYPES)?;
            self.read_colors(gb)?;
            self.read_patterns(gb)?;
            self.read_motion_values(gb, X_OFF)?;
            self.read_motion_values(gb, Y_OFF)?;
            self.read_dcs(gb, INTRA_DC, false)?;
            self.read_dcs(gb, INTER_DC, true)?;
            self.read_runs(gb)?;

            let mut bx = 0;
            while bx < bw {
                let at = 8 * by * stride + 8 * bx;
                let blk = self.value_u8(BLOCK_TYPES);
                // A 16x16 block on an odd line is part of the one above, already decoded.
                if ((by & 1) != 0 || (bx & 1) != 0) && blk == SCALED_BLOCK {
                    bx += 2;
                    continue;
                }
                match blk {
                    SKIP_BLOCK => copy8(&mut dst.data, at, &prev.data, at, stride),
                    SCALED_BLOCK => {
                        self.scaled_block(gb, dst, at)?;
                        bx += 1;
                    }
                    MOTION_BLOCK => self.put_pixels(dst, prev, at, ref_end)?,
                    RUN_BLOCK => {
                        let data = &mut dst.data;
                        self.runs(gb, |pos, v| {
                            data[at + (pos & 7) + (pos >> 3) * stride] = v;
                        })?;
                    }
                    RESIDUE_BLOCK => {
                        self.put_pixels(dst, prev, at, ref_end)?;
                        let mut block = [0i16; 64];
                        let masks = gb.get(7) as i32;
                        read_residue(gb, &mut block, masks);
                        dsp::add_pixels8(&mut dst.data, at, stride, &block);
                    }
                    INTRA_BLOCK => {
                        let data = &mut dst.data;
                        self.intra_block(gb, |block| dsp::idct_put(data, at, stride, block))?;
                    }
                    FILL_BLOCK => {
                        let v = self.value_u8(COLORS);
                        fill_block(&mut dst.data, at, stride, 8, v);
                    }
                    INTER_BLOCK => {
                        self.put_pixels(dst, prev, at, ref_end)?;
                        let mut block = [0i32; 64];
                        let mut coef_idx = [0usize; 64];
                        block[0] = self.value_i16(INTER_DC);
                        let (count, q) = read_dct_coeffs(gb, &mut block, &mut coef_idx)?;
                        unquantize(&mut block, &tables::INTER_QUANT[q], count, &coef_idx);
                        dsp::idct_add(&mut dst.data, at, stride, &block);
                    }
                    PATTERN_BLOCK => {
                        let col = [self.value_u8(COLORS), self.value_u8(COLORS)];
                        for r in 0..8 {
                            let mut v = self.value_u8(PATTERN);
                            for c in 0..8 {
                                dst.data[at + r * stride + c] = col[usize::from(v & 1)];
                                v >>= 1;
                            }
                        }
                    }
                    RAW_BLOCK => {
                        let block = self.raw_block();
                        for r in 0..8 {
                            dst.data[at + r * stride..][..8].copy_from_slice(&block[r * 8..][..8]);
                        }
                    }
                    other => return Err(bad(&format!("block type {other} is unknown"))),
                }
                bx += 1;
            }
        }
        gb.align32();
        Ok(())
    }

    /// A 16x16 block: an 8x8 one of its own sub-type drawn at twice the size.
    fn scaled_block(
        &mut self,
        gb: &mut BitReader,
        dst: &mut PlaneBuf,
        at: usize,
    ) -> Result<(), MediaError> {
        let stride = dst.stride;
        let mut ublock = [0u8; 64];
        let sub = self.value_u8(SUB_BLOCK_TYPES);
        match sub {
            RUN_BLOCK => self.runs(gb, |pos, v| ublock[pos] = v)?,
            INTRA_BLOCK => self.intra_block(gb, |block| {
                let mut temp = [0u8; 64];
                dsp::idct_put(&mut temp, 0, 8, block);
                ublock = temp;
            })?,
            FILL_BLOCK => {
                let v = self.value_u8(COLORS);
                fill_block(&mut dst.data, at, stride, 16, v);
                return Ok(());
            }
            PATTERN_BLOCK => {
                let col = [self.value_u8(COLORS), self.value_u8(COLORS)];
                for j in 0..8 {
                    let mut v = self.value_u8(PATTERN);
                    for i in 0..8 {
                        ublock[i + j * 8] = col[usize::from(v & 1)];
                        v >>= 1;
                    }
                }
            }
            RAW_BLOCK => {
                for j in 0..8 {
                    for i in 0..8 {
                        ublock[i + j * 8] = self.value_u8(COLORS);
                    }
                }
            }
            other => return Err(bad(&format!("16x16 block type {other} is unknown"))),
        }
        dsp::scale_block(&ublock, &mut dst.data, at, stride);
        Ok(())
    }
}

/// A Bink 1 video decoder over the packets of one movie.
pub struct Decoder {
    coder: Coder,
    has_alpha: bool,
    swap_planes: bool,
    full_range: bool,
    /// The picture last decoded, which the next one's motion refers to.
    last: Frame,
    scratch: Frame,
    width: usize,
    height: usize,
}

impl Decoder {
    /// A decoder for a movie of this revision (the fourth byte of its signature), video flags and size.
    pub fn new(revision: u8, flags: u32, width: u32, height: u32) -> Result<Self, MediaError> {
        if !matches!(revision, b'f' | b'g' | b'h' | b'i' | b'k') {
            return Err(MediaError::Decode(format!(
                "Bink 1 revision '{}' is not one OpenRage decodes",
                char::from(revision)
            )));
        }
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(MediaError::Decode(format!(
                "a {width} x {height} Bink picture is not one OpenRage decodes"
            )));
        }
        let (w, h) = (width as usize, height as usize);
        let has_alpha = flags & FLAG_ALPHA != 0;
        Ok(Self {
            coder: Coder::new(revision, w, h),
            has_alpha,
            swap_planes: revision >= b'h',
            full_range: revision == b'k',
            last: Frame::new(w, h, has_alpha),
            scratch: Frame::new(w, h, has_alpha),
            width: w,
            height: h,
        })
    }

    /// Decodes one video packet; the picture it returns lives until the next call.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Yuv420<'_>, MediaError> {
        let mut gb = BitReader::new(packet);
        let total_bits = (packet.len() * 8) as i64;
        let version = self.coder.version;
        if self.has_alpha {
            if version >= b'i' {
                gb.skip(32);
            }
            self.plane(&mut gb, 3, false)?;
        }
        if version >= b'i' {
            gb.skip(32);
        }
        for plane in 0..3 {
            let idx = if plane == 0 || !self.swap_planes {
                plane
            } else {
                plane ^ 3
            };
            self.plane(&mut gb, idx, plane != 0)?;
            if gb.bit_pos() as i64 >= total_bits {
                break;
            }
        }
        std::mem::swap(&mut self.last, &mut self.scratch);
        let (w, h) = (self.width, self.height);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let planes = &self.last.planes;
        Ok(Yuv420 {
            width: w,
            height: h,
            full_range: self.full_range,
            y: planes[0].view(w, h),
            u: planes[1].view(cw, ch),
            v: planes[2].view(cw, ch),
            alpha: planes.get(3).map(|a| a.view(w, h)),
        })
    }

    fn plane(&mut self, gb: &mut BitReader, idx: usize, is_chroma: bool) -> Result<(), MediaError> {
        self.coder.decode_plane(
            gb,
            &mut self.scratch.planes[idx],
            &self.last.planes[idx],
            is_chroma,
        )
    }

    /// Forgets the previous picture, so the next packet must be a key frame.
    pub fn reset(&mut self) {
        self.last = Frame::new(self.width, self.height, self.has_alpha);
    }
}

fn put_dc(b: &mut Bundle, dst: &mut usize, v: i32) {
    b.data[*dst..*dst + 2].copy_from_slice(&(v as i16).to_le_bytes());
    *dst += 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bits written as text, the first one read being the first one written.
    fn reader(bits: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (i, bit) in bits.chars().filter(|c| !c.is_whitespace()).enumerate() {
            if i % 8 == 0 {
                bytes.push(0);
            }
            if bit == '1' {
                *bytes.last_mut().expect("a byte") |= 1 << (i % 8);
            }
        }
        bytes
    }

    #[test]
    fn every_code_table_covers_every_prefix_and_decodes_each_leaf_from_its_code() {
        for (n, vlc) in vlcs().iter().enumerate() {
            assert!(
                vlc.table.iter().all(|&(_, len)| len > 0),
                "tree {n} leaves a prefix with no code"
            );
            for leaf in 0..16 {
                let (len, code) = (tables::TREE_LENS[n][leaf], tables::TREE_BITS[n][leaf]);
                let written: String = (0..len)
                    .map(|i| if code >> i & 1 == 1 { '1' } else { '0' })
                    .collect();
                let bytes = reader(&written);
                let mut gb = BitReader::new(&bytes);
                assert_eq!(vlc.get(&mut gb), leaf as u8, "tree {n} leaf {leaf}");
                assert_eq!(
                    gb.bit_pos(),
                    usize::from(len),
                    "tree {n} leaf {leaf} takes its code's length"
                );
            }
        }
    }

    #[test]
    fn a_tree_is_the_identity_listed_leaves_then_the_rest_or_merged_by_bits() {
        let identity: [u8; 16] = std::array::from_fn(|i| i as u8);
        let bytes = reader("0000");
        let mut gb = BitReader::new(&bytes);
        let tree = read_tree(&mut gb).expect("a tree");
        assert_eq!((tree.vlc, tree.syms, gb.bit_pos()), (0, identity, 4));

        // Tree 1, listing: leaves 5, 2 and 9 first, then the others in order.
        let bytes = reader("1000 1 010 1010 0100 1001");
        let mut gb = BitReader::new(&bytes);
        let tree = read_tree(&mut gb).expect("a listed tree");
        let rest = [0u8, 1, 3, 4, 6, 7, 8, 10, 11, 12, 13, 14, 15];
        assert_eq!(tree.vlc, 1);
        assert_eq!(&tree.syms[..3], [5, 2, 9]);
        assert_eq!(&tree.syms[3..], rest);

        // Tree 1, merged over one round: eight bits of 1 take each pair's second leaf first.
        let bytes = reader("1000 0 00 11111111");
        let mut gb = BitReader::new(&bytes);
        let tree = read_tree(&mut gb).expect("a merged tree");
        let swapped: [u8; 16] = std::array::from_fn(|i| (i ^ 1) as u8);
        assert_eq!(tree.syms, swapped);
        assert_eq!(gb.bit_pos(), 4 + 1 + 2 + 8);

        let mut gb = BitReader::new(&[]);
        assert!(read_tree(&mut gb).is_err(), "a tree needs its four bits");
    }

    #[test]
    fn dct_coefficients_are_read_by_bit_plane_and_the_quantiser_follows_them() {
        // One bit plane (bits = 0): the three runs at 4, 24 and 44 are skipped, coefficient 1 is
        // significant and positive, 2 significant and negative, 3 is skipped; the quantiser is 7.
        let bytes = reader("1000 0 0 0 1 0 1 1 0 1110");
        let mut gb = BitReader::new(&bytes);
        let (mut block, mut idx) = ([0i32; 64], [0usize; 64]);
        let (count, quant) = read_dct_coeffs(&mut gb, &mut block, &mut idx).expect("a block");
        assert_eq!((count, quant), (2, 7));
        assert_eq!(&idx[..2], [1, 2]);
        assert_eq!(block[usize::from(tables::SCAN[1])], 1);
        assert_eq!(block[usize::from(tables::SCAN[2])], -1);
        assert_eq!(block.iter().filter(|&&v| v != 0).count(), 2);
    }

    #[test]
    fn a_residue_reads_signed_masks_into_the_scanned_positions() {
        // The mask is 1; the entry at 4 is entered, its first coefficient is significant and
        // negative and the other three wait; nothing else is entered.
        let bytes = reader("000 1 0 1 1 1 1 0 0 0 0");
        let mut gb = BitReader::new(&bytes);
        let mut block = [0i16; 64];
        read_residue(&mut gb, &mut block, 64);
        assert_eq!(block[usize::from(tables::SCAN[4])], -1);
        assert_eq!(block.iter().filter(|&&v| v != 0).count(), 1);
    }

    #[test]
    fn a_packet_that_ends_early_is_refused_and_so_is_a_revision_the_port_does_not_decode() {
        let mut decoder = Decoder::new(b'i', 0, 16, 16).expect("a 16x16 movie");
        assert!(decoder.decode(&[0u8; 8]).is_err());
        assert!(decoder.decode(&[]).is_err());
        let Err(why) = Decoder::new(b'b', 0, 16, 16) else {
            panic!("revision b is refused");
        };
        assert!(why.to_string().contains("'b'"), "{why}");
        assert!(Decoder::new(b'i', 0, 0, 16).is_err());
        assert!(Decoder::new(b'i', 0, 9000, 16).is_err());
    }
}

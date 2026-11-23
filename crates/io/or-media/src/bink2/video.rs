//! The Bink 2 video decoder: one packet in, one planar 4:2:0 picture out.
//!
//! Origin: NihAV (https://git.nihav.org/?p=nihav.git), commit
//! 13f31633cf325b0036d697af7bf05c611b4b8967 of 2026-09-28, the file nihav-rad/src/codecs/bink2.rs
//! (`Bink2Decoder`, `decode_frame_new` and the readers it calls), Copyright (c) Kostya Shishkov,
//! AGPL-3.0-only. Ported to safe Rust with the reference's order of reads and its integer
//! arithmetic kept, except in three sums the install's own decoder fixed; the revisions `g` to `k`
//! and GTA VI's `n` are decoded, and a frame in the coding of `KB2f` and before, which no shipped
//! movie uses, is refused.
//!
//! NihAV applies no in-loop filter and reads a vector where it points. What the install's own decoder
//! does instead is derived from RAD Game Tools' proprietary `bink2w64.dll`, version 2020.07 (the
//! Enhanced install's), by reading its disassembly for interoperability: the clamp of a square's
//! position to the picture (`Reach`, the function at 0x18001d350), the record of each 8×8 block
//! the filter reads (the function at 0x180011ae0, `filter::Records`) and the filter itself, which
//! `filter.rs` names. Listed in LICENSE-third-party.md.

use std::sync::OnceLock;

use super::dsp::{self, Mv, Ref};
use super::filter::{self, Records};
use super::tables;
use crate::bits::BitReader;
use crate::yuv::{Plane, Yuv420};
use crate::MediaError;

/// The video flag that says the picture carries an alpha plane.
const FLAG_ALPHA: u32 = 0x0010_0000;

/// The frame flag of the coding `KB2g` and later use.
const FRAME_NEW: u32 = 0x2000;

/// The largest picture the container accepts.
const MAX_WIDTH: u32 = 7680;
const MAX_HEIGHT: u32 = 4800;

/// The coded blocks of 8×8 a transform stage works on: four of them make a 16×16 square.
type Blocks = [[i32; 64]; 4];

fn bad(what: &str) -> MediaError {
    MediaError::Decode(format!("this Bink 2 picture is corrupt: {what}"))
}

/// A DC as the transform takes it: scaled by 8, and rounded by the 32 that its final shift by 6 wants, in an
/// inter block as in an intra one.
fn dc_coefficient(dc: i32) -> i32 {
    dc * 8 + 32
}

/// A Huffman code the next bits start, read lowest bit first, as a lookup over its longest code.
struct Vlc {
    bits: u32,
    /// For each `bits`-bit prefix, the symbol it starts and the length of its code.
    table: Vec<(u8, u8)>,
}

impl Vlc {
    fn new(codes: &[u16], lens: &[u8]) -> Self {
        let bits = u32::from(lens.iter().copied().max().unwrap_or(0));
        let mut table = vec![(0u8, 0u8); 1 << bits];
        for (symbol, (&len, &code)) in lens.iter().zip(codes).enumerate() {
            for filler in 0..1usize << (bits - u32::from(len)) {
                table[usize::from(code) | filler << len] = (symbol as u8, len);
            }
        }
        Self { bits, table }
    }
}

/// The three codes a KB2g and later frame reads.
struct Codes {
    ac_dense: Vlc,
    ac_sparse: Vlc,
    mv: Vlc,
}

fn codes() -> &'static Codes {
    static CODES: OnceLock<Codes> = OnceLock::new();
    CODES.get_or_init(|| Codes {
        ac_dense: Vlc::new(tables::AC_CODES1, tables::AC_BITS1),
        ac_sparse: Vlc::new(tables::AC_CODES2, tables::AC_BITS2),
        mv: Vlc::new(tables::MV_CODES, tables::MV_BITS),
    })
}

/// A bit reader that refuses a read the packet cannot satisfy, as the reference's does.
struct Bits<'a> {
    data: &'a [u8],
    inner: BitReader<'a>,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            inner: BitReader::new(data),
        }
    }

    fn read(&mut self, n: u32) -> Result<u32, MediaError> {
        if n == 0 {
            return Ok(0);
        }
        if self.inner.bits_left() < i64::from(n) {
            return Err(bad("a read runs past the end of the packet"));
        }
        Ok(self.inner.get(n))
    }

    fn bit(&mut self) -> Result<bool, MediaError> {
        Ok(self.read(1)? != 0)
    }

    /// Moves to byte `offset` of the packet.
    fn seek(&mut self, offset: usize) -> Result<(), MediaError> {
        let rest = self
            .data
            .get(offset..)
            .ok_or_else(|| bad("a slice starts beyond the end of the packet"))?;
        self.inner = BitReader::new(rest);
        Ok(())
    }

    /// A run of bits that are not `stop`, at most `max` of them, ended by a `stop` bit unless the
    /// run reaches `max`: the count.
    fn unary(&mut self, max: u32, stop: u32) -> Result<u32, MediaError> {
        let mut count = 0;
        loop {
            if self.read(1)? == stop {
                return Ok(count);
            }
            count += 1;
            if count == max {
                return Ok(count);
            }
        }
    }

    fn vlc(&mut self, vlc: &Vlc) -> Result<u8, MediaError> {
        let (symbol, len) = vlc.table[self.inner.peek(vlc.bits) as usize];
        if len == 0 || i64::from(len) > self.inner.bits_left() {
            return Err(bad("a bit pattern that is no code"));
        }
        self.inner.skip(usize::from(len));
        Ok(symbol)
    }

    /// An unsigned value of an escape-coded run of bits: below 4 as it stands, else a count of
    /// extra bits and the value they add to.
    fn level(&mut self) -> Result<i32, MediaError> {
        let prefix = self.unary(12, 0)? as i32;
        if prefix < 4 {
            Ok(prefix)
        } else {
            let extra = self.read(prefix as u32 - 3)? as i32;
            Ok((1 << (prefix - 3)) + extra + 2)
        }
    }

    /// A signed AC level: never zero, its sign the last bit.
    fn level_nonzero(&mut self) -> Result<i32, MediaError> {
        let prefix = self.unary(12, 0)? as i32 + 1;
        let value = if prefix >= 4 {
            let extra = self.read(prefix as u32 - 3)? as i32;
            (1 << (prefix - 3)) + extra + 2
        } else {
            prefix
        };
        Ok(if self.bit()? { -value } else { value })
    }
}

/// The median of three, which is how most predictions here are made.
fn median<T: Ord + Copy>(a: T, b: T, c: T) -> T {
    a.max(b).min(a.min(b).max(c))
}

/// The quantiser of each coded 16×16 half-row of blocks, predicted from the line above and the block before.
#[derive(Default)]
struct QuantInfo {
    quants: Vec<u8>,
    stride: usize,
    pos: usize,
    prev_pos: usize,
}

impl QuantInfo {
    fn resize(&mut self, blocks_wide: usize) {
        self.stride = blocks_wide * 2;
        self.quants.resize(self.stride * 2, 0);
        self.pos = 0;
        self.prev_pos = self.stride;
    }

    fn update_line(&mut self) {
        std::mem::swap(&mut self.pos, &mut self.prev_pos);
    }

    fn pred(&self, at: usize, is_top: bool) -> u8 {
        let is_left = at < 2;
        if is_top {
            if is_left {
                16
            } else {
                self.quants[self.pos + at - 2]
            }
        } else if is_left {
            self.quants[self.prev_pos + at]
        } else {
            median(
                self.quants[self.prev_pos + at - 2],
                self.quants[self.prev_pos + at],
                self.quants[self.pos + at - 2],
            )
        }
    }

    fn set(&mut self, at: usize, q: u8) {
        self.quants[self.pos + at] = q;
    }
}

/// `a` and `b` extrapolated one step, kept between them.
fn dc_pred2(a: i32, b: i32) -> i32 {
    a.max(b).min(a.min(b).max(a * 2 - b))
}

/// The gradient `b - a + c` of three neighbours, kept between the smallest and the largest.
fn dc_pred(a: i32, b: i32, c: i32) -> i32 {
    a.max(b).max(c).min(a.min(b).min(c).max(b - a + c))
}

/// Reads a block's DC residuals, scaled by the quantiser; `false` when none are coded.
fn decode_dcs(br: &mut Bits, dcs: &mut [i32], q: u8) -> Result<bool, MediaError> {
    if !br.bit()? {
        dcs.fill(0);
        return Ok(false);
    }
    let quant = tables::QUANT_DC[usize::from(q.max(8))];
    for dc in dcs.iter_mut() {
        *dc = br.level()?;
        if *dc != 0 {
            *dc = ((i64::from(*dc) * i64::from(quant) + 0x1FF) >> 10) as i32;
            if br.bit()? {
                *dc = -*dc;
            }
        }
    }
    Ok(true)
}

/// The DCs of a luma plane's 4×4 grid of 8×8 blocks in a 32×32 square, as the square before and
/// the line above leave them.
#[derive(Default)]
struct YDcInfo {
    dcs: [i32; 16],
    residuals: [i32; 16],
    prev_line: Vec<i32>,
    line: Vec<i32>,
}

impl YDcInfo {
    fn resize(&mut self, blocks_wide: usize) {
        self.prev_line.resize(blocks_wide * 4 + 4, 0);
        self.line.resize(blocks_wide * 4 + 4, 0);
    }

    fn update_line(&mut self) {
        std::mem::swap(&mut self.prev_line, &mut self.line);
    }

    /// After a block with no DCs of its own, the ones its pixels give.
    fn set_from_pixels(&mut self, bx: usize, dc: [i32; 7]) {
        let [dc5, dc7, dc13, dc10, dc11, dc14, dc15] = dc;
        self.dcs[5] = dc5;
        self.dcs[7] = dc7;
        self.dcs[13] = dc13;
        self.dcs[15] = dc15;
        self.line[bx * 4 + 4..][..4].copy_from_slice(&[dc10, dc11, dc14, dc15]);
    }

    fn decode(&mut self, br: &mut Bits, q: u8) -> Result<bool, MediaError> {
        decode_dcs(br, &mut self.residuals, q)
    }

    /// The DCs of an intra square: each residual added to a prediction from its neighbours.
    fn predict(&mut self, bx: usize, is_top: bool, is_left: bool, min_dc: i32, max_dc: i32) {
        let p = &self.prev_line[bx * 4..];
        let d = &self.dcs;
        let r = &self.residuals;
        let c = |v: i32| v.max(min_dc).min(max_dc);
        let (a0, a1, a2) = match (is_left, is_top) {
            (true, true) => {
                let a0 = c(r[0] + (1 << 10));
                let a1 = c(r[1] + a0);
                (a0, a1, c(r[2] + dc_pred2(a0, a1)))
            }
            (true, false) => {
                let a0 = c(r[0] + dc_pred2(p[4], p[5]));
                let a1 = c(r[1] + dc_pred(p[4], p[5], a0));
                (a0, a1, c(r[2] + dc_pred2(a0, a1)))
            }
            (false, true) => {
                let a0 = c(r[0] + dc_pred2(d[5], d[7]));
                (a0, c(r[1] + a0), c(r[2] + dc_pred(d[5], d[7], a0)))
            }
            (false, false) => {
                let a0 = c(r[0] + dc_pred(p[3], p[4], d[5]));
                let a1 = c(r[1] + dc_pred(p[4], p[5], a0));
                (a0, a1, c(r[2] + dc_pred(d[5], d[7], a0)))
            }
        };
        let a3 = c(r[3] + dc_pred(a0, a1, a2));
        let (a4, a5) = if is_top {
            let a4 = c(r[4] + dc_pred2(a1, a3));
            (a4, c(r[5] + a4))
        } else {
            let a4 = c(r[4] + dc_pred(p[5], p[6], a1));
            (a4, c(r[5] + dc_pred(p[6], p[7], a4)))
        };
        let a6 = c(r[6] + dc_pred(a1, a3, a4));
        let a7 = c(r[7] + dc_pred(a4, a5, a6));
        let a8 = if is_left {
            c(r[8] + dc_pred2(a2, a3))
        } else {
            c(r[8] + dc_pred(d[7], d[13], a2))
        };
        let a9 = c(r[9] + dc_pred(a2, a3, a8));
        let a10 = if is_left {
            c(r[10] + dc_pred2(a8, a9))
        } else {
            c(r[10] + dc_pred(d[13], d[15], a8))
        };
        let a11 = c(r[11] + dc_pred(a8, a9, a10));
        let a12 = c(r[12] + dc_pred(a3, a6, a9));
        let a13 = c(r[13] + dc_pred(a6, a7, a12));
        let a14 = c(r[14] + dc_pred(a9, a11, a12));
        let a15 = c(r[15] + dc_pred(a12, a13, a14));
        self.dcs = [
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15,
        ];
        self.line[bx * 4 + 4..][..4].copy_from_slice(&[a10, a11, a14, a15]);
    }

    /// The DCs of an inter square: residuals added to predictions from inside the square alone.
    fn predict_inter(&mut self, min_dc: i32, max_dc: i32) {
        let r = &self.residuals;
        let c = |v: i32| v.max(min_dc).min(max_dc);
        let a0 = c(r[0]);
        let a1 = c(r[1] + a0);
        let a2 = c(r[2] + dc_pred2(a0, a1));
        let a3 = c(r[3] + dc_pred(a0, a1, a2));
        let a4 = c(r[4] + dc_pred2(a1, a3));
        let a5 = c(r[5] + a4);
        let a6 = c(r[6] + dc_pred(a1, a3, a4));
        let a7 = c(r[7] + dc_pred(a4, a5, a6));
        let a8 = c(r[8] + dc_pred2(a2, a3));
        let a9 = c(r[9] + dc_pred(a2, a3, a8));
        let a10 = c(r[10] + dc_pred2(a8, a9));
        let a11 = c(r[11] + dc_pred(a8, a9, a10));
        let a12 = c(r[12] + dc_pred(a3, a6, a9));
        let a13 = c(r[13] + dc_pred(a6, a7, a12));
        let a14 = c(r[14] + dc_pred(a9, a11, a12));
        let a15 = c(r[15] + dc_pred(a12, a13, a14));
        self.dcs = [
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15,
        ];
    }
}

/// The DCs of a chroma plane's 2×2 grid of 8×8 blocks in a 16×16 square.
#[derive(Default)]
struct CDcInfo {
    dcs: [i32; 4],
    residuals: [i32; 4],
    prev_line: Vec<i32>,
    line: Vec<i32>,
}

impl CDcInfo {
    fn resize(&mut self, blocks_wide: usize) {
        self.prev_line.resize(blocks_wide * 2 + 2, 0);
        self.line.resize(blocks_wide * 2 + 2, 0);
    }

    fn update_line(&mut self) {
        std::mem::swap(&mut self.prev_line, &mut self.line);
    }

    fn set_from_pixels(&mut self, bx: usize, dc: [i32; 3]) {
        let [dc1, dc2, dc3] = dc;
        self.dcs[1] = dc1;
        self.dcs[2] = dc2;
        self.dcs[3] = dc3;
        self.line[bx * 2 + 2..][..2].copy_from_slice(&[dc2, dc3]);
    }

    fn decode(&mut self, br: &mut Bits, q: u8) -> Result<bool, MediaError> {
        decode_dcs(br, &mut self.residuals, q)
    }

    fn predict(&mut self, bx: usize, is_top: bool, is_left: bool, min_dc: i32, max_dc: i32) {
        let p = &self.prev_line[bx * 2..];
        let d = &self.dcs;
        let r = &self.residuals;
        let c = |v: i32| v.max(min_dc).min(max_dc);
        let (a0, a1);
        match (is_left, is_top) {
            (true, true) => {
                a0 = c(r[0] + (1 << 10));
                a1 = c(r[1] + a0);
            }
            (true, false) => {
                a0 = c(r[0] + dc_pred2(p[2], p[3]));
                a1 = c(r[1] + dc_pred(p[2], p[3], a0));
            }
            (false, true) => {
                a0 = c(r[0] + dc_pred2(d[1], d[3]));
                a1 = c(r[1] + a0);
            }
            (false, false) => {
                a0 = c(r[0] + dc_pred(p[1], p[2], d[1]));
                a1 = c(r[1] + dc_pred(p[2], p[3], a0));
            }
        }
        let a2 = if is_left {
            c(r[2] + dc_pred2(a0, a1))
        } else {
            c(r[2] + dc_pred(d[1], d[3], a0))
        };
        let a3 = c(r[3] + dc_pred(a0, a1, a2));
        self.dcs = [a0, a1, a2, a3];
        self.line[bx * 2 + 2..][..2].copy_from_slice(&[a2, a3]);
    }

    fn predict_inter(&mut self, min_dc: i32, max_dc: i32) {
        let r = &self.residuals;
        let c = |v: i32| v.max(min_dc).min(max_dc);
        let a0 = c(r[0]);
        let a1 = c(r[1] + a0);
        let a2 = c(r[2] + dc_pred2(a0, a1));
        let a3 = c(r[3] + dc_pred(a0, a1, a2));
        self.dcs = [a0, a1, a2, a3];
    }
}

/// The motion vectors of the four 16×16 squares of each 32×32 block of a line and the one above it.
#[derive(Default)]
struct MvInfo {
    mvs: Vec<Mv>,
    stride: usize,
    pos: usize,
    prev_pos: usize,
}

impl MvInfo {
    fn resize(&mut self, blocks_wide: usize) {
        self.stride = blocks_wide * 4;
        self.pos = 0;
        self.prev_pos = self.stride;
        self.mvs.resize(self.stride * 2, Mv::default());
    }

    fn update_line(&mut self) {
        std::mem::swap(&mut self.pos, &mut self.prev_pos);
    }

    fn get(&self, bx: usize, square: usize) -> Mv {
        self.mvs[self.pos + bx * 4 + square]
    }

    fn zero(&mut self, bx: usize) {
        let at = self.pos + bx * 4;
        self.mvs[at..at + 4].fill(Mv::default());
    }

    /// The vectors a motion or inter block states: one for the block or one a square, each the
    /// difference from a prediction its neighbours make.
    fn decode(
        &mut self,
        br: &mut Bits,
        bx: usize,
        is_top: bool,
        is_left: bool,
    ) -> Result<(), MediaError> {
        let count = if br.bit()? { 1 } else { 4 };
        let at = self.pos + bx * 4;
        let above = self.prev_pos + bx * 4;
        let mv = &mut self.mvs;
        for comp in 0..2 {
            for i in 0..count {
                let symbol = br.vlc(&codes().mv)?;
                let coded = tables::MV_SYMS[usize::from(symbol)];
                let value = if coded != tables::MV_ESC {
                    i16::from(coded)
                } else {
                    let len = br.unary(12, 1)?;
                    let bits = len + 4;
                    let uval = (br.read(bits)? as i32) + (1 << bits) - 1;
                    if uval & 1 != 0 {
                        (-(uval >> 1) - 1) as i16
                    } else {
                        (uval >> 1) as i16
                    }
                };
                if comp == 0 {
                    mv[at + i].x = value;
                } else {
                    mv[at + i].y = value;
                }
            }
        }
        if count > 1 {
            if is_top {
                if !is_left {
                    mv[at] = mv[at] + Mv::pred(mv[at - 4], mv[at - 3], mv[at - 1]);
                    mv[at + 2] = mv[at + 2] + Mv::pred(mv[at - 3], mv[at - 1], mv[at]);
                    mv[at + 1] = mv[at + 1] + Mv::pred(mv[at - 3], mv[at], mv[at + 2]);
                    mv[at + 3] = mv[at + 3] + Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
                } else {
                    mv[at + 1] = mv[at + 1] + mv[at];
                    mv[at + 2] = mv[at + 2] + mv[at];
                    mv[at + 3] = mv[at + 3] + Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
                }
            } else if is_left {
                mv[at] = mv[at] + Mv::pred(mv[above], mv[above + 2], mv[above + 3]);
                mv[at + 1] = mv[at + 1] + Mv::pred(mv[above + 2], mv[above + 3], mv[at]);
                mv[at + 2] = mv[at + 2] + Mv::pred(mv[above + 2], mv[at], mv[at + 1]);
                mv[at + 3] = mv[at + 3] + Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
            } else {
                mv[at] = mv[at] + Mv::pred(mv[above + 2], mv[above - 1], mv[at - 3]);
                mv[at + 1] = mv[at + 1] + Mv::pred(mv[above + 2], mv[above + 3], mv[at]);
                mv[at + 2] = mv[at + 2] + Mv::pred(mv[at - 3], mv[at - 1], mv[at]);
                mv[at + 3] = mv[at + 3] + Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
            }
        } else {
            let mut whole = mv[at];
            if is_top {
                if !is_left {
                    whole = whole + Mv::pred(mv[at - 4], mv[at - 3], mv[at - 1]);
                }
            } else if !is_left {
                whole = whole + Mv::pred(mv[above - 1], mv[above + 2], mv[at - 3]);
            } else {
                whole = whole + Mv::pred(mv[above], mv[above + 2], mv[above + 3]);
            }
            mv[at..at + 4].fill(whole);
        }
        Ok(())
    }

    /// The vectors of an intra block in an inter frame, which a later block predicts from.
    fn predict(&mut self, bx: usize, is_top: bool, is_left: bool) {
        let at = self.pos + bx * 4;
        let above = self.prev_pos + bx * 4;
        let mv = &mut self.mvs;
        if is_top {
            if is_left {
                mv[at..at + 4].fill(Mv::default());
            } else {
                mv[at] = Mv::pred(mv[at - 4], mv[at - 3], mv[at - 1]);
                mv[at + 2] = Mv::pred(mv[at - 3], mv[at - 1], mv[at]);
                mv[at + 1] = Mv::pred(mv[at - 1], mv[at], mv[at + 2]);
                mv[at + 3] = Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
            }
        } else if is_left {
            mv[at] = Mv::pred(mv[above], mv[above + 2], mv[above + 3]);
            mv[at + 1] = Mv::pred(mv[at], mv[above + 2], mv[above + 3]);
            mv[at + 2] = Mv::pred(mv[above + 2], mv[at + 1], mv[at]);
            mv[at + 3] = Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
        } else {
            mv[at] = Mv::pred(mv[at - 3], mv[above - 1], mv[above + 2]);
            mv[at + 1] = Mv::pred(mv[at], mv[above + 2], mv[above + 3]);
            mv[at + 2] = Mv::pred(mv[at - 3], mv[at - 1], mv[at]);
            mv[at + 3] = Mv::pred(mv[at], mv[at + 1], mv[at + 2]);
        }
    }
}

/// A plane's bytes, padded to whole blocks of 32 luma pixels, every byte of which a picture writes.
struct PlaneBuf {
    data: Vec<u8>,
    stride: usize,
    rows: usize,
}

impl PlaneBuf {
    fn new(stride: usize, rows: usize) -> Self {
        Self {
            data: vec![0; stride * rows],
            stride,
            rows,
        }
    }

    fn reference(&self) -> Ref<'_> {
        Ref {
            data: &self.data,
            stride: self.stride,
            rows: self.rows,
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
        let (stride, rows) = (width.next_multiple_of(32), height.next_multiple_of(32));
        let mut planes = vec![
            PlaneBuf::new(stride, rows),
            PlaneBuf::new(stride / 2, rows / 2),
            PlaneBuf::new(stride / 2, rows / 2),
        ];
        if alpha {
            planes.push(PlaneBuf::new(stride, rows));
        }
        Self { planes }
    }
}

/// How the LRU of block types is walked: the code read picks one and moves it to the front.
fn next_block_type(lru: &mut [u8; 4], code: u32) -> u8 {
    match code {
        0 => lru[0],
        1 => {
            lru.swap(0, 1);
            lru[0]
        }
        2 => {
            lru.swap(2, 3);
            lru[2]
        }
        _ => {
            lru.swap(1, 2);
            lru[1]
        }
    }
}

/// Reads the flag per 16 rows or columns that says whether it is a letterbox border or not.
fn decode_flags(
    br: &mut Bits,
    flags: &mut Vec<bool>,
    start: usize,
    count: usize,
) -> Result<(), MediaError> {
    if start > 0 {
        flags.push(false);
    }
    if !br.bit()? {
        for _ in 0..count {
            flags.push(br.bit()?);
        }
        return Ok(());
    }
    let mut left = count;
    let mut mode = 0;
    let mut last = false;
    while left > 0 {
        if !br.bit()? {
            last = if mode == 3 { !last } else { br.bit()? };
            flags.push(last);
            left -= 1;
            let take = left.min(4);
            for _ in 0..take {
                flags.push(br.bit()?);
            }
            left -= take;
            mode = 2;
        } else {
            let width: u32 = if left < 4 {
                2
            } else if left < 16 {
                4
            } else {
                5
            };
            last = if mode == 3 { !last } else { br.bit()? };
            let base = if mode == 3 { width + 1 } else { width + 2 };
            let mut run = (base as usize).min(left);
            if run != left {
                let more = br.read(width)? as usize;
                run += more;
                mode = if more == (1 << width) - 1 { 1 } else { 3 };
            }
            if run > left {
                return Err(bad("a run of flags is longer than what is left"));
            }
            flags.extend(std::iter::repeat_n(last, run));
            left -= run;
        }
    }
    Ok(())
}

/// The quantiser a block states against the one predicted for it.
fn get_new_quant(br: &mut Bits, prev: u8) -> Result<u8, MediaError> {
    let mut code = br.unary(4, 1)? as u8;
    if code == 0 {
        return Ok(prev);
    }
    if code == 3 {
        code += br.read(1)? as u8;
    } else if code == 4 {
        code = br.read(5)? as u8 + 5;
    }
    let q = if br.bit()? {
        prev.checked_sub(code)
    } else {
        prev.checked_add(code)
    };
    q.filter(|&q| usize::from(q) < tables::QUANT_DC.len())
        .ok_or_else(|| bad("a quantiser outside the table"))
}

/// Which of a luma square's sixteen 8×8 blocks carry coefficients, as bits 0 to 15, and in bits 16
/// to 31 which of them use the dense code.
fn decode_cbp_luma(br: &mut Bits, prev: u32, edge_state: u32) -> Result<u32, MediaError> {
    let count = (prev as u16).count_ones() as usize;
    let (min_count, mask) = if count < 8 {
        (count, 0x0000)
    } else {
        (16 - count, 0xFFFF)
    };
    let mut cbp;
    if br.bit()? {
        cbp = 0;
    } else if min_count >= 4 {
        cbp = br.read(16)?;
    } else {
        cbp = 0;
        for i in 0..4 {
            if !br.bit()? {
                cbp |= br.read(4)? << (i * 4);
            }
        }
    }
    cbp ^= mask;
    if (edge_state & 0x40000 == 0 || cbp != 0) && br.bit()? {
        cbp |= cbp << 16;
    }
    Ok(cbp)
}

/// Which of a chroma square's four 8×8 blocks carry coefficients, and which use the dense code.
fn decode_cbp_chroma(br: &mut Bits, last: u32) -> Result<u32, MediaError> {
    if br.bit()? {
        Ok((last & 0xF0000) | tables::CHROMA_CBPS[(last & 0xF) as usize])
    } else {
        let coded = br.read(4)?;
        if br.bit()? {
            Ok(coded | (coded << 16))
        } else {
            Ok(coded)
        }
    }
}

/// Reads the AC levels of the coded blocks among four, dequantised, at their zigzag positions, and
/// says how many levels each block coded.
fn decode_acs_4blocks(
    br: &mut Bits,
    dst: &mut Blocks,
    quant_mat: &[[i32; 64]; 4],
    q: u8,
    cbp: u32,
) -> Result<[u8; 4], MediaError> {
    let codes = codes();
    let code = if cbp >> 16 != 0 {
        &codes.ac_dense
    } else {
        &codes.ac_sparse
    };
    let qmat = &quant_mat[usize::from(q & 3)];
    let shift = q >> 2;
    let mut counts = [0u8; 4];
    for (n, block) in dst.iter_mut().enumerate() {
        if (cbp >> n) & 1 == 0 {
            continue;
        }
        let mut escape = 0;
        let mut idx = 1;
        while idx < 64 {
            if escape >= 1 {
                escape -= 1;
            } else {
                let symbol = br.vlc(code)?;
                idx += if symbol == 11 {
                    br.read(6)? as usize
                } else {
                    tables::AC_RUNS[usize::from(symbol)]
                };
                if idx > 63 {
                    break;
                }
                escape = if symbol == 13 { 6 } else { 0 };
            }
            let level = br.level_nonzero()?;
            counts[n] += 1;
            let pos = tables::ZIGZAG[idx];
            let scaled = ((i64::from(level) * i64::from(qmat[pos])) << shift) + 0x40;
            block[pos] = (scaled >> 7) as i32;
            idx += 1;
        }
    }
    Ok(counts)
}

/// The state of one decoded line of blocks: the coded block patterns, which carry from block to block.
#[derive(Default, Clone, Copy)]
struct Cbps {
    y: u32,
    u: u32,
    v: u32,
    a: u32,
}

/// What each plane's block contributes to the 32×32 block being decoded: transformed blocks.
struct Residue {
    y: [Blocks; 4],
    u: Blocks,
    v: Blocks,
    a: [Blocks; 4],
}

impl Residue {
    fn new() -> Self {
        Self {
            y: [[[0; 64]; 4]; 4],
            u: [[0; 64]; 4],
            v: [[0; 64]; 4],
            a: [[[0; 64]; 4]; 4],
        }
    }
}

/// An intra luma square: the pattern, the DCs and the AC levels of its sixteen blocks.
fn decode_luma_intra(
    br: &mut Bits,
    prev_cbp: u32,
    q: u8,
    dst: &mut [Blocks; 4],
    edge_state: u32,
    dc: &mut YDcInfo,
    bx: usize,
) -> Result<(u32, [[u8; 4]; 4]), MediaError> {
    let cbp = decode_cbp_luma(br, prev_cbp, edge_state)?;
    dc.decode(br, q)?;
    dc.predict(bx, edge_state & 0x88 != 0, edge_state & 0x20 != 0, 0, 0x7FF);
    let mut counts = [[0u8; 4]; 4];
    for (i, blocks) in dst.iter_mut().enumerate() {
        counts[i] = decode_acs_4blocks(br, blocks, tables::QUANT_INTRA_LUMA, q, cbp >> (i * 4))?;
        for (block, &value) in blocks.iter_mut().zip(&dc.dcs[i * 4..]) {
            block[0] = dc_coefficient(value);
        }
    }
    Ok((cbp, counts))
}

fn decode_chroma_intra(
    br: &mut Bits,
    prev_cbp: u32,
    q: u8,
    dst: &mut Blocks,
    edge_state: u32,
    dc: &mut CDcInfo,
    bx: usize,
) -> Result<(u32, [u8; 4]), MediaError> {
    let cbp = decode_cbp_chroma(br, prev_cbp)?;
    dc.decode(br, q)?;
    dc.predict(bx, edge_state & 0x88 != 0, edge_state & 0x20 != 0, 0, 0x7FF);
    let counts = decode_acs_4blocks(br, dst, tables::QUANT_INTRA_CHROMA, q, cbp)?;
    for (block, &value) in dst.iter_mut().zip(&dc.dcs) {
        block[0] = dc_coefficient(value);
    }
    Ok((cbp, counts))
}

/// The DC range an inter block's residual keeps to: wider when the frame says so.
fn inter_dc_range(edge_state: u32) -> (i32, i32) {
    if edge_state & 0x40000 != 0 {
        (-0x7FF, 0x7FF)
    } else {
        (-0x3FF, 0x3FF)
    }
}

fn decode_luma_inter(
    br: &mut Bits,
    prev_cbp: u32,
    q: u8,
    dst: &mut [Blocks; 4],
    edge_state: u32,
    dc: &mut YDcInfo,
) -> Result<(u32, [[u8; 4]; 4]), MediaError> {
    let cbp = decode_cbp_luma(br, prev_cbp, edge_state)?;
    let (min_dc, max_dc) = inter_dc_range(edge_state);
    dc.decode(br, q)?;
    dc.predict_inter(min_dc, max_dc);
    let mut counts = [[0u8; 4]; 4];
    for (i, blocks) in dst.iter_mut().enumerate() {
        counts[i] = decode_acs_4blocks(br, blocks, tables::QUANT_INTER, q, cbp >> (i * 4))?;
        for (block, &value) in blocks.iter_mut().zip(&dc.dcs[i * 4..]) {
            block[0] = dc_coefficient(value);
        }
    }
    Ok((cbp, counts))
}

fn decode_chroma_inter(
    br: &mut Bits,
    prev_cbp: u32,
    q: u8,
    dst: &mut Blocks,
    edge_state: u32,
    dc: &mut CDcInfo,
) -> Result<(u32, [u8; 4]), MediaError> {
    let cbp = decode_cbp_chroma(br, prev_cbp)?;
    let (min_dc, max_dc) = inter_dc_range(edge_state);
    dc.decode(br, q)?;
    dc.predict_inter(min_dc, max_dc);
    let counts = decode_acs_4blocks(br, dst, tables::QUANT_INTER, q, cbp)?;
    for (block, &value) in dst.iter_mut().zip(&dc.dcs) {
        block[0] = dc_coefficient(value);
    }
    Ok((cbp, counts))
}

/// Where a square may read: the size of the picture the game stores, its width and height rounded up to 16,
/// in half pixels.
#[derive(Clone, Copy)]
struct Reach {
    width: i32,
    height: i32,
    kb2j: bool,
}

impl Reach {
    /// The position (in half pixels, a 16×16 square's corner) moved back inside the picture where the filter's
    /// taps stay on it, which is where the game reads whatever the vector says.
    fn clamp(self, pos: i32, limit: i32) -> i32 {
        let odd = (pos & 1) as usize;
        let hi = limit - [32, 42][odd];
        if self.kb2j {
            let lo = [0, 5][odd];
            if (pos.wrapping_sub(lo) as u32) > hi as u32 {
                lo + if pos < lo { 0 } else { hi }
            } else {
                pos
            }
        } else {
            let lo = [0, 4][odd];
            if (pos.wrapping_sub(lo) as u32) > hi as u32 {
                if (pos as u32) < lo as u32 {
                    lo
                } else {
                    hi
                }
            } else {
                pos
            }
        }
    }

    /// `mv` for the square at (`xoff`, `yoff`) in pixels, moved so it stays on the picture.
    fn keep(self, xoff: usize, yoff: usize, mv: Mv) -> Mv {
        let (x, y) = (xoff as i32 * 2, yoff as i32 * 2);
        Mv {
            x: (self.clamp(x + i32::from(mv.x), self.width) - x) as i16,
            y: (self.clamp(y + i32::from(mv.y), self.height) - y) as i16,
        }
    }
}

/// Predicts every plane of the 32×32 block at (`bx`, `by`) from `prev` moved by `mvs`, a vector a square.
fn predict_block(
    cur: &mut Frame,
    prev: &Frame,
    (bx, by): (usize, usize),
    mvs: [Mv; 4],
    alpha: bool,
    reach: Reach,
) -> Result<(), MediaError> {
    for (square, &mv) in mvs.iter().enumerate() {
        let xoff = bx * 32 + (square & 1) * 16;
        let yoff = by * 32 + (square & 2) * 8;
        let mv = reach.keep(xoff, yoff, mv);
        let planes = if alpha { 4 } else { 3 };
        for plane in 0..planes {
            let (to, from) = (&mut cur.planes[plane], &prev.planes[plane]);
            let stride = to.stride;
            if plane == 0 || plane == 3 {
                let row = &mut to.data[by * 32 * stride..];
                dsp::mc_luma(row, stride, from.reference(), xoff, yoff, mv)?;
            } else {
                let row = &mut to.data[by * 16 * stride..];
                dsp::mc_chroma(row, stride, from.reference(), xoff >> 1, yoff >> 1, mv)?;
            }
        }
    }
    Ok(())
}

/// Writes an intra block's transformed squares to its planes.
fn put_block(cur: &mut Frame, (bx, by): (usize, usize), residue: &mut Residue, alpha: bool) {
    let put = dsp::put_mb4;
    let planes = if alpha { 4 } else { 3 };
    for (plane, to) in cur.planes.iter_mut().enumerate().take(planes) {
        let stride = to.stride;
        if plane == 0 || plane == 3 {
            let squares = if plane == 0 {
                &mut residue.y
            } else {
                &mut residue.a
            };
            for (n, square) in squares.iter_mut().enumerate() {
                let at = by * 32 * stride + bx * 32 + (n & 1) * 16 + (n >> 1) * 16 * stride;
                put(&mut to.data, at, stride, square);
            }
        } else {
            let blocks = if plane == 1 {
                &mut residue.u
            } else {
                &mut residue.v
            };
            put(&mut to.data, by * 16 * stride + bx * 16, stride, blocks);
        }
    }
}

/// What the filter needs of an inter block's residue: the record of each luma block in rows of four, and
/// of each chroma block in rows of two when the chroma codes any.
#[derive(Default)]
struct InterRecords {
    luma: [u16; 16],
    chroma: Option<[[u16; 4]; 2]>,
}

/// Adds the residue of an inter block to its prediction, drawn across the edges of its 8×8 blocks
/// unless the frame's flags say not to.
fn put_inter_block(
    cur: &mut Frame,
    (bx, by): (usize, usize),
    residue: &mut Residue,
    alpha: bool,
    records: &InterRecords,
    flags: u32,
) {
    let vertical = flags & 0x4_4000 == 0;
    let horizontal = flags & 0x4_8000 == 0;
    let add = |to: &mut PlaneBuf, at: usize, flat: &[i16], size: usize| {
        for y in 0..size {
            let row = &mut to.data[at + y * to.stride..][..size];
            for (d, &c) in row.iter_mut().zip(&flat[y * size..]) {
                *d = dsp::clip8(i32::from(*d) + i32::from(c));
            }
        }
    };
    let mut luma = [0i16; 1024];
    for (n, quad) in residue.y.iter_mut().enumerate() {
        for (b, block) in quad.iter_mut().enumerate() {
            dsp::idct(block);
            let (x, y) = ((n & 1) * 16 + (b & 1) * 8, (n >> 1) * 16 + (b >> 1) * 8);
            for row in 0..8 {
                for col in 0..8 {
                    luma[(y + row) * 32 + x + col] = block[row * 8 + col] as i16;
                }
            }
        }
    }
    filter::smooth_residue(&mut luma, 32, &records.luma, vertical, horizontal);
    let stride = cur.planes[0].stride;
    add(&mut cur.planes[0], by * 32 * stride + bx * 32, &luma, 32);
    for (plane, blocks) in [(1, &mut residue.u), (2, &mut residue.v)] {
        let mut chroma = [0i16; 256];
        for (b, block) in blocks.iter_mut().enumerate() {
            dsp::idct(block);
            let (x, y) = ((b & 1) * 8, (b >> 1) * 8);
            for row in 0..8 {
                for col in 0..8 {
                    chroma[(y + row) * 16 + x + col] = block[row * 8 + col] as i16;
                }
            }
        }
        if let Some(both) = &records.chroma {
            filter::smooth_residue(&mut chroma, 16, &both[plane - 1], vertical, horizontal);
        }
        let stride = cur.planes[plane].stride;
        add(
            &mut cur.planes[plane],
            by * 16 * stride + bx * 16,
            &chroma,
            16,
        );
    }
    if alpha {
        let stride = cur.planes[3].stride;
        for (n, square) in residue.a.iter_mut().enumerate() {
            let at = by * 32 * stride + bx * 32 + (n & 1) * 16 + (n >> 1) * 16 * stride;
            dsp::add_mb4(&mut cur.planes[3].data, at, stride, square);
        }
    }
}

/// The DCs of a decoded block's pixels, which the blocks after it predict from.
fn refresh_dcs(cur: &Frame, (bx, by): (usize, usize), state: &mut Dcs, alpha: bool) {
    let luma = |plane: usize| -> [i32; 7] {
        let p = &cur.planes[plane];
        let stride = p.stride;
        let base = by * 32 * stride + bx * 32;
        let dc = |x: usize, y: usize| dsp::calc_dc(&p.data[base + x + y * stride..], stride);
        [
            dc(24, 0),
            dc(24, 8),
            dc(24, 16),
            dc(0, 24),
            dc(8, 24),
            dc(16, 24),
            dc(24, 24),
        ]
    };
    let chroma = |plane: usize| -> [i32; 3] {
        let p = &cur.planes[plane];
        let stride = p.stride;
        let base = by * 16 * stride + bx * 16;
        let dc = |x: usize, y: usize| dsp::calc_dc(&p.data[base + x + y * stride..], stride);
        [dc(8, 0), dc(0, 8), dc(8, 8)]
    };
    state.y.set_from_pixels(bx, luma(0));
    state.u.set_from_pixels(bx, chroma(1));
    state.v.set_from_pixels(bx, chroma(2));
    if alpha {
        state.a.set_from_pixels(bx, luma(3));
    }
}

/// The DC predictors of every plane.
#[derive(Default)]
struct Dcs {
    y: YDcInfo,
    u: CDcInfo,
    v: CDcInfo,
    a: YDcInfo,
}

/// The Bink 2 video decoder of one movie.
pub struct Decoder {
    width: usize,
    height: usize,
    has_alpha: bool,
    /// The block row each slice ends before, for as many slices as the revision and flags state.
    slice_ends: Vec<usize>,
    /// How many column flags a frame states after the first, which is always clear.
    col_flag_count: usize,
    /// The picture being decoded, and the last one decoded, which the next one's motion refers to.
    cur: Frame,
    prev: Frame,
    have_prev: bool,
    quants: QuantInfo,
    dcs: Dcs,
    mvs: MvInfo,
}

impl Decoder {
    /// A decoder for a movie of `revision` (the last letter of `KB2g`), with the video `flags` of its head.
    pub fn new(revision: u8, flags: u32, width: u32, height: u32) -> Result<Self, MediaError> {
        if !matches!(revision, b'g'..=b'k' | b'n') {
            return Err(MediaError::Decode(format!(
                "Bink 2 revision '{}' is not one OpenRage decodes",
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
        let aligned = h.next_multiple_of(32);
        let mut slice_ends = Vec::new();
        if revision == b'g' {
            if aligned < 128 {
                slice_ends.push(aligned >> 5);
            } else {
                slice_ends.extend([(h + 31) >> 6, aligned >> 5]);
            }
        } else {
            let slices = [2usize, 3, 4, 8][(flags & 3) as usize];
            let (mut start, mut end) = (0, aligned + 32 * slices - 1);
            for i in 0..slices - 1 {
                start += ((end - start) / (slices - i)) & !31;
                end -= 32;
                slice_ends.push(start >> 5);
            }
            slice_ends.push(aligned >> 5);
        }
        // A flag per 8 columns but the first: `KB2n` counts them over the width rounded up to 32, the
        // revisions before it over the width rounded up to 16. The rows are counted to 16 in both.
        let col_flag_count = if revision == b'n' {
            w.div_ceil(32) * 4 - 1
        } else {
            w.div_ceil(16) * 2 - 1
        };
        Ok(Self {
            width: w,
            height: h,
            has_alpha,
            slice_ends,
            col_flag_count,
            cur: Frame::new(w, h, has_alpha),
            prev: Frame::new(w, h, has_alpha),
            have_prev: false,
            quants: QuantInfo::default(),
            dcs: Dcs::default(),
            mvs: MvInfo::default(),
        })
    }

    /// Decodes one video packet, a key frame when `key`; the picture it returns lives until the next call.
    pub fn decode(&mut self, packet: &[u8], key: bool) -> Result<Yuv420<'_>, MediaError> {
        self.decode_frame(packet, key)?;
        std::mem::swap(&mut self.cur, &mut self.prev);
        self.have_prev = true;
        let (w, h) = (self.width, self.height);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let planes = &self.prev.planes;
        Ok(Yuv420 {
            width: w,
            height: h,
            full_range: true,
            y: planes[0].view(w, h),
            u: planes[1].view(cw, ch),
            v: planes[2].view(cw, ch),
            alpha: planes.get(3).map(|a| a.view(w, h)),
        })
    }

    /// Forgets the previous picture, so the next packet must be a key frame.
    pub fn reset(&mut self) {
        self.have_prev = false;
    }

    fn decode_frame(&mut self, packet: &[u8], is_intra: bool) -> Result<(), MediaError> {
        let mut br = Bits::new(packet);
        let frame_flags = br.read(32)?;
        if frame_flags & FRAME_NEW == 0 {
            return Err(MediaError::Decode(
                "this Bink 2 frame is in the coding of KB2f and before, which OpenRage does not decode"
                    .into(),
            ));
        }
        let mut offsets = [0usize; 7];
        for offset in &mut offsets[..self.slice_ends.len() - 1] {
            *offset = br.read(32)? as usize;
        }
        let (blocks_wide, blocks_high) = (self.width.div_ceil(32), self.height.div_ceil(32));
        let mut do_alpha = self.has_alpha;
        if frame_flags & 0x80000 != 0 && self.has_alpha {
            do_alpha = false;
            self.cur.planes[3].data.fill((frame_flags >> 24) as u8);
        }
        let mut row_flags = Vec::with_capacity(blocks_high * 4);
        let mut col_flags = Vec::with_capacity(blocks_wide * 4);
        if frame_flags & 0x10000 != 0 {
            if frame_flags & 0x8000 == 0 {
                let len = self.height.div_ceil(16);
                decode_flags(&mut br, &mut row_flags, 1, len * 2 - 1)?;
            }
            if frame_flags & 0x4000 == 0 {
                decode_flags(&mut br, &mut col_flags, 1, self.col_flag_count)?;
            }
        }
        row_flags.resize(blocks_high * 4, false);
        col_flags.resize(blocks_wide * 4, false);

        let height = self.height;
        let reach = Reach {
            width: self.width.next_multiple_of(16) as i32 * 2,
            height: self.height.next_multiple_of(16) as i32 * 2,
            kb2j: frame_flags & 0x20000 != 0,
        };
        let Self {
            cur,
            prev,
            have_prev,
            quants,
            dcs,
            mvs,
            slice_ends,
            ..
        } = self;
        let mut records = Records::new(blocks_wide, blocks_high);
        let mut square_mvs = vec![Mv::default(); blocks_wide * 2 * blocks_high * 2];
        let mut start_by = 0;
        for (slice, &end_by) in slice_ends.iter().enumerate() {
            if slice != 0 {
                br.seek(offsets[slice - 1])?;
            }
            let mut row_state = frame_flags & 0x2E000;
            if is_intra {
                row_state |= 0x10000;
            }
            quants.resize(blocks_wide);
            dcs.y.resize(blocks_wide);
            dcs.u.resize(blocks_wide);
            dcs.v.resize(blocks_wide);
            dcs.a.resize(blocks_wide);
            mvs.resize(blocks_wide);
            for (by, &row_flag) in row_flags.iter().enumerate().take(end_by).skip(start_by) {
                let mut cbp = Cbps::default();
                let mut cbp_inter = Cbps::default();
                let row_flag = u32::from(row_flag) * 4;
                row_state =
                    (row_state & 0x3FF_FFFF) | ((row_state >> 4) & 0xC00_0000) | (row_flag << 28);
                if by == start_by {
                    row_state |= 0x80;
                }
                if by + 2 >= end_by {
                    row_state |= 0x100;
                }
                let mut lru: [u8; 4] = [2, 3, 1, 0];
                let mut edge_state = 0;
                let is_top = by == start_by;
                for (bx, &col_flag) in col_flags.iter().enumerate().take(blocks_wide) {
                    let mut blk_state = row_state | (edge_state & 0x3FC_0000);
                    if bx == 0 {
                        blk_state |= 0x20;
                    }
                    if bx + 2 < blocks_wide {
                        blk_state |= 0x40;
                    }
                    if bx & 1 != 0 {
                        blk_state |= 0x200;
                    }
                    let col_flag = u32::from(col_flag) * 4;
                    let edge_state_c = ((blk_state >> 4) & 0x3C_0000)
                        | (blk_state & 0xFC03_FFFF)
                        | ((col_flag & 0xF) << 22);
                    let edge_state_y = (frame_flags & 0x40000) | (blk_state & 0x3_FFFF);
                    edge_state = edge_state_c;
                    let is_left = bx == 0;
                    let at = (bx, by);

                    let btype = if is_intra {
                        0
                    } else {
                        let code = br.unary(3, 1)?;
                        next_block_type(&mut lru, code)
                    };
                    if !is_intra {
                        for half in [bx * 2, bx * 2 + 1] {
                            let q = quants.pred(half, is_top);
                            quants.set(half, q);
                        }
                    }
                    let mut residue = Residue::new();
                    match btype {
                        0 => {
                            let q = get_new_quant(&mut br, quants.pred(bx * 2, is_top))?;
                            quants.set(bx * 2, q);
                            let (cbp_y, counts_y) = decode_luma_intra(
                                &mut br,
                                cbp.y,
                                q,
                                &mut residue.y,
                                edge_state_y,
                                &mut dcs.y,
                                bx,
                            )?;
                            cbp.y = cbp_y;
                            let (cbp_v, counts_v) = decode_chroma_intra(
                                &mut br,
                                cbp.v,
                                q,
                                &mut residue.v,
                                edge_state_c,
                                &mut dcs.v,
                                bx,
                            )?;
                            cbp.v = cbp_v;
                            let (cbp_u, counts_u) = decode_chroma_intra(
                                &mut br,
                                cbp.u,
                                q,
                                &mut residue.u,
                                edge_state_c,
                                &mut dcs.u,
                                bx,
                            )?;
                            cbp.u = cbp_u;
                            records.set_intra(
                                at,
                                (&dcs.y.dcs, &counts_y),
                                (&dcs.u.dcs, &counts_u),
                                (&dcs.v.dcs, &counts_v),
                            );
                            if do_alpha {
                                cbp.a = decode_luma_intra(
                                    &mut br,
                                    cbp.a,
                                    q,
                                    &mut residue.a,
                                    edge_state_y,
                                    &mut dcs.a,
                                    bx,
                                )?
                                .0;
                            }
                            put_block(cur, at, &mut residue, do_alpha);
                            if !is_intra {
                                mvs.predict(bx, is_top, is_left);
                            }
                        }
                        1 => {
                            if !*have_prev {
                                return Err(MediaError::Decode(
                                    "this Bink 2 frame refers to a picture before it".into(),
                                ));
                            }
                            predict_block(cur, prev, at, [Mv::default(); 4], do_alpha, reach)?;
                            mvs.zero(bx);
                        }
                        2 | 3 => {
                            mvs.decode(&mut br, bx, is_top, is_left)?;
                            if !*have_prev {
                                return Err(MediaError::Decode(
                                    "this Bink 2 frame refers to a picture before it".into(),
                                ));
                            }
                            let moved: [Mv; 4] = std::array::from_fn(|square| mvs.get(bx, square));
                            for (square, &mv) in moved.iter().enumerate() {
                                let at = (by * 2 + (square >> 1)) * blocks_wide * 2;
                                square_mvs[at + bx * 2 + (square & 1)] = mv;
                            }
                            predict_block(cur, prev, at, moved, do_alpha, reach)?;
                            if btype == 3 {
                                let q = get_new_quant(&mut br, quants.pred(bx * 2 + 1, is_top))?;
                                quants.set(bx * 2 + 1, q);
                                let (cbp_y, counts_y) = decode_luma_inter(
                                    &mut br,
                                    cbp_inter.y,
                                    q,
                                    &mut residue.y,
                                    edge_state_y,
                                    &mut dcs.y,
                                )?;
                                cbp_inter.y = cbp_y;
                                let mut inter_records = InterRecords::default();
                                for (quad, counts) in counts_y.iter().enumerate() {
                                    for (block, &count) in counts.iter().enumerate() {
                                        let at = ((quad >> 1) * 2 + (block >> 1)) * 4
                                            + (quad & 1) * 2
                                            + (block & 1);
                                        inter_records.luma[at] = filter::inter_record(
                                            dcs.y.dcs[quad * 4 + block],
                                            count,
                                        );
                                    }
                                }
                                if br.bit()? {
                                    let (cbp_v, counts_v) = decode_chroma_inter(
                                        &mut br,
                                        cbp_inter.v,
                                        q,
                                        &mut residue.v,
                                        edge_state_y,
                                        &mut dcs.v,
                                    )?;
                                    cbp_inter.v = cbp_v;
                                    let (cbp_u, counts_u) = decode_chroma_inter(
                                        &mut br,
                                        cbp_inter.u,
                                        q,
                                        &mut residue.u,
                                        edge_state_y,
                                        &mut dcs.u,
                                    )?;
                                    cbp_inter.u = cbp_u;
                                    let record = |dcs: &[i32; 4], counts: &[u8; 4]| {
                                        std::array::from_fn(|block| {
                                            filter::inter_record(dcs[block], counts[block])
                                        })
                                    };
                                    inter_records.chroma = Some([
                                        record(&dcs.u.dcs, &counts_u),
                                        record(&dcs.v.dcs, &counts_v),
                                    ]);
                                } else {
                                    cbp_inter.v = 0;
                                    cbp_inter.u = 0;
                                }
                                if do_alpha {
                                    cbp_inter.a = decode_luma_inter(
                                        &mut br,
                                        cbp_inter.a,
                                        q,
                                        &mut residue.a,
                                        edge_state_y,
                                        &mut dcs.a,
                                    )?
                                    .0;
                                }
                                put_inter_block(
                                    cur,
                                    at,
                                    &mut residue,
                                    do_alpha,
                                    &inter_records,
                                    frame_flags,
                                );
                            }
                        }
                        _ => unreachable!("the LRU holds the four block types"),
                    }
                    if btype != 0 {
                        refresh_dcs(cur, at, dcs, do_alpha);
                    }
                }
                quants.update_line();
                dcs.y.update_line();
                dcs.u.update_line();
                dcs.v.update_line();
                dcs.a.update_line();
                mvs.update_line();
                row_state = (row_state & !0x190) | ((row_state & 4) << 2);
            }
            start_by = end_by;
        }
        if let [y, u, v, ..] = &mut cur.planes[..] {
            let planes = [
                filter::Plane {
                    data: &mut y.data,
                    stride: y.stride,
                },
                filter::Plane {
                    data: &mut u.data,
                    stride: u.stride,
                },
                filter::Plane {
                    data: &mut v.data,
                    stride: v.stride,
                },
            ];
            filter::apply(
                planes,
                &records,
                &square_mvs,
                frame_flags,
                &row_flags,
                &col_flags,
                height,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet of the frame `flags` and then bits written as text, the first read being the first written.
    fn packet(flags: u32, bits: &str) -> Vec<u8> {
        let mut bytes = flags.to_le_bytes().to_vec();
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

    /// A key frame of one block that states no DC and no coefficient: the quantiser is the
    /// predicted one, and the luma and both chroma blocks code nothing.
    const GREY_BLOCK: &str = "1 1 0 0  1 0  1 0";

    fn grey_picture(decoder: &mut Decoder) {
        let picture = decoder
            .decode(&packet(0xE000, GREY_BLOCK), true)
            .expect("a key frame that codes nothing decodes");
        // The first DC of a picture is 1024, a flat block of 128.
        for plane in [picture.y, picture.u, picture.v] {
            assert!(
                (0..plane.height).all(|y| plane.row(y).iter().all(|&v| v == 128)),
                "every sample is 128"
            );
        }
    }

    #[test]
    fn a_key_frame_that_codes_nothing_is_flat_grey_of_the_first_dc() {
        let mut decoder = Decoder::new(b'g', 0, 32, 32).expect("a 32 x 32 picture");
        grey_picture(&mut decoder);
    }

    #[test]
    fn a_skip_block_repeats_the_picture_before_it_and_has_nothing_to_repeat_at_the_start() {
        let skip = packet(0xE000, "0 0 0");
        let mut decoder = Decoder::new(b'g', 0, 32, 32).expect("a picture");
        let Err(why) = decoder.decode(&skip, false) else {
            panic!("a skip with no picture before it");
        };
        assert!(why.to_string().contains("before it"), "{why}");

        grey_picture(&mut decoder);
        let picture = decoder.decode(&skip, false).expect("a skip repeats");
        assert!(picture.y.row(31).iter().all(|&v| v == 128));
        // A decoder that forgot its picture refuses the skip again.
        decoder.reset();
        assert!(decoder.decode(&skip, false).is_err());
    }

    #[test]
    fn a_motion_block_with_a_zero_vector_repeats_the_picture_too() {
        // The first block type in the list is motion; then one vector, zero in x and in y.
        let motion = packet(0xE000, "1 1 1 1");
        let mut decoder = Decoder::new(b'g', 0, 32, 32).expect("a picture");
        grey_picture(&mut decoder);
        let picture = decoder.decode(&motion, false).expect("a motion block");
        assert!(picture.u.row(15).iter().all(|&v| v == 128));
    }

    #[test]
    fn a_frame_in_the_coding_before_kb2g_a_revision_before_it_and_a_picture_too_big_are_refused() {
        let mut decoder = Decoder::new(b'g', 0, 32, 32).expect("a picture");
        let Err(why) = decoder.decode(&packet(0xC000, GREY_BLOCK), true) else {
            panic!("a frame without the KB2g flag");
        };
        assert!(why.to_string().contains("KB2f"), "{why}");
        assert!(Decoder::new(b'f', 0, 32, 32).is_err());
        assert!(Decoder::new(b'g', 0, 0, 32).is_err());
        assert!(Decoder::new(b'j', 0, 7681, 32).is_err());
        // A packet that ends before its flags says so and does not panic.
        assert!(decoder.decode(&[1, 2], true).is_err());
    }

    #[test]
    fn a_kb2n_picture_states_its_column_flags_over_the_width_rounded_to_32() {
        // 48 wide: three columns of 16, so a `KB2k` frame states 5 column flags and a `KB2n` one 7.
        // Rows and columns are raw bits, every one clear; then two blocks that code nothing.
        let frame = |columns: usize| {
            let bits = format!(
                "0 000  0 {}  {GREY_BLOCK} {GREY_BLOCK}",
                "0".repeat(columns)
            );
            let mut bytes = packet(0x1_2000, &bits);
            // The second slice holds no block row; its offset is the packet's end.
            let end = (bytes.len() + 4) as u32;
            bytes.splice(4..4, end.to_le_bytes());
            bytes
        };
        let grey = |revision, columns| {
            let mut decoder = Decoder::new(revision, 0, 48, 32).expect("a 48 x 32 picture");
            decoder
                .decode(&frame(columns), true)
                .map(|picture| (0..32).all(|y| picture.y.row(y).iter().all(|&v| v == 128)))
                .unwrap_or(false)
        };
        assert!(grey(b'n', 7), "KB2n reads seven column flags");
        assert!(grey(b'k', 5), "KB2k reads five");
        assert!(
            !grey(b'n', 5),
            "a KB2n frame with five does not decode as written"
        );
        assert!(!grey(b'k', 7), "nor a KB2k frame with seven");
    }

    #[test]
    fn the_slices_of_a_picture_follow_its_revision_and_flags() {
        let ends = |revision, flags, height| {
            Decoder::new(revision, flags, 64, height)
                .expect("a picture")
                .slice_ends
        };
        // KB2g: one slice under 128 rows, else two, the first ending where the rounded half does.
        assert_eq!(ends(b'g', 0, 96), [3]);
        assert_eq!(ends(b'g', 0, 360), [6, 12]);
        // KB2h and later: the flags' low two bits name two, three, four or eight slices.
        assert_eq!(ends(b'j', 0x10, 1080), [17, 34]);
        assert_eq!(ends(b'j', 0x12, 1080), [9, 18, 26, 34]);
        assert_eq!(ends(b'j', 0x13, 2160).len(), 8);
        assert_eq!(ends(b'j', 0x13, 2160).last(), Some(&68));
    }

    #[test]
    fn a_square_is_read_from_where_the_filters_taps_stay_on_the_picture() {
        // 1920×1080 stored 1088 high, in half pixels.
        let before = Reach {
            width: 3840,
            height: 2176,
            kb2j: false,
        };
        let after = Reach {
            width: 3840,
            height: 2176,
            kb2j: true,
        };
        for reach in [before, after] {
            for pos in [100, 101, 2176 - 32] {
                assert_eq!(reach.clamp(pos, 2176), pos, "inside the picture");
            }
            assert_eq!(
                reach.clamp(2176, 2176),
                2176 - 32,
                "a whole pixel past the bottom"
            );
        }
        // A half pixel past the bottom: kept from KB2j on, lost before.
        assert_eq!(before.clamp(2177, 2176), 2176 - 42);
        assert_eq!(after.clamp(2177, 2176), 5 + 2176 - 42);
        assert_eq!(after.clamp(-3, 2176), 5);
        assert_eq!(after.clamp(-4, 2176), 0);
    }

    #[test]
    fn a_square_below_the_picture_repeats_the_one_above_it_and_never_reads_the_rows_the_game_does_not_store(
    ) {
        // 32×16: the lower half of its one block lies below the picture.
        let (mut prev, mut cur) = (Frame::new(32, 16, false), Frame::new(32, 16, false));
        for (plane, rows) in [16, 8, 8].into_iter().enumerate() {
            let stride = prev.planes[plane].stride;
            for (i, byte) in prev.planes[plane].data.iter_mut().enumerate() {
                *byte = if i / stride < rows {
                    (i * 7 + plane) as u8
                } else {
                    0xEE
                };
            }
        }
        let reach = Reach {
            width: 64,
            height: 32,
            kb2j: false,
        };
        predict_block(&mut cur, &prev, (0, 0), [Mv::default(); 4], false, reach)
            .expect("on the picture");
        for (plane, rows) in [16, 8, 8].into_iter().enumerate() {
            let stride = cur.planes[plane].stride;
            let (above, below) = cur.planes[plane].data.split_at(rows * stride);
            assert_eq!(&below[..rows * stride], above, "plane {plane}");
        }
    }

    #[test]
    fn every_code_of_the_tables_reads_back_its_symbol_lowest_bit_first() {
        let tables: [(&Vlc, &[u16], &[u8]); 3] = [
            (&codes().ac_dense, tables::AC_CODES1, tables::AC_BITS1),
            (&codes().ac_sparse, tables::AC_CODES2, tables::AC_BITS2),
            (&codes().mv, tables::MV_CODES, tables::MV_BITS),
        ];
        for (vlc, codes, lens) in tables {
            for (symbol, (&code, &len)) in codes.iter().zip(lens).enumerate() {
                let text: String = (0..len)
                    .map(|i| if code >> i & 1 == 1 { '1' } else { '0' })
                    .collect();
                let bytes = packet(0, &text);
                let mut br = Bits::new(&bytes[4..]);
                assert_eq!(br.vlc(vlc).expect("a code"), symbol as u8);
                assert_eq!(
                    br.inner.bit_pos(),
                    usize::from(len),
                    "it takes its own length"
                );
            }
        }
    }

    #[test]
    fn a_read_the_packet_cannot_satisfy_is_refused_where_a_reader_would_pad_with_zeros() {
        let mut br = Bits::new(&[0xFF]);
        assert_eq!(br.read(8).expect("all of it"), 0xFF);
        assert!(br.read(1).is_err());
        assert!(br.bit().is_err());
        assert!(br.seek(2).is_err(), "a slice that starts past the end");
        assert!(br.seek(1).is_ok());
        assert!(br.unary(3, 1).is_err());
        // A run of three non-stop bits ends at its limit without a stop bit.
        let mut br = Bits::new(&[0b0000_0000]);
        assert_eq!(br.unary(3, 1).expect("a limited run"), 3);
        assert_eq!(br.inner.bit_pos(), 3);
    }

    #[test]
    fn a_levels_prefix_picks_a_small_value_or_a_count_of_extra_bits() {
        // A 0 straight away: the prefix is 0 and so is the level.
        let bytes = packet(0, "0");
        assert_eq!(Bits::new(&bytes[4..]).level().expect("a level"), 0);
        // Four 1-bits then a 0: prefix 4, one extra bit: 1 << 1 plus the bit plus 2.
        let bytes = packet(0, "1 1 1 1 0  1");
        assert_eq!(
            Bits::new(&bytes[4..]).level().expect("a level"),
            (1 << 1) + 1 + 2
        );
        // A signed level is never zero: a 0 is the prefix 1, level 1, and then the sign bit.
        let bytes = packet(0, "0  1");
        assert_eq!(Bits::new(&bytes[4..]).level_nonzero().expect("a level"), -1);
    }

    #[test]
    fn flags_are_raw_bits_or_runs_and_a_run_longer_than_what_is_left_is_refused() {
        let flags = |bits: &str, start, count| {
            let bytes = packet(0, bits);
            let mut br = Bits::new(&bytes[4..]);
            let mut out = Vec::new();
            decode_flags(&mut br, &mut out, start, count).map(|()| out)
        };
        assert_eq!(
            flags("0  1 0 1 0", 1, 4).expect("raw"),
            [false, true, false, true, false]
        );
        // Runs: a 1 for the coding, a 1 for a run, the bit 1: the four that are left are all 1.
        assert_eq!(
            flags("1  1  1", 1, 4).expect("a run"),
            [false, true, true, true, true]
        );
        // Five left: the run of 7 is cut to 5, and 5 is what is left, so nothing more is read.
        assert_eq!(flags("1  1  1", 0, 5).expect("a run"), [true; 5]);
        // Sixteen left: a run of 7 and 5 extra bits, 31, is more than what is left.
        assert!(flags("1  1  1  1 1 1 1 1", 0, 16).is_err());
    }

    #[test]
    fn a_new_quantiser_is_a_step_from_the_predicted_one_inside_the_table() {
        let quant = |bits: &str, prev| {
            let bytes = packet(0, bits);
            get_new_quant(&mut Bits::new(&bytes[4..]), prev)
        };
        assert_eq!(quant("1", 16).expect("no change"), 16);
        // Code 1 (a 0 then a 1), then the sign bit: up by one.
        assert_eq!(quant("0 1  0", 16).expect("up"), 17);
        assert_eq!(quant("0 1  1", 16).expect("down"), 15);
        assert!(quant("0 1  1", 0).is_err(), "below zero");
        assert!(quant("0 1  0", 36).is_err(), "past the table");
    }

    #[test]
    fn the_dc_predictors_stay_inside_their_range_and_start_a_picture_at_1024() {
        assert_eq!(dc_pred2(10, 20), 10);
        assert_eq!(
            dc_pred(5, 9, 7),
            9,
            "the gradient 9 - 5 + 7 = 11 is held to the largest"
        );
        let mut dcs = YDcInfo::default();
        dcs.resize(2);
        dcs.predict(0, true, true, 0, 0x7FF);
        assert!(dcs.dcs.iter().all(|&dc| dc == 1024));
        dcs.residuals[0] = 5000;
        dcs.predict(0, true, true, 0, 0x7FF);
        assert_eq!(dcs.dcs[0], 0x7FF, "held to the range");
    }
}

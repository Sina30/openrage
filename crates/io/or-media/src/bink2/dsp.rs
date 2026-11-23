//! The Bink 2 integer transform, motion compensation and the DC of a block, over planes of bytes.
//!
//! Origin: NihAV (https://git.nihav.org/?p=nihav.git), commit
//! 13f31633cf325b0036d697af7bf05c611b4b8967 of 2026-09-28, the file nihav-rad/src/codecs/bink2.rs
//! (`bink2_idct`, `Bink2DSP`), Copyright (c) Kostya Shishkov, AGPL-3.0-only. Ported to safe Rust
//! with the reference's integer arithmetic kept, except that a block's columns go through the transform
//! before its rows and the second pass of a chroma vector fractional both ways rounds to nearest,
//! which the install's own decoder fixed; a vector that leaves the reference picture is refused as
//! the reference refuses it, and an intermediate that a corrupt stream could push past 32 bits is held
//! in 64. Listed in LICENSE-third-party.md.

use std::ops::Add;

use crate::MediaError;

/// A motion vector in half-pixels of luma, which is quarter-pixels of chroma.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Mv {
    pub x: i16,
    pub y: i16,
}

impl Mv {
    /// The median of each component of the three vectors.
    pub(super) fn pred(a: Mv, b: Mv, c: Mv) -> Mv {
        let mid = |a: i16, b: i16, c: i16| a.max(b).min(a.min(b).max(c));
        Mv {
            x: mid(a.x, b.x, c.x),
            y: mid(a.y, b.y, c.y),
        }
    }
}

impl Add for Mv {
    type Output = Mv;

    fn add(self, other: Mv) -> Mv {
        Mv {
            x: self.x.wrapping_add(other.x),
            y: self.y.wrapping_add(other.y),
        }
    }
}

/// A plane a picture is predicted from: `stride` bytes a row, `rows` rows, all of them written.
#[derive(Clone, Copy)]
pub(super) struct Ref<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub rows: usize,
}

pub(super) fn clip8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

fn bad() -> MediaError {
    MediaError::Decode("this Bink 2 picture is corrupt: a motion vector leaves the picture".into())
}

/// One of the eight-point transform's two passes over a line of eight values.
fn idct_line(s: &[i32]) -> [i64; 8] {
    let s: [i64; 8] = std::array::from_fn(|i| i64::from(s[i]));
    let a = |v: i64| v + (v >> 2);
    let b = |v: i64| v >> 1;
    let c = |v: i64| v - (v >> 2) - (v >> 4);
    let d = |v: i64| v + (v >> 2) - (v >> 4);
    let e = |v: i64| v >> 2;
    let t00 = s[3] + s[5];
    let t01 = s[3] - s[5];
    let t02 = a(s[2]) + b(s[6]);
    let t03 = b(s[2]) - a(s[6]);
    let t0 = s[0] + s[4] + t02;
    let t1 = s[0] + s[4] - t02;
    let t2 = s[0] - s[4];
    let t3 = s[1] + t00;
    let t4 = s[1] - t00;
    let t5 = t01 + s[7];
    let t6 = t01 - s[7];
    let t7 = t4 + c(t6);
    let t8 = c(t4) - t6;
    let t9 = d(t3) + e(t5);
    let t10 = e(t3) - d(t5);
    let t11 = t2 + t03;
    let t12 = t2 - t03;
    [
        t0 + t9,
        t11 + t7,
        t12 + t8,
        t1 + t10,
        t1 - t10,
        t12 - t8,
        t11 - t7,
        t0 - t9,
    ]
}

/// The inverse transform of an 8×8 block in place: the first pass takes the columns, the second
/// takes the rows of what that left and scales by 1/64. The order is the one the shipped movies'
/// pixels fix, since the integer steps do not commute.
pub(super) fn idct(coeffs: &mut [i32; 64]) {
    let mut tmp = [0i32; 64];
    for i in 0..8 {
        let column: [i32; 8] = std::array::from_fn(|j| coeffs[8 * j + i]);
        for (k, v) in idct_line(&column).into_iter().enumerate() {
            tmp[i + 8 * k] = v as i32;
        }
    }
    for i in 0..8 {
        for (k, v) in idct_line(&tmp[8 * i..8 * i + 8]).into_iter().enumerate() {
            coeffs[8 * k + i] = (v >> 6) as i32;
        }
    }
}

/// Writes four transformed 8×8 blocks as a 16×16 square at `at`: two blocks a row, two rows.
fn put_square(
    dst: &mut [u8],
    at: usize,
    stride: usize,
    blocks: &[[i32; 64]; 4],
    mix: impl Fn(u8, i32) -> u8,
) {
    for (n, block) in blocks.iter().enumerate() {
        let (left, top) = ((n & 1) * 8, (n >> 1) * 8);
        for y in 0..8 {
            let row = &mut dst[at + (top + y) * stride + left..][..8];
            for (d, &c) in row.iter_mut().zip(&block[y * 8..y * 8 + 8]) {
                *d = mix(*d, c);
            }
        }
    }
}

/// Transforms four blocks and stores them as the 16×16 square at `at`.
pub(super) fn put_mb4(dst: &mut [u8], at: usize, stride: usize, blocks: &mut [[i32; 64]; 4]) {
    blocks.iter_mut().for_each(idct);
    put_square(dst, at, stride, blocks, |_, c| clip8(c));
}

/// Transforms four blocks and adds them to the 16×16 square at `at`.
pub(super) fn add_mb4(dst: &mut [u8], at: usize, stride: usize, blocks: &mut [[i32; 64]; 4]) {
    blocks.iter_mut().for_each(idct);
    put_square(dst, at, stride, blocks, |d, c| clip8(i32::from(d) + c));
}

/// The DC of the 8×8 block whose top left is `src[0]`: the rounded average of each column's
/// eight bytes, taken pairwise, summed over the eight columns.
pub(super) fn calc_dc(src: &[u8], stride: usize) -> i32 {
    let avg = |a: u16, b: u16| (a + b + 1) >> 1;
    let mut sum = 0u16;
    for x in 0..8 {
        let s = |y: usize| u16::from(src[x + stride * y]);
        let top = avg(avg(s(0), s(1)), avg(s(2), s(3)));
        let bottom = avg(avg(s(4), s(5)), avg(s(6), s(7)));
        sum += avg(top, bottom);
    }
    i32::from(sum)
}

/// The half-pixel filter over six samples `step` apart, `src[off]` and `src[off + step]` the middle two.
fn luma_filter<T: Copy + Into<i32>>(src: &[T], off: usize, step: usize) -> i32 {
    let at = |i: usize| -> i32 { src[i].into() };
    let t0 = at(off) + at(off + step);
    let t1 = at(off - step) + at(off + 2 * step);
    let t2 = at(off - 2 * step) + at(off + 3 * step);
    (((t0 * 19) >> 1) - t1 * 2 + (t2 >> 1) + 8) >> 4
}

/// Predicts the 16×16 luma square at (`xoff`, `yoff`) of the picture from `from` moved by `mv`;
/// `dst` starts at the row of 32 the square lies in.
pub(super) fn mc_luma(
    dst: &mut [u8],
    stride: usize,
    from: Ref<'_>,
    xoff: usize,
    yoff: usize,
    mv: Mv,
) -> Result<(), MediaError> {
    let sx = xoff as isize + (mv.x >> 1) as isize;
    let sy = yoff as isize + (mv.y >> 1) as isize;
    let mode = (mv.x & 1) + (mv.y & 1) * 2;
    let (dx, add_x) = if mv.x & 1 != 0 { (2, 5) } else { (0, 0) };
    let (dy, add_y) = if mv.y & 1 != 0 { (2, 5) } else { (0, 0) };
    if sx - dx < 0
        || sx - dx + add_x + 16 > from.stride as isize
        || sy - dy < 0
        || sy - dy + add_y + 16 > from.rows as isize
    {
        return Err(bad());
    }
    let pstride = from.stride;
    let src = from.data;
    let first = sx as usize + sy as usize * pstride;
    let dst = &mut dst[xoff + (yoff & 31) * stride..];
    match mode {
        0 => {
            for row in 0..16 {
                dst[row * stride..][..16].copy_from_slice(&src[first + row * pstride..][..16]);
            }
        }
        1 => {
            for row in 0..16 {
                for i in 0..16 {
                    dst[row * stride + i] = clip8(luma_filter(src, first + row * pstride + i, 1));
                }
            }
        }
        2 => {
            for row in 0..16 {
                for i in 0..16 {
                    dst[row * stride + i] =
                        clip8(luma_filter(src, first + row * pstride + i, pstride));
                }
            }
        }
        _ => {
            let mut tmp = [0i16; 21 * 16];
            for (row, out) in tmp.chunks_mut(16).enumerate() {
                for (i, v) in out.iter_mut().enumerate() {
                    *v = luma_filter(src, first + row * pstride + i - 2 * pstride, 1) as i16;
                }
            }
            for row in 0..16 {
                for i in 0..16 {
                    dst[row * stride + i] = clip8(luma_filter(&tmp, (row + 2) * 16 + i, 16));
                }
            }
        }
    }
    Ok(())
}

/// A weighted pair of neighbours: three parts to one, two to two or one to three, rounded by `shift`.
fn chroma_interp(e0: i32, e1: i32, mode: isize, shift: u32) -> i32 {
    let half = (1 << shift) >> 1;
    match mode {
        0 => e0,
        1 => (e0 * 3 + e1 + half) >> shift,
        2 => (e0 * 2 + e1 * 2 + half) >> shift,
        _ => (e0 + e1 * 3 + half) >> shift,
    }
}

/// Predicts the 8×8 chroma square at (`xoff`, `yoff`) from `from` moved by `mv` in quarter-pixels.
pub(super) fn mc_chroma(
    dst: &mut [u8],
    stride: usize,
    from: Ref<'_>,
    xoff: usize,
    yoff: usize,
    mv: Mv,
) -> Result<(), MediaError> {
    let sx = xoff as isize + (mv.x >> 2) as isize;
    let sy = yoff as isize + (mv.y >> 2) as isize;
    let (mx, my) = ((mv.x & 3) as isize, (mv.y & 3) as isize);
    let (add_x, add_y) = (isize::from(mx != 0), isize::from(my != 0));
    if sx < 0
        || sx + add_x + 8 > from.stride as isize
        || sy < 0
        || sy + add_y + 8 > from.rows as isize
    {
        return Err(bad());
    }
    let pstride = from.stride;
    let src = from.data;
    let first = sx as usize + sy as usize * pstride;
    let dst = &mut dst[xoff + (yoff & 15) * stride..];
    let at = |row: usize, i: usize| i32::from(src[first + row * pstride + i]);
    if mx == 0 && my == 0 {
        for row in 0..8 {
            dst[row * stride..][..8].copy_from_slice(&src[first + row * pstride..][..8]);
        }
    } else if my == 0 {
        for row in 0..8 {
            for i in 0..8 {
                dst[row * stride + i] = chroma_interp(at(row, i), at(row, i + 1), mx, 2) as u8;
            }
        }
    } else if mx == 0 {
        for row in 0..8 {
            for i in 0..8 {
                dst[row * stride + i] = chroma_interp(at(row, i), at(row + 1, i), my, 2) as u8;
            }
        }
    } else {
        let mut tmp = [0i32; 9 * 8];
        for row in 0..9 {
            for i in 0..8 {
                tmp[row * 8 + i] = chroma_interp(at(row, i), at(row, i + 1), mx, 0);
            }
        }
        for row in 0..8 {
            for i in 0..8 {
                let (e0, e1) = (tmp[row * 8 + i], tmp[(row + 1) * 8 + i]);
                dst[row * stride + i] = chroma_interp(e0, e1, my, 4) as u8;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dc_only_block_transforms_to_a_flat_square_of_its_value_over_eight() {
        // The DC is stored as 8 times the level plus 32 so the final shift by 6 rounds.
        for level in [0, 1, 5, 100, -7] {
            let mut block = [0i32; 64];
            block[0] = level * 8 + 32;
            idct(&mut block);
            let want = (level * 8 + 32) >> 6;
            assert!(
                block.iter().all(|&v| v == want),
                "level {level}: {block:?} is not flat at {want}"
            );
        }
    }

    #[test]
    fn the_transform_of_one_ac_coefficient_alternates_in_sign_down_the_block_and_is_flat_along_its_rows(
    ) {
        let mut block = [0i32; 64];
        block[1] = 64 * 64;
        idct(&mut block);
        let column: Vec<i32> = (0..8).map(|y| block[y * 8]).collect();
        assert!(column[0] > 0 && column[7] < 0, "{column:?}");
        assert!((column[0] + column[7]).abs() <= 1, "{column:?}");
        for y in 0..8 {
            assert!(
                block[y * 8..y * 8 + 8].iter().all(|&v| v == column[y]),
                "row {y} of the block is flat"
            );
        }
    }

    #[test]
    fn the_median_of_three_vectors_takes_each_component_apart() {
        let m = |x, y| Mv { x, y };
        assert_eq!(Mv::pred(m(1, 9), m(5, -3), m(3, 4)), m(3, 4));
        assert_eq!(Mv::pred(m(7, 0), m(7, 0), m(-7, 0)), m(7, 0));
        assert_eq!(
            m(i16::MAX, 0) + m(1, 0),
            m(i16::MIN, 0),
            "a sum wraps instead of panicking"
        );
    }

    #[test]
    fn the_dc_of_a_flat_block_is_eight_times_its_value_and_averages_pairwise_with_rounding() {
        let flat = [77u8; 8 * 8];
        assert_eq!(calc_dc(&flat, 8), 8 * 77);
        // Rows alternate 0 and 1: pair averages round up to 1 at every level, so each column is 1.
        let mut stripes = [0u8; 64];
        for (i, v) in stripes.iter_mut().enumerate() {
            *v = ((i / 8) % 2) as u8;
        }
        assert_eq!(calc_dc(&stripes, 8), 8);
    }

    /// A reference plane whose byte at (x, y) is `f(x, y)`.
    fn plane(width: usize, rows: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        (0..width * rows).map(|i| f(i % width, i / width)).collect()
    }

    #[test]
    fn a_whole_pixel_vector_copies_the_square_it_names() {
        let (w, h) = (64, 64);
        let data = plane(w, h, |x, y| (x * 3 + y * 7) as u8);
        let from = Ref {
            data: &data,
            stride: w,
            rows: h,
        };
        let mut dst = vec![0u8; w * 32];
        // Two pixels right, one down, from the square at (16, 0): vector (4, 2) in half-pixels.
        mc_luma(&mut dst, w, from, 16, 0, Mv { x: 4, y: 2 }).expect("inside the picture");
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(dst[16 + x + y * w], data[(16 + 2 + x) + (y + 1) * w]);
            }
        }
        let mut dst = vec![0u8; w * 16];
        mc_chroma(&mut dst, w, from, 8, 0, Mv { x: 8, y: 4 }).expect("inside the picture");
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(dst[8 + x + y * w], data[(8 + 2 + x) + (y + 1) * w]);
            }
        }
    }

    #[test]
    fn a_half_pixel_vector_over_a_flat_plane_stays_flat_and_over_a_ramp_lands_between() {
        let (w, h) = (64, 64);
        let flat = vec![100u8; w * h];
        let mut dst = vec![0u8; w * 32];
        let from = Ref {
            data: &flat,
            stride: w,
            rows: h,
        };
        for mv in [Mv { x: 1, y: 0 }, Mv { x: 0, y: 1 }, Mv { x: 1, y: 1 }] {
            mc_luma(&mut dst, w, from, 16, 16, mv).expect("inside");
            assert!(dst[16 + 16 * w..][..16].iter().all(|&v| v == 100), "{mv:?}");
        }
        let ramp = plane(w, h, |x, _| (x * 4) as u8);
        let from = Ref {
            data: &ramp,
            stride: w,
            rows: h,
        };
        mc_luma(&mut dst, w, from, 16, 16, Mv { x: 1, y: 0 }).expect("inside");
        let got = dst[20 + 16 * w];
        assert!(
            (80..=84).contains(&got),
            "between the samples at 20 and 21: {got}"
        );
    }

    #[test]
    fn a_vector_that_leaves_the_picture_is_refused_and_one_that_touches_its_edge_is_not() {
        let (w, h) = (32, 32);
        let data = vec![0u8; w * h];
        let from = Ref {
            data: &data,
            stride: w,
            rows: h,
        };
        let mut dst = vec![0u8; w * 32];
        assert!(mc_luma(&mut dst, w, from, 0, 0, Mv { x: -2, y: 0 }).is_err());
        assert!(mc_luma(&mut dst, w, from, 16, 0, Mv { x: 2, y: 0 }).is_err());
        // The filter reaches two samples before and three after: a half-pixel step needs the margin.
        assert!(mc_luma(&mut dst, w, from, 0, 0, Mv { x: 1, y: 0 }).is_err());
        assert!(mc_luma(&mut dst, w, from, 0, 0, Mv { x: 0, y: 0 }).is_ok());
        assert!(mc_luma(&mut dst, w, from, 16, 16, Mv { x: -32, y: 0 }).is_ok());
        assert!(mc_chroma(&mut dst, w, from, 24, 0, Mv { x: 4, y: 0 }).is_err());
        assert!(mc_chroma(&mut dst, w, from, 0, 0, Mv { x: -1, y: 0 }).is_err());
    }
}

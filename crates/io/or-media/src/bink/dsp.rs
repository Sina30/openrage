//! The pixel arithmetic of Bink 1 video: its integer inverse DCT, residue add and 2x block scale.
//!
//! Origin: FFmpeg n7.0 (commit 083443d67cb159ce469e5d902346b8d0c2cd1c93), libavcodec, the file
//! binkdsp.c, Copyright (c) 2009 Konstantin Shishkov, LGPL-2.1-or-later. Ported to safe Rust with
//! C's wrapping integer arithmetic kept exactly, since a frame must match the reference bit for
//! bit. Listed in LICENSE-third-party.md.

const A1: i32 = 2896;
const A2: i32 = 2217;
const A3: i32 = 3784;
const A4: i32 = -5352;

/// `(x * y) >> 11` in 32-bit arithmetic that wraps, as the reference's unsigned multiply does.
fn mul(x: i32, y: i32) -> i32 {
    (x as u32).wrapping_mul(y as u32) as i32 >> 11
}

/// One 8-point transform over `src[i * step]`, returning the eight outputs.
fn transform(src: &[i32; 64], at: usize, step: usize) -> [i32; 8] {
    let s = |i: usize| src[at + i * step];
    let a0 = s(0).wrapping_add(s(4));
    let a1 = s(0).wrapping_sub(s(4));
    let a2 = s(2).wrapping_add(s(6));
    let a3 = mul(A1, s(2).wrapping_sub(s(6)));
    let a4 = s(5).wrapping_add(s(3));
    let a5 = s(5).wrapping_sub(s(3));
    let a6 = s(1).wrapping_add(s(7));
    let a7 = s(1).wrapping_sub(s(7));
    let b0 = a4.wrapping_add(a6);
    let b1 = mul(A3, a5.wrapping_add(a7));
    let b2 = mul(A4, a5).wrapping_sub(b0).wrapping_add(b1);
    let b3 = mul(A1, a6.wrapping_sub(a4)).wrapping_sub(b2);
    let b4 = mul(A2, a7).wrapping_add(b3).wrapping_sub(b1);
    [
        a0.wrapping_add(a2).wrapping_add(b0),
        a1.wrapping_add(a3).wrapping_sub(a2).wrapping_add(b2),
        a1.wrapping_sub(a3).wrapping_add(a2).wrapping_add(b3),
        a0.wrapping_sub(a2).wrapping_sub(b4),
        a0.wrapping_sub(a2).wrapping_add(b4),
        a1.wrapping_sub(a3).wrapping_add(a2).wrapping_sub(b3),
        a1.wrapping_add(a3).wrapping_sub(a2).wrapping_sub(b2),
        a0.wrapping_add(a2).wrapping_sub(b0),
    ]
}

/// The column pass, with the reference's shortcut for a column whose only value is its top one.
fn columns(block: &[i32; 64]) -> [i32; 64] {
    let mut temp = [0i32; 64];
    for i in 0..8 {
        let flat = (1..8).all(|row| block[i + row * 8] == 0);
        let column = if flat {
            [block[i]; 8]
        } else {
            transform(block, i, 8)
        };
        for (row, value) in column.into_iter().enumerate() {
            temp[i + row * 8] = value;
        }
    }
    temp
}

/// The row pass: eight rows of `(x + 0x7F) >> 8`.
fn row(temp: &[i32; 64], r: usize) -> [i32; 8] {
    transform(temp, r * 8, 1).map(|x| x.wrapping_add(0x7F) >> 8)
}

/// Inverse DCT of `block`, written over `dst` as bytes truncated the way C stores an `int`.
pub(super) fn idct_put(dst: &mut [u8], at: usize, stride: usize, block: &[i32; 64]) {
    let temp = columns(block);
    for r in 0..8 {
        for (j, value) in row(&temp, r).into_iter().enumerate() {
            dst[at + r * stride + j] = value as u8;
        }
    }
}

/// Inverse DCT of `block`, added to `dst` modulo 256.
pub(super) fn idct_add(dst: &mut [u8], at: usize, stride: usize, block: &[i32; 64]) {
    let temp = columns(block);
    for r in 0..8 {
        for (j, value) in row(&temp, r).into_iter().enumerate() {
            let pixel = &mut dst[at + r * stride + j];
            *pixel = pixel.wrapping_add(value as u8);
        }
    }
}

/// A residue block added to `dst` modulo 256.
pub(super) fn add_pixels8(dst: &mut [u8], at: usize, stride: usize, block: &[i16; 64]) {
    for r in 0..8 {
        for j in 0..8 {
            let pixel = &mut dst[at + r * stride + j];
            *pixel = pixel.wrapping_add(block[r * 8 + j] as u8);
        }
    }
}

/// An 8x8 block drawn at twice its size: every source pixel becomes a 2x2 square at `at`.
pub(super) fn scale_block(src: &[u8; 64], dst: &mut [u8], at: usize, stride: usize) {
    for j in 0..8 {
        for i in 0..8 {
            let v = src[j * 8 + i];
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                dst[at + (2 * j + dy) * stride + 2 * i + dx] = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_with_only_a_dc_is_flat_at_the_dc_over_256() {
        let mut block = [0i32; 64];
        block[0] = 800;
        let mut dst = [0u8; 64];
        idct_put(&mut dst, 0, 8, &block);
        assert!(dst.iter().all(|&p| p == dst[0]), "{dst:?}");
        assert_eq!(dst[0], ((800 + 0x7F) >> 8) as u8);
    }

    /// The one-dimensional basis of the transform for its first AC term, worked by hand from the
    /// reference's macro with a coefficient of 25,600 (100 times 256, so every product is in whole
    /// numbers): the row's eight outputs before rounding are 25,600, 21,700, 14,500, 5,088 and the
    /// same four negated.
    const BASIS: [i8; 8] = [100, 85, 57, 20, -20, -57, -85, -100];

    #[test]
    fn a_horizontal_frequency_draws_its_basis_on_every_row_and_a_vertical_one_on_every_column() {
        let mut across = [0i32; 64];
        across[1] = 25_600;
        let mut put = [0u8; 64];
        idct_put(&mut put, 0, 8, &across);
        for r in 0..8 {
            let row: Vec<i8> = put[r * 8..r * 8 + 8].iter().map(|&p| p as i8).collect();
            assert_eq!(row, BASIS, "row {r}");
        }

        let mut down = [0i32; 64];
        down[8] = 25_600;
        let mut put = [0u8; 64];
        idct_put(&mut put, 0, 8, &down);
        for r in 0..8 {
            let row: Vec<i8> = put[r * 8..r * 8 + 8].iter().map(|&p| p as i8).collect();
            assert_eq!(row, [BASIS[r]; 8], "row {r}");
        }
    }

    #[test]
    fn an_added_transform_wraps_modulo_256_and_a_put_overwrites() {
        let mut block = [0i32; 64];
        block[0] = 16384; // 64 on every pixel
        let mut added = [200u8; 64];
        idct_add(&mut added, 0, 8, &block);
        assert!(
            added.iter().all(|&p| p == 200u8.wrapping_add(64)),
            "{added:?}"
        );
        let mut put = [200u8; 64];
        idct_put(&mut put, 0, 8, &block);
        assert!(put.iter().all(|&p| p == 64));
    }

    #[test]
    fn a_residue_adds_modulo_256_and_a_scaled_block_doubles_every_pixel() {
        let mut residue = [0i16; 64];
        residue[0] = -1;
        residue[63] = 300;
        let mut dst = [10u8; 64];
        add_pixels8(&mut dst, 0, 8, &residue);
        assert_eq!(
            (dst[0], dst[1], dst[63]),
            (9, 10, 10u8.wrapping_add(300u16 as u8))
        );

        let mut src = [0u8; 64];
        src[0] = 1;
        src[9] = 2;
        let mut big = [0u8; 256];
        scale_block(&src, &mut big, 0, 16);
        assert_eq!(&big[0..4], [1, 1, 0, 0]);
        assert_eq!(&big[16..20], [1, 1, 0, 0]);
        assert_eq!(&big[2 * 16 + 2..2 * 16 + 4], [2, 2]);
        assert_eq!(&big[3 * 16 + 2..3 * 16 + 4], [2, 2]);
        assert_eq!(big[4 * 16], 0);
    }
}

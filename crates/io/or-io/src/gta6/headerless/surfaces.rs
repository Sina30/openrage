//! The dictionary-shaped record a drawable points at from 0xD0: codes, and per code a surface of
//! triangles with a weight each and the tables that pick a triangle by its share of the weight.

use super::bytes::{f32_at, u32_at, Bytes};
use super::{HeaderlessError, Result};

const TRIANGLE: usize = 0x70;
const ENTRY: usize = 0xB0;

/// One entry: triangles in the drawable's frame, which no level of detail's geometry shares.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// The code that keys it, a hash of no name known.
    pub code: u32,
    pub triangles: Vec<[[f32; 3]; 3]>,
    /// Each triangle's weight, its area times the mean of its normals' w lanes in most.
    pub weights: Vec<f32>,
    /// Whether the entry holds its 24-byte record per triangle, whose words are not known.
    pub neighbours: bool,
    /// The word at +0xA0: 0x100 or 0x101.
    pub word: u32,
}

fn refuse<T>(why: &'static str) -> Result<T> {
    Err(HeaderlessError::Surfaces(why))
}

/// The surfaces of the record at `at`.
pub(super) fn surfaces_at(v: Bytes, at: usize) -> Result<Vec<Surface>> {
    let r = v.get("record at 0xD0", at, 0x40)?;
    let count = u16::from_le_bytes([r[0x28], r[0x29]]);
    let twins = [0x2A, 0x38, 0x3A].map(|o| u16::from_le_bytes([r[o], r[o + 1]]));
    if r[..0x18].iter().any(|b| *b != 0) || (u32_at(r, 0x18), u32_at(r, 0x1C)) != (1, 0) {
        return refuse("its head is not zero and a count of one reference");
    }
    if count == 0 || twins != [count; 3] || u32_at(r, 0x2C) != 0 || u32_at(r, 0x3C) != 0 {
        return refuse("its codes and entries are not one count stated four times");
    }
    let count = usize::from(count);
    let codes = v.array("codes", v.to("codes", at + 0x20)?, count, 4)?;
    let codes: Vec<u32> = (0..count).map(|k| u32_at(codes, 4 * k)).collect();
    if codes.windows(2).any(|w| w[0] >= w[1]) {
        return refuse("its codes do not ascend");
    }
    let entries = v.to("entries", at + 0x30)?;
    v.array("entries", entries, count, 8)?;
    (codes.into_iter().enumerate())
        .map(|(k, code)| surface(v, v.to("entry", entries + 8 * k)?, code))
        .collect()
}

/// The array at `at`: its pointer, two equal counts, the pointer null exactly for none.
fn array<'a>(v: Bytes<'a>, at: usize, stride: usize) -> Result<(usize, &'a [u8])> {
    let (count, again) = (v.u32("count", at + 8)?, v.u32("count", at + 0xC)?);
    let target = v.ptr("array", at)?;
    if count != again || target.is_none() != (count == 0) {
        return refuse(
            "an entry's array states two counts that differ, or a pointer for no element",
        );
    }
    let bytes = match target {
        Some(t) => v.array("entry's array", t, count as usize, stride)?,
        None => &[],
    };
    Ok((count as usize, bytes))
}

fn surface(v: Bytes, at: usize, code: u32) -> Result<Surface> {
    let e = v.get("entry", at, ENTRY)?;
    let zero = |r: std::ops::Range<usize>| e[r].iter().all(|b| *b == 0);
    let word = u32_at(e, 0xA0);
    if !zero(0..0x10) || (u32_at(e, 0x10), u32_at(e, 0x14)) != (1, 0) || !zero(0x70..0x80) {
        return refuse("an entry's head is not zero and a one, or its bytes at +0x70 are set");
    }
    if !matches!(word, 0x100 | 0x101)
        || !zero(0xA4..ENTRY)
        || v.ptr("entry's record", at + 0x68)?.is_none()
    {
        return refuse(
            "an entry's word at +0xA0 or its record at +0x68 is not one every measured entry has",
        );
    }
    let (n, a) = array(v, at + 0x18, TRIANGLE)?;
    let (b, _) = array(v, at + 0x28, 0x18)?;
    let (c_count, c) = array(v, at + 0x38, 4)?;
    let (d_count, d) = array(v, at + 0x48, 4)?;
    let (e_count, table) = array(v, at + 0x58, 4)?;
    if n == 0 || (b != 0 && b != n) || c_count != n + 1 || d_count != n || e_count != 4 * n + 1 {
        return refuse("an entry's arrays do not hold n triangles, n or no records, n + 1 and n floats and 4n + 1 words");
    }
    let lo: [f32; 4] = std::array::from_fn(|k| f32_at(e, 0x80 + 4 * k));
    let hi: [f32; 4] = std::array::from_fn(|k| f32_at(e, 0x90 + 4 * k));
    let reach = lo.iter().chain(&hi).fold(1f32, |m, x| m.max(x.abs()));
    let mut triangles = Vec::with_capacity(n);
    let mut weights = Vec::with_capacity(n);
    for t in a.as_chunks::<TRIANGLE>().0 {
        let corner = |k: usize| -> [f32; 4] { std::array::from_fn(|l| f32_at(t, 16 * k + 4 * l)) };
        let inside = (0..3).all(|k| {
            (0..4).all(|l| {
                let x = corner(k)[l];
                lo[l] - 1e-3 * reach <= x && x <= hi[l] + 1e-3 * reach
            })
        });
        let w: [u32; 4] = std::array::from_fn(|l| u32_at(t, 0x60 + 4 * l));
        if !inside || w.iter().any(|x| *x != w[0]) {
            return refuse("a triangle leaves its entry's box, or its four weight lanes differ");
        }
        triangles.push(std::array::from_fn(|k| {
            let [x, y, z, _] = corner(k);
            [x, y, z]
        }));
        weights.push(f32::from_bits(w[0]));
    }
    let cumulative: Vec<f32> = (0..=n).map(|k| f32_at(c, 4 * k)).collect();
    let inverse: Vec<f32> = (0..n).map(|k| f32_at(d, 4 * k)).collect();
    let buckets: Vec<u32> = (0..=4 * n).map(|k| u32_at(table, 4 * k)).collect();
    tables_hold(&weights, &cumulative, &inverse, &buckets)?;
    Ok(Surface {
        code,
        triangles,
        weights,
        neighbours: b > 0,
        word,
    })
}

/// Whether the weights' cumulative shares, their inverse steps and the 4n + 1 buckets into the
/// shares are those the weights give.
fn tables_hold(
    weights: &[f32],
    cumulative: &[f32],
    inverse: &[f32],
    buckets: &[u32],
) -> Result<()> {
    let n = weights.len();
    let sum: f32 = weights.iter().sum();
    if sum == 0.0 {
        if cumulative.iter().all(|x| x.is_nan()) && buckets.iter().all(|b| *b == u32::MAX) {
            return Ok(());
        }
        return refuse("an entry of no weight does not hold NaN shares and empty buckets");
    }
    if cumulative[0] != 0.0 || (cumulative[n] - 1.0).abs() >= 1e-5 {
        return refuse("an entry's cumulative shares do not run from 0 to 1");
    }
    for (k, w) in weights.iter().enumerate() {
        let (step, share) = (cumulative[k + 1] - cumulative[k], w / sum);
        if (step - share).abs() >= 1e-5 + 1e-3 * share {
            return refuse(
                "an entry's cumulative share steps are not its triangles' shares of the weight",
            );
        }
        if step > 0.0 && (inverse[k] * step - 1.0).abs() >= 1e-3 {
            return refuse("an entry's inverse steps are not one over its share steps");
        }
    }
    if buckets.windows(2).any(|w| w[0] > w[1]) || buckets.iter().any(|b| *b as usize >= n) {
        return refuse("an entry's buckets do not ascend below its triangle count");
    }
    // Bucket j holds the triangle whose share interval holds max(0, j - 1) / (4n + 1), within one
    // at the floats' boundaries.
    let held = (buckets.iter().enumerate())
        .filter(|(j, b)| {
            let x = j.saturating_sub(1) as f32 / (4 * n + 1) as f32;
            let want = cumulative
                .partition_point(|c| *c <= x)
                .saturating_sub(1)
                .min(n - 1);
            (**b as usize).abs_diff(want) <= 1 && cumulative[**b as usize] <= x + 1e-5
        })
        .count();
    if 50 * held < 49 * buckets.len() {
        return refuse(
            "an entry's buckets do not name the triangles their place in the shares gives",
        );
    }
    Ok(())
}

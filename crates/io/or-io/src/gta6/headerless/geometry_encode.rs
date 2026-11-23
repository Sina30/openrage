//! The inverses of the vertex decodes a geometry's reader makes, and the words a geometry derives
//! from its vertices and triangles: the position quantisation block and the meshlet list.

use std::f32::consts::TAU;

use super::bytes::{f32_at, u32_at};
use super::streams::frame;
use crate::{f32_to_half, half_to_f32};

/// Bits of a quantised position's lanes A, B and C.
const LANE_BITS: [u32; 3] = [24, 24, 16];
/// The axis each lane gives, by the block's axis word, which names the axis lane C gives.
const LANE_AXES: [[usize; 3]; 3] = [[2, 1, 0], [2, 0, 1], [0, 1, 2]];
/// Quanta a kept block may be passed by before it is derived again from the new box.
const KEPT_SLACK: f64 = 2.0;

/// A quantised position's block: each lane's origin and step, and the axis word.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Quantisation {
    pub(super) origin: [f32; 3],
    pub(super) step: [f32; 3],
    pub(super) axis: u32,
}

impl Quantisation {
    /// The block of a box: lane C along its smallest f32 extent, the lowest axis on a tie, each
    /// step the extent times the f32 of one over the lane's top quantum.
    pub(super) fn of_box(min: [f32; 3], max: [f32; 3]) -> Self {
        let extent: [f32; 3] = std::array::from_fn(|a| max[a] - min[a]);
        let axis = (1..3).fold(0, |low, a| if extent[a] < extent[low] { a } else { low });
        let axes = LANE_AXES[axis];
        Self {
            origin: axes.map(|a| min[a]),
            step: std::array::from_fn(|l| {
                let top = ((1u64 << LANE_BITS[l]) - 1) as f64;
                extent[axes[l]].max(1e-12) * (1.0 / top) as f32
            }),
            axis: axis as u32,
        }
    }

    /// The block at 0x60 of a vertex data header of 0x80 bytes; `None` for an axis word past 2.
    pub(super) fn read(header: &[u8]) -> Option<Self> {
        let axis = u32_at(header, 0x6C);
        (axis < 3).then(|| Self {
            origin: std::array::from_fn(|l| f32_at(header, 0x60 + 4 * l)),
            step: std::array::from_fn(|l| f32_at(header, 0x70 + 4 * l)),
            axis,
        })
    }

    /// The block over 0x60 to 0x80 of `header`, 4.0 last.
    pub(super) fn write(&self, header: &mut [u8]) {
        for (l, (origin, step)) in self.origin.iter().zip(&self.step).enumerate() {
            header[0x60 + 4 * l..0x64 + 4 * l].copy_from_slice(&origin.to_le_bytes());
            header[0x70 + 4 * l..0x74 + 4 * l].copy_from_slice(&step.to_le_bytes());
        }
        header[0x6C..0x70].copy_from_slice(&self.axis.to_le_bytes());
        header[0x7C..0x80].copy_from_slice(&4.0f32.to_le_bytes());
    }

    fn axes(&self) -> [usize; 3] {
        LANE_AXES[self.axis as usize]
    }

    /// The nearest quanta of `p`, clamped to each lane, and whether every lane held it within
    /// [`KEPT_SLACK`] quanta; computed in f64, as the reader decodes.
    pub(super) fn quanta(&self, p: [f32; 3]) -> ([u32; 3], bool) {
        let mut held = true;
        let axes = self.axes();
        let q = std::array::from_fn(|l| {
            let top = ((1u64 << LANE_BITS[l]) - 1) as f64;
            let (x, o, s) = (
                f64::from(p[axes[l]]),
                f64::from(self.origin[l]),
                f64::from(self.step[l]),
            );
            let q = match s == 0.0 {
                true if x == o => 0.0,
                true => f64::INFINITY,
                false => ((x - o) / s).round(),
            };
            held &= (-KEPT_SLACK..=top + KEPT_SLACK).contains(&q);
            q.clamp(0.0, top) as u32
        });
        (q, held)
    }

    /// The position quanta `q` decode to, as the reader decodes them.
    pub(super) fn decode(&self, q: [u32; 3]) -> [f64; 3] {
        let mut p = [0.0; 3];
        for (l, axis) in self.axes().iter().enumerate() {
            p[*axis] = f64::from(self.origin[l]) + f64::from(q[l]) * f64::from(self.step[l]);
        }
        p
    }
}

/// Three lanes as an 8-byte quantised element: lane A in bytes 3, 2, 1, lane B in bytes 5, 4, 0
/// and lane C in bytes 7, 6.
pub(super) fn pack_lanes([a, b, c]: [u32; 3]) -> [u8; 8] {
    let lo = (a << 8) | (b & 0xFF);
    let hi = (b >> 8) | (c << 16);
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&lo.to_le_bytes());
    out[4..].copy_from_slice(&hi.to_le_bytes());
    out
}

/// `bits`-bit unsigned normalised lanes over [-1, 1], as the reader's lanes decode them.
fn unorm(x: f32, bits: u32) -> u32 {
    let max = ((1u32 << bits) - 1) as f32;
    ((x + 1.0) * (max / 2.0)).round().clamp(0.0, max) as u32
}

/// The octahedral pair of a direction, the lower hemisphere folded over the diagonals.
fn octahedral_pair(n: [f32; 3]) -> [f32; 2] {
    let sum = n[0].abs() + n[1].abs() + n[2].abs();
    if sum == 0.0 {
        return [0.0, 0.0];
    }
    let p = n.map(|x| x / sum);
    match p[2] < 0.0 {
        true => [
            (1.0 - p[1].abs()) * 1f32.copysign(p[0]),
            (1.0 - p[0].abs()) * 1f32.copysign(p[1]),
        ],
        false => [p[0], p[1]],
    }
}

/// The angle-form frame word of normal `n` and tangent `t`, whose `w` sets bit 30: the angle is
/// measured about the normal the word decodes to, in the basis the reader builds about it.
pub(super) fn frame_word(n: [f32; 3], t: [f32; 4]) -> u32 {
    let [x, y] = octahedral_pair(n);
    let base = unorm(x, 10) | unorm(y, 10) << 10 | 1 << 31;
    // At angle 0 the decoded tangent is the basis's first vector; at three quarters of a turn
    // its second, to within the f32 cosine of that angle.
    let (_, b1) = frame(base);
    let (_, b2) = frame(base | 768 << 20);
    let dot = |b: [f32; 4]| t[0] * b[0] + t[1] * b[1] + t[2] * b[2];
    let theta = (-dot(b2)).atan2(dot(b1));
    let angle = ((theta * 1024.0 / TAU).round() as i32).rem_euclid(1024) as u32;
    base | angle << 20 | u32::from(t[3] > 0.0) << 30
}

/// A 10:10:10 word of `v`, its bits 30 and 31 `high`.
pub(super) fn unorm10_word(v: [f32; 3], high: u32) -> u32 {
    unorm(v[0], 10) | unorm(v[1], 10) << 10 | unorm(v[2], 10) << 20 | (high & 3) << 30
}

/// Four bytes over 255.
pub(super) fn unorm_bytes(v: [f32; 4]) -> [u8; 4] {
    v.map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// A texcoord pair as two s16 over 8192; `None` where a value passes the s16 range.
pub(super) fn s16_pair(uv: [f32; 2]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    for (k, x) in uv.iter().enumerate() {
        let q = (x * 8192.0).round();
        if !(f32::from(i16::MIN)..=f32::from(i16::MAX)).contains(&q) {
            return None;
        }
        out[2 * k..2 * k + 2].copy_from_slice(&(q as i16).to_le_bytes());
    }
    Some(out)
}

/// Most vertices and triangles a meshlet holds, its local indices being 7 bits.
const MESHLET_LIMIT: usize = 128;

/// A meshlet: its vertices in the order its triangles first use them, and its triangles as
/// indices into that list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Meshlet {
    pub(super) vertices: Vec<u32>,
    pub(super) triangles: Vec<[u8; 3]>,
}

/// `triangles` cut in their order into meshlets: one closes before a triangle that would make it
/// hold more than 128 vertices or 128 triangles.
pub(super) fn meshlets(triangles: &[[u32; 3]]) -> Vec<Meshlet> {
    let mut out = Vec::new();
    let mut current = Meshlet::default();
    for t in triangles {
        let new = (0..3)
            .filter(|&c| !current.vertices.contains(&t[c]) && !t[..c].contains(&t[c]))
            .count();
        if current.triangles.len() == MESHLET_LIMIT || current.vertices.len() + new > MESHLET_LIMIT
        {
            out.push(std::mem::take(&mut current));
        }
        let local = t.map(|v| {
            let at = match current.vertices.iter().position(|x| *x == v) {
                Some(at) => at,
                None => {
                    current.vertices.push(v);
                    current.vertices.len() - 1
                }
            };
            at as u8
        });
        current.triangles.push(local);
    }
    if !current.triangles.is_empty() {
        out.push(current);
    }
    out
}

/// The radius word half and the centre words of a sphere about `points` that holds every one:
/// the centre of their box on the 1/16 m grid, coarsened by the exponent until each field fits
/// 12 bits, and the f16 radius rounded up.
pub(super) fn meshlet_bounds(points: &[[f64; 3]]) -> Result<(u32, u32, u32), &'static str> {
    let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in points {
        for a in 0..3 {
            low[a] = low[a].min(p[a]);
            high[a] = high[a].max(p[a]);
        }
    }
    let centre: [f64; 3] = std::array::from_fn(|a| (low[a] + high[a]) / 2.0);
    let (e, fields) = (0..16)
        .find_map(|e: i32| {
            let fields = centre.map(|c| 2048.0 + (c * 2f64.powi(4 - e)).floor());
            fields
                .iter()
                .all(|f| (0.0..=4095.0).contains(f))
                .then_some((e, fields))
        })
        .ok_or("a meshlet's centre lies past the range its exponent reaches")?;
    let stored = fields.map(|f| (f - 2048.0) * 2f64.powi(e - 4));
    let radius = (points.iter())
        .map(|p| {
            let d: [f64; 3] = std::array::from_fn(|a| p[a] - stored[a]);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(0.0, f64::max);
    let mut half = f32_to_half(radius as f32);
    while f64::from(half_to_f32(half)) < radius && half < 0x7C00 {
        half += 1;
    }
    if half >= 0x7C00 {
        return Err("a meshlet's radius passes the f16 range");
    }
    let [x, y, z] = fields.map(|f| f as u32);
    Ok((u32::from(half) << 16, x | y << 12 | (e as u32) << 28, z))
}

/// The index data of `meshlets` over `positions`: the header, four words a meshlet, the triangle
/// words, the vertex list two entries a word, and zero words to a multiple of four.
pub(super) fn index_words(
    meshlets: &[Meshlet],
    positions: &[[f64; 3]],
) -> Result<Vec<u32>, &'static str> {
    let m = meshlets.len();
    let t: usize = meshlets.iter().map(|k| k.triangles.len()).sum();
    let list: usize = meshlets.iter().map(|k| k.vertices.len()).sum();
    if m > 0xFFFF {
        return Err("it needs more than 65,535 meshlets");
    }
    if list > 0xFFFF {
        return Err(
            "its meshlets' vertex lists pass 65,535 entries, which their offsets cannot reach",
        );
    }
    let mut words = Vec::with_capacity((3 + 4 * m + t + list.div_ceil(2)).next_multiple_of(4));
    words.extend([0x8000_0000 | m as u32, t as u32, list as u32]);
    let (mut first_vertex, mut first_triangle) = (0usize, 0usize);
    for k in meshlets {
        if first_triangle > 0xFFFF {
            return Err("a meshlet's first triangle passes the 65,535 its offset reaches");
        }
        let points: Vec<[f64; 3]> = (k.vertices.iter())
            .map(|v| positions.get(*v as usize).copied())
            .collect::<Option<_>>()
            .ok_or("a triangle names a vertex past the vertex count")?;
        let (radius, centre, z) = meshlet_bounds(&points)?;
        let counts = (k.vertices.len() as u32 - 1) | (k.triangles.len() as u32 - 1) << 7;
        words.extend([
            counts | radius,
            first_vertex as u32 | (first_triangle as u32) << 16,
            centre,
            z,
        ]);
        first_vertex += k.vertices.len();
        first_triangle += k.triangles.len();
    }
    for (index, k) in meshlets.iter().enumerate() {
        let tag = ((index & 0x7FF) as u32) << 21;
        words.extend(
            (k.triangles.iter())
                .map(|[a, b, c]| u32::from(*a) | u32::from(*b) << 7 | u32::from(*c) << 14 | tag),
        );
    }
    let entries: Vec<u16> = (meshlets.iter())
        .flat_map(|k| k.vertices.iter().map(|v| *v as u16))
        .collect();
    words.extend(
        (entries.chunks(2))
            .map(|pair| u32::from(pair[0]) | u32::from(pair.get(1).copied().unwrap_or(0)) << 16),
    );
    words.resize(words.len().next_multiple_of(4), 0);
    Ok(words)
}

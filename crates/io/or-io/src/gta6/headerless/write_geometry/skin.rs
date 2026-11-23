//! A geometry's weights, blend indices and texcoord tangents written from an edit's values, in the
//! form each channel declares, for any vertex count but the one form whose high bits no writer
//! can derive.

use super::{chosen, elements, of_channel, u32_at, unorm10_word, Channel, GeometryEdit};
use super::{GeometryWriteError, Written};
use crate::gta6::headerless::Vertices;

/// Why a new vertex count is refused on a geometry whose blend indices take the 0x8C form.
const NO_FILL: &str = "bits 12 to 31 of the 0x8C blend indices are a per-vertex field that cannot \
                       be derived, so the vertex count is kept";

pub(super) fn no_fill(channel: u8) -> GeometryWriteError {
    GeometryWriteError::Channel {
        channel,
        why: NO_FILL,
    }
}

/// Eight weights as bytes summing to 255, each within one of its exact share: largest remainder.
pub(super) fn weight_bytes(lanes: [f32; 8]) -> [u8; 8] {
    let w = lanes.map(|x| match x.is_finite() && x > 0.0 {
        true => f64::from(x),
        false => 0.0,
    });
    let total: f64 = w.iter().sum();
    if total <= 0.0 {
        return [0; 8];
    }
    let scaled = w.map(|x| x / total * 255.0);
    let mut out = scaled.map(|x| x.floor().min(255.0) as u8);
    let short = 255u32.saturating_sub(out.iter().map(|&b| u32::from(b)).sum());
    let mut order: Vec<usize> = (0..8).collect();
    let fraction = |k: usize| scaled[k] - scaled[k].floor();
    order.sort_by(|&a, &b| fraction(b).total_cmp(&fraction(a)));
    for &k in order.iter().take(short as usize) {
        out[k] = out[k].saturating_add(1);
    }
    out
}

/// Weight channel `channel`, 16 or 17, of `n` vertices: both channels' lanes rounded together.
pub(super) fn weights(
    channel: u8,
    edit: &GeometryEdit,
    held: &Vertices,
    n: usize,
    was: &[u8],
) -> Written<Vec<u8>> {
    let (given, read) = (&edit.weights, &held.weights);
    let first = chosen(of_channel(given, 16), of_channel(read, 16), n, 16)?;
    let second = match read.iter().any(|(k, _)| *k == 17) {
        true => Some(chosen(of_channel(given, 17), of_channel(read, 17), n, 17)?),
        false => None,
    };
    let held_at = |c: u8, i: usize| of_channel(&held.weights, c).get(i).copied();
    let same = |i: usize| {
        held_at(16, i) == Some(first[i]) && second.is_none_or(|s| held_at(17, i) == Some(s[i]))
    };
    let half = usize::from(channel == 17) * 4;
    elements(n, 4, was, same, |i, e| {
        let (a, b) = (first[i], second.map_or([0.0; 4], |s| s[i]));
        let bytes = weight_bytes([a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]]);
        e.copy_from_slice(&bytes[half..half + 4]);
        Ok(())
    })
}

/// Blend-index channel `c`, 20 or 21, of `n` vertices from the edit's palette indices; a 0x8C
/// lane keeps its bits 12 to 31 as read, and a one-index form takes lane 0.
pub(super) fn indices(
    c: &Channel,
    edit: &GeometryEdit,
    held: &Vertices,
    n: usize,
    was: &[u8],
) -> Written<Vec<u8>> {
    let channel = c.index;
    let refuse = |why| GeometryWriteError::Channel { channel, why };
    let read = of_channel(&held.joints, channel);
    let lanes = chosen(of_channel(&edit.joints, channel), read, n, channel)?;
    let byte = |x: u16| u8::try_from(x).map_err(|_| refuse("an index passes the 255 a byte holds"));
    let size = usize::from(c.size);
    let encode = |i: usize, e: &mut [u8]| -> Written<()> {
        let l = lanes[i];
        match (c.code, c.size) {
            (0x1E, 4) => {
                for (b, x) in e.iter_mut().zip(l) {
                    *b = byte(x)?;
                }
            }
            (0x0C, 8) => {
                for (k, x) in l.iter().enumerate() {
                    e[2 * k..2 * k + 2].copy_from_slice(&x.to_le_bytes());
                }
            }
            (0x3E, 1) => e[0] = byte(l[0])?,
            (0x39, 2) => e.copy_from_slice(&l[0].to_le_bytes()),
            (0x03, 16) => {
                let old = was.get(16 * i..16 * i + 16).ok_or(refuse(NO_FILL))?;
                for (k, x) in l.iter().enumerate() {
                    if *x >= 0x1000 {
                        return Err(refuse("an index passes the 4,095 its 12 bits hold"));
                    }
                    let word = (u32_at(old, 4 * k) & !0xFFF) | u32::from(*x);
                    e[4 * k..4 * k + 4].copy_from_slice(&word.to_le_bytes());
                }
            }
            _ => return Err(refuse("its blend indices are in a form no writer encodes")),
        }
        Ok(())
    };
    elements(n, size, was, |i| read.get(i) == Some(&lanes[i]), encode)
}

/// Texcoord tangent channel `channel`, 9 to 11, of `n` vertices: a 10:10:10 word, its sign bits 0
/// where `w` is positive, else 3.
pub(super) fn uv_tangents(
    channel: u8,
    edit: &GeometryEdit,
    held: &Vertices,
    n: usize,
    was: &[u8],
) -> Written<Vec<u8>> {
    let read = of_channel(&held.uv_tangents, channel);
    let tangents = chosen(of_channel(&edit.uv_tangents, channel), read, n, channel)?;
    elements(
        n,
        4,
        was,
        |i| read.get(i) == Some(&tangents[i]),
        |i, e| {
            let [x, y, z, w] = tangents[i];
            let sign = if w > 0.0 { 0 } else { 3 };
            e.copy_from_slice(&unorm10_word([x, y, z], sign).to_le_bytes());
            Ok(())
        },
    )
}

//! Channel 33's 8-byte form at another vertex count. Where lane 0 sets bit 15, lanes 1 to 3 hold
//! a reference position in units of 1/8192 m about 32,768: the vertex's own position moved by an
//! offset of the buffer's, which the kept vertices give and a new vertex takes.

use super::{GeometryWriteError, Written};

/// How far, in metres, a kept vertex's reference may lie from its position moved by the offset,
/// besides [`SPREAD`] of the geometry's extent on that axis.
const NEAR: f64 = 2.0 / 8192.0;
const SPREAD: f64 = 0.001;

fn refuse(why: &'static str) -> GeometryWriteError {
    GeometryWriteError::Channel { channel: 33, why }
}

fn lanes(e: &[u8]) -> [u16; 4] {
    std::array::from_fn(|k| u16::from_le_bytes([e[2 * k], e[2 * k + 1]]))
}

fn metres(q: u16) -> f64 {
    (f64::from(q) - 32768.0) / 8192.0
}

/// Channel 33 of `positions`, from the lanes `was` holds for the `held` positions: a vertex at its
/// held position keeps its lanes, any other takes its position moved by the offset.
pub(super) fn reference(held: &[[f32; 3]], positions: &[[f32; 3]], was: &[u8]) -> Written<Vec<u8>> {
    let read: Vec<[u16; 4]> = was.as_chunks::<8>().0.iter().map(|c| lanes(c)).collect();
    if read.is_empty() || read.len() != held.len() {
        return Err(refuse("its lanes and its positions are not of one count"));
    }
    if read.iter().any(|l| l[0] & 0x8000 == 0) {
        return Err(refuse(
            "a vertex's lane 0 clears bit 15, and such lanes are kept only at the vertex count",
        ));
    }
    let lead = read[0][0];
    if read.iter().any(|l| l[0] != lead) {
        return Err(refuse(
            "lane 0 takes more than one value, and a new vertex's is not known",
        ));
    }
    let (mut offset, mut extent) = ([0.0f64; 3], [0.0f64; 3]);
    for a in 0..3 {
        let mut apart: Vec<f64> = (read.iter().zip(held))
            .map(|(l, p)| metres(l[a + 1]) - f64::from(p[a]))
            .collect();
        apart.sort_by(f64::total_cmp);
        offset[a] = apart[apart.len() / 2];
        let (lo, hi) = (held.iter()).fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(f64::from(p[a])), hi.max(f64::from(p[a])))
        });
        extent[a] = hi - lo;
    }
    let far = (read.iter().zip(held))
        .filter(|(l, p)| {
            (0..3).any(|a| {
                let off = metres(l[a + 1]) - f64::from(p[a]) - offset[a];
                off.abs() > NEAR + SPREAD * extent[a]
            })
        })
        .count();
    // A buffer whose lanes follow its positions does so on 99 % of its vertices.
    if far * 100 > read.len() {
        return Err(refuse(
            "its lanes are not its positions moved by one offset, so a new vertex's are not derived",
        ));
    }
    let mut out = Vec::with_capacity(8 * positions.len());
    for (i, p) in positions.iter().enumerate() {
        match (held.get(i), was.get(8 * i..8 * i + 8)) {
            (Some(h), Some(e)) if h == p => out.extend_from_slice(e),
            _ => {
                out.extend(lead.to_le_bytes());
                for a in 0..3 {
                    let q = ((f64::from(p[a]) + offset[a]) * 8192.0 + 32768.0).round();
                    if !(0.0..=65535.0).contains(&q) {
                        return Err(refuse(
                            "a new vertex's reference passes the 4 m either side its lanes hold",
                        ));
                    }
                    out.extend((q as u16).to_le_bytes());
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFSET: [f32; 3] = [0.5, -0.25, 1.0];

    fn element(lead: u16, p: [f32; 3]) -> Vec<u8> {
        let q = |a: usize| (f64::from(p[a] + OFFSET[a]) * 8192.0 + 32768.0).round() as u16;
        [lead, q(0), q(1), q(2)]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect()
    }

    #[test]
    fn a_new_vertex_takes_its_position_moved_by_the_offset_the_kept_ones_hold() {
        let held = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let was: Vec<u8> = held.iter().flat_map(|p| element(0x806F, *p)).collect();
        let mut positions = held.to_vec();
        positions.push([1.0, 1.0, 0.0]);
        let out = reference(&held, &positions, &was).expect("derived");
        assert_eq!(out[..24], was[..]);
        assert_eq!(out[24..], element(0x806F, [1.0, 1.0, 0.0])[..]);
        let back = lanes(&out[24..]);
        assert_eq!(metres(back[1]), 1.5);

        let mut clear = was.clone();
        clear[1] = 0x00;
        let refused = reference(&held, &positions, &clear).expect_err("a clear bit 15");
        assert!(refused.to_string().contains("bit 15"), "{refused}");

        let mut other = was.clone();
        other[0] = 0x70;
        let refused = reference(&held, &positions, &other).expect_err("two lane-0 values");
        assert!(
            refused.to_string().contains("more than one value"),
            "{refused}"
        );

        let mut moved = was.clone();
        moved[16 + 2..16 + 4].copy_from_slice(&0u16.to_le_bytes());
        let refused = reference(&held, &positions, &moved).expect_err("no one offset");
        assert!(refused.to_string().contains("one offset"), "{refused}");
    }
}

#![allow(dead_code)]
//! Meshes the mesh doctor's suites build, so that a failure names a shape and not a file.

use or_interchange::{LodSlot, Primitive};
use or_io::codec::mesh::GrcFvfChannel;

/// A unit sphere cut into `rings` bands of `segments` quads, with a texture seam where the last column meets the first, a normal, a tangent, two sets of texture coordinates and a skin of four bones banded by latitude.
pub fn sphere(rings: usize, segments: usize) -> Primitive {
    let mut p = Primitive {
        lod: LodSlot::Level(0),
        uv_channels: vec![GrcFvfChannel::Tex0 as u8, GrcFvfChannel::Tex1 as u8],
        colour_channels: vec![0],
        uvs: vec![Vec::new(), Vec::new()],
        colours: vec![Vec::new()],
        ..Primitive::default()
    };
    let columns = segments + 1;
    for r in 0..=rings {
        let theta = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..columns {
            let phi = std::f32::consts::TAU * s as f32 / segments as f32;
            let position = [
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            ];
            p.positions.push(position);
            p.normals.push(position);
            p.tangents.push([-phi.sin(), phi.cos(), 0.0, 1.0]);
            let uv = [s as f32 / segments as f32, r as f32 / rings as f32];
            p.uvs[0].push(uv);
            p.uvs[1].push([uv[0] * 0.5, uv[1] * 0.5]);
            p.colours[0].push([1.0, 1.0, 1.0, r as f32 / rings as f32]);
            let band = r as f32 / rings as f32 * 3.0;
            let low = band.floor().min(2.0) as u16;
            let across = band - f32::from(low);
            p.joints.push([low, low + 1, 0, 0]);
            p.weights.push([1.0 - across, across, 0.0, 0.0]);
        }
    }
    let at = |r: usize, s: usize| (r * columns + s) as u32;
    for r in 0..rings {
        for s in 0..segments {
            let (a, b, c, d) = (at(r, s), at(r + 1, s), at(r + 1, s + 1), at(r, s + 1));
            // At a pole one corner of each quad lands on another, which makes a triangle with no area.
            if r != rings - 1 {
                p.indices.extend([a, b, c]);
            }
            if r != 0 {
                p.indices.extend([a, c, d]);
            }
        }
    }
    p
}

/// A flat sheet of `n` by `n` quads in the unit square of the z = 0 plane, open on all four sides.
pub fn sheet(n: usize) -> Primitive {
    let mut p = Primitive {
        lod: LodSlot::Level(0),
        uv_channels: vec![GrcFvfChannel::Tex0 as u8],
        uvs: vec![Vec::new()],
        ..Primitive::default()
    };
    for y in 0..=n {
        for x in 0..=n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            p.positions.push([u, v, 0.0]);
            p.normals.push([0.0, 0.0, 1.0]);
            p.uvs[0].push([u, v]);
        }
    }
    let at = |x: usize, y: usize| (y * (n + 1) + x) as u32;
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1));
            p.indices.extend([a, b, c, a, c, d]);
        }
    }
    p
}

/// The distance from `p` to the triangle `a b c`.
pub fn distance_to_triangle(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let v = |x: [f32; 3]| x.map(f64::from);
    let (p, a, b, c) = (v(p), v(a), v(b), v(c));
    let sub = |x: [f64; 3], y: [f64; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let closest = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                let t = d1 / (d1 - d3);
                [a[0] + t * ab[0], a[1] + t * ab[1], a[2] + t * ab[2]]
            } else {
                let cp = sub(p, c);
                let (d5, d6) = (dot(ab, cp), dot(ac, cp));
                if d6 >= 0.0 && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                        let t = d2 / (d2 - d6);
                        [a[0] + t * ac[0], a[1] + t * ac[1], a[2] + t * ac[2]]
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
                            let t = (d4 - d3) / ((d4 - d3) + (d5 - d6));
                            let bc = sub(c, b);
                            [b[0] + t * bc[0], b[1] + t * bc[1], b[2] + t * bc[2]]
                        } else {
                            let denominator = 1.0 / (va + vb + vc);
                            let (s, t) = (vb * denominator, vc * denominator);
                            [
                                a[0] + ab[0] * s + ac[0] * t,
                                a[1] + ab[1] * s + ac[1] * t,
                                a[2] + ab[2] * s + ac[2] * t,
                            ]
                        }
                    }
                }
            }
        }
    };
    let d = sub(p, closest);
    dot(d, d).sqrt() as f32
}

/// How far the vertices of `from` lie from the surface `onto` draws.
pub fn farthest_from_surface(from: &Primitive, onto: &Primitive) -> f32 {
    let triangles: Vec<[[f32; 3]; 3]> = onto
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [0, 1, 2].map(|k| onto.positions[t[k] as usize]))
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    from.positions
        .iter()
        .map(|p| {
            triangles
                .iter()
                .map(|t| distance_to_triangle(*p, t[0], t[1], t[2]))
                .fold(f32::MAX, f32::min)
        })
        .fold(0.0, f32::max)
}

/// A sheet like [`sheet`] with a texture seam down its middle: the column at `n / 2` is two vertices, and the texture jumps by a quarter across them.
pub fn sheet_with_a_seam(n: usize) -> Primitive {
    let mid = n / 2;
    let mut p = Primitive {
        lod: LodSlot::Level(0),
        uv_channels: vec![GrcFvfChannel::Tex0 as u8],
        uvs: vec![Vec::new()],
        ..Primitive::default()
    };
    // Per row: columns 0..=mid are the left side, then mid..=n again as the right side.
    let columns = n + 2;
    for y in 0..=n {
        for x in 0..=n + 1 {
            let (column, right) = if x <= mid { (x, false) } else { (x - 1, true) };
            let (u, v) = (column as f32 / n as f32, y as f32 / n as f32);
            p.positions.push([u, v, 0.0]);
            p.normals.push([0.0, 0.0, 1.0]);
            p.uvs[0].push([if right { u + 0.25 } else { u }, v]);
        }
    }
    let at = |x: usize, y: usize| (y * columns + x) as u32;
    for y in 0..n {
        for x in 0..n {
            // Cells left of the seam use the left copy of the column, cells right of it the right copy.
            let shift = usize::from(x >= mid);
            let (a, b, c, d) = (
                at(x + shift, y),
                at(x + 1 + shift, y),
                at(x + 1 + shift, y + 1),
                at(x + shift, y + 1),
            );
            p.indices.extend([a, b, c, a, c, d]);
        }
    }
    p
}

/// The edges of `p`, welded by position, that one triangle draws and that are not on the border of the unit square: a crack.
pub fn cracks(p: &Primitive) -> usize {
    use std::collections::HashMap;
    let key = |i: u32| p.positions[i as usize].map(f32::to_bits);
    let mut edges: HashMap<([u32; 3], [u32; 3]), usize> = HashMap::new();
    for t in p.indices.as_chunks::<3>().0 {
        for k in 0..3 {
            let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let on_border = |k: [u32; 3]| {
        let [x, y, _] = k.map(f32::from_bits);
        x == 0.0 || x == 1.0 || y == 0.0 || y == 1.0
    };
    edges
        .into_iter()
        .filter(|((a, b), count)| {
            *count == 1
                && !(on_border(*a) && on_border(*b) && {
                    let (a, b) = (a.map(f32::from_bits), b.map(f32::from_bits));
                    (a[0] == b[0] && (a[0] == 0.0 || a[0] == 1.0))
                        || (a[1] == b[1] && (a[1] == 0.0 || a[1] == 1.0))
                })
        })
        .count()
}

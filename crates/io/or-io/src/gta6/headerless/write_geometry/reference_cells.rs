//! Channel 33's 8-byte form at another vertex count where lane 0 clears bit 15: lane 0 names a
//! cell whose eight corners the kept vertices fit, and lanes 1 to 3 are the vertex's trilinear
//! parameters in it as `round(t · 65535)`. In a buffer holding both forms a new vertex takes the
//! form of the kept vertices it shares a triangle with.

use std::collections::BTreeMap;

use super::{reference, GeometryWriteError, Written};

/// A cell's corners, corner `i + 2j + 4l` at parameters `(i, j, l)`.
type Corners = [[f64; 3]; 8];
/// A kept vertex's lanes 1 to 3 and its position.
type Row = ([u16; 3], [f64; 3]);

const FULL: f64 = 65535.0;
/// How far past 0 and 1 a parameter may lie in a cell that holds the position.
const HOLD: f64 = 2.0 / FULL;
/// The fewest distinct lane triples a cell is fitted from.
const FIT: usize = 12;
/// The fewest held-out triples that judge a cell, and the share of them it must derive.
const JUDGED: usize = 20;
const PERCENT: usize = 97;
/// Metres: the largest residual a fitted cell keeps, how far outside a cell's corner box a
/// position may lie, and how near two parameter points count as one.
const RESIDUAL: f64 = 1e-4;
const MARGIN: f64 = 1e-5;
const NEAR: f64 = 2e-5;

fn refuse(why: &'static str) -> GeometryWriteError {
    GeometryWriteError::Channel { channel: 33, why }
}

/// Channel 33's refusal for a form whose lanes no rule derives.
pub(super) fn unknown_form() -> GeometryWriteError {
    refuse("what this form's lanes hold is not known, so a new vertex's are not derived")
}

/// A cell new vertices may take: its lane 0, corners and corner box.
struct Cell {
    lane0: u16,
    corners: Corners,
    low: [f64; 3],
    high: [f64; 3],
}

fn lanes(e: &[u8; 8]) -> [u16; 4] {
    std::array::from_fn(|k| u16::from_le_bytes([e[2 * k], e[2 * k + 1]]))
}

fn params(q: [u16; 3]) -> [f64; 3] {
    q.map(|x| f64::from(x) / FULL)
}

fn encode(t: [f64; 3]) -> [u16; 3] {
    t.map(|x| (x * FULL).round_ties_even().clamp(0.0, FULL) as u16)
}

fn basis([u, v, w]: [f64; 3]) -> [f64; 8] {
    let (a, b, c) = (1.0 - u, 1.0 - v, 1.0 - w);
    [
        a * b * c,
        u * b * c,
        a * v * c,
        u * v * c,
        a * b * w,
        u * b * w,
        a * v * w,
        u * v * w,
    ]
}

fn point(c: &Corners, t: [f64; 3]) -> [f64; 3] {
    let b = basis(t);
    std::array::from_fn(|a| b.iter().zip(c).map(|(bk, ck)| bk * ck[a]).sum())
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(&b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// `a x = r` by Gauss-Jordan elimination with partial pivoting; `None` at a pivot under 1e-10.
fn solve<const N: usize, const M: usize>(
    mut a: [[f64; N]; N],
    mut r: [[f64; M]; N],
) -> Option<[[f64; M]; N]> {
    for c in 0..N {
        let pivot = (c..N).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))?;
        if a[pivot][c].abs() < 1e-10 {
            return None;
        }
        a.swap(c, pivot);
        r.swap(c, pivot);
        let (row_a, row_r) = (a[c], r[c]);
        for (row, (ar, rr)) in a.iter_mut().zip(r.iter_mut()).enumerate() {
            if row != c && ar[c] != 0.0 {
                let f = ar[c] / row_a[c];
                for (x, y) in ar.iter_mut().zip(&row_a).skip(c) {
                    *x -= f * y;
                }
                for (x, y) in rr.iter_mut().zip(&row_r) {
                    *x -= f * y;
                }
            }
        }
    }
    Some(std::array::from_fn(|i| {
        std::array::from_fn(|k| r[i][k] / a[i][i])
    }))
}

/// The corners fitted by least squares to `rows` (lanes 1 to 3 and the position), and the largest
/// residual on an axis; `None` with too few rows or a vanishing pivot.
fn fit(rows: &[Row]) -> Option<(Corners, f64)> {
    if rows.len() < FIT {
        return None;
    }
    let (mut normal, mut right) = ([[0.0f64; 8]; 8], [[0.0f64; 3]; 8]);
    for (q, p) in rows {
        let b = basis(params(*q));
        for (j, bj) in b.iter().enumerate() {
            for (n, bk) in normal[j].iter_mut().zip(&b) {
                *n += bj * bk;
            }
            for (r, pa) in right[j].iter_mut().zip(p) {
                *r += bj * pa;
            }
        }
    }
    let corners = solve(normal, right)?;
    let residual = (rows.iter())
        .flat_map(|(q, p)| {
            let x = point(&corners, params(*q));
            (0..3).map(move |a| (x[a] - p[a]).abs())
        })
        .fold(0.0, f64::max);
    Some((corners, residual))
}

/// The parameters whose point in `c` is `p`, by Newton's method from the centre; unclamped.
fn invert(c: &Corners, p: [f64; 3]) -> Option<[f64; 3]> {
    let mut t = [0.5f64; 3];
    for _ in 0..12 {
        let x = point(c, t);
        let [u, v, w] = t;
        let (a, b, d) = (1.0 - u, 1.0 - v, 1.0 - w);
        let du = [-b * d, b * d, -v * d, v * d, -b * w, b * w, -v * w, v * w];
        let dv = [-a * d, -u * d, a * d, u * d, -a * w, -u * w, a * w, u * w];
        let dw = [-a * b, -u * b, -a * v, -u * v, a * b, u * b, a * v, u * v];
        let along = |dk: &[f64; 8], axis: usize| -> f64 {
            dk.iter().zip(c).map(|(s, ck)| s * ck[axis]).sum()
        };
        let jacobian: [[f64; 3]; 3] =
            std::array::from_fn(|axis| [along(&du, axis), along(&dv, axis), along(&dw, axis)]);
        let step = solve(jacobian, std::array::from_fn(|axis| [p[axis] - x[axis]]))?;
        t = std::array::from_fn(|k| t[k] + step[k][0]);
        if step.iter().all(|s| s[0].abs() < 1e-12) {
            break;
        }
        if t.iter().any(|x| x.abs() > 1e3) {
            return None;
        }
    }
    Some(t)
}

/// Whether `c` derives `stored` from `p`: within 2 units, or the two parameter points within
/// [`NEAR`] of each other.
fn within(c: &Corners, p: [f64; 3], stored: [u16; 3]) -> bool {
    let Some(t) = invert(c, p) else {
        return false;
    };
    let derived = encode(t);
    derived
        .iter()
        .zip(&stored)
        .all(|(d, s)| d.abs_diff(*s) <= 2)
        || distance(point(c, params(derived)), point(c, params(stored))) <= NEAR
}

/// The cells of the lane-0 values with bit 15 clear whose unclamped kept vertices fit eight
/// corners, and whose fit on the even-ranked distinct triples derives the odd-ranked ones.
fn fit_cells(read: &[[u16; 4]], held: &[[f32; 3]]) -> Vec<Cell> {
    let mut groups: BTreeMap<u16, BTreeMap<[u16; 3], [f64; 3]>> = BTreeMap::new();
    for (l, p) in read.iter().zip(held) {
        let q = [l[1], l[2], l[3]];
        if l[0] & 0x8000 == 0 && q.iter().all(|x| !matches!(*x, 0 | 0xFFFF)) {
            (groups.entry(l[0]).or_default())
                .entry(q)
                .or_insert(p.map(f64::from));
        }
    }
    (groups.into_iter())
        .filter_map(|(lane0, triples)| {
            let rows: Vec<Row> = triples.into_iter().collect();
            let (corners, residual) = fit(&rows)?;
            let even: Vec<Row> = rows.iter().step_by(2).copied().collect();
            let (half, half_residual) = fit(&even)?;
            let judged = rows.iter().skip(1).step_by(2);
            let count = judged.clone().count();
            let hits = judged.filter(|(q, p)| within(&half, *p, *q)).count();
            let derivable = residual <= RESIDUAL
                && half_residual <= RESIDUAL
                && count >= JUDGED
                && hits * 100 >= PERCENT * count;
            derivable.then(|| Cell {
                lane0,
                corners,
                low: std::array::from_fn(|a| {
                    corners.iter().map(|c| c[a]).fold(f64::INFINITY, f64::min) - MARGIN
                }),
                high: std::array::from_fn(|a| {
                    corners
                        .iter()
                        .map(|c| c[a])
                        .fold(f64::NEG_INFINITY, f64::max)
                        + MARGIN
                }),
            })
        })
        .collect()
}

/// The cells that hold `p`, each with `p`'s parameters in it.
fn holders(cells: &[Cell], p: [f64; 3]) -> Vec<(&Cell, [f64; 3])> {
    (cells.iter())
        .filter(|c| (0..3).all(|a| c.low[a] <= p[a] && p[a] <= c.high[a]))
        .filter_map(|c| Some((c, invert(&c.corners, p)?)))
        .filter(|(_, t)| t.iter().all(|x| (-HOLD..=1.0 + HOLD).contains(x)))
        .collect()
}

/// How far inside its cell a parameter triple lies.
fn inner(t: [f64; 3]) -> f64 {
    t.iter()
        .map(|x| x.min(1.0 - x))
        .fold(f64::INFINITY, f64::min)
}

/// Channel 33 of `positions` from the lanes `was` holds for the `held` positions: a vertex at its
/// held position keeps its lanes, a new one takes the form and lanes its kept neighbours give.
pub(super) fn channel33(
    held: &[[f32; 3]],
    positions: &[[f32; 3]],
    triangles: &[[u32; 3]],
    was: &[u8],
) -> Written<Vec<u8>> {
    let read: Vec<[u16; 4]> = was.as_chunks::<8>().0.iter().map(lanes).collect();
    if read.is_empty() || read.len() != held.len() {
        return Err(refuse("its lanes and its positions are not of one count"));
    }
    let set = |l: &[u16; 4]| l[0] & 0x8000 != 0;
    if read.iter().all(set) {
        return reference::reference(held, positions, was);
    }
    let mixed = read.iter().any(set);
    let kept = |i: usize| held.get(i).is_some_and(|h| positions.get(i) == Some(h));
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); positions.len()];
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (a as usize, b as usize);
            if a < next.len() && b < next.len() {
                next[a].push(b);
                next[b].push(a);
            }
        }
    }
    let mut out = Vec::with_capacity(8 * positions.len());
    let mut bit15: Vec<(usize, u16)> = Vec::new();
    let mut found: Option<Vec<Cell>> = None;
    for (i, p) in positions.iter().enumerate() {
        if kept(i) {
            out.extend(read[i].iter().flat_map(|x| x.to_le_bytes()));
            continue;
        }
        let near: Vec<[u16; 4]> = (next[i].iter())
            .filter(|j| kept(**j))
            .map(|j| read[*j])
            .collect();
        if mixed {
            let first = near.first().ok_or(refuse(
                "a new vertex shares a triangle with no kept vertex, so its form is not known",
            ))?;
            if near.iter().any(|l| set(l) != set(first)) {
                return Err(refuse(
                    "a new vertex shares triangles with kept vertices of both forms, so its form \
                     is not known",
                ));
            }
            if set(first) {
                if near.iter().any(|l| l[0] != first[0]) {
                    return Err(refuse(
                        "a new vertex's kept neighbours name more than one lane 0, so its own is \
                         not known",
                    ));
                }
                bit15.push((i, first[0]));
                out.extend([0u8; 8]);
                continue;
            }
        }
        let cells = found.get_or_insert_with(|| fit_cells(&read, held));
        let held_by = holders(cells, p.map(f64::from));
        let (cell, t) = (held_by.iter())
            .find(|(c, _)| near.iter().any(|l| l[0] == c.lane0))
            .or_else(|| (held_by.iter()).max_by(|a, b| inner(a.1).total_cmp(&inner(b.1))))
            .ok_or(refuse(
                "a new vertex lies outside every cell its kept lanes derive, and the clamped form \
                 is not derived",
            ))?;
        out.extend(cell.lane0.to_le_bytes());
        out.extend(encode(*t).iter().flat_map(|x| x.to_le_bytes()));
    }
    if !bit15.is_empty() {
        // The bit-15 vertices alone, under one lane 0, give the offset a new one takes.
        let keep: Vec<usize> = (0..read.len()).filter(|j| set(&read[*j])).collect();
        let sub_held: Vec<[f32; 3]> = keep.iter().map(|j| held[*j]).collect();
        let sub_was: Vec<u8> = (keep.iter())
            .flat_map(|j| [0x8000u16, read[*j][1], read[*j][2], read[*j][3]])
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut sub_positions = sub_held.clone();
        sub_positions.extend(bit15.iter().map(|(i, _)| positions[*i]));
        let derived = reference::reference(&sub_held, &sub_positions, &sub_was)?;
        let short = || refuse("its lanes and its positions are not of one count");
        for (k, (i, lane0)) in bit15.iter().enumerate() {
            let from = 8 * (keep.len() + k);
            let tail = derived.get(from + 2..from + 8).ok_or_else(short)?;
            let to = out.get_mut(8 * i..8 * i + 8).ok_or_else(short)?;
            to[..2].copy_from_slice(&lane0.to_le_bytes());
            to[2..].copy_from_slice(tail);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANE0: u16 = 0x0007;
    const SET: u16 = 0x8011;
    const OFFSET: [f32; 3] = [0.5, -0.25, 1.0];

    /// A skewed hexahedron, so the cell is not affine.
    fn corners() -> Corners {
        std::array::from_fn(|k| {
            let (i, j, l) = ((k & 1) as f64, ((k >> 1) & 1) as f64, ((k >> 2) & 1) as f64);
            [
                i + 0.1 * j + 0.03 * i * j * l,
                0.5 * j + 0.05 * l,
                0.25 * l + 0.02 * i + 0.01 * i * j,
            ]
        })
    }

    fn element(l: [u16; 4]) -> Vec<u8> {
        l.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn at(q: [u16; 3]) -> [f32; 3] {
        point(&corners(), params(q)).map(|x| x as f32)
    }

    /// 125 kept vertices of cell [`LANE0`] on a grid of parameters.
    fn grid() -> (Vec<[f32; 3]>, Vec<u8>) {
        let steps = [0.1, 0.3, 0.5, 0.7, 0.9].map(|t: f64| (t * FULL).round() as u16);
        let (mut held, mut was) = (Vec::new(), Vec::new());
        for a in steps {
            for b in steps {
                for c in steps {
                    held.push(at([a, b, c]));
                    was.extend(element([LANE0, a, b, c]));
                }
            }
        }
        (held, was)
    }

    fn reference_lanes(p: [f32; 3]) -> [u16; 4] {
        let q = |a: usize| (f64::from(p[a] + OFFSET[a]) * 8192.0 + 32768.0).round() as u16;
        [SET, q(0), q(1), q(2)]
    }

    #[test]
    fn a_new_vertex_takes_the_parameters_of_the_cell_that_holds_it() {
        let (held, was) = grid();
        let q = [20000, 40000, 30000];
        let mut positions = held.clone();
        positions.push(at(q));
        let n = held.len() as u32;
        let out = channel33(&held, &positions, &[[0, 1, n]], &was).expect("derived");
        assert_eq!(out[..was.len()], was[..]);
        assert_eq!(out[was.len()..], element([LANE0, q[0], q[1], q[2]])[..]);

        positions[held.len()] = [1.5, 0.25, 0.1];
        let refused = channel33(&held, &positions, &[[0, 1, n]], &was).expect_err("outside");
        assert!(
            refused.to_string().contains("outside every cell"),
            "{refused}"
        );

        // Eleven distinct triples fit no cell.
        let few = 11;
        positions = held[..few].to_vec();
        positions.push(at(q));
        let refused = channel33(
            &held[..few],
            &positions,
            &[[0, 1, few as u32]],
            &was[..8 * few],
        )
        .expect_err("no cell");
        assert!(
            refused.to_string().contains("outside every cell"),
            "{refused}"
        );
    }

    #[test]
    fn a_new_vertex_of_a_buffer_of_both_forms_takes_its_neighbours_form() {
        let (mut held, mut was) = grid();
        let first = held.len();
        let set = [
            [2.0, 0.0, 0.0],
            [2.5, 0.0, 0.0],
            [2.0, 0.5, 0.0],
            [2.5, 0.5, 0.25],
        ];
        for p in set {
            held.push(p);
            was.extend(element(reference_lanes(p)));
        }
        let new = [2.25, 0.25, 0.125];
        let mut positions = held.clone();
        positions.push(new);
        let n = held.len() as u32;
        let s = first as u32;
        let out = channel33(&held, &positions, &[[s, s + 1, n]], &was).expect("derived");
        assert_eq!(out[..was.len()], was[..]);
        assert_eq!(out[was.len()..], element(reference_lanes(new))[..]);

        let q = [20000, 40000, 30000];
        positions[held.len()] = at(q);
        let out = channel33(&held, &positions, &[[0, 1, n]], &was).expect("derived");
        assert_eq!(out[was.len()..], element([LANE0, q[0], q[1], q[2]])[..]);

        let refused = channel33(&held, &positions, &[[0, s, n]], &was).expect_err("both forms");
        assert!(refused.to_string().contains("both forms"), "{refused}");

        positions.push(new);
        let refused =
            channel33(&held, &positions, &[[n, n + 1, n]], &was).expect_err("no kept neighbour");
        assert!(refused.to_string().contains("no kept vertex"), "{refused}");

        let mut other = was.clone();
        let lane0 = 8 * (first + 1);
        other[lane0..lane0 + 2].copy_from_slice(&0x8012u16.to_le_bytes());
        positions.truncate(held.len());
        positions.push(new);
        let refused =
            channel33(&held, &positions, &[[s, s + 1, n]], &other).expect_err("two lane 0s");
        assert!(
            refused.to_string().contains("more than one lane 0"),
            "{refused}"
        );
    }

    #[test]
    fn newton_inverts_the_trilinear_point() {
        let c = corners();
        let t = [0.2, 0.7, 0.4];
        let back = invert(&c, point(&c, t)).expect("converges");
        assert!(
            back.iter().zip(&t).all(|(a, b)| (a - b).abs() < 1e-9),
            "{back:?}"
        );
    }
}

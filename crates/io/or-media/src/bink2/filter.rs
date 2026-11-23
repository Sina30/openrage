//! The in-loop filter of a Bink 2 picture.
//!
//! Origin: derived from RAD Game Tools' proprietary `bink2w64.dll`, version 2020.07, the file the
//! Enhanced install ships, by reading its disassembly and decompilation for interoperability (owner
//! decision of 2026-10-06, docs/direction/Project_Context.md; listed in LICENSE-third-party.md). The addresses are
//! virtual, from the image base 0x180000000:
//! - the pass after each 32×32 block, and the horizontal edges it defers: 0x18001d990, 0x18001d660;
//! - a horizontal edge of 16 columns, a vertical edge of 8 rows and the masks of the samples they move:
//!   0x18000d690, 0x18000d8a0, the table at 0x180041200;
//! - the 8×8 pixels of a block that codes no coefficient: 0x18000c7a0, 0x18000cc40, 0x18000d0e0;
//! - the residue of an inter block drawn across its own edges: 0x180014870, 0x180014910, 0x18000e0f0.
//!
//! The Legacy install's 1.994a DLL gives the same pictures.

use super::dsp::Mv;

/// Which of the four samples across an edge move, for the code `n` of a pair of blocks: bits 0 and 1
/// the side before the edge (none, the nearest, the two nearest), bits 2 and 3 the side after it.
pub(super) fn moves(n: u8) -> [bool; 4] {
    [n & 2 != 0, n & 3 != 0, n & 12 != 0, n & 8 != 0]
}

/// The four samples `p` across an edge, between the second and the third, drawn together as far as
/// `moves` lets them.
pub(super) fn smooth(p: [u8; 4], moves: [bool; 4]) -> [u8; 4] {
    let d = i16::from(p[2]) - i16::from(p[1]);
    let near = (4 * d + 8) >> 4;
    let far = (2 * d + 8) >> 4;
    let clip = |v: i16| v.clamp(0, 255) as u8;
    let pick = |i: usize, delta: i16, sign: i16| {
        if moves[i] {
            clip(i16::from(p[i]) + sign * delta)
        } else {
            p[i]
        }
    };
    [
        pick(0, far, 1),
        pick(1, near, 1),
        pick(2, near, -1),
        pick(3, far, -1),
    ]
}

/// The high half of the signed product of `x` and the 16-bit constant `k`.
fn mulhi(x: i16, k: i16) -> i16 {
    ((i32::from(x) * i32::from(k)) >> 16) as i16
}

const K0: i16 = 0x6800;
const K1: i16 = 0x1C00;
const K2: i16 = 0x3000;
const K3: i16 = 0x4800;

/// A line of eight values that leans from `before` to `after`, the two deltas against the line's own value.
fn lean(before: i16, after: i16) -> [i16; 8] {
    [
        mulhi(before, K0),
        mulhi(before, K3).wrapping_add(after >> 7),
        mulhi(before, K2).wrapping_add(after >> 5),
        mulhi(before, K1).wrapping_add(after >> 4),
        mulhi(after, K1).wrapping_add(before >> 4),
        mulhi(after, K2).wrapping_add(before >> 5),
        mulhi(after, K3).wrapping_add(before >> 7),
        mulhi(after, K0),
    ]
}

/// The values of the nine blocks around one, as the records of the blocks hold them.
#[derive(Clone, Copy)]
pub(super) struct Around {
    pub centre: u16,
    pub left: u16,
    pub right: u16,
    pub up: u16,
    pub down: u16,
    pub up_left: u16,
    pub up_right: u16,
    pub down_left: u16,
    pub down_right: u16,
}

/// The 8×8 pixels of a block that codes no coefficient, leaning toward the values of the blocks around
/// it that lie within `threshold` of it.
pub(super) fn fill(a: &Around, threshold: i16) -> [[u8; 8]; 8] {
    let dc = |v: u16| (v & 0x1FFF) as i16;
    let near = |v: u16| {
        let (v, c) = (v as i16, a.centre as i16);
        v.wrapping_sub(c).max(c.wrapping_sub(v)) < threshold
    };
    let pick = |v: u16, otherwise: i16| if near(v) { dc(v) } else { otherwise };
    let c = dc(a.centre);
    let (l, r, u, d) = (
        pick(a.left, c),
        pick(a.right, c),
        pick(a.up, c),
        pick(a.down, c),
    );
    let (ul, ur, dl, dr) = (
        pick(a.up_left, l),
        pick(a.up_right, r),
        pick(a.down_left, l),
        pick(a.down_right, r),
    );
    let profile = |base: i16, before: i16, after: i16| {
        let line = lean(before.wrapping_sub(base), after.wrapping_sub(base));
        line.map(|v| base.wrapping_add(4).wrapping_add(v))
    };
    let top = profile(u, ul, ur);
    let middle = profile(c, l, r);
    let bottom = profile(d, dl, dr);
    let mut out = [[0u8; 8]; 8];
    for x in 0..8 {
        let to_top = top[x].wrapping_sub(middle[x]);
        let to_bottom = bottom[x].wrapping_sub(middle[x]);
        let column = lean(to_top, to_bottom);
        for (y, row) in out.iter_mut().enumerate() {
            row[x] = (middle[x].wrapping_add(column[y]) >> 3).clamp(0, 255) as u8;
        }
    }
    out
}

/// The record of each 8×8 block of a picture: bit 15 set for a block coded intra, bits 14 and 13 how
/// few coefficients it codes (3 none, 2 one to three, 1 four to seven, 0 more), the low 13 bits its DC.
/// A block predicted from the picture before it has no record.
pub(super) struct Records {
    /// Luma blocks across and down: four for each 32×32 block.
    pub wide: usize,
    pub high: usize,
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
}

fn record(dc: i32, count: u8) -> u16 {
    let few = u16::from(count < 4) + u16::from(count < 8) + u16::from(count == 0);
    0x8000 | few << 13 | (dc as u16 & 0x1FFF)
}

impl Records {
    pub(super) fn new(blocks_wide: usize, blocks_high: usize) -> Self {
        let (wide, high) = (blocks_wide * 4, blocks_high * 4);
        Self {
            wide,
            high,
            y: vec![0; wide * high],
            u: vec![0; wide * high / 4],
            v: vec![0; wide * high / 4],
        }
    }

    /// Notes the blocks of the intra 32×32 block at (`bx`, `by`).
    pub(super) fn set_intra(
        &mut self,
        (bx, by): (usize, usize),
        luma: (&[i32; 16], &[[u8; 4]; 4]),
        u: (&[i32; 4], &[u8; 4]),
        v: (&[i32; 4], &[u8; 4]),
    ) {
        for quad in 0..4 {
            for block in 0..4 {
                let x = bx * 4 + (quad & 1) * 2 + (block & 1);
                let y = by * 4 + (quad >> 1) * 2 + (block >> 1);
                self.y[y * self.wide + x] = record(luma.0[quad * 4 + block], luma.1[quad][block]);
            }
        }
        let wide = self.wide / 2;
        for block in 0..4 {
            let at = (by * 2 + (block >> 1)) * wide + bx * 2 + (block & 1);
            self.u[at] = record(u.0[block], u.1[block]);
            self.v[at] = record(v.0[block], v.1[block]);
        }
    }
}

/// The record of an inter block's residue: how few coefficients it codes, and its DC, with no intra bit.
pub(super) fn inter_record(dc: i32, count: u8) -> u16 {
    record(dc, count) & 0x7FFF
}

/// A plane of the picture being filtered.
pub(super) struct Plane<'a> {
    pub data: &'a mut [u8],
    pub stride: usize,
}

/// The code of the two blocks either side of an edge, none when neither is to move.
fn code(before: u16, after: u16) -> u8 {
    let n = ((before >> 13) & 3) | (((after >> 13) & 3) << 2);
    if n == 15 {
        0
    } else {
        n as u8
    }
}

/// The records of one plane in rows of `wide` blocks of 8×8, of which `rows` lie on the picture; a luma
/// block has one vector to each 16×16 square, a chroma block one of its own.
struct Layer<'a> {
    grid: &'a [u16],
    wide: usize,
    high: usize,
    rows: usize,
    chroma: bool,
    /// The vectors of the picture, a row of `squares` to each row of 16×16 squares.
    mvs: &'a [Mv],
    squares: usize,
}

impl Layer<'_> {
    /// The record of the block at (`x`, `y`), none beyond the picture.
    fn at(&self, x: isize, y: isize) -> u16 {
        if x < 0 || y < 0 || x as usize >= self.wide || y as usize >= self.high {
            0
        } else {
            self.grid[y as usize * self.wide + x as usize]
        }
    }

    fn mv(&self, x: usize, y: usize) -> Mv {
        let per = if self.chroma { 1 } else { 2 };
        self.mvs[y / per * self.squares + x / per]
    }

    /// The code of the edge between the blocks `before` and `after`, which lies on a square's own edge
    /// when `on_square`: a block predicted from the picture before it is drawn across only where
    /// its vector and its neighbour's differ by more than a pixel.
    fn edge(&self, before: (usize, usize), after: (usize, usize), on_square: bool) -> u8 {
        let (a, b) = (
            self.grid[before.1 * self.wide + before.0],
            self.grid[after.1 * self.wide + after.0],
        );
        if (a | b) & 0x8000 != 0 {
            return code(a, b);
        }
        let (p, q) = (self.mv(before.0, before.1), self.mv(after.0, after.1));
        let apart = (i32::from(p.x) - i32::from(q.x)).abs() > 1
            || (i32::from(p.y) - i32::from(q.y)).abs() > 1;
        if on_square && apart {
            10
        } else {
            0
        }
    }

    /// Draws the pixels of every block that codes no coefficient toward the blocks around it.
    fn fills(&self, plane: &mut Plane, threshold: i16) {
        for by in 0..self.rows {
            for bx in 0..self.wide {
                let centre = self.grid[by * self.wide + bx];
                if centre < 0xE000 {
                    continue;
                }
                let (x, y) = (bx as isize, by as isize);
                let around = Around {
                    centre,
                    left: self.at(x - 1, y),
                    right: self.at(x + 1, y),
                    up: self.at(x, y - 1),
                    down: self.at(x, y + 1),
                    up_left: self.at(x - 1, y - 1),
                    up_right: self.at(x + 1, y - 1),
                    down_left: self.at(x - 1, y + 1),
                    down_right: self.at(x + 1, y + 1),
                };
                for (r, line) in fill(&around, threshold).iter().enumerate() {
                    let to = (by * 8 + r) * plane.stride + bx * 8;
                    plane.data[to..to + 8].copy_from_slice(line);
                }
            }
        }
    }

    /// Draws every edge between a block and the one to its left, but those `skipped` names by the
    /// column of 4×4 blocks they lie on.
    fn vertical_edges(&self, plane: &mut Plane, skipped: impl Fn(usize) -> bool) {
        let step = if self.chroma { 2 } else { 1 };
        for by in 0..self.rows {
            for bx in 1..self.wide {
                if skipped(bx * step) {
                    continue;
                }
                let n = self.edge((bx - 1, by), (bx, by), self.chroma || bx % 2 == 0);
                if n == 0 {
                    continue;
                }
                for r in 0..8 {
                    let i = (by * 8 + r) * plane.stride + bx * 8;
                    let at = |k: usize| plane.data[i - 2 + k];
                    let s = smooth([at(0), at(1), at(2), at(3)], moves(n));
                    plane.data[i - 2..i + 2].copy_from_slice(&s);
                }
            }
        }
    }

    /// Draws every edge between a block and the one above it, but those `skipped` names by the row of
    /// 4×4 blocks they lie on.
    fn horizontal_edges(&self, plane: &mut Plane, skipped: impl Fn(usize) -> bool) {
        let step = if self.chroma { 2 } else { 1 };
        let stride = plane.stride;
        for by in 1..self.rows {
            if skipped(by * step) {
                continue;
            }
            for bx in 0..self.wide {
                let n = self.edge((bx, by - 1), (bx, by), self.chroma || by % 2 == 0);
                if n == 0 {
                    continue;
                }
                for c in 0..8 {
                    let i = (by * 8 - 2) * stride + bx * 8 + c;
                    let at = |k: usize| plane.data[i + k * stride];
                    let s = smooth([at(0), at(1), at(2), at(3)], moves(n));
                    for (k, v) in s.iter().enumerate() {
                        plane.data[i + k * stride] = *v;
                    }
                }
            }
        }
    }
}

/// Filters a picture in place: first the blocks that code nothing, then the edges between blocks, the
/// vertical before the horizontal. `mvs` holds the vector of each 16×16 square, and the flags of the
/// frame and of its rows and columns say which of the passes and the edges the frame has.
pub(super) fn apply(
    planes: [Plane; 3],
    rec: &Records,
    mvs: &[Mv],
    frame_flags: u32,
    row_flags: &[bool],
    col_flags: &[bool],
    height: usize,
) {
    let threshold = ((frame_flags & 0xFF) << 3) as i16;
    let rows = height.div_ceil(16) * 2;
    let squares = rec.wide / 2;
    let [py, pu, pv] = planes;
    let sets = [(py, &rec.y, false), (pu, &rec.u, true), (pv, &rec.v, true)];
    for (mut plane, grid, chroma) in sets {
        let layer = Layer {
            grid,
            wide: if chroma { rec.wide / 2 } else { rec.wide },
            high: if chroma { rec.high / 2 } else { rec.high },
            rows: if chroma { rows / 2 } else { rows },
            chroma,
            mvs,
            squares,
        };
        if threshold != 0 {
            layer.fills(&mut plane, threshold);
        }
        if frame_flags & 0x4000 == 0 {
            // A picture two blocks of 32 across never draws the left edge of the second.
            layer.vertical_edges(&mut plane, |at| {
                col_flags.get(at).copied().unwrap_or(false) || (rec.wide == 8 && at == 4)
            });
        }
        if frame_flags & 0x8000 == 0 {
            layer.horizontal_edges(&mut plane, |at| row_flags.get(at).copied().unwrap_or(false));
        }
    }
}

/// The residue of a square of blocks, `size` samples across, smoothed in place across the edges between
/// its 8×8 blocks, which `records` describe in rows of `size / 8`: a block that codes few coefficients
/// is drawn toward the block beside it, and two that code none only where their DCs differ by much.
pub(super) fn smooth_residue(
    res: &mut [i16],
    size: usize,
    records: &[u16],
    vertical: bool,
    horizontal: bool,
) {
    let blocks = size / 8;
    let across = |a: u16, b: u16| -> Option<[bool; 4]> {
        if (a | b) <= 0x1FFF {
            return None;
        }
        if a & b >= 0x6000 {
            let steps = u32::from(a ^ 0x1000).wrapping_sub(u32::from(b ^ 0x1000));
            if steps.wrapping_add(0x20) <= 0x40 {
                return None;
            }
        }
        Some(moves((((a >> 13) & 3) | (((b >> 13) & 3) << 2)) as u8))
    };
    let draw = |p: [i16; 4], moves: [bool; 4]| -> [i16; 4] {
        let d = p[2].wrapping_sub(p[1]);
        let near = d.wrapping_add(2) >> 2;
        let far = d.wrapping_add(4) >> 3;
        [
            if moves[0] {
                p[0].wrapping_add(far)
            } else {
                p[0]
            },
            if moves[1] {
                p[1].wrapping_add(near)
            } else {
                p[1]
            },
            if moves[2] {
                p[2].wrapping_sub(near)
            } else {
                p[2]
            },
            if moves[3] {
                p[3].wrapping_sub(far)
            } else {
                p[3]
            },
        ]
    };
    if vertical {
        for by in 0..blocks {
            for bx in 1..blocks {
                let Some(moves) = across(records[by * blocks + bx - 1], records[by * blocks + bx])
                else {
                    continue;
                };
                for r in 0..8 {
                    let i = (by * 8 + r) * size + bx * 8;
                    let s = draw([res[i - 2], res[i - 1], res[i], res[i + 1]], moves);
                    res[i - 2..i + 2].copy_from_slice(&s);
                }
            }
        }
    }
    if horizontal {
        for by in 1..blocks {
            for bx in 0..blocks {
                let Some(moves) =
                    across(records[(by - 1) * blocks + bx], records[by * blocks + bx])
                else {
                    continue;
                };
                for c in 0..8 {
                    let i = (by * 8 - 2) * size + bx * 8 + c;
                    let s = draw(
                        [res[i], res[i + size], res[i + 2 * size], res[i + 3 * size]],
                        moves,
                    );
                    for (k, v) in s.iter().enumerate() {
                        res[i + k * size] = *v;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of `blocks` 32×32 blocks across and one down, of flat luma `left` before its block `at` and
    /// `right` from it on, whose blocks hold `records` (luma only; chroma is left as predicted).
    fn picture(blocks: usize, at: usize, left: u8, right: u8) -> (Vec<u8>, Records) {
        let mut luma = vec![left; blocks * 32 * 32];
        for row in luma.chunks_mut(blocks * 32) {
            row[at * 32..].fill(right);
        }
        (luma, Records::new(blocks, 1))
    }

    fn filter(luma: &mut [u8], rec: &Records, mvs: &[Mv], flags: u32) {
        let blocks = rec.wide / 4;
        let (mut u, mut v) = (vec![0u8; blocks * 16 * 16], vec![0u8; blocks * 16 * 16]);
        let planes = [
            Plane {
                data: luma,
                stride: blocks * 32,
            },
            Plane {
                data: &mut u,
                stride: blocks * 16,
            },
            Plane {
                data: &mut v,
                stride: blocks * 16,
            },
        ];
        let none = vec![false; rec.wide];
        apply(planes, rec, mvs, flags, &none, &none, 32);
    }

    #[test]
    fn the_samples_an_edge_moves_follow_the_code_of_its_two_sides() {
        assert_eq!(moves(0), [false; 4]);
        assert_eq!(
            moves(1),
            [false, true, false, false],
            "the nearest sample before the edge"
        );
        assert_eq!(
            moves(3),
            [true, true, false, false],
            "the two nearest before it"
        );
        assert_eq!(
            moves(4),
            [false, false, true, false],
            "the nearest after it"
        );
        assert_eq!(
            moves(12),
            [false, false, true, true],
            "the two nearest after it"
        );
        assert_eq!(moves(10), [true, true, true, true]);
    }

    #[test]
    fn an_edge_draws_its_samples_together_by_a_quarter_and_an_eighth_of_the_step_and_stays_a_byte()
    {
        // A step of 64: the nearest pair moves 16 and the pair beyond them 8, as far as the codes let them.
        assert_eq!(smooth([0, 0, 64, 64], moves(15)), [8, 16, 48, 56]);
        assert_eq!(smooth([0, 0, 64, 64], moves(3)), [8, 16, 64, 64]);
        assert_eq!(
            smooth([10, 10, 10, 10], moves(15)),
            [10; 4],
            "no step, no change"
        );
        assert_eq!(
            smooth([255, 255, 0, 0], [true; 4]),
            [223, 191, 64, 32],
            "a step down moves the same way"
        );
        assert_eq!(smooth([0, 0, 255, 255], [true; 4]), [32, 64, 191, 223]);
        assert_eq!(
            smooth([255, 100, 200, 0], [true; 4]),
            [255, 125, 175, 0],
            "the ends stop at a byte"
        );
    }

    #[test]
    fn a_block_that_codes_nothing_among_its_likes_is_flat_at_its_dc_over_eight() {
        let flat = 0xE000 | 616;
        let around = Around {
            centre: flat,
            left: flat,
            right: flat,
            up: flat,
            down: flat,
            up_left: flat,
            up_right: flat,
            down_left: flat,
            down_right: flat,
        };
        assert!(fill(&around, 8).iter().flatten().all(|&v| v == 77));
    }

    #[test]
    fn a_neighbour_beyond_the_threshold_is_not_leaned_toward() {
        let (centre, far) = (0xE000 | 616, 0xE000 | 1600);
        let around = Around {
            centre,
            left: far,
            right: far,
            up: far,
            down: far,
            up_left: far,
            up_right: far,
            down_left: far,
            down_right: far,
        };
        let alone = fill(&around, 8);
        assert!(alone.iter().flatten().all(|&v| v == 77), "{alone:?}");
        let leaning = fill(&around, 8 * 255);
        assert!(
            leaning[0][0] > 77,
            "within the threshold its side leans: {leaning:?}"
        );
    }

    #[test]
    fn a_record_counts_the_coefficients_into_two_bits_and_keeps_thirteen_of_the_dc() {
        let few = |count| record(5, count) >> 13 & 3;
        assert_eq!(
            [few(0), few(1), few(3), few(4), few(7), few(8), few(60)],
            [3, 2, 2, 1, 1, 0, 0]
        );
        assert_eq!(
            record(0x2005, 0) & 0x1FFF,
            5,
            "the DC is cut to thirteen bits"
        );
        assert_eq!(
            record(1, 9) & 0x8000,
            0x8000,
            "a record states an intra block"
        );
        assert_eq!(inter_record(1, 9) & 0x8000, 0);
    }

    #[test]
    fn an_edge_between_a_block_that_codes_nothing_and_one_that_codes_much_moves_the_first_alone() {
        // Three blocks across: the first holds blocks that code nothing, the others many coefficients.
        let (mut luma, mut rec) = picture(3, 1, 100, 140);
        for (i, v) in rec.y.iter_mut().enumerate() {
            let few = if (i % 12) < 4 { 0 } else { 8 };
            *v = record(0, few);
        }
        let mvs = [Mv::default(); 6 * 2];
        filter(&mut luma, &rec, &mvs, 0x2000);
        let row = &luma[..96];
        assert_eq!(
            &row[28..36],
            [100, 100, 105, 110, 140, 140, 140, 140],
            "{row:?}"
        );
        assert!(row[..28].iter().all(|&v| v == 100) && row[36..].iter().all(|&v| v == 140));
    }

    #[test]
    fn two_blocks_that_code_nothing_are_not_drawn_together() {
        let (mut luma, mut rec) = picture(3, 1, 100, 140);
        rec.y.fill(record(0, 0));
        let before = luma.clone();
        filter(&mut luma, &rec, &[Mv::default(); 12], 0x2000);
        assert_eq!(luma, before);
    }

    #[test]
    fn the_flags_of_a_frame_turn_its_vertical_edges_off() {
        let (mut luma, mut rec) = picture(3, 1, 100, 140);
        for (i, v) in rec.y.iter_mut().enumerate() {
            *v = record(0, if (i % 12) < 4 { 0 } else { 8 });
        }
        let before = luma.clone();
        filter(&mut luma, &rec, &[Mv::default(); 12], 0x6000);
        assert_eq!(luma, before);
    }

    #[test]
    fn a_picture_two_blocks_across_never_draws_the_left_edge_of_the_second() {
        let (mut luma, mut rec) = picture(2, 1, 100, 140);
        for (i, v) in rec.y.iter_mut().enumerate() {
            *v = record(0, if (i % 8) < 4 { 0 } else { 8 });
        }
        let before = luma.clone();
        filter(&mut luma, &rec, &[Mv::default(); 8], 0x2000);
        assert_eq!(luma, before);
    }

    #[test]
    fn blocks_predicted_from_squares_that_moved_apart_by_more_than_a_pixel_are_drawn_across_their_edge(
    ) {
        let (mut luma, rec) = picture(3, 0, 100, 100);
        // Flat 100 left of the square boundary at 16 and 140 from it on, as two squares that moved apart.
        for row in luma.chunks_mut(96) {
            row[16..].fill(140);
        }
        let mut mvs = [Mv::default(); 12];
        mvs[1] = Mv { x: 2, y: 0 };
        let mut apart = luma.clone();
        filter(&mut apart, &rec, &mvs, 0x2000);
        assert_eq!(
            &apart[12..20],
            [100, 100, 105, 110, 130, 135, 140, 140],
            "{:?}",
            &apart[..32]
        );
        mvs[1] = Mv { x: 1, y: 1 };
        let mut together = luma.clone();
        filter(&mut together, &rec, &mvs, 0x2000);
        assert_eq!(together, luma, "a pixel apart is not enough");
    }

    #[test]
    fn residue_beside_a_block_that_codes_little_is_drawn_across_the_edge_only_where_dcs_differ() {
        let mut flat = [0i16; 256];
        for row in flat.chunks_mut(16) {
            row[8..].fill(40);
        }
        let near = [inter_record(0, 0), inter_record(8, 0), 0, 0];
        let far = [inter_record(0, 0), inter_record(0x100, 0), 0, 0];
        let mut a = flat;
        smooth_residue(&mut a, 16, &near, true, true);
        assert_eq!(a, flat, "the DCs are within a step of each other");
        let mut b = flat;
        smooth_residue(&mut b, 16, &far, true, false);
        assert_eq!(&b[4..12], [0, 0, 5, 10, 30, 35, 40, 40], "{:?}", &b[..16]);
        let mut c = flat;
        smooth_residue(&mut c, 16, &far, false, false);
        assert_eq!(c, flat, "the frame's flags can turn it off");
    }
}

use std::cell::Cell;

use super::*;
use crate::decl::{Aligned, Image};

const VIRTUAL: u64 = 0x5000_0000;
const LEN: usize = 0x700;
const TETRAHEDRA: usize = 0x540;
/// The weight lists of the type-5 database's group.
const LISTS: u64 = 5;
/// Two tetrahedra sharing three positions, one list at two positions and twice in the second.
const TETRAHEDRA_READ: [Tetrahedron; 2] = [
    Tetrahedron {
        lists: [0, 1, 2, 3],
        vertices: [
            [0.0, 0.25, 0.0],
            [1.0, 0.25, 0.0],
            [0.0, 0.75, 0.0],
            [0.0, 0.25, 1.0],
        ],
    },
    Tetrahedron {
        lists: [1, 2, 3, 1],
        vertices: [
            [1.0, 0.25, 0.0],
            [0.0, 0.75, 0.0],
            [0.0, 0.25, 1.0],
            [1.0, 0.75, 1.0],
        ],
    },
];
const FIVE: usize = 0x40;
const EIGHT: usize = 0x100;
const FIVE_COLUMNS: usize = 0x200;
const EIGHT_COLUMNS: usize = 0x220;
const TABLE_HEADS: usize = 0x260;
const GRIDS: usize = 0x280;
const SIMPLICES: usize = 0x2D0;
const FIVE_ROWS: usize = 0x300;
const EIGHT_ROWS: usize = 0x4C0;
const BOUNDS: [[f32; 2]; 3] = [[-1.5, 2.5], [0.25, 0.75], [-4.0, 4.0]];
const SIZES: [f32; 3] = [1.0, 0.5, 2.0];
const RANGES: [[f32; 2]; 5] = [
    [-4.0, 6.0],
    [-2.0, 0.5],
    [0.25, 3.0],
    [1.0, 9.0],
    [-0.75, 0.125],
];

struct Body(Vec<u8>);

impl Body {
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn f32(&mut self, at: usize, v: f32) {
        self.put(at, &v.to_le_bytes());
    }
    /// The array `{pointer, u16 count, u16 capacity, u32}` at `at` of `count` elements at `to`.
    fn array(&mut self, at: usize, to: usize, count: u16) {
        self.put(at, &(VIRTUAL + to as u64).to_le_bytes());
        self.u16(at + 8, count);
        self.u16(at + 10, count);
    }
    fn columns(&mut self, at: usize, to: usize, count: u16) {
        self.array(at, to, count);
        for c in 0..usize::from(count) {
            self.u32(to + 8 * c, 0xC011_0000 + c as u32);
            self.u16(to + 8 * c + 4, c as u16);
            self.u16(to + 8 * c + 6, 2 - (c % 3) as u16);
        }
    }
    fn rows(&mut self, at: usize, to: usize, count: u16) -> Vec<[u16; 16]> {
        self.array(at, to, count);
        let rows: Vec<[u16; 16]> = (0..count)
            .map(|r| std::array::from_fn(|h| 0x3C00 + r * 16 + h as u16))
            .collect();
        for (r, row) in rows.iter().enumerate() {
            for (h, half) in row.iter().enumerate() {
                self.u16(to + 32 * r + 2 * h, *half);
            }
        }
        rows
    }
    fn grid(&mut self, at: usize, words: [u32; 2]) -> Grid {
        let mut grid = Grid {
            words,
            cells: [[0; 2]; 3],
            floats: [0.0; 12],
        };
        for (d, (bounds, size)) in BOUNDS.iter().zip(SIZES).enumerate() {
            for (k, bound) in bounds.iter().enumerate() {
                grid.floats[2 * d + k] = *bound;
                grid.cells[d][k] = (bound / size).floor() as i32;
            }
            grid.floats[6 + d] = size;
            grid.floats[9 + d] = 1.0 / size;
        }
        self.u32(at, words[0]);
        self.u32(at + 4, words[1]);
        for (i, cell) in grid.cells.as_flattened().iter().enumerate() {
            self.put(at + 8 + 4 * i, &cell.to_le_bytes());
        }
        for (i, float) in grid.floats.iter().enumerate() {
            self.f32(at + 0x20 + 4 * i, *float);
        }
        grid
    }
}

/// A type-5 database of a grid of 6 cuts in 3 bands, and a type-8 one of one table over a grid
/// of a single band and two simplices.
fn body() -> (Body, TableDatabase, SimplexDatabase) {
    let mut b = Body(vec![0; LEN]);
    b.u32(FIVE + 8, 5);
    b.columns(FIVE + 0x10, FIVE_COLUMNS, 3);
    let five_grid = b.grid(FIVE + 0x40, [6, 3]);
    let five_rows = b.rows(FIVE + 0x30, FIVE_ROWS, 2 * 6 + 2);
    b.array(FIVE + 0x20, TETRAHEDRA, TETRAHEDRA_READ.len() as u16);
    for (t, tetrahedron) in TETRAHEDRA_READ.iter().enumerate() {
        let at = TETRAHEDRA + 112 * t;
        let (lists, vertices) = (tetrahedron.lists, tetrahedron.vertices);
        for (k, (list, vertex)) in lists.iter().zip(&vertices).enumerate() {
            b.u16(at + 2 * k, *list);
            for (d, x) in vertex.iter().enumerate() {
                b.f32(at + 0x10 + 16 * k + 4 * d, *x);
            }
        }
    }
    b.u32(EIGHT + 8, 8);
    b.columns(EIGHT + 0x10, EIGHT_COLUMNS, 5);
    b.array(EIGHT + 0x20, TABLE_HEADS, 1);
    let eight_rows = b.rows(TABLE_HEADS, EIGHT_ROWS, 4);
    b.array(EIGHT + 0x30, GRIDS, 1);
    let eight_grid = b.grid(GRIDS, [1, 1]);
    b.array(EIGHT + 0x40, SIMPLICES, 2);
    let simplices: Vec<[u16; 12]> = (0..2u16)
        .map(|s| std::array::from_fn(|w| 0x100 * (s + 1) + w as u16))
        .collect();
    for (s, corners) in simplices.iter().enumerate() {
        for (w, word) in corners.iter().enumerate() {
            b.u16(SIMPLICES + 24 * s + 2 * w, *word);
        }
    }
    let corner_grid = b.grid(EIGHT + 0x50, [2, 2]);
    b.u32(EIGHT + 0xA0, 7);
    for (i, [lo, hi]) in RANGES.into_iter().enumerate() {
        b.f32(EIGHT + 0xA4 + 8 * i, lo);
        b.f32(EIGHT + 0xA8 + 8 * i, hi);
    }
    let columns = |n: u32| -> Vec<Column> {
        (0..n)
            .map(|c| (0xC011_0000 + c, [c as u16, 2 - (c % 3) as u16]))
            .collect()
    };
    let five = TableDatabase {
        columns: columns(3),
        tetrahedra: TETRAHEDRA_READ.to_vec(),
        rows: five_rows,
        grid: five_grid,
    };
    let eight = SimplexDatabase {
        columns: columns(5),
        tables: vec![eight_rows],
        grids: vec![eight_grid],
        simplices,
        grid: corner_grid,
        lists: 7,
        ranges: RANGES,
    };
    (b, five, eight)
}

fn read(
    b: &Body,
) -> (
    Result<TableDatabase, String>,
    Result<SimplexDatabase, String>,
) {
    let aligned = Aligned::from_bytes(&b.0);
    let s = Section {
        image: Image::new(aligned.bytes(), b.0.len()),
        left: Cell::new(b.0.len() * 2),
    };
    (s.table_database(FIVE, LISTS), s.simplex_database(EIGHT))
}

#[test]
fn databases_of_types_5_and_8_read_their_columns_tables_grids_and_simplices() {
    let (b, five, eight) = body();
    let (read_five, read_eight) = read(&b);
    assert_eq!(read_five, Ok(five));
    assert_eq!(read_eight, Ok(eight));
}

#[test]
fn a_database_whose_rows_grid_simplices_or_ranges_break_their_rules_is_refused() {
    let (mut rows, ..) = body();
    rows.u16(FIVE + 0x30 + 8, 2 * 6 + 1);
    let (mut band, ..) = body();
    band.u32(FIVE + 0x40 + 4, 5);
    let (mut cell, ..) = body();
    cell.put(FIVE + 0x40 + 8, &(-1i32).to_le_bytes());
    let (mut inverse, ..) = body();
    inverse.f32(FIVE + 0x40 + 0x44, 0.9);
    for b in [rows, band, cell, inverse] {
        assert!(read(&b).0.is_err());
    }
    let (mut corners, ..) = body();
    corners.u32(EIGHT + 0x50, 3);
    let (mut falling, ..) = body();
    falling.f32(EIGHT + 0xA8, -6.0);
    let (mut tail, ..) = body();
    tail.u32(EIGHT + 0xCC, 1);
    let (mut columns, ..) = body();
    columns.u16(EIGHT + 0x10 + 8, 4);
    for b in [corners, falling, tail, columns] {
        assert!(read(&b).1.is_err());
    }
}

#[test]
fn a_tetrahedron_off_its_lists_grid_volume_or_zeros_is_refused() {
    let second = TETRAHEDRA + 112;
    let (mut w, ..) = body();
    w.f32(TETRAHEDRA + 0x1C, 1.0);
    let (mut list, ..) = body();
    list.u16(TETRAHEDRA, LISTS as u16);
    let (mut outside, ..) = body();
    outside.f32(TETRAHEDRA + 0x14, 0.9);
    let (mut flat, ..) = body();
    flat.f32(TETRAHEDRA + 0x40, 1.0);
    flat.f32(TETRAHEDRA + 0x44, 0.75);
    flat.f32(TETRAHEDRA + 0x48, 0.0);
    let (mut relabelled, ..) = body();
    relabelled.u16(second, 0);
    let (mut tail, ..) = body();
    tail.0[TETRAHEDRA + 0x50] = 1;
    let (mut word, ..) = body();
    word.0[TETRAHEDRA + 8] = 1;
    let (mut capacity, ..) = body();
    capacity.u16(FIVE + 0x20 + 10, 3);
    for b in [w, list, outside, flat, relabelled, tail, word, capacity] {
        assert!(read(&b).0.is_err());
    }
}

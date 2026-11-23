//! A motion's databases of types 5 and 8: tables of 32-byte rows over grids of cells, as type 6's
//! are. What a row's halves mean is not known.

use std::collections::HashMap;

use super::motion::{Grid, GRID_LEN};
use super::{AnimError, Section};

const TABLE_COLUMNS: usize = 3;
const SIMPLEX_COLUMNS: usize = 5;
const COLUMN_LEN: usize = 8;
const TABLE_HEAD_LEN: usize = 16;
const SIMPLEX_LEN: usize = 24;
const TETRAHEDRON_LEN: usize = 112;
/// How far a grid's size times its inverse may be from 1, and a cell's bound from its edge.
const SLACK: f64 = 1e-3;
/// How far a tetrahedron's vertex may lie outside its grid's bounds.
const VERTEX_SLACK: f64 = 1e-4;
/// The determinant of a tetrahedron's edges at or below which, in magnitude, it is flat.
const FLAT: f64 = 1e-9;

/// A database column: a hash, a parameter index and a component; see [`super::column_parameter`].
pub type Column = (u32, [u16; 2]);

/// Four points in the space of a type-5 database's three columns, each naming a weight list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tetrahedron {
    /// Each vertex's weight list, counted from the first its group holds.
    pub lists: [u16; 4],
    pub vertices: [[f32; 3]; 4],
}

/// Type 5: three columns, one table and its grid.
#[derive(Debug, Clone, PartialEq)]
pub struct TableDatabase {
    pub columns: Vec<Column>,
    /// The array at 0x20; one position always names one weight list.
    pub tetrahedra: Vec<Tetrahedron>,
    /// `2a + 2` rows for the grid's first word a: two interleaved chains of `a + 1` steps.
    pub rows: Vec<[u16; 16]>,
    pub grid: Grid,
}

/// Type 8: five columns, tables each over its grid, and corners over a grid of their own.
#[derive(Debug, Clone, PartialEq)]
pub struct SimplexDatabase {
    pub columns: Vec<Column>,
    pub tables: Vec<Vec<[u16; 16]>>,
    pub grids: Vec<Grid>,
    /// Twelve words each, by their values four corners of three cell coordinates.
    pub simplices: Vec<[u16; 12]>,
    /// Its two words are the number of simplices.
    pub grid: Grid,
    /// The word at 0xA0: the motion's weight-list count.
    pub lists: u32,
    /// One rising `[lo, hi]` range for each column.
    pub ranges: [[f32; 2]; SIMPLEX_COLUMNS],
}

impl Section<'_> {
    /// The type-5 database at `at`, of a group of `lists` weight lists.
    pub(super) fn table_database(&self, at: usize, lists: u64) -> Result<TableDatabase, String> {
        let columns = self.columns(at + 0x10, TABLE_COLUMNS)?;
        let grid = self.cell_grid(at + 0x40)?;
        let tetrahedra = self.tetrahedra(at + 0x20, lists, &grid)?;
        let rows = self.rows(at + 0x30)?;
        rows_fit(at, rows.len(), &grid)?;
        Ok(TableDatabase {
            columns,
            tetrahedra,
            rows,
            grid,
        })
    }

    /// The array at `at` of 112-byte tetrahedra `{u16 list x4, u64 0, float4 x4 with w 0, 32 zero
    /// bytes}`: lists below `lists`, vertices inside `grid`, none flat, one list for each position.
    fn tetrahedra(&self, at: usize, lists: u64, grid: &Grid) -> Result<Vec<Tetrahedron>, String> {
        let e = |e: AnimError| e.to_string();
        let count = self.u16("database tetrahedra", at + 8).map_err(e)?;
        if self.u16("database tetrahedra", at + 10).map_err(e)? != count
            || self.u32("database tetrahedra", at + 12).map_err(e)? != 0
        {
            return Err(format!(
                "the tetrahedra at {at:#X} state a capacity other than their count or a non-zero word after it"
            ));
        }
        let bound = |d: usize, k: usize| f64::from(grid.floats[2 * d + k]);
        // A position's bits, -0 taken as +0, and the weight list it names.
        let mut named: HashMap<[u32; 3], u16> = HashMap::new();
        let mut tetrahedra = Vec::new();
        for t in self
            .array("database tetrahedra", at, TETRAHEDRON_LEN)
            .map_err(e)?
        {
            let b = self
                .bytes("database tetrahedron", t, TETRAHEDRON_LEN)
                .map_err(e)?;
            let float = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            let lists_named: [u16; 4] =
                std::array::from_fn(|k| u16::from_le_bytes([b[2 * k], b[2 * k + 1]]));
            let vertices: [[f32; 3]; 4] =
                std::array::from_fn(|k| std::array::from_fn(|d| float(0x10 + 16 * k + 4 * d)));
            let inside = vertices.iter().all(|v| {
                (0..3).all(|d| {
                    let x = f64::from(v[d]);
                    x >= bound(d, 0) - VERTEX_SLACK && x <= bound(d, 1) + VERTEX_SLACK
                })
            });
            let zeros = b[8..0x10].iter().chain(&b[0x50..]).all(|x| *x == 0)
                && (0..4).all(|k| float(0x1C + 16 * k) == 0.0);
            if !zeros
                || !inside
                || lists_named.iter().any(|l| u64::from(*l) >= lists)
                || volume(&vertices).abs() <= FLAT
            {
                return Err(format!(
                    "the tetrahedron at {t:#X} names a weight list past the {lists} its group holds, leaves its grid, is flat, or holds a non-zero byte where every one holds zero"
                ));
            }
            for (v, list) in vertices.iter().zip(lists_named) {
                let key = v.map(|x| if x == 0.0 { 0 } else { x.to_bits() });
                if *named.entry(key).or_insert(list) != list {
                    return Err(format!(
                        "the tetrahedron at {t:#X} names a second weight list at the position {v:?}"
                    ));
                }
            }
            tetrahedra.push(Tetrahedron {
                lists: lists_named,
                vertices,
            });
        }
        Ok(tetrahedra)
    }

    /// The type-8 database at `at`.
    pub(super) fn simplex_database(&self, at: usize) -> Result<SimplexDatabase, String> {
        let e = |e: AnimError| e.to_string();
        let columns = self.columns(at + 0x10, SIMPLEX_COLUMNS)?;
        let heads = self
            .array("database tables", at + 0x20, TABLE_HEAD_LEN)
            .map_err(e)?;
        let grid_heads = self
            .array("database grids", at + 0x30, GRID_LEN)
            .map_err(e)?;
        if heads.is_empty() || heads.len() != grid_heads.len() {
            return Err(format!(
                "the database at {at:#X} holds {} tables and {} grids, where one grid is for each table",
                heads.len(),
                grid_heads.len()
            ));
        }
        let (mut tables, mut grids) = (Vec::new(), Vec::new());
        for (t, g) in heads.into_iter().zip(grid_heads) {
            let grid = self.cell_grid(g)?;
            let rows = self.rows(t)?;
            rows_fit(t, rows.len(), &grid)?;
            tables.push(rows);
            grids.push(grid);
        }
        let mut simplices = Vec::new();
        for s in self
            .array("database simplices", at + 0x40, SIMPLEX_LEN)
            .map_err(e)?
        {
            let bytes = self.bytes("database simplex", s, SIMPLEX_LEN).map_err(e)?;
            let mut corners = [0u16; 12];
            for (word, pair) in corners.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *word = u16::from_le_bytes(*pair);
            }
            simplices.push(corners);
        }
        let grid = self.cell_grid(at + 0x50)?;
        if grid.words.iter().any(|w| *w as usize != simplices.len()) {
            return Err(format!(
                "the database at {at:#X} holds {} simplices and its grid states {:?}",
                simplices.len(),
                grid.words
            ));
        }
        let mut ranges = [[0f32; 2]; SIMPLEX_COLUMNS];
        for (i, range) in ranges.iter_mut().enumerate() {
            for (k, bound) in range.iter_mut().enumerate() {
                *bound = self
                    .f32("database range", at + 0xA4 + 8 * i + 4 * k)
                    .map_err(e)?;
            }
        }
        let rising = ranges
            .iter()
            .all(|[lo, hi]| lo.is_finite() && hi.is_finite() && lo <= hi);
        if !rising || self.u32("database", at + 0xCC).map_err(e)? != 0 {
            return Err(format!(
                "the database at {at:#X} has a column range that falls or a non-zero word at 0xCC"
            ));
        }
        Ok(SimplexDatabase {
            columns,
            tables,
            grids,
            simplices,
            grid,
            lists: self.u32("database", at + 0xA0).map_err(e)?,
            ranges,
        })
    }

    /// The `count` columns of the array at `at`.
    fn columns(&self, at: usize, count: usize) -> Result<Vec<Column>, String> {
        let e = |e: AnimError| e.to_string();
        let heads = self.array("database columns", at, COLUMN_LEN).map_err(e)?;
        if heads.len() != count {
            return Err(format!(
                "the database columns at {at:#X} are {}, not {count}",
                heads.len()
            ));
        }
        heads
            .into_iter()
            .map(|c| {
                let words = [
                    self.u16("database column", c + 4).map_err(e)?,
                    self.u16("database column", c + 6).map_err(e)?,
                ];
                Ok((self.u32("database column", c).map_err(e)?, words))
            })
            .collect()
    }

    /// The grid at `at`: each cell bound `floor(bound / size)`, its second word dividing its first.
    fn cell_grid(&self, at: usize) -> Result<Grid, String> {
        let grid = self.grid(at)?;
        let f = grid.floats.map(f64::from);
        let cells_hold = (0..3).all(|d| {
            let (size, inverse) = (f[6 + d], f[9 + d]);
            let edges = (0..2).all(|k| {
                let x = f[2 * d + k] * inverse;
                let cell = f64::from(grid.cells[d][k]);
                (x + SLACK).floor() == cell || (x - SLACK).floor() == cell
            });
            size > 0.0 && (inverse * size - 1.0).abs() < SLACK && f[2 * d] <= f[2 * d + 1] && edges
        });
        let [a, b] = grid.words;
        if !cells_hold || b == 0 || !a.is_multiple_of(b) {
            return Err(format!(
                "the database grid at {at:#X} has a cell that is not floor(bound / size), or its {b} does not divide its {a}"
            ));
        }
        Ok(grid)
    }
}

/// Whether a table of `rows` rows over `grid` holds `2a + 2`, a its grid's first word.
fn rows_fit(at: usize, rows: usize, grid: &Grid) -> Result<(), String> {
    if rows as u64 != 2 * u64::from(grid.words[0]) + 2 {
        return Err(format!(
            "the database table at {at:#X} holds {rows} rows, where its grid's first word {} gives {}",
            grid.words[0],
            2 * u64::from(grid.words[0]) + 2
        ));
    }
    Ok(())
}

/// The determinant of the edges from the first vertex of `v`: six times its signed volume.
fn volume(v: &[[f32; 3]; 4]) -> f64 {
    let edge =
        |k: usize| -> [f64; 3] { std::array::from_fn(|d| f64::from(v[k][d]) - f64::from(v[0][d])) };
    let (a, b, c) = (edge(1), edge(2), edge(3));
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

#[cfg(test)]
#[path = "databases_tests.rs"]
mod tests;

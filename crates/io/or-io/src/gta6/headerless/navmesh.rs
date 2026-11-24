//! The navigation mesh of one 128 m cell: a 0xF0-byte root, GTA V's split arrays and quadtree,
//! a 12-byte adjacency record and a 44-byte polygon.
//!
//! Not read, since what they hold is not known: a polygon's words at +0x18 (but for the side record
//! bits), +0x20 (but for its link run), +0x24 (but for its byte 1) and +0x28, and a footprint's
//! +0x54 and +0x58. The
//! adjacency record's bytes 6 to 11, a polygon's byte +0x25 and a special link's +0x18 are kept
//! raw: no rule of them was found.

use std::collections::BTreeSet;

use super::bytes::{f32_at, sections, u32_at, u64_at, Bytes};
use super::{HeaderlessError, Result};

/// The bytes of one split array chunk: every chunk but the last holds `CHUNK / stride` items.
const CHUNK: usize = 16384;
/// The most quadtree nodes and levels a body was measured to hold, with room.
const MAX_NODES: usize = 65536;
const MAX_DEPTH: usize = 16;
/// A side record's bits at polygon +0x18, which no polygon proper carries.
pub const SIDE_RECORD: u32 = 0x8800;
/// The adjacency record's mesh where its edge leads nowhere.
pub const NOWHERE: u32 = 0xFFFF_FFFF;
/// A polygon flag bit at +0x08: an area below 2 m².
pub const SMALL: u32 = 0x01;
/// A polygon flag bit at +0x08: water.
pub const WATER: u32 = 0x80;
/// A footprint's kind bit: eight outline corners where set, four where clear.
pub const EIGHT_CORNERS: u16 = 0x800;
/// A footprint's outline unit in metres: one vertex quantum of a 128 m cell.
pub const FOOTPRINT_UNIT: f32 = 1.0 / 512.0;
/// How far from its border line a side record's corner lies, at most; one stands 0.06 m inside.
const ON_BORDER: f32 = 0.25;
/// Two cells' chain edges join where their heights agree within this over their overlap.
const JOIN_HEIGHT: f32 = 0.2;
/// The shortest overlap of two chain edges that joins them: less is the inset of a slot's corner.
const JOIN_OVERLAP: f32 = 0.06;

#[derive(Debug)]
pub struct NavMesh {
    /// The root's +0x10, a small odd number.
    pub flags: u32,
    /// `x + 512 y` of the cell on the 128 m grid from (-32768, -32768); 0 for a mesh of no cell.
    pub cell: u32,
    pub build_id: u32,
    /// Its root's box is the box every vertex is quantised into.
    pub tree: NavNode,
    pub vertices: Vec<[u16; 3]>,
    /// The corners of every polygon, run after run.
    pub indices: Vec<u16>,
    /// One per index: what lies across the edge from that corner to the next.
    pub edges: Vec<NavEdge>,
    /// The polygons proper, then the side records along the cell's border.
    pub polygons: Vec<NavPolygon>,
    /// How many of `polygons` come before the first side record.
    pub polygons_proper: usize,
    pub nodes: usize,
    pub depth: usize,
    pub leaves: usize,
    pub special_links: Vec<SpecialLink>,
    pub special_link_indices: Vec<u16>,
    pub footprints: Vec<Footprint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavEdge {
    /// The mesh's own cell, or [`NOWHERE`].
    pub mesh: u32,
    /// The neighbour's number modulo 65,536, which [`NavMesh::neighbour`] resolves.
    pub polygon: u16,
    /// Bytes 6 and 7, raw.
    pub word_06: u16,
    /// Bytes 8 to 11: flags in the low five bits; on a side record, a float above them.
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavPolygon {
    pub first: u32,
    pub corners: u8,
    /// +0x08: [`SMALL`] and [`WATER`] among others.
    pub flags: u32,
    /// x min, x max, y min, y max, z min, z max in whole metres.
    pub min_max: [i16; 6],
    /// The mean of the corners in x and y, as 255 steps across the box `min_max`.
    pub centroid: [u8; 2],
    /// +0x18, whose bits but [`SIDE_RECORD`] are not known.
    pub side_flags: u32,
    /// +0x25, raw: it grows with the slope on the whole, but no function of the normal gives it.
    pub steepness: u8,
    /// +0x20: [`NavPolygon::link_run`] in bits 0 to 21; bits 22 to 31 are not known.
    pub word_20: u32,
}

impl NavPolygon {
    /// Its run of [`NavMesh::special_link_indices`]: +0x20's count in bits 0-5, start in 6-21.
    pub fn link_run(&self) -> std::ops::Range<usize> {
        let start = ((self.word_20 >> 6) & 0xFFFF) as usize;
        start..start + (self.word_20 & 0x3F) as usize
    }
}

/// A quadtree node: a leaf names the polygons it overlaps, a branch has children.
#[derive(Debug, Clone, PartialEq)]
pub struct NavNode {
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    /// A leaf's polygon numbers modulo 65,536, which [`NavMesh::leaf_polygons`] resolves.
    pub polygons: Option<Vec<u16>>,
    pub children: [Option<Box<NavNode>>; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialLink {
    /// Quantised as a vertex is.
    pub from: [u16; 3],
    pub to: [u16; 3],
    /// Polygon numbers modulo 65,536, which [`NavMesh::link_polygon`] resolves.
    pub from_polygon: u16,
    pub to_polygon: u16,
    pub from_mesh: u32,
    pub to_mesh: u32,
    /// +0x18, raw: bits 0 to 2 are zero in every measured link.
    pub word_18: u32,
    /// +0x1C: the heading in 1/256 turn, 0 along +y and growing counter-clockwise.
    pub heading: u32,
}

impl SpecialLink {
    /// The heading in degrees, counter-clockwise from +y.
    pub fn heading_degrees(&self) -> f32 {
        self.heading as f32 * 360.0 / 256.0
    }
}

/// A record of the root's +0xB8 array: a convex outline around a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Footprint {
    /// Zero, or a key exactly two records of the mesh share.
    pub key: u64,
    pub position: [f32; 3],
    /// [`EIGHT_CORNERS`]; its bits 2, 3 and 10 are not known.
    pub kind: u16,
    /// Four or eight corners as x and y offsets in [`FOOTPRINT_UNIT`]s, the pairs after them zero.
    pub outline: [[i16; 2]; 8],
}

impl Footprint {
    /// The outline's corners in metres, at the position's height.
    pub fn corners(&self) -> Vec<[f32; 3]> {
        let count = if self.kind & EIGHT_CORNERS != 0 { 8 } else { 4 };
        let [x, y, z] = self.position;
        (self.outline[..count].iter())
            .map(|[dx, dy]| {
                [
                    x + f32::from(*dx) * FOOTPRINT_UNIT,
                    y + f32::from(*dy) * FOOTPRINT_UNIT,
                    z,
                ]
            })
            .collect()
    }
}

/// A side of a cell, along which side records lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Border {
    /// The largest x.
    East,
    West,
    /// The largest y.
    North,
    South,
}

impl Border {
    const ALL: [Self; 4] = [Self::East, Self::West, Self::North, Self::South];

    /// The cell on the other side of this border of `cell`, on the 512 by 512 grid.
    pub fn across(self, cell: u32) -> Option<u32> {
        let (x, y) = (cell % 512, cell / 512);
        match self {
            Self::East => (x < 511).then(|| cell + 1),
            Self::West => (x > 0).then(|| cell - 1),
            Self::North => (y < 511).then(|| cell + 512),
            Self::South => (y > 0).then(|| cell - 512),
        }
    }

    /// The cells a link of `cell` may end in but its own: across each border, then each corner.
    pub fn around(cell: u32) -> Vec<u32> {
        let corners = [
            (Self::East, Self::North),
            (Self::East, Self::South),
            (Self::West, Self::North),
            (Self::West, Self::South),
        ];
        let sides = Self::ALL.into_iter().filter_map(|b| b.across(cell));
        let corners = (corners.into_iter()).filter_map(|(a, b)| b.across(a.across(cell)?));
        sides.chain(corners).collect()
    }

    fn opposite(self) -> Self {
        match self {
            Self::East => Self::West,
            Self::West => Self::East,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }

    /// The axis the border runs along.
    fn along(self) -> usize {
        match self {
            Self::East | Self::West => 1,
            Self::North | Self::South => 0,
        }
    }
}

/// An edge of a side record's chain, which runs along its cell's border, and the polygon proper
/// inside the cell its adjacency names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderEdge {
    pub border: Border,
    /// The side record's index among the polygons.
    pub record: usize,
    pub polygon: Option<usize>,
    pub from: [f32; 3],
    pub to: [f32; 3],
}

/// Polygon `polygon` of one cell meets polygon `other` of the cell across `border`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Join {
    pub border: Border,
    pub polygon: usize,
    pub other: usize,
}

/// The polygons below `limit` whose number modulo 65,536 is `id`.
fn named(id: u16, limit: usize) -> impl Iterator<Item = usize> {
    (usize::from(id)..limit).step_by(1 << 16)
}

impl NavMesh {
    /// A vertex's position, dequantised into the quadtree root's box.
    pub fn position(&self, vertex: [u16; 3]) -> [f32; 3] {
        let (min, max) = (self.tree.box_min, self.tree.box_max);
        std::array::from_fn(|a| min[a] + f32::from(vertex[a]) * (max[a] - min[a]) / 65535.0)
    }

    /// Every side record's chain edges: corner 1 to 2 and on round to corner 0.
    // The edge from corner 0 to 1 is the one that faces the next cell.
    pub fn border_edges(&self) -> Vec<BorderEdge> {
        let (min, max) = (self.tree.box_min, self.tree.box_max);
        let mut out = Vec::new();
        for (record, p) in self.polygons.iter().enumerate().skip(self.polygons_proper) {
            let (first, n) = (p.first as usize, usize::from(p.corners));
            let (Some(corners), Some(_)) = (
                self.indices.get(first..first + n),
                self.edges.get(first..first + n),
            ) else {
                continue;
            };
            let at: Vec<[f32; 3]> = (corners.iter())
                .filter_map(|i| self.vertices.get(usize::from(*i)))
                .map(|q| self.position(*q))
                .collect();
            let on =
                |axis: usize, line: f32| at.iter().all(|c| (c[axis] - line).abs() <= ON_BORDER);
            let Some(border) = Border::ALL.into_iter().find(|b| match b {
                Border::East => on(0, max[0]),
                Border::West => on(0, min[0]),
                Border::North => on(1, max[1]),
                Border::South => on(1, min[1]),
            }) else {
                continue;
            };
            if at.len() < 2 {
                continue;
            }
            for j in 1..at.len() {
                let polygon = self
                    .neighbour(first + j)
                    .filter(|p| *p < self.polygons_proper);
                out.push(BorderEdge {
                    border,
                    record,
                    polygon,
                    from: at[j],
                    to: at[(j + 1) % at.len()],
                });
            }
        }
        out
    }

    /// The corners of polygon `polygon`, as vertex numbers.
    fn corners(&self, polygon: usize) -> &[u16] {
        (self.polygons.get(polygon))
            .and_then(|p| {
                let first = p.first as usize;
                self.indices.get(first..first + usize::from(p.corners))
            })
            .unwrap_or_default()
    }

    /// The polygon of this cell across the edge at `index`, from its corner to the next: of the
    /// polygons its number names, the one that holds both the edge's vertices.
    pub fn neighbour(&self, index: usize) -> Option<usize> {
        let e = self.edges.get(index)?;
        if e.mesh != self.cell {
            return None;
        }
        let candidates: Vec<usize> = named(e.polygon, self.polygons.len()).collect();
        if let [one] = candidates[..] {
            return Some(one);
        }
        let owner = (self.polygons)
            .partition_point(|p| p.first as usize <= index)
            .checked_sub(1)?;
        let own = self.corners(owner);
        let k = index - self.polygons[owner].first as usize;
        let ends = [*own.get(k)?, *own.get((k + 1) % own.len())?];
        let mut holding =
            (candidates.into_iter()).filter(|c| ends.iter().all(|v| self.corners(*c).contains(v)));
        match (holding.next(), holding.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }

    /// The polygons proper a leaf names: each its numbers name whose box meets the leaf's within 1 m.
    // A number that names two such polygons stands for both: the leaf is a list of candidates.
    pub fn leaf_polygons(&self, leaf: &NavNode) -> Vec<usize> {
        let meets = |p: &NavPolygon| {
            (0..3).all(|a| {
                let (lo, hi) = (f32::from(p.min_max[2 * a]), f32::from(p.min_max[2 * a + 1]));
                lo <= leaf.box_max[a] + 1.0 && hi >= leaf.box_min[a] - 1.0
            })
        };
        let mut out: Vec<usize> = (leaf.polygons.iter().flatten())
            .flat_map(|id| named(*id, self.polygons_proper))
            .filter(|p| meets(&self.polygons[*p]))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The polygon of this cell a special link's end at `at` with number `id` lies in: of the
    /// polygons the number names, the one whose box holds the end. `None` for another mesh's end.
    pub fn link_polygon(&self, mesh: u32, id: u16, at: [u16; 3]) -> Option<usize> {
        if mesh != self.cell {
            return None;
        }
        let candidates: Vec<usize> = named(id, self.polygons.len()).collect();
        if let [one] = candidates[..] {
            return Some(one);
        }
        let point = self.position(at);
        let mut holding = candidates.into_iter().filter(|p| {
            let b = self.polygons[*p].min_max;
            (0..3).all(|a| (f32::from(b[2 * a])..=f32::from(b[2 * a + 1])).contains(&point[a]))
        });
        match (holding.next(), holding.next()) {
            (Some(p), None) => Some(p),
            _ => None,
        }
    }
}

/// Where `a`'s polygons proper meet `b`'s across the border the two cells share.
// They meet where their side records' chain edges overlap along it at heights that agree,
// so stacked levels join apart.
pub fn joins(a: &NavMesh, b: &NavMesh) -> Vec<Join> {
    join_edges((a.cell, &a.border_edges()), (b.cell, &b.border_edges()))
}

/// [`joins`] of two cells given by their cell and their [`NavMesh::border_edges`].
pub fn join_edges(
    (a_cell, a_edges): (u32, &[BorderEdge]),
    (b_cell, b_edges): (u32, &[BorderEdge]),
) -> Vec<Join> {
    let Some(border) = Border::ALL
        .into_iter()
        .find(|s| s.across(a_cell) == Some(b_cell))
    else {
        return Vec::new();
    };
    let t = border.along();
    let span = |e: &BorderEdge| (e.from[t].min(e.to[t]), e.from[t].max(e.to[t]));
    let height = |e: &BorderEdge, at: f32| {
        let run = e.to[t] - e.from[t];
        if run.abs() < 1e-6 {
            (e.from[2] + e.to[2]) / 2.0
        } else {
            e.from[2] + (e.to[2] - e.from[2]) * (at - e.from[t]) / run
        }
    };
    let theirs: Vec<&BorderEdge> = (b_edges.iter())
        .filter(|e| e.border == border.opposite() && e.polygon.is_some())
        .collect();
    let mut out = BTreeSet::new();
    for e in a_edges.iter().filter(|e| e.border == border) {
        let Some(polygon) = e.polygon else { continue };
        for &f in &theirs {
            let (lo, hi) = (span(e).0.max(span(f).0), span(e).1.min(span(f).1));
            let mid = (lo + hi) / 2.0;
            if hi - lo >= JOIN_OVERLAP && (height(e, mid) - height(f, mid)).abs() <= JOIN_HEIGHT {
                out.extend(f.polygon.map(|other| Join {
                    border,
                    polygon,
                    other,
                }));
            }
        }
    }
    out.into_iter().collect()
}

pub fn read_navmesh(virtual_section: &[u8]) -> Result<NavMesh> {
    sections(virtual_section, Some(&[]), |v, _| navmesh_at(v))
}

fn navmesh_at(v: Bytes) -> Result<NavMesh> {
    let vertex_bytes = split(v, "vertex list", 0x70, 6)?;
    let index_bytes = split(v, "index list", 0x80, 2)?;
    let edge_bytes = split(v, "adjacency list", 0x88, 12)?;
    let polygon_bytes = split(v, "polygon list", 0x98, 44)?;
    let (vertex_count, polygon_count) =
        (v.u32("vertex count", 0xC0)?, v.u32("polygon count", 0xC4)?);
    let index_count = v.u32("index count", 0x90)?;
    let cell = v.u32("cell", 0xC8)?;
    let u16s = |b: &[u8]| -> Vec<u16> {
        (b.as_chunks::<2>().0.iter())
            .map(|w| u16::from_le_bytes(*w))
            .collect()
    };
    let vertices: Vec<[u16; 3]> = (vertex_bytes.as_chunks::<6>().0.iter())
        .map(|q| std::array::from_fn(|a| u16::from_le_bytes([q[2 * a], q[2 * a + 1]])))
        .collect();
    let indices = u16s(&index_bytes);
    agree(
        "vertex list and the vertex count",
        vertices.len(),
        vertex_count,
    )?;
    agree("index list and the index count", indices.len(), index_count)?;
    agree(
        "adjacency list and the index count",
        edge_bytes.len() / 12,
        index_count,
    )?;
    agree(
        "polygon list and the polygon count",
        polygon_bytes.len() / 44,
        polygon_count,
    )?;
    if let Some(i) = indices.iter().find(|i| u32::from(**i) >= vertex_count) {
        return Err(past(
            "polygon corner's vertex",
            (*i).into(),
            vertex_count.into(),
        ));
    }
    let edges = (edge_bytes.as_chunks::<12>().0.iter())
        .map(|e| NavEdge {
            mesh: u32_at(e, 0),
            polygon: u16::from_le_bytes([e[4], e[5]]),
            word_06: u16::from_le_bytes([e[6], e[7]]),
            flags: u32_at(e, 8),
        })
        .collect::<Vec<_>>();
    for e in &edges {
        if e.mesh != cell && e.mesh != NOWHERE {
            return Err(HeaderlessError::Disagree {
                what: "adjacency record's mesh and the mesh's own cell",
                first: e.mesh.into(),
                second: cell.into(),
            });
        }
        if e.mesh == cell && u32::from(e.polygon) >= polygon_count {
            return Err(past(
                "adjacency record's polygon",
                e.polygon.into(),
                polygon_count.into(),
            ));
        }
    }
    let mut first = 0;
    let polygons = (polygon_bytes.as_chunks::<44>().0.iter())
        .map(|p| {
            let packed = u32_at(p, 4);
            let polygon = NavPolygon {
                first: packed >> 4,
                corners: (packed & 0xF) as u8,
                flags: u32_at(p, 8),
                min_max: std::array::from_fn(|k| {
                    i16::from_le_bytes([p[0xC + 2 * k], p[0xD + 2 * k]])
                }),
                side_flags: u32_at(p, 0x18),
                centroid: [p[0x1C], p[0x1D]],
                steepness: p[0x25],
                word_20: u32_at(p, 0x20),
            };
            agree(
                "polygon's first index and the corners before it",
                first,
                polygon.first,
            )?;
            first += usize::from(polygon.corners);
            Ok(polygon)
        })
        .collect::<Result<Vec<_>>>()?;
    agree("polygons' corners and the index count", first, index_count)?;

    // The polygons proper end at the first record with the side bits; the leaves name them by
    // their number modulo 65,536.
    let side = |p: &NavPolygon| p.side_flags & SIDE_RECORD == SIDE_RECORD;
    let polygons_proper = polygons.iter().position(side).unwrap_or(polygons.len());
    if let Some(record) = (polygons.iter().skip(polygons_proper)).find(|p| !side(p)) {
        return Err(HeaderlessError::Constant {
            what: "side record's bits 0x8800 at +0x18",
            at: 0x98,
            found: record.side_flags.into(),
        });
    }
    for (i, p) in polygons.iter().enumerate() {
        let least = if i < polygons_proper { 3 } else { 2 };
        if p.corners < least {
            return Err(HeaderlessError::Constant {
                what: "corner count, 3 or more for a polygon proper and 2 for a side record,",
                at: 0x98,
                found: p.corners.into(),
            });
        }
    }
    let mut walk = Walk::default();
    let tree = node(v, v.to("quadtree", 0xA0)?, 0, polygons_proper, &mut walk)?;

    let links = counted(v, "special links", 0xA8, 0xD4, 32)?;
    let special_links: Vec<SpecialLink> = (links.as_chunks::<32>().0.iter())
        .map(|l| {
            let q = |o: usize| -> [u16; 3] {
                std::array::from_fn(|a| u16::from_le_bytes([l[o + 2 * a], l[o + 1 + 2 * a]]))
            };
            SpecialLink {
                from: q(0),
                to: q(6),
                from_polygon: u16::from_le_bytes([l[0xC], l[0xD]]),
                to_polygon: u16::from_le_bytes([l[0xE], l[0xF]]),
                from_mesh: u32_at(l, 0x10),
                to_mesh: u32_at(l, 0x14),
                word_18: u32_at(l, 0x18),
                heading: u32_at(l, 0x1C),
            }
        })
        .collect();
    let special_link_indices = u16s(counted(v, "special link indices", 0xB0, 0xD8, 2)?);
    if let Some(i) = (special_link_indices.iter()).find(|i| usize::from(**i) >= special_links.len())
    {
        return Err(past(
            "special link index",
            (*i).into(),
            special_links.len() as u64,
        ));
    }
    // Every record's run starts where the runs before it end, side records' too.
    let mut runs = 0;
    for p in &polygons {
        let run = p.link_run();
        if !run.is_empty() && run.start != runs {
            return Err(HeaderlessError::Disagree {
                what: "polygon's link run start at +0x20 and the runs before it",
                first: run.start as u64,
                second: runs as u64,
            });
        }
        runs += run.len();
    }
    agree(
        "polygons' link runs and the link index count",
        runs,
        v.u32("special link index count", 0xD8)?,
    )?;
    let footprints = (counted(v, "footprints", 0xB8, 0xDC, 96)?
        .as_chunks::<96>()
        .0
        .iter())
    .map(|f| Footprint {
        key: u64_at(f, 0),
        position: std::array::from_fn(|a| f32_at(f, 8 + 4 * a)),
        kind: u16::from_le_bytes([f[0x14], f[0x15]]),
        outline: std::array::from_fn(|k| {
            std::array::from_fn(|a| {
                i16::from_le_bytes([f[0x16 + 4 * k + 2 * a], f[0x17 + 4 * k + 2 * a]])
            })
        }),
    })
    .collect();
    // The vertex, index, adjacency, polygon and special link payloads, and nothing else.
    let counted_bytes = 6 * u64::from(vertex_count)
        + 14 * u64::from(index_count)
        + 44 * u64::from(polygon_count)
        + 32 * special_links.len() as u64;
    agree(
        "memory total at 0xCC and the arrays it counts",
        counted_bytes,
        v.u32("memory total", 0xCC)?,
    )?;

    Ok(NavMesh {
        flags: v.u32("flags", 0x10)?,
        cell,
        build_id: v.u32("build id", 0xE8)?,
        tree,
        vertices,
        indices,
        edges,
        polygons,
        polygons_proper,
        nodes: walk.nodes,
        depth: walk.depth,
        leaves: walk.leaves,
        special_links,
        special_link_indices,
        footprints,
    })
}

fn agree(what: &'static str, found: impl TryInto<u64>, stated: u32) -> Result<()> {
    let found = found.try_into().unwrap_or(u64::MAX);
    if found == u64::from(stated) {
        Ok(())
    } else {
        Err(HeaderlessError::Disagree {
            what,
            first: found,
            second: stated.into(),
        })
    }
}

fn past(what: &'static str, index: u64, count: u64) -> HeaderlessError {
    HeaderlessError::IndexPastCount { what, index, count }
}

/// The items of the split array the root's slot `slot` points at, chunk after chunk.
fn split(v: Bytes, what: &'static str, slot: usize, stride: usize) -> Result<Vec<u8>> {
    let at = v.to(what, slot)?;
    let count = v.u64(what, at + 0x08)?;
    let table = v.to(what, at + 0x10)?;
    let starts = v.to(what, at + 0x18)?;
    let chunks = usize::try_from(v.u64(what, at + 0x20)?).unwrap_or(usize::MAX);
    let rows = v.array(what, table, chunks, 16)?;
    let starts = v.array(what, starts, chunks, 4)?;
    let capacity = (CHUNK / stride) as u64;
    let mut items = Vec::new();
    for (i, row) in rows.as_chunks::<16>().0.iter().enumerate() {
        agree(
            "split array chunk's start and the items before it",
            items.len() / stride,
            u32_at(starts, 4 * i),
        )?;
        let n = u64_at(row, 8);
        if n > capacity || (i + 1 < chunks && n != capacity) {
            return Err(HeaderlessError::Disagree {
                what: "split array chunk's count and the capacity its stride gives",
                first: n,
                second: capacity,
            });
        }
        let chunk = v.to(what, table + 16 * i)?;
        items.extend_from_slice(v.array(what, chunk, n as usize, stride)?);
    }
    if (items.len() / stride) as u64 != count {
        return Err(HeaderlessError::Disagree {
            what: "split array's count and its chunks' counts",
            first: count,
            second: (items.len() / stride) as u64,
        });
    }
    Ok(items)
}

/// The array the root's slot `slot` points at, of the count the root's `count` states.
fn counted<'a>(
    v: Bytes<'a>,
    what: &'static str,
    slot: usize,
    count_at: usize,
    stride: usize,
) -> Result<&'a [u8]> {
    let count = v.u32(what, count_at)? as usize;
    if count == 0 {
        return Ok(&[]);
    }
    v.array(what, v.to(what, slot)?, count, stride)
}

/// The quadtree's node, leaf and level counts, as the walk goes.
#[derive(Default)]
struct Walk {
    nodes: usize,
    leaves: usize,
    depth: usize,
}

/// The quadtree node at `at`, 0x60 bytes whose pointers sit at four modulo eight; a leaf names
/// polygons below `polygons`.
fn node(v: Bytes, at: usize, depth: usize, polygons: usize, walk: &mut Walk) -> Result<NavNode> {
    walk.nodes += 1;
    walk.depth = walk.depth.max(depth);
    if walk.nodes > MAX_NODES || depth > MAX_DEPTH {
        return Err(HeaderlessError::TreeTooLarge {
            nodes: MAX_NODES,
            depth: MAX_DEPTH,
        });
    }
    v.get("quadtree node", at, 0x60)?;
    let named = match v.ptr("quadtree leaf", at + 0x2C)? {
        Some(leaf) => {
            let count = usize::from(v.u16("leaf polygon count", leaf + 0x18)?);
            let ids = match count {
                0 => &[][..],
                _ => v.array(
                    "leaf polygons",
                    v.to("leaf polygons", leaf + 0x08)?,
                    count,
                    2,
                )?,
            };
            let named: Vec<u16> = (ids.as_chunks::<2>().0.iter())
                .map(|w| u16::from_le_bytes(*w))
                .collect();
            if let Some(p) = named.iter().find(|p| usize::from(**p) >= polygons) {
                return Err(past("leaf's polygon", (*p).into(), polygons as u64));
            }
            walk.leaves += 1;
            Some(named)
        }
        None => None,
    };
    let mut children: [Option<Box<NavNode>>; 4] = Default::default();
    for (i, slot) in children.iter_mut().enumerate() {
        if let Some(child) = v.ptr("quadtree child", at + 0x34 + 8 * i)? {
            *slot = Some(Box::new(node(v, child, depth + 1, polygons, walk)?));
        }
    }
    Ok(NavNode {
        box_min: v.vec3("quadtree box minimum", at)?,
        box_max: v.vec3("quadtree box maximum", at + 0x10)?,
        polygons: named,
        children,
    })
}

//! A navigation polygon proper deleted from its cell's body, or cut off where its side records
//! drop a corner: its record, corners and edges and every vertex only it named go, the side records
//! naming it are chained again and one left naming no polygon goes, a special link with an end in a
//! record that went goes, the records after any that went are renumbered wherever this body names
//! them, and the arrays that shrank give their bytes back by the resource rule.
//!
//! ⚠ Other bodies name this cell's polygons by number in their special links and are saved apart:
//! an edit is refused unless the links of each body given resolve before and after it, and
//! `relinked` makes those links again. Every number is stored modulo 65,536 and resolved by the
//! corners or the box of the polygons it names, before and after.

use std::collections::HashSet;

use crate::container::rpf::{grow_section, Rpf9GrowError, Rpf9Section, Rpf9Sections};

use super::bytes::{sections, Bytes};
use super::write_navmesh_side::{rechained, Corner};
use super::write_physics::put;
use super::{
    read_navmesh, HeaderlessError, NavEdge, NavMesh, NavNode, NavPolygon, Result, SpecialLink,
};

/// The bytes of one split array chunk, as the reader takes them.
const CHUNK: usize = 16384;
/// A polygon record's bytes.
const RECORD: usize = 44;
/// Polygon +0x20's link run: the count in bits 0-5 and the start in bits 6-21.
pub(super) const RUN: u32 = 0x3F_FFFF;
/// The most entries a link run's six bits count.
const RUN_MOST: usize = 63;
/// The records a 16-bit polygon number tells apart.
pub(super) const NUMBERS: usize = 1 << 16;
/// The reader's bounds on the quadtree.
const MAX_NODES: usize = 65536;
const MAX_DEPTH: usize = 16;
/// The root's slot and the item stride of each split array: vertices, indices, adjacency, polygons.
const LISTS: [(usize, usize, &str); 4] = [
    (0x70, 6, "vertex list"),
    (0x80, 2, "index list"),
    (0x88, 12, "adjacency list"),
    (0x98, RECORD, "polygon list"),
];
const POLYGONS: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum NavDeleteError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Grow(#[from] Rpf9GrowError),
    #[error("polygon {polygon} is not one of the cell's {proper} polygons proper: a side record goes with the last polygon proper its pieces name, never alone")]
    NotProper { polygon: usize, proper: usize },
    #[error("polygon {from}'s edge at corner {corner} still leads into polygon {polygon}: disable it first")]
    EdgeInto {
        from: usize,
        corner: usize,
        polygon: usize,
    },
    #[error("side record {record} would name no polygon and go, but an edge of polygon {from} still leads into it")]
    SideRecordNamed { record: usize, from: usize },
    #[error("{what} {index}: a number stored modulo 65,536 tells no one polygon apart, before the edit or after it")]
    Ambiguous { what: &'static str, index: usize },
    #[error(
        "a special link index or a polygon's link run start would pass the 16 bits it is stored in"
    )]
    IndexPast,
    #[error("the {list} would lose its last chunk and hold nothing: a cell of no polygon is not written")]
    ChunkEmpties { list: &'static str },
    #[error("the special link indices would go from {from} entries to {to}, and an array that appears is not written")]
    IndexListAppears { from: usize, to: usize },
    #[error("the {what} would grow in place")]
    Grows { what: &'static str },
    #[error("the body does not read back as the deletion: {0}")]
    ReadBack(String),
}

pub(super) type Done<T> = std::result::Result<T, NavDeleteError>;

/// A cell [`deletion`] or [`disabling`] made again, with the stored record and edge of each.
#[derive(Debug)]
pub struct NavRebuild {
    pub mesh: NavMesh,
    pub records: Vec<usize>,
    pub edges: Vec<usize>,
}

/// Whether number `n` names, among `records` records, one at or past `from`.
pub(super) fn names_from(n: u16, records: usize, from: usize) -> bool {
    let n = usize::from(n);
    n < records && n + (records - 1 - n) / NUMBERS * NUMBERS >= from
}

/// How a body files its links: under the to number as well as the from number, and whether an
/// empty run starts at the running total or at 0.
#[derive(Debug, Clone, Copy)]
pub(super) struct Filing {
    both: bool,
    empty_at_total: bool,
}

/// The link index list and each record's +0x20 bits 0-21 `filing` gives `links` over `records`
/// records: under each record, in link order, the first of each set of equal links that a filed
/// end numbers, once, at most 63; `None` where an index or a start passes 16 bits.
pub(super) fn file(
    links: &[SpecialLink],
    records: usize,
    filing: Filing,
) -> Option<(Vec<u16>, Vec<u32>)> {
    let mut seen = HashSet::new();
    let mut under: Vec<Vec<u16>> = vec![Vec::new(); records];
    for (i, l) in links.iter().enumerate() {
        let key = (
            l.from,
            l.to,
            l.from_polygon,
            l.to_polygon,
            l.from_mesh,
            l.to_mesh,
            l.word_18,
            l.heading,
        );
        if !seen.insert(key) {
            continue;
        }
        let i = u16::try_from(i).ok()?;
        let filed = [Some(l.from_polygon), filing.both.then_some(l.to_polygon)];
        for n in filed.into_iter().flatten() {
            if let Some(run) = under.get_mut(usize::from(n)) {
                if run.len() < RUN_MOST && run.last() != Some(&i) {
                    run.push(i);
                }
            }
        }
    }
    let mut list = Vec::new();
    let mut words = Vec::with_capacity(records);
    for run in under {
        let total = u32::try_from(list.len()).ok().filter(|s| *s <= 0xFFFF)?;
        let start = match run.is_empty() && !filing.empty_at_total {
            true => 0,
            false => total,
        };
        words.push((start << 6) | run.len() as u32);
        list.extend(run);
    }
    Some((list, words))
}

/// The [`Filing`] that gives back the link index list and runs `m` holds; where none does, each
/// link under both its ends and an empty run at the running total, as every cell with links holds.
pub(super) fn filing(m: &NavMesh) -> Filing {
    // A body of no cell files each link under its from number alone; no other body links into it.
    if m.cell == 0 {
        return Filing {
            both: false,
            empty_at_total: true,
        };
    }
    for both in [true, false] {
        for empty_at_total in [true, false] {
            let f = Filing {
                both,
                empty_at_total,
            };
            let Some((list, words)) = file(&m.special_links, m.polygons.len(), f) else {
                continue;
            };
            let runs = (m.polygons.iter().zip(&words)).all(|(p, w)| p.word_20 & RUN == *w);
            if list == m.special_link_indices && runs {
                return f;
            }
        }
    }
    Filing {
        both: true,
        empty_at_total: true,
    }
}

/// Drops polygon `number` from every leaf and lowers each number above it by one.
fn drop_from(node: &mut NavNode, number: u16) {
    if let Some(ids) = node.polygons.as_mut() {
        ids.retain(|n| *n != number);
        for n in ids.iter_mut().filter(|n| **n > number) {
            *n -= 1;
        }
    }
    for child in node.children.iter_mut().flatten() {
        drop_from(child, number);
    }
}

/// The polygons proper leaf `leaf` of `m` names, polygon `p` gone and those after it renumbered.
fn leaf_after(m: &NavMesh, leaf: &NavNode, p: usize) -> Vec<usize> {
    (m.leaf_polygons(leaf).into_iter())
        .filter(|q| *q != p)
        .map(|q| if q > p { q - 1 } else { q })
        .collect()
}

/// `node` of `m` with polygon `p` gone from each leaf by [`leaf_after`], its numbers ascending
/// and each once.
fn leaves_without(m: &NavMesh, node: &NavNode, p: usize) -> NavNode {
    let polygons = node.polygons.as_ref().map(|_| {
        let mut ids: Vec<u16> = (leaf_after(m, node, p).into_iter())
            .map(|q| q as u16)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    });
    NavNode {
        box_min: node.box_min,
        box_max: node.box_max,
        polygons,
        children: std::array::from_fn(|i| {
            (node.children[i].as_deref()).map(|c| Box::new(leaves_without(m, c, p)))
        }),
    }
}

/// Whether each leaf of `new`, in `want`, names what [`leaf_after`] gives for its leaf of `old`.
fn leaves_agree(m: &NavMesh, old: &NavNode, want: &NavMesh, new: &NavNode, p: usize) -> bool {
    let here = old.polygons.is_none() || want.leaf_polygons(new) == leaf_after(m, old, p);
    here && (old.children.iter().zip(&new.children)).all(|pair| match pair {
        (Some(a), Some(b)) => leaves_agree(m, a, want, b, p),
        (None, None) => true,
        _ => false,
    })
}

/// Each leaf's numbers, the leaves in the order the reader walks them.
fn leaf_ids<'a>(node: &'a NavNode, out: &mut Vec<&'a [u16]>) {
    if let Some(ids) = &node.polygons {
        out.push(ids);
    }
    for child in node.children.iter().flatten() {
        leaf_ids(child, out);
    }
}

/// Cell `m` with polygon proper `polygon` deleted, or why not; `others` may link into the cell.
pub fn polygon_deleted(m: &NavMesh, polygon: usize, others: &[&NavMesh]) -> Done<NavMesh> {
    deletion(m, polygon, others).map(|r| r.mesh)
}

/// Cell `m` with polygon proper `polygon` deleted and its side records chained again.
pub fn deletion(m: &NavMesh, polygon: usize, others: &[&NavMesh]) -> Done<NavRebuild> {
    rebuilt(m, polygon, true, others)
}

/// Cell `m` with polygon proper `polygon` cut off and its side records chained again.
pub fn disabling(m: &NavMesh, polygon: usize, others: &[&NavMesh]) -> Done<NavRebuild> {
    rebuilt(m, polygon, false, others)
}

/// Cell `m` with polygon proper `p` deleted where `gone`, else every edge out of it and into it
/// leading nowhere; each side record naming it chained again, and gone where it then names none.
fn rebuilt(m: &NavMesh, p: usize, gone: bool, others: &[&NavMesh]) -> Done<NavRebuild> {
    let records = m.polygons.len();
    if p >= m.polygons_proper.min(records) {
        return Err(NavDeleteError::NotProper {
            polygon: p,
            proper: m.polygons_proper,
        });
    }
    let number = p as u16;
    let mut unresolved: Vec<(usize, u16)> = Vec::new();
    let mut runs: Vec<Vec<Corner>> = Vec::with_capacity(records);
    for r in &m.polygons {
        let first = r.first as usize;
        let mut run = Vec::with_capacity(usize::from(r.corners));
        for j in first..first + usize::from(r.corners) {
            let (Some(&vertex), Some(&edge)) = (m.indices.get(j), m.edges.get(j)) else {
                return Err(HeaderlessError::IndexPastCount {
                    what: "polygon corner",
                    index: j as u64,
                    count: m.indices.len() as u64,
                }
                .into());
            };
            let to = m.neighbour(j);
            if edge.mesh == m.cell && to.is_none() {
                unresolved.push((j, edge.polygon));
            }
            run.push(Corner {
                vertex,
                edge,
                to,
                stored: j,
            });
        }
        runs.push(run);
    }
    if let Some(&(j, _)) = unresolved.iter().find(|(_, n)| gone || *n == number) {
        return Err(NavDeleteError::Ambiguous {
            what: "edge",
            index: j,
        });
    }

    let mut keep = vec![true; records];
    keep[p] = !gone;
    for (q, run) in runs.iter_mut().enumerate() {
        if q == p {
            if !gone {
                run.iter_mut()
                    .filter(|c| !c.leads_nowhere())
                    .for_each(Corner::cut);
            }
            continue;
        }
        let Some(corner) = run.iter().position(|c| c.to == Some(p)) else {
            continue;
        };
        if q < m.polygons_proper {
            if gone {
                return Err(NavDeleteError::EdgeInto {
                    from: q,
                    corner,
                    polygon: p,
                });
            }
            run.iter_mut()
                .filter(|c| c.to == Some(p))
                .for_each(Corner::cut);
        } else if !rechained(run, p) {
            keep[q] = false;
        }
    }
    let removed: Vec<usize> = (0..records).filter(|q| !keep[*q]).collect();
    if let (Some(&(j, _)), false) = (unresolved.first(), removed.is_empty()) {
        return Err(NavDeleteError::Ambiguous {
            what: "edge",
            index: j,
        });
    }
    for (q, run) in runs.iter().enumerate().filter(|(q, _)| keep[*q]) {
        if let Some(t) = run.iter().find_map(|c| c.to.filter(|t| !keep[*t])) {
            return Err(NavDeleteError::SideRecordNamed { record: t, from: q });
        }
    }
    let mut new_of = vec![None; records];
    let mut next = 0usize;
    for (q, n) in new_of.iter_mut().enumerate() {
        if keep[q] {
            *n = Some(next);
            next += 1;
        }
    }

    // Each special link's own-cell ends, resolved where a record goes; a link with an end in a
    // record that goes goes with it, and so do its copies.
    let mut link_ends: Vec<[Option<usize>; 2]> = Vec::new();
    let mut link_kept = vec![true; m.special_links.len()];
    if !removed.is_empty() {
        for (i, l) in m.special_links.iter().enumerate() {
            let mut at = [None, None];
            let both = [
                (l.from_mesh, l.from_polygon, l.from),
                (l.to_mesh, l.to_polygon, l.to),
            ];
            for (k, (mesh, id, end)) in both.into_iter().enumerate() {
                if mesh != m.cell {
                    continue;
                }
                let t = (m.link_polygon(mesh, id, end)).ok_or(NavDeleteError::Ambiguous {
                    what: "special link",
                    index: i,
                })?;
                link_kept[i] &= keep[t];
                at[k] = Some(t);
            }
            if link_kept[i] {
                link_ends.push(at);
            }
        }
    }

    let mut used = vec![false; m.vertices.len()];
    for (_, run) in runs.iter().enumerate().filter(|(q, _)| keep[*q]) {
        for c in run {
            if let Some(u) = used.get_mut(usize::from(c.vertex)) {
                *u = true;
            }
        }
    }
    let mut below = 0usize;
    let moved: Vec<usize> = (used.iter())
        .map(|u| {
            let at = below;
            below += usize::from(*u);
            at
        })
        .collect();
    let vertices: Vec<[u16; 3]> = (m.vertices.iter().zip(&used))
        .filter(|(_, u)| **u)
        .map(|(v, _)| *v)
        .collect();
    let (mut indices, mut edges) = (Vec::new(), Vec::new());
    let (mut expect, mut from_edges) = (Vec::new(), Vec::new());
    let (mut polygons, mut from_records) = (Vec::new(), Vec::new());
    for (q, run) in runs.iter().enumerate().filter(|(q, _)| keep[*q]) {
        let mut r = m.polygons[q];
        r.first = indices.len() as u32;
        r.corners = run.len() as u8;
        polygons.push(r);
        from_records.push(q);
        for c in run {
            let vertex = (moved.get(usize::from(c.vertex)).map(|n| *n as u16)).ok_or(
                HeaderlessError::IndexPastCount {
                    what: "polygon corner's vertex",
                    index: c.vertex.into(),
                    count: moved.len() as u64,
                },
            )?;
            indices.push(vertex);
            let mut e = c.edge;
            let to = c.to.and_then(|t| new_of[t]);
            if let Some(t) = to {
                e.polygon = t as u16;
            }
            edges.push(e);
            expect.push(to);
            from_edges.push(c.stored);
        }
    }
    for (what, empty) in [
        (LISTS[0].2, vertices.is_empty()),
        (LISTS[1].2, indices.is_empty()),
        (LISTS[3].2, polygons.is_empty()),
    ] {
        if empty {
            return Err(NavDeleteError::ChunkEmpties { list: what });
        }
    }

    let mut special_links = m.special_links.clone();
    let mut special_link_indices = m.special_link_indices.clone();
    if !removed.is_empty() {
        special_links = (m.special_links.iter().zip(&link_kept))
            .filter(|(_, kept)| **kept)
            .map(|(l, _)| *l)
            .collect();
        for (l, at) in special_links.iter_mut().zip(&link_ends) {
            if let Some(t) = at[0] {
                l.from_polygon = new_of[t].unwrap_or(t) as u16;
            }
            if let Some(t) = at[1] {
                l.to_polygon = new_of[t].unwrap_or(t) as u16;
            }
        }
        let (list, words) =
            file(&special_links, polygons.len(), filing(m)).ok_or(NavDeleteError::IndexPast)?;
        special_link_indices = list;
        for (r, word) in polygons.iter_mut().zip(words) {
            r.word_20 = (r.word_20 & !RUN) | word;
        }
    }
    let tree = match (gone, records > NUMBERS) {
        (false, _) => m.tree.clone(),
        (true, false) => {
            let mut tree = m.tree.clone();
            drop_from(&mut tree, number);
            tree
        }
        (true, true) => leaves_without(m, &m.tree, p),
    };
    let want = NavMesh {
        flags: m.flags,
        cell: m.cell,
        build_id: m.build_id,
        tree,
        vertices,
        indices,
        edges,
        polygons,
        polygons_proper: m.polygons_proper - usize::from(gone),
        nodes: m.nodes,
        depth: m.depth,
        leaves: m.leaves,
        special_links,
        special_link_indices,
        footprints: m.footprints.clone(),
    };

    // Every number written resolves to the polygon it was written for.
    for (j, t) in expect.iter().enumerate() {
        if want.edges[j].mesh == want.cell && want.neighbour(j) != *t {
            return Err(NavDeleteError::Ambiguous {
                what: "edge",
                index: from_edges[j],
            });
        }
    }
    for (i, (l, at)) in want.special_links.iter().zip(&link_ends).enumerate() {
        let both = [
            (l.from_mesh, l.from_polygon, l.from, at[0]),
            (l.to_mesh, l.to_polygon, l.to, at[1]),
        ];
        for (mesh, id, end, t) in both {
            if t.is_some_and(|t| want.link_polygon(mesh, id, end) != new_of[t]) {
                return Err(NavDeleteError::Ambiguous {
                    what: "special link",
                    index: i,
                });
            }
        }
    }
    if gone && records > NUMBERS && !leaves_agree(m, &m.tree, &want, &want.tree, p) {
        return Err(NavDeleteError::Ambiguous {
            what: "a quadtree leaf after polygon",
            index: p,
        });
    }
    let made = NavRebuild {
        mesh: want,
        records: from_records,
        edges: from_edges,
    };
    // Another body of this same cell numbers its own polygons.
    for o in others.iter().filter(|o| o.cell != m.cell) {
        super::write_navmesh_links::relinked(o, m, &made)?;
    }
    Ok(made)
}

/// A split array as laid out: its header, its chunk rows and starts, and each chunk's place and
/// item count.
struct Split {
    header: usize,
    rows: usize,
    starts: usize,
    chunks: Vec<(usize, usize)>,
}

/// A quadtree leaf's payload, its id array and its count.
struct Leaf {
    payload: usize,
    ids: Option<usize>,
    count: usize,
}

/// Where a cell body's arrays lie, and every word its reader follows as a pointer.
struct Layout {
    pointers: Vec<usize>,
    lists: Vec<Split>,
    links: Option<usize>,
    link_indices: Option<usize>,
    leaves: Vec<Leaf>,
}

fn layout(virtual_section: &[u8]) -> Result<Layout> {
    sections(virtual_section, Some(&[]), |v, _| walk(v))
}

fn walk(v: Bytes) -> Result<Layout> {
    let mut pointers = vec![0x08, 0xA0, 0xA8, 0xB0, 0xB8];
    let mut lists = Vec::with_capacity(LISTS.len());
    for (slot, _, what) in LISTS {
        pointers.push(slot);
        let header = v.to(what, slot)?;
        pointers.extend([header + 0x10, header + 0x18]);
        let rows = v.to(what, header + 0x10)?;
        let starts = v.to(what, header + 0x18)?;
        let count = usize::try_from(v.u64(what, header + 0x20)?).unwrap_or(usize::MAX);
        v.get(what, rows, count.saturating_mul(16))?;
        let mut chunks = Vec::with_capacity(count);
        for i in 0..count {
            pointers.push(rows + 16 * i);
            let n = usize::try_from(v.u64(what, rows + 16 * i + 8)?).unwrap_or(usize::MAX);
            chunks.push((v.to(what, rows + 16 * i)?, n));
        }
        lists.push(Split {
            header,
            rows,
            starts,
            chunks,
        });
    }
    let mut leaves = Vec::new();
    let mut stack = vec![(v.to("quadtree", 0xA0)?, 0usize)];
    let mut nodes = 0;
    // ⚠ The node's pointers sit at 4 modulo 8.
    while let Some((at, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_NODES || depth > MAX_DEPTH {
            return Err(HeaderlessError::TreeTooLarge {
                nodes: MAX_NODES,
                depth: MAX_DEPTH,
            });
        }
        pointers.extend([0x2C, 0x34, 0x3C, 0x44, 0x4C].map(|o| at + o));
        if let Some(leaf) = v.ptr("quadtree leaf", at + 0x2C)? {
            pointers.push(leaf + 0x08);
            leaves.push(Leaf {
                payload: leaf,
                ids: v.ptr("leaf polygons", leaf + 0x08)?,
                count: usize::from(v.u16("leaf polygon count", leaf + 0x18)?),
            });
        }
        // Pushed last child first, so the walk takes them in the reader's order.
        for o in [0x4C, 0x44, 0x3C, 0x34] {
            if let Some(child) = v.ptr("quadtree child", at + o)? {
                stack.push((child, depth + 1));
            }
        }
    }
    Ok(Layout {
        pointers,
        lists,
        links: v.ptr("special links", 0xA8)?,
        link_indices: v.ptr("special link indices", 0xB0)?,
        leaves,
    })
}

/// The items of `list`, chunk after chunk.
fn items(v: &[u8], list: &Split, stride: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for &(at, n) in &list.chunks {
        let bytes = (n.checked_mul(stride))
            .and_then(|len| v.get(at..at.checked_add(len)?))
            .ok_or(HeaderlessError::PastEnd {
                what: "split array chunk",
                at: at as u64,
                section: "virtual",
                len: v.len() as u64,
            })?;
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

fn edge_bytes(e: &NavEdge) -> Vec<u8> {
    [
        &e.mesh.to_le_bytes()[..],
        &e.polygon.to_le_bytes(),
        &e.word_06.to_le_bytes(),
        &e.flags.to_le_bytes(),
    ]
    .concat()
}

/// The 32-byte records of `links`, each field where the reader takes it.
fn link_bytes(links: &[SpecialLink]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * links.len());
    for l in links {
        for q in l.from.into_iter().chain(l.to) {
            out.extend(q.to_le_bytes());
        }
        out.extend(l.from_polygon.to_le_bytes());
        out.extend(l.to_polygon.to_le_bytes());
        for word in [l.from_mesh, l.to_mesh, l.word_18, l.heading] {
            out.extend(word.to_le_bytes());
        }
    }
    out
}

/// Each record of `kept` as stored at its place in `from`, its +0x04 and +0x20 as `kept` holds them.
fn polygon_bytes(stored: &[u8], from: &[usize], kept: &[NavPolygon]) -> Done<Vec<u8>> {
    let records = stored.as_chunks::<RECORD>().0;
    let mut out = Vec::with_capacity(RECORD * kept.len());
    for (&q, p) in from.iter().zip(kept) {
        let record = records.get(q).ok_or(HeaderlessError::IndexPastCount {
            what: "stored polygon record",
            index: q as u64,
            count: records.len() as u64,
        })?;
        let mut record = record.to_vec();
        record[4..8].copy_from_slice(&((p.first << 4) | u32::from(p.corners)).to_le_bytes());
        record[0x20..0x24].copy_from_slice(&p.word_20.to_le_bytes());
        out.extend(record);
    }
    Ok(out)
}

/// `bytes` over the `old` bytes at `at`, the rest of them zeroed.
fn put_over(v: &mut [u8], at: usize, old: usize, bytes: &[u8], what: &'static str) -> Done<()> {
    if bytes.len() > old {
        return Err(NavDeleteError::Grows { what });
    }
    put(v, at, bytes)?;
    put(v, at + bytes.len(), &vec![0; old - bytes.len()])?;
    Ok(())
}

fn a16(n: usize) -> usize {
    n.next_multiple_of(16)
}

/// Whether two reads of a cell hold the same values.
fn same(a: &NavMesh, b: &NavMesh) -> bool {
    let prints = |m: &NavMesh| -> Vec<_> {
        (m.footprints.iter())
            .map(|f| (f.key, f.position.map(f32::to_bits), f.kind, f.outline))
            .collect()
    };
    (a.flags, a.cell, a.build_id) == (b.flags, b.cell, b.build_id)
        && a.tree == b.tree
        && a.vertices == b.vertices
        && a.indices == b.indices
        && a.edges == b.edges
        && a.polygons == b.polygons
        && a.polygons_proper == b.polygons_proper
        && (a.nodes, a.depth, a.leaves) == (b.nodes, b.depth, b.leaves)
        && a.special_links == b.special_links
        && a.special_link_indices == b.special_link_indices
        && prints(a) == prints(b)
}

/// [`deletion`] made over the cell `sections` hold, read back; a refusal changes nothing.
pub fn delete_polygon(
    sections: &mut Rpf9Sections,
    polygon: usize,
    others: &[&NavMesh],
) -> Done<NavMesh> {
    let m = read_navmesh(&sections.virtual_section)?;
    let made = deletion(&m, polygon, others)?;
    written(sections, &m, made)
}

/// [`disabling`] made over the cell `sections` hold, read back; a refusal changes nothing.
pub fn disable_polygon(
    sections: &mut Rpf9Sections,
    polygon: usize,
    others: &[&NavMesh],
) -> Done<NavMesh> {
    let m = read_navmesh(&sections.virtual_section)?;
    let made = disabling(&m, polygon, others)?;
    written(sections, &m, made)
}

/// `made`, the cell `m` that `sections` hold made again with fewer items, written over it.
// A split array keeps a chunk for each `16384 / stride` items: one left empty goes with its row and
// its start.
pub(super) fn written(sections: &mut Rpf9Sections, m: &NavMesh, made: NavRebuild) -> Done<NavMesh> {
    let want = made.mesh;
    let mut s = sections.clone();
    let before = layout(&s.virtual_section)?;
    let stored = items(&s.virtual_section, &before.lists[POLYGONS], RECORD)?;
    let lists: [Vec<u8>; 4] = [
        (want.vertices.iter().flatten())
            .flat_map(|w| w.to_le_bytes())
            .collect(),
        want.indices.iter().flat_map(|i| i.to_le_bytes()).collect(),
        want.edges.iter().flat_map(edge_bytes).collect(),
        polygon_bytes(&stored, &made.records, &want.polygons)?,
    ];
    let (old_list, new_list) = (
        2 * m.special_link_indices.len(),
        2 * want.special_link_indices.len(),
    );
    if old_list == 0 && new_list > 0 {
        return Err(NavDeleteError::IndexListAppears {
            from: old_list / 2,
            to: new_list / 2,
        });
    }
    // An index list that grows takes its room at its allocation's end first.
    if let Some(at) = before.link_indices {
        let (end, grown) = (a16(at + old_list), a16(at + new_list));
        if grown > end {
            let by = (grown - end) as isize;
            grow_section(&mut s, &before.pointers, Rpf9Section::Virtual, end, by)?;
        }
    }

    let l = layout(&s.virtual_section)?;
    let v = &mut s.virtual_section;
    // Each array's start, the bytes it held and the bytes it holds now.
    let mut shrunk: Vec<(usize, usize, usize)> = Vec::new();
    for (k, (list, bytes)) in l.lists.iter().zip(&lists).enumerate() {
        let (_, stride, what) = LISTS[k];
        let capacity = CHUNK / stride;
        let full = capacity * stride;
        let held = (bytes.len() / stride).div_ceil(capacity);
        let chunks = list.chunks.len();
        if held == 0 {
            return Err(NavDeleteError::ChunkEmpties { list: what });
        }
        if held > chunks {
            return Err(NavDeleteError::Grows { what });
        }
        for (i, &(at, n)) in list.chunks.iter().enumerate() {
            let part = bytes
                .get(i * full..((i + 1) * full).min(bytes.len()))
                .unwrap_or_default();
            let old = n.saturating_mul(stride);
            put_over(v, at, old, part, what)?;
            if i + 1 == held {
                let count = (part.len() / stride) as u64;
                put(v, list.rows + 16 * i + 8, &count.to_le_bytes())?;
            }
            if i + 1 >= held {
                shrunk.push((at, old, part.len()));
            }
        }
        if held < chunks {
            put_over(v, list.rows + 16 * held, 16 * (chunks - held), &[], what)?;
            put_over(v, list.starts + 4 * held, 4 * (chunks - held), &[], what)?;
            put(v, list.header + 0x20, &(held as u64).to_le_bytes())?;
            shrunk.push((list.rows, 16 * chunks, 16 * held));
            shrunk.push((list.starts, 4 * chunks, 4 * held));
        }
        let count = (bytes.len() / stride) as u64;
        put(v, list.header + 0x08, &count.to_le_bytes())?;
    }
    let total = 6 * want.vertices.len()
        + 14 * want.indices.len()
        + 44 * want.polygons.len()
        + 32 * want.special_links.len();
    for (at, word) in [
        (0x90, want.indices.len()),
        (0xC0, want.vertices.len()),
        (0xC4, want.polygons.len()),
        (0xCC, total),
        (0xD8, want.special_link_indices.len()),
    ] {
        put(v, at, &(word as u32).to_le_bytes())?;
    }
    if let Some(at) = l.links {
        let (old, bytes) = (32 * m.special_links.len(), link_bytes(&want.special_links));
        put_over(v, at, old, &bytes, "special links")?;
        put(v, 0xD4, &(want.special_links.len() as u32).to_le_bytes())?;
        // An emptied link list is null with count 0, as in every cell with no link.
        if bytes.is_empty() {
            put(v, 0xA8, &0u64.to_le_bytes())?;
        }
        shrunk.push((at, old, bytes.len()));
    }
    if let Some(at) = l.link_indices {
        let bytes: Vec<u8> = (want.special_link_indices.iter())
            .flat_map(|i| i.to_le_bytes())
            .collect();
        put_over(
            v,
            at,
            old_list.max(new_list),
            &bytes,
            "special link indices",
        )?;
        // An emptied index list is null with count 0, as in every cell with no link.
        if new_list == 0 {
            put(v, 0xB0, &0u64.to_le_bytes())?;
        }
        shrunk.push((at, old_list, new_list));
    }
    let mut kept = Vec::new();
    leaf_ids(&want.tree, &mut kept);
    if kept.len() != l.leaves.len() {
        return Err(NavDeleteError::ReadBack(format!(
            "the body walks {} leaves where the cell holds {}",
            l.leaves.len(),
            kept.len()
        )));
    }
    for (leaf, ids) in l.leaves.iter().zip(kept) {
        // A null id array holds no ids.
        let Some(at) = leaf.ids else { continue };
        let bytes: Vec<u8> = ids.iter().flat_map(|i| i.to_le_bytes()).collect();
        let old = 2 * leaf.count;
        put_over(v, at, old, &bytes, "leaf polygons")?;
        put(v, leaf.payload + 0x18, &(ids.len() as u16).to_le_bytes())?;
        // An emptied leaf holds count 0 and a null pointer, as every shipped one does.
        if ids.is_empty() {
            put(v, leaf.payload + 0x08, &0u64.to_le_bytes())?;
        }
        shrunk.push((at, old, bytes.len()));
    }
    read_navmesh(&s.virtual_section).map_err(|e| NavDeleteError::ReadBack(e.to_string()))?;

    // Highest first, so a removal moves no array still to shrink.
    shrunk.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
    for (at, old, new) in shrunk {
        let (keep, end) = (a16(at + new), a16(at + old));
        // An array that ends the section past a 16-byte step keeps its zeroed tail.
        if end <= keep || end > s.virtual_section.len() {
            continue;
        }
        let pointers = layout(&s.virtual_section)?.pointers;
        let by = -((end - keep) as isize);
        grow_section(&mut s, &pointers, Rpf9Section::Virtual, keep, by)?;
    }

    let again =
        read_navmesh(&s.virtual_section).map_err(|e| NavDeleteError::ReadBack(e.to_string()))?;
    if !same(&again, &want) {
        return Err(NavDeleteError::ReadBack(
            "a value of the cell reads back otherwise than the deletion made it".to_string(),
        ));
    }
    *sections = s;
    Ok(again)
}

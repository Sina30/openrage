//! A navigation polygon proper deleted from its cell's body: its record, corners and edges and
//! every vertex only it named go, the records after it are renumbered wherever this body names
//! them, and the arrays that shrank give their bytes back by the resource rule.
//!
//! ⚠ Other bodies name this cell's polygons by number in their special links and are saved apart:
//! a deletion that would renumber one of those is refused, not written.

use std::collections::HashSet;

use crate::container::rpf::{grow_section, Rpf9GrowError, Rpf9Section, Rpf9Sections};

use super::bytes::{sections, Bytes};
use super::write_physics::put;
use super::{
    read_navmesh, HeaderlessError, NavEdge, NavMesh, NavNode, NavPolygon, Result, SpecialLink,
};

/// The bytes of one split array chunk, as the reader takes them.
const CHUNK: usize = 16384;
/// A polygon record's bytes.
const RECORD: usize = 44;
/// Polygon +0x20's link run: the count in bits 0-5 and the start in bits 6-21.
const RUN: u32 = 0x3F_FFFF;
/// The most entries a link run's six bits count.
const RUN_MOST: usize = 63;
/// The records a 16-bit polygon number tells apart.
const NUMBERS: usize = 1 << 16;
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
    #[error("polygon {polygon} is not one of the cell's {proper} polygons proper: a side record goes only with the side records rebuilt, which is not done")]
    NotProper { polygon: usize, proper: usize },
    #[error("the cell holds {records} polygon records, past the 65,536 a 16-bit number tells apart, so a renumbered number could name another polygon")]
    TooMany { records: usize },
    #[error(
        "a body of no cell is not deleted in: which bodies' special links end in it is not stated"
    )]
    NoCell,
    #[error("polygon {from}'s edge at corner {corner} still leads into polygon {polygon}: disable it first")]
    EdgeInto {
        from: usize,
        corner: usize,
        polygon: usize,
    },
    #[error("side record {record}'s corner-1 edge names polygon {polygon}, and no shipped corner-1 edge leads nowhere, so it is not cut and the polygon stays")]
    SideRecordInto { record: usize, polygon: usize },
    #[error("special link {link} of this cell ends in polygon {polygon}, and no special link is deleted")]
    LinkAt { link: usize, polygon: usize },
    #[error("special link {link} of cell {cell}, a body saved apart, ends in this cell's polygon number {number}, which the deletion removes or renumbers")]
    NeighbourLink { cell: u32, link: usize, number: u16 },
    #[error("the special link indices and the polygons' runs follow no form of the rule that files each link under the polygons its ends number, so they are not rebuilt")]
    IndexRule,
    #[error(
        "the {list} would lose its last chunk, and a split array that loses a chunk is not written"
    )]
    ChunkEmpties { list: &'static str },
    #[error("the special link indices would go from {from} entries to {to}, and an array that appears or empties is not written")]
    IndexListAppears { from: usize, to: usize },
    #[error("the {what} would grow in place")]
    Grows { what: &'static str },
    #[error("the body does not read back as the deletion: {0}")]
    ReadBack(String),
}

type Done<T> = std::result::Result<T, NavDeleteError>;

/// A link's two ends, each its mesh and its polygon number.
fn ends(l: &SpecialLink) -> [(u32, u16); 2] {
    [(l.from_mesh, l.from_polygon), (l.to_mesh, l.to_polygon)]
}

/// How a body files its links: under the to number as well as the from number, and whether an
/// empty run starts at the running total or at 0.
#[derive(Debug, Clone, Copy)]
struct Filing {
    both: bool,
    empty_at_total: bool,
}

/// The link index list and each record's +0x20 bits 0-21 `filing` gives `links` over `records`
/// records: under each record, in link order, the first of each set of equal links that a filed
/// end numbers, once, at most 63; `None` where an index or a start passes 16 bits.
fn file(links: &[SpecialLink], records: usize, filing: Filing) -> Option<(Vec<u16>, Vec<u32>)> {
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

/// The [`Filing`] that gives back the link index list and runs `m` holds.
fn filing(m: &NavMesh) -> Done<Filing> {
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
                return Ok(f);
            }
        }
    }
    Err(NavDeleteError::IndexRule)
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
    let p = polygon;
    let records = m.polygons.len();
    if p >= m.polygons_proper.min(records) {
        return Err(NavDeleteError::NotProper {
            polygon: p,
            proper: m.polygons_proper,
        });
    }
    if records > NUMBERS {
        return Err(NavDeleteError::TooMany { records });
    }
    if m.cell == 0 {
        return Err(NavDeleteError::NoCell);
    }
    // Below 65,537 records an edge's number is its record.
    for (q, record) in m.polygons.iter().enumerate().filter(|(q, _)| *q != p) {
        for k in 0..usize::from(record.corners) {
            let into = (m.edges.get(record.first as usize + k))
                .is_some_and(|e| e.mesh == m.cell && usize::from(e.polygon) == p);
            if !into {
                continue;
            }
            return Err(match q >= m.polygons_proper && k == 1 {
                true => NavDeleteError::SideRecordInto {
                    record: q,
                    polygon: p,
                },
                false => NavDeleteError::EdgeInto {
                    from: q,
                    corner: k,
                    polygon: p,
                },
            });
        }
    }
    let number = p as u16;
    let at_p = |l: &SpecialLink| ends(l).contains(&(m.cell, number));
    if let Some(link) = m.special_links.iter().position(at_p) {
        return Err(NavDeleteError::LinkAt { link, polygon: p });
    }
    // Another body of this same cell numbers its own polygons.
    for o in others.iter().filter(|o| o.cell != m.cell) {
        for (link, l) in o.special_links.iter().enumerate() {
            let renumbered = |(mesh, n): &(u32, u16)| *mesh == m.cell && usize::from(*n) >= p;
            if let Some((_, n)) = ends(l).into_iter().find(renumbered) {
                return Err(NavDeleteError::NeighbourLink {
                    cell: o.cell,
                    link,
                    number: n,
                });
            }
        }
    }
    let form = filing(m)?;
    let first = m.polygons[p].first as usize;
    let run = first..first + usize::from(m.polygons[p].corners);

    let mut used = vec![false; m.vertices.len()];
    for (j, i) in m.indices.iter().enumerate() {
        if run.contains(&j) {
            continue;
        }
        if let Some(u) = used.get_mut(usize::from(*i)) {
            *u = true;
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
    let indices = (m.indices.iter().enumerate())
        .filter(|(j, _)| !run.contains(j))
        .map(|(_, i)| {
            (moved.get(usize::from(*i)).map(|n| *n as u16)).ok_or(HeaderlessError::IndexPastCount {
                what: "polygon corner's vertex",
                index: (*i).into(),
                count: moved.len() as u64,
            })
        })
        .collect::<Result<Vec<u16>>>()?;
    let edge_count = m.edges.len().saturating_sub(run.len());
    for (list, stride, old, new) in [
        (LISTS[0].2, LISTS[0].1, m.vertices.len(), vertices.len()),
        (LISTS[1].2, LISTS[1].1, m.indices.len(), indices.len()),
        (LISTS[2].2, LISTS[2].1, m.edges.len(), edge_count),
        (LISTS[3].2, LISTS[3].1, records, records - 1),
    ] {
        let capacity = CHUNK / stride;
        if new.div_ceil(capacity) != old.div_ceil(capacity) {
            return Err(NavDeleteError::ChunkEmpties { list });
        }
    }

    let renumber = |n: u16| match usize::from(n) > p {
        true => n - 1,
        false => n,
    };
    let edges: Vec<NavEdge> = (m.edges.iter().enumerate())
        .filter(|(j, _)| !run.contains(j))
        .map(|(_, e)| {
            let mut e = *e;
            if e.mesh == m.cell {
                e.polygon = renumber(e.polygon);
            }
            e
        })
        .collect();
    let mut next = 0u32;
    let mut polygons: Vec<NavPolygon> = Vec::with_capacity(records - 1);
    for (q, r) in m.polygons.iter().enumerate() {
        if q == p {
            continue;
        }
        let mut r = *r;
        r.first = next;
        next += u32::from(r.corners);
        polygons.push(r);
    }
    let special_links: Vec<SpecialLink> = (m.special_links.iter())
        .map(|l| {
            let mut l = *l;
            if l.from_mesh == m.cell {
                l.from_polygon = renumber(l.from_polygon);
            }
            if l.to_mesh == m.cell {
                l.to_polygon = renumber(l.to_polygon);
            }
            l
        })
        .collect();
    let (special_link_indices, runs) =
        file(&special_links, polygons.len(), form).ok_or(NavDeleteError::IndexRule)?;
    for (r, run) in polygons.iter_mut().zip(runs) {
        r.word_20 = (r.word_20 & !RUN) | run;
    }
    let mut tree = m.tree.clone();
    drop_from(&mut tree, number);
    Ok(NavMesh {
        flags: m.flags,
        cell: m.cell,
        build_id: m.build_id,
        tree,
        vertices,
        indices,
        edges,
        polygons,
        polygons_proper: m.polygons_proper - 1,
        nodes: m.nodes,
        depth: m.depth,
        leaves: m.leaves,
        special_links,
        special_link_indices,
        footprints: m.footprints.clone(),
    })
}

/// A split array as laid out: its header, its chunk rows and each chunk's place and item count.
struct Split {
    header: usize,
    rows: usize,
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

/// The stored records but record `gone`, each kept with its words but +0x04 and +0x20 as `kept`.
fn polygon_bytes(stored: &[u8], gone: usize, kept: &[NavPolygon]) -> Vec<u8> {
    let mut out = Vec::with_capacity(stored.len());
    let records = (stored.as_chunks::<RECORD>().0.iter().enumerate())
        .filter(|(q, _)| *q != gone)
        .map(|(_, r)| r);
    for (record, p) in records.zip(kept) {
        let mut record = record.to_vec();
        record[4..8].copy_from_slice(&((p.first << 4) | u32::from(p.corners)).to_le_bytes());
        record[0x20..0x24].copy_from_slice(&p.word_20.to_le_bytes());
        out.extend(record);
    }
    out
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

/// [`polygon_deleted`] made over the cell `sections` hold, read back; a refusal changes nothing.
pub fn delete_polygon(
    sections: &mut Rpf9Sections,
    polygon: usize,
    others: &[&NavMesh],
) -> Done<NavMesh> {
    let m = read_navmesh(&sections.virtual_section)?;
    let want = polygon_deleted(&m, polygon, others)?;
    let mut s = sections.clone();
    let before = layout(&s.virtual_section)?;
    let stored = items(&s.virtual_section, &before.lists[POLYGONS], RECORD)?;
    let lists: [Vec<u8>; 4] = [
        (want.vertices.iter().flatten())
            .flat_map(|w| w.to_le_bytes())
            .collect(),
        want.indices.iter().flat_map(|i| i.to_le_bytes()).collect(),
        want.edges.iter().flat_map(edge_bytes).collect(),
        polygon_bytes(&stored, polygon, &want.polygons),
    ];
    let (old_list, new_list) = (
        2 * m.special_link_indices.len(),
        2 * want.special_link_indices.len(),
    );
    if (old_list == 0) != (new_list == 0) {
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
        let full = CHUNK / stride * stride;
        let last = list.chunks.len().saturating_sub(1);
        for (i, &(at, n)) in list.chunks.iter().enumerate() {
            let part = bytes
                .get(i * full..((i + 1) * full).min(bytes.len()))
                .unwrap_or_default();
            let old = n.saturating_mul(stride);
            put_over(v, at, old, part, what)?;
            if i == last {
                let count = (part.len() / stride) as u64;
                put(v, list.rows + 16 * i + 8, &count.to_le_bytes())?;
                shrunk.push((at, old, part.len()));
            }
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
        for (i, link) in want.special_links.iter().enumerate() {
            put(v, at + 32 * i + 0x0C, &link.from_polygon.to_le_bytes())?;
            put(v, at + 32 * i + 0x0E, &link.to_polygon.to_le_bytes())?;
        }
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

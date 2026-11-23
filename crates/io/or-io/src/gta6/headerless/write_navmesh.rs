//! A navigation polygon disabled in place, as shipped cells hold an isolated polygon: the edges out
//! of it and into it written as leading nowhere. Nothing moves and no count changes; its flags, the
//! leaves and the special links that name it are kept.

use super::bytes::sections;
use super::write_physics::{items, put, refuse};
use super::{read_navmesh, HeaderlessError, NavMesh, Result, SpecialLink, NOWHERE};

/// An adjacency record's bytes.
const EDGE: usize = 12;
/// The polygon number an edge that leads nowhere names.
const NO_POLYGON: u16 = 0xFFFF;

/// What disabling a polygon proper cuts, and what still reaches it after.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolygonCut {
    /// The adjacency records, ascending, that lead out of the polygon or into it.
    pub edges: Vec<usize>,
    /// The side records whose corner-1 edge names the polygon, kept: no shipped one leads nowhere.
    pub side_records: Vec<usize>,
    /// This cell's special links with an end in the polygon, a copy of an earlier link left out.
    pub links: Vec<usize>,
}

/// What disabling polygon proper `polygon` of `m` cuts and what it leaves naming it.
pub fn polygon_cut(m: &NavMesh, polygon: usize) -> Result<PolygonCut> {
    if polygon >= m.polygons_proper {
        return Err(refuse(
            "only a polygon proper is disabled: a side record is the cell's border",
        ));
    }
    // An edge names a polygon by its number modulo 65,536.
    let number = polygon as u16;
    let mut cut = PolygonCut::default();
    for (q, record) in m.polygons.iter().enumerate() {
        let first = record.first as usize;
        for k in 0..usize::from(record.corners) {
            let j = first + k;
            let Some(e) = m.edges.get(j) else {
                continue;
            };
            if e.mesh == NOWHERE {
                continue;
            }
            if q != polygon {
                if e.mesh != m.cell || e.polygon != number {
                    continue;
                }
                match m.neighbour(j) {
                    Some(t) if t == polygon => {}
                    Some(_) => continue,
                    None => {
                        return Err(refuse(
                            "an edge names the polygon's number modulo 65,536 and its corners do not tell which polygon it leads to",
                        ))
                    }
                }
                if q >= m.polygons_proper && k == 1 {
                    cut.side_records.push(q);
                    continue;
                }
            }
            cut.edges.push(j);
        }
    }
    let at = |mesh: u32, id: u16, end: [u16; 3]| m.link_polygon(mesh, id, end) == Some(polygon);
    cut.links = canonical(&m.special_links, |l| {
        at(l.from_mesh, l.from_polygon, l.from) || at(l.to_mesh, l.to_polygon, l.to)
    });
    Ok(cut)
}

/// The special links of `other`, a neighbouring cell, with an end named polygon `polygon` of `cell`.
// An end is named by its mesh and its number modulo 65,536.
pub fn links_into(other: &NavMesh, cell: u32, polygon: usize) -> Vec<usize> {
    let number = polygon as u16;
    canonical(&other.special_links, |l| {
        (l.from_mesh == cell && l.from_polygon == number)
            || (l.to_mesh == cell && l.to_polygon == number)
    })
}

/// The links `keep` holds, by index, each once: a record equal to an earlier one is its copy.
fn canonical(links: &[SpecialLink], keep: impl Fn(&SpecialLink) -> bool) -> Vec<usize> {
    let mut seen: Vec<&SpecialLink> = Vec::new();
    let mut out = Vec::new();
    for (i, l) in links.iter().enumerate() {
        if keep(l) && !seen.contains(&l) {
            seen.push(l);
            out.push(i);
        }
    }
    out
}

/// The adjacency records `edges` written as leading nowhere, as every shipped such edge is.
// Mesh 0xFFFF_FFFF, polygon 0xFFFF and the low byte of +0x06 cleared, its high byte and +0x08 kept.
pub fn cut_edges(virtual_section: &mut [u8], edges: &[usize]) -> Result<()> {
    let read = read_navmesh(virtual_section)?;
    let places = sections(virtual_section, Some(&[]), |v, _| items(v, 0x88, EDGE))?;
    if places.len() != read.edges.len() {
        return Err(refuse("the cell holds another number of edges"));
    }
    let corner_one: Vec<usize> = (read.polygons.iter().skip(read.polygons_proper))
        .map(|p| p.first as usize + 1)
        .collect();
    for &j in edges {
        let (Some(&at), Some(e)) = (places.get(j), read.edges.get(j)) else {
            return Err(HeaderlessError::IndexPastCount {
                what: "edge to cut",
                index: j as u64,
                count: read.edges.len() as u64,
            });
        };
        if corner_one.contains(&j) {
            return Err(refuse(
                "a side record's corner-1 edge leads to a polygon proper in every shipped cell",
            ));
        }
        put(virtual_section, at, &NOWHERE.to_le_bytes())?;
        put(virtual_section, at + 4, &NO_POLYGON.to_le_bytes())?;
        put(virtual_section, at + 6, &(e.word_06 & 0xFF00).to_le_bytes())?;
    }
    read_navmesh(virtual_section).map(|_| ())
}

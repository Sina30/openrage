//! The special links of a body of another cell made again once an edit renumbers a cell's records:
//! an end in a record that went takes its link with it, an end in one after it takes its new
//! number, and the index list and every record's run are filed again as the body files them.
//!
//! ⚠ A neighbour's end is told apart among the records its number names modulo 65,536 by its
//! position, dequantised in the box of the body that holds the link.

use crate::container::rpf::Rpf9Sections;

use super::write_navmesh_delete::{
    file, filing, names_from, written, Done, NavDeleteError, NavRebuild, NUMBERS, RUN,
};
use super::{read_navmesh, NavMesh, SpecialLink};

/// The record of `cell` number `n` names whose box holds `point`, where it names more than one.
fn resolved(cell: &NavMesh, n: u16, point: [f32; 3]) -> Option<usize> {
    let candidates: Vec<usize> = (usize::from(n)..cell.polygons.len())
        .step_by(NUMBERS)
        .collect();
    if let [one] = candidates[..] {
        return Some(one);
    }
    let mut holding = candidates.into_iter().filter(|p| {
        let b = cell.polygons[*p].min_max;
        (0..3).all(|a| (f32::from(b[2 * a])..=f32::from(b[2 * a + 1])).contains(&point[a]))
    });
    match (holding.next(), holding.next()) {
        (Some(p), None) => Some(p),
        _ => None,
    }
}

/// The special links of `other`, a body of another cell, once `made` made cell `m` again.
// `None` where none changes.
pub fn relinked(other: &NavMesh, m: &NavMesh, made: &NavRebuild) -> Done<Option<Vec<SpecialLink>>> {
    let records = m.polygons.len();
    let mut new_of = vec![None; records];
    for (n, &q) in made.records.iter().enumerate() {
        if let Some(slot) = new_of.get_mut(q) {
            *slot = Some(n);
        }
    }
    let Some(from) = (0..records).find(|q| new_of[*q] != Some(*q)) else {
        return Ok(None);
    };
    if other.cell == m.cell {
        return Ok(None);
    }
    let mut links = Vec::with_capacity(other.special_links.len());
    let mut changed = false;
    for (i, l) in other.special_links.iter().enumerate() {
        let ambiguous = || NavDeleteError::Ambiguous {
            what: "neighbouring cell's special link",
            index: i,
        };
        let ends = [
            (l.from_mesh, l.from_polygon, l.from),
            (l.to_mesh, l.to_polygon, l.to),
        ];
        let mut numbers = [l.from_polygon, l.to_polygon];
        let mut kept = true;
        for (k, (mesh, n, end)) in ends.into_iter().enumerate() {
            if mesh != m.cell || !names_from(n, records, from) {
                continue;
            }
            let point = other.position(end);
            let t = resolved(m, n, point).ok_or_else(ambiguous)?;
            let Some(q) = new_of[t] else {
                kept = false;
                continue;
            };
            let number = q as u16;
            if resolved(&made.mesh, number, point) != Some(q) {
                return Err(ambiguous());
            }
            numbers[k] = number;
        }
        if !kept {
            changed = true;
            continue;
        }
        changed |= numbers != [l.from_polygon, l.to_polygon];
        links.push(SpecialLink {
            from_polygon: numbers[0],
            to_polygon: numbers[1],
            ..*l
        });
    }
    Ok(changed.then_some(links))
}

/// `m` with `links` in place of its special links, the index list and the runs filed again.
pub fn with_links(m: &NavMesh, links: Vec<SpecialLink>) -> Done<NavMesh> {
    let (special_link_indices, words) =
        file(&links, m.polygons.len(), filing(m)).ok_or(NavDeleteError::IndexPast)?;
    let mut polygons = m.polygons.clone();
    for (r, word) in polygons.iter_mut().zip(words) {
        r.word_20 = (r.word_20 & !RUN) | word;
    }
    Ok(NavMesh {
        flags: m.flags,
        cell: m.cell,
        build_id: m.build_id,
        tree: m.tree.clone(),
        vertices: m.vertices.clone(),
        indices: m.indices.clone(),
        edges: m.edges.clone(),
        polygons,
        polygons_proper: m.polygons_proper,
        nodes: m.nodes,
        depth: m.depth,
        leaves: m.leaves,
        special_links: links,
        special_link_indices,
        footprints: m.footprints.clone(),
    })
}

/// [`with_links`] written over the body `sections` hold, read back.
// A refusal changes nothing, and the links it holds already change no byte.
pub fn write_links(sections: &mut Rpf9Sections, links: Vec<SpecialLink>) -> Done<NavMesh> {
    let m = read_navmesh(&sections.virtual_section)?;
    if m.special_links == links {
        return Ok(m);
    }
    let made = NavRebuild {
        mesh: with_links(&m, links)?,
        records: (0..m.polygons.len()).collect(),
        edges: (0..m.edges.len()).collect(),
    };
    written(sections, &m, made)
}

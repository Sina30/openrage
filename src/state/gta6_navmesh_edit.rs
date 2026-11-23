//! A scanned GTA VI cell's polygon disabled as the cell's own form of an isolated polygon, one step
//! of undo: the edges out of it and into it cut in the view's model, and a save writing each edge
//! cut there over the body read again. A side record naming it is chained again, as a deletion is.

use std::sync::Arc;

use or_io::gta5::navmesh::NO_NEIGHBOUR;
use or_io::gta5::{AiSplitArray, CNavMesh, TAdjPoly};
use or_io::gta6::headerless::{
    cut_edges, links_into, polygon_cut, read_navmesh, NavMesh, PolygonCut,
};

use super::edit_history::Change;
use super::{AppState, DocPayload, DocumentId};

type Edges = Option<Box<AiSplitArray<TAdjPoly>>>;

/// The lookup slot of a view's edge to a polygon its fifteen bits cannot name (past them, or one
/// of several its number names modulo 65,536): it leads nowhere there and is still open, so a
/// disable can cut it.
const PAST: usize = 1;

/// Whether the cell's edge at `index` leads to a polygon of the cell: no edge names another cell.
fn leads_somewhere(m: &NavMesh, index: usize) -> bool {
    m.edges.get(index).is_some_and(|e| e.mesh == m.cell)
}

/// The view's edge for the cell's edge at `index`.
pub(super) fn view_edge(m: &NavMesh, index: usize) -> Result<TAdjPoly, or_io::decl::OwnedError> {
    let polygon = super::gta6_navmesh::view_neighbour(m, index);
    match polygon == NO_NEIGHBOUR && leads_somewhere(m, index) {
        true => TAdjPoly::leading_to(PAST, NO_NEIGHBOUR),
        false => TAdjPoly::leading_to(0, polygon),
    }
}

/// Whether a view's edge leads anywhere, past the fifteen bits included.
fn is_open(edge: &TAdjPoly) -> bool {
    let words = |e: &TAdjPoly| (e.as_integer1, e.as_integer2);
    let past = TAdjPoly::leading_to(PAST, NO_NEIGHBOUR).ok();
    edge.neighbour().is_some() || past.is_some_and(|p| words(&p) == words(edge))
}

fn edges_mut(payload: &mut DocPayload) -> Option<&mut Edges> {
    match payload {
        DocPayload::Resource(or_io::Asset::NavMesh(mesh)) => Some(&mut mesh.adjacent_polys_array),
        _ => None,
    }
}

/// Indices as a person reads a list of them.
fn listed(items: &[usize]) -> String {
    let items: Vec<String> = items.iter().map(usize::to_string).collect();
    items.join(", ")
}

impl AppState {
    fn gta6_cell(&self, doc: DocumentId) -> Option<(std::path::PathBuf, Arc<NavMesh>)> {
        let path = self.document(doc)?.path.clone()?;
        let cell = super::gta6_navmesh::scanned_cell(&path)?;
        Some((path, cell))
    }

    /// Polygon `polygon` of the scanned GTA VI cell `doc` cut off, as one step; what it did and
    /// what still reaches it, or why not. `None` for any other document.
    pub(super) fn disable_gta6_polygon(
        &mut self,
        doc: DocumentId,
        polygon: usize,
    ) -> Option<Result<String, String>> {
        let (path, cell) = self.gta6_cell(doc)?;
        Some(self.cut_gta6_polygon(doc, &path, &cell, polygon))
    }

    fn cut_gta6_polygon(
        &mut self,
        doc: DocumentId,
        path: &std::path::Path,
        cell: &NavMesh,
        polygon: usize,
    ) -> Result<String, String> {
        let cut = polygon_cut(cell, polygon).map_err(|e| e.to_string())?;
        let near = super::gta6_navmesh::neighbours(path, cell);
        let (cuts, in_place) = match self.disable_rechained(doc, path, cell, polygon, &near) {
            Some(made) => (made?, false),
            None => (self.cut_in_view(doc, &cut, polygon)?, true),
        };
        let mut said = if cuts == 0 {
            tr!(
                "Polygon {polygon} has no edge into or out of it left to cut.",
                polygon = polygon
            )
        } else {
            tr_n!(
                "Disabled polygon {polygon}: {count} edge now leads nowhere.",
                "Disabled polygon {polygon}: {count} edges now lead nowhere.",
                cuts,
                polygon = polygon
            )
        };
        if in_place && !cut.side_records.is_empty() {
            said.push(' ');
            said.push_str(&tr!(
                "Side records {records} still name it at their corner 1, which every shipped side record leads somewhere from, so it still joins the next cell across that border.",
                records = listed(&cut.side_records)
            ));
        }
        if !cut.links.is_empty() {
            said.push(' ');
            said.push_str(&tr!(
                "Special links {links} of this cell still end in it.",
                links = listed(&cut.links)
            ));
        }
        let mut closed = Vec::new();
        for (_, other, body) in &near {
            let Some(body) = body else {
                closed.push(*other as usize);
                continue;
            };
            let links = links_into(body, cell.cell, polygon);
            if !links.is_empty() {
                said.push(' ');
                said.push_str(&tr!(
                    "Special links {links} of the neighbouring cell {cell} still end in it.",
                    links = listed(&links),
                    cell = other
                ));
            }
        }
        if !closed.is_empty() {
            said.push(' ');
            said.push_str(&tr!(
                "The bodies of the neighbouring cells {cells} are not open, so whether their special links end in it is not known.",
                cells = listed(&closed)
            ));
        }
        Ok(said)
    }

    /// The edges `cut` names cut in the view of `doc`, one step of undo where one was open; how many.
    fn cut_in_view(
        &mut self,
        doc: DocumentId,
        cut: &PolygonCut,
        polygon: usize,
    ) -> Result<usize, String> {
        let nowhere = TAdjPoly::leading_to(0, NO_NEIGHBOUR).map_err(|e| e.to_string())?;
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let held = (edges_mut(&mut document.payload))
            .ok_or_else(|| tr!("The cell's view holds no edges."))?;
        let before = held.clone();
        let edges = (held.as_deref_mut()).ok_or_else(|| tr!("The cell's view holds no edges."))?;
        let mut cuts = 0usize;
        let all = edges
            .sub_arrays
            .iter_mut()
            .flat_map(|s| s.elements.iter_mut());
        for (j, edge) in all.enumerate() {
            if is_open(edge) && cut.edges.binary_search(&j).is_ok() {
                *edge = nowhere.clone();
                cuts += 1;
            }
        }
        if cuts > 0 {
            let label = tr!("Disable polygon {polygon}", polygon = polygon);
            self.record(
                doc,
                label,
                Change::part("gta6 cut edges", before, edges_mut),
            );
            // ⚠ Never folded into the next edit, as a GTA V cell's disable is not.
            if let Some(document) = self.document_mut(doc) {
                document.history.seal();
            }
        }
        Ok(cuts)
    }

    /// Why a polygon of the scanned GTA VI cell `doc` is not deleted while a neighbouring cell's
    /// link numbers it or one after it; `None` for another document.
    pub(super) fn gta6_delete_refusal(&self, doc: DocumentId) -> Option<String> {
        self.gta6_cell(doc)?;
        Some(tr!("A GTA VI cell's polygon is not deleted: the links of the neighbouring cells' bodies, which are saved apart, would be renumbered too. Disable it instead.").to_string())
    }
}

/// Per edge of the view's model of a scanned cell, whether it leads anywhere.
pub(super) fn open_edges(mesh: &CNavMesh) -> Vec<bool> {
    (mesh.adjacent_polys_array.as_deref()).map_or(Vec::new(), |l| l.iter().map(is_open).collect())
}

/// The edges a disable cut, written over the cell `virtual_section` holds: each the model leads
/// nowhere where the body names a polygon.
pub(super) fn write_cuts(virtual_section: &mut [u8], open: &[bool]) -> Result<(), String> {
    let read = read_navmesh(virtual_section).map_err(|e| e.to_string())?;
    if open.len() != read.edges.len() {
        return Err(
            tr!("The cell read again holds another number of edges than the one edited.")
                .to_string(),
        );
    }
    let cut: Vec<usize> = (0..read.edges.len())
        .filter(|&j| !open[j] && leads_somewhere(&read, j))
        .collect();
    if cut.is_empty() {
        return Ok(());
    }
    cut_edges(virtual_section, &cut).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use or_io::container::rpf::{Rpf9Form, Rpf9Sections};
    use or_io::gta6::headerless::{NavEdge, NavPolygon, NOWHERE};

    use super::*;
    use crate::state::gta6_resource_save::EditedSave;
    use crate::state::opening_an_rpf9_archive::{
        navmesh_cell, row_at, rows, rpf9_head, scanned, settle, written, zlib,
    };

    /// The cell's grid place, as the built cell states it at 0xC8.
    const CELL: u32 = 256 + 512 * 256;

    /// The built water triangle with its first edge leading back into itself.
    fn linked_cell() -> Vec<u8> {
        let mut v = navmesh_cell();
        // The adjacency list's items start 0x50 into its header at 0x1D0.
        v[0x220..0x224].copy_from_slice(&CELL.to_le_bytes());
        v[0x224..0x226].copy_from_slice(&0u16.to_le_bytes());
        v[0x226..0x228].copy_from_slice(&0x1207u16.to_le_bytes());
        v
    }

    fn opened(dir: &std::path::Path, body: &[u8]) -> (AppState, DocumentId) {
        let mut bytes = rpf9_head(1, 0x1A0);
        bytes.resize(0x1010, 0);
        bytes.extend(zlib(body));
        let mut state = AppState::new();
        let archive = scanned(&mut state, written(dir, &bytes));
        state.open_scanned_body(archive, row_at(&rows(&state, archive), 0x1010));
        settle(&mut state);
        let doc = (state.active_document)
            .filter(|&d| d != archive)
            .unwrap_or_else(|| panic!("the cell: {}", state.error_dialog_message));
        (state, doc)
    }

    fn open(state: &AppState, doc: DocumentId) -> Vec<bool> {
        match state.document(doc).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::NavMesh(mesh))) => open_edges(mesh),
            _ => panic!("a navigation mesh"),
        }
    }

    #[test]
    fn a_scanned_cell_s_polygon_disables_as_one_step_and_saves_its_edges_cut() {
        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let (mut state, doc) = opened(dir.path(), &linked_cell());
        assert_eq!(open(&state, doc), [true, false, false]);

        let said = state
            .disable_navmesh_polygon(doc, 0)
            .expect("a polygon proper");
        assert!(said.starts_with("Disabled polygon 0: 1 edge"), "{said}");
        assert_eq!(open(&state, doc), [false; 3]);
        let again = state
            .disable_navmesh_polygon(doc, 0)
            .expect("said, not refused");
        assert!(again.contains("left to cut"), "{again}");
        assert_eq!(state.undo().as_deref(), Some("Disable polygon 0"));
        assert_eq!(open(&state, doc), [true, false, false]);
        assert!(state.undo().is_none(), "the second made no step");
        assert!(state.redo().is_some());

        let refused = state.delete_navmesh_polygon(doc, 0).unwrap_err();
        assert!(refused.contains("are not open"), "{refused}");

        let document = state.document(doc).expect("open");
        let Some(Ok(EditedSave::Body { edit, .. })) =
            crate::state::gta6_physics_edit::edited_cell(document)
        else {
            panic!("the cell saves as its body read again and edited");
        };
        let mut sections = Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: linked_cell(),
            physical: Vec::new(),
        };
        edit(None, &mut sections).expect("the edit writes");
        let saved = read_navmesh(&sections.virtual_section).expect("the saved cell reads");
        assert_eq!(
            saved.edges[0],
            NavEdge {
                mesh: NOWHERE,
                polygon: 0xFFFF,
                word_06: 0x1200,
                flags: 0,
            }
        );
        assert_eq!(
            saved.polygons[0].flags, 0x80,
            "the flags as read, the water bit"
        );
    }

    #[test]
    fn an_edge_the_model_leads_nowhere_is_cut_only_where_the_body_names_a_polygon() {
        let mut v = linked_cell();
        write_cuts(&mut v, &[true, false, false]).expect("nothing to cut");
        assert_eq!(v, linked_cell());
        write_cuts(&mut v, &[false, false, false]).expect("edge 0 cut");
        assert_eq!(read_navmesh(&v).expect("it reads").edges[0].mesh, NOWHERE);
        assert!(
            write_cuts(&mut v, &[false]).is_err(),
            "another number of edges"
        );
    }

    #[test]
    fn an_edge_past_fifteen_bits_leads_nowhere_in_the_view_and_is_still_open_to_cut() {
        let mut big = read_navmesh(&linked_cell()).expect("the cell reads");
        let past = NO_NEIGHBOUR + 3;
        // Fillers whose corners name no index, so no edge runs through them.
        let filler = NavPolygon {
            first: 100,
            ..big.polygons[0]
        };
        big.polygons.resize(past + 1, filler);
        big.edges[0].polygon = past as u16;
        let edge = view_edge(&big, 0).expect("an edge");
        assert_eq!(edge.neighbour(), None, "past the view's fifteen bits");
        assert!(is_open(&edge), "and still open");
        let cut = TAdjPoly::leading_to(0, NO_NEIGHBOUR).expect("an edge");
        assert!(!is_open(&cut));
        let to_no_cell = view_edge(&big, 1).expect("an edge");
        assert!(!is_open(&to_no_cell));
        big.edges[0].polygon = 7;
        let near = view_edge(&big, 0).expect("an edge");
        assert_eq!(near.neighbour(), Some((0, 7)));
    }
}

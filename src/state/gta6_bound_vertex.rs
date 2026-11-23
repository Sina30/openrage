//! A scanned GTA VI bound's vertex moved, as a step of undo; a save quantises it and fits the BVH
//! over it again in the body read again, growing the child's box where the move leaves it.

use std::sync::Arc;

use or_io::gta6::headerless::{vertex_move_holds, BoundGeometry, ChildBound, Resource};

use super::edit_history::Change;
use super::{AppState, DocPayload, DocumentId};

/// Geometry child `child` of the bound a body is, or of the one a drawable or a fragment embeds.
fn geometry_mut(payload: &mut DocPayload, child: usize) -> Option<&mut BoundGeometry> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    let b = match Arc::get_mut(resource)? {
        Resource::Bound(b) => b,
        Resource::Drawable(d) => d.bound.as_mut()?,
        Resource::DrawableWithBound(r) => r.bound.as_mut()?,
        _ => return None,
    };
    match b.children.get_mut(child)?.bound.as_mut()?.as_mut().ok()? {
        ChildBound::Geometry(g) => Some(g),
        _ => None,
    }
}

impl AppState {
    /// Vertex `vertex` of geometry child `child` of the scanned GTA VI bound `doc` put at
    /// `position`, in the child's frame, as a step of its history.
    pub(crate) fn move_gta6_bound_vertex(
        &mut self,
        doc: DocumentId,
        child: usize,
        vertex: usize,
        position: [f32; 3],
    ) -> Result<(), String> {
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let g = geometry_mut(&mut document.payload, child).ok_or_else(|| {
            tr!("Only a geometry child of a bound body has a vertex moved, and not while a task reads the body.")
        })?;
        vertex_move_holds(g, vertex, position).map_err(|e| e.to_string())?;
        let held =
            (g.vertices.get_mut(vertex)).ok_or_else(|| tr!("The child holds no such vertex."))?;
        let before = std::mem::replace(held, position);
        let key = format!("gta6 bound vertex {child} {vertex}");
        let change = Change::part(key, before, move |p| {
            geometry_mut(p, child)?.vertices.get_mut(vertex)
        });
        self.record(doc, tr!("Move a vertex"), change);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use or_io::gta6::headerless::{Bound, BoundChild, Bvh, Primitive};

    use super::*;
    use crate::state::Document;

    fn vertices(state: &AppState) -> Vec<[f32; 3]> {
        let Some(DocPayload::Gta6Drawable(r)) = state.document(1).map(|d| &d.payload) else {
            panic!("a GTA VI body");
        };
        let Resource::Bound(b) = &**r else {
            panic!("a bound");
        };
        match &b.children[0].bound {
            Some(Ok(ChildBound::Geometry(g))) => g.vertices.clone(),
            _ => panic!("a geometry child"),
        }
    }

    fn opened() -> AppState {
        opened_as(Resource::Bound)
    }

    /// A body of the one-child bound `wrap` holds.
    fn opened_as(wrap: fn(Bound) -> Resource) -> AppState {
        let geometry = BoundGeometry {
            box_min: [-2.0; 3],
            box_max: [2.0; 3],
            vertices: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            primitives: vec![Primitive::Triangle {
                vertices: [0, 1, 2],
                neighbours: [0xFFFF; 3],
                edges: [0xFF; 3],
            }],
            material_ids: Vec::new(),
            primitive_materials: Vec::new(),
            bvh: Some(Bvh {
                box_min: [-2.0; 3],
                box_max: [2.0; 3],
                nodes: 1,
                leaves: 1,
            }),
            unknown_shapes: Vec::new(),
        };
        let bound = Bound {
            radius: 3.5,
            box_min: [-2.0; 3],
            box_max: [2.0; 3],
            centroid: [0.0; 3],
            children: vec![BoundChild {
                matrix: [[0.0; 4]; 4],
                flags: None,
                bound: Some(Ok(ChildBound::Geometry(geometry))),
                material_id: None,
            }],
            bvh: None,
        };
        let mut state = AppState::new();
        let payload = DocPayload::Gta6Drawable(Arc::new(wrap(bound)));
        let path = Some(PathBuf::from("0x1010.bound"));
        state.documents.push(Document::new(1, path, payload));
        state.active_document = Some(1);
        state
    }

    #[test]
    fn a_vertex_moves_as_a_step_inside_its_child_s_box_or_past_it() {
        let mut state = opened();
        let read = vertices(&state);
        state
            .move_gta6_bound_vertex(1, 0, 1, [1.5, -0.25, 0.5])
            .expect("inside the box");
        assert_eq!(vertices(&state)[1], [1.5, -0.25, 0.5]);
        state
            .move_gta6_bound_vertex(1, 0, 2, [0.0, 2.5, 0.0])
            .expect("past the box, which the save grows");
        assert_eq!(vertices(&state)[2], [0.0, 2.5, 0.0]);
        assert!(state.move_gta6_bound_vertex(1, 0, 3, [0.0; 3]).is_err());
        assert!(state.move_gta6_bound_vertex(1, 1, 0, [0.0; 3]).is_err());
        let nan = [f32::NAN, 0.0, 0.0];
        assert!(state.move_gta6_bound_vertex(1, 0, 1, nan).is_err());
        assert_eq!(state.undo().as_deref(), Some("Move a vertex"));
        assert_eq!(vertices(&state)[2], read[2]);
        assert_eq!(state.undo().as_deref(), Some("Move a vertex"));
        assert_eq!(vertices(&state), read);
        assert!(state.undo().is_none(), "a refused move made no step");
    }

    #[test]
    fn a_vertex_of_the_bound_a_drawable_embeds_moves_as_a_step() {
        let mut state = opened_as(|bound| {
            let mut drawable = crate::state::opening_a_ttpf::drawable_keyed(1);
            drawable.bound = Some(bound);
            Resource::Drawable(drawable)
        });
        let embedded = |state: &AppState| -> [f32; 3] {
            let Some(DocPayload::Gta6Drawable(r)) = state.document(1).map(|d| &d.payload) else {
                panic!("a GTA VI body");
            };
            let Resource::Drawable(d) = &**r else {
                panic!("a drawable");
            };
            let child = d.bound.as_ref().map(|b| &b.children[0].bound);
            match child {
                Some(Some(Ok(ChildBound::Geometry(g)))) => g.vertices[1],
                _ => panic!("a geometry child"),
            }
        };
        state
            .move_gta6_bound_vertex(1, 0, 1, [1.5, -0.25, 0.5])
            .expect("a drawable's bound moves a vertex");
        assert_eq!(embedded(&state), [1.5, -0.25, 0.5]);
        assert_eq!(state.undo().as_deref(), Some("Move a vertex"));
        assert_eq!(embedded(&state), [1.0, 0.0, 0.0]);
    }
}

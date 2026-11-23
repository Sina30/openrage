//! The primitive of the active scanned GTA VI bound a click lands on, or the primitive child.

use or_io::gta6::headerless::Resource;

use super::pick::{nearest_polygon, Hit, Ray};
use crate::render::gta6_drawable::{child_meshes, primitive_meshes_of};
use crate::state::gta6_bound::{child_row, polygon_row};
use crate::state::{AppState, DocPayload};

/// A child and the primitive of it a pick hit.
type Primitive = (usize, usize);

/// The primitive of a geometry child nearest along `ray` as its polygon row, or a primitive child as its child row.
pub(super) fn gta6_bound_under(state: &AppState, ray: &Ray) -> Option<Hit> {
    let doc = state.active_document?;
    let DocPayload::Gta6Drawable(resource) = &state.document(doc)?.payload else {
        return None;
    };
    let Resource::Bound(bound) = &**resource else {
        return None;
    };
    // How far along the row hit nearest is, the row, and its child and primitive where it is one.
    let mut best: Option<(f32, u64, Option<Primitive>)> = None;
    for (c, child) in bound.children.iter().enumerate() {
        let primitives = primitive_meshes_of(child);
        let meshes: Vec<_> = match primitives.is_empty() {
            true => (child_meshes(child).into_iter())
                .map(|m| (child_row(c), None, m))
                .collect(),
            false => (primitives.into_iter())
                .map(|(p, m)| (polygon_row(c, p), Some((c, p)), m))
                .collect(),
        };
        for (row, primitive, mesh) in meshes {
            let within = best.map_or(f32::INFINITY, |(t, ..)| t);
            if let Some((t, _)) = nearest_polygon(&mesh, ray, within) {
                best = Some((t, row, primitive));
            }
        }
    }
    let (t, row, primitive) = best?;
    let label = match primitive {
        Some((child, p)) => format!("Primitive {p} of child {child}"),
        None => "Bound".to_string(),
    };
    Some(Hit {
        t,
        doc,
        row,
        label,
        model: None,
    })
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use or_io::gta6::headerless::{Bound, BoundChild, BoundGeometry, ChildBound, Primitive};
    use or_renderer::OrbitCamera;

    use super::*;
    use crate::render::viewport::pick::select_under;
    use crate::state::gta6_bound::{polygon_rows, selected_child};
    use crate::state::{Document, Prefs};

    /// A ray through the middle of a viewport whose camera is `distance` from `at` along `from`.
    fn looking_at(at: Vec3, from: Vec3, distance: f32) -> Ray {
        let camera = OrbitCamera {
            target: at,
            yaw: from.y.atan2(from.x),
            pitch: from.z.asin(),
            distance,
            ..OrbitCamera::default()
        };
        let rect = egui::Rect::from_min_size(egui::pos2(300.0, 40.0), egui::vec2(800.0, 500.0));
        Ray::at(&camera, rect, (1200, 750), rect.center()).expect("a ray")
    }

    fn child(bound: ChildBound, origin: [f32; 3], material_id: Option<u64>) -> BoundChild {
        let mut matrix = [[0.0; 4]; 4];
        (matrix[0][0], matrix[1][1], matrix[2][2]) = (1.0, 1.0, 1.0);
        matrix[3] = [origin[0], origin[1], origin[2], 1.0];
        BoundChild {
            matrix,
            flags: None,
            bound: Some(Ok(bound)),
            material_id,
        }
    }

    /// A sphere child at x = -6, then a geometry at x = 4 of two triangles side by side in the
    /// plane y = 0, of materials 0x2A1 and 0x13C, and a sphere primitive over the second.
    fn opened() -> AppState {
        let geometry = BoundGeometry {
            box_min: [0.0, 0.0, 0.0],
            box_max: [4.0, 0.0, 1.0],
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
                [3.0, 0.0, 1.0],
                [2.8, 0.0, 0.3],
            ],
            primitives: vec![
                Primitive::Triangle {
                    vertices: [0, 1, 2],
                    neighbours: [0xFFFF; 3],
                    edges: [0xFF; 3],
                },
                Primitive::Triangle {
                    vertices: [3, 4, 5],
                    neighbours: [0xFFFF; 3],
                    edges: [0xFF; 3],
                },
                Primitive::Sphere {
                    centre: 6,
                    radius: 0.25,
                },
            ],
            material_ids: vec![0x9000_0000_0000_02A1, 0x13C],
            primitive_materials: vec![0, 1, 1],
            bvh: None,
            unknown_shapes: Vec::new(),
        };
        let sphere = ChildBound::Sphere {
            centre: [0.0; 3],
            radius: 0.5,
        };
        let bound = Bound {
            radius: 8.0,
            box_min: [-7.0, -1.0, -1.0],
            box_max: [8.0, 1.0, 2.0],
            centroid: [0.0; 3],
            children: vec![
                child(sphere, [-6.0, 0.0, 0.0], Some(0x77)),
                child(ChildBound::Geometry(geometry), [4.0, 0.0, 0.0], None),
            ],
            bvh: None,
        };
        let mut state = AppState::from_prefs(Prefs::default());
        let payload = DocPayload::Gta6Drawable(std::sync::Arc::new(Resource::Bound(bound)));
        (state.documents).push(Document::new(1, Some("0x10.bound".into()), payload));
        state.active_document = Some(1);
        state
    }

    #[test]
    fn a_click_on_a_gta_vi_bound_selects_the_primitive_under_it_and_the_inspector_names_its_key() {
        let mut state = opened();
        let click = |state: &mut AppState, at: Vec3| {
            select_under(state, &looking_at(at, Vec3::NEG_Y, 10.0));
            state.selection.selected(1)
        };
        // The second triangle of the geometry, which its matrix moves 4 along x, clear of the sphere.
        assert_eq!(
            click(&mut state, Vec3::new(4.0 + 2.3, 0.0, 0.1)),
            Some(polygon_row(1, 1))
        );
        assert_eq!(
            click(&mut state, Vec3::new(4.0 + 0.9, 0.0, 0.2)),
            Some(polygon_row(1, 0))
        );
        assert_eq!(
            click(&mut state, Vec3::new(4.0 + 2.83, 0.0, 0.32)),
            Some(polygon_row(1, 2)),
            "the sphere primitive is nearer the eye than the triangle behind it"
        );
        assert_eq!(
            click(&mut state, Vec3::new(-5.97, 0.0, 0.04)),
            Some(child_row(0)),
            "a primitive child is picked as its child"
        );
        assert_eq!(
            click(&mut state, Vec3::new(0.0, 0.0, 5.0)),
            Some(child_row(0)),
            "a click on nothing leaves the selection"
        );

        let DocPayload::Gta6Drawable(resource) = &state.document(1).expect("open").payload else {
            panic!("not a GTA VI body");
        };
        let Resource::Bound(bound) = &**resource else {
            panic!("not a bound");
        };
        let picked = Some(polygon_row(1, 1));
        let (index, held) = selected_child(bound, picked).expect("the child holding it");
        assert_eq!(index, 1);
        assert_eq!(
            polygon_rows(held, picked),
            vec![
                ("Primitive".to_string(), "1 (triangle)".to_string()),
                ("Material key".to_string(), "0x013C".to_string()),
            ]
        );
        let first = polygon_rows(held, Some(polygon_row(1, 0)));
        let key = 0x9000_0000_0000_02A1u64 & 0x7FFF;
        assert_eq!(first[1].1, format!("{key:#06X}"));
        assert!(polygon_rows(held, Some(child_row(1))).is_empty());
    }
}

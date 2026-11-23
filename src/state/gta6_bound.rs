//! A scanned GTA VI bound's children as rows: what the Outline names each, and the fields the
//! inspector shows of the bound or of the child selected.

use or_io::gta6::headerless::{material_key, Bound, BoundChild, Bvh, ChildBound, Primitive};

/// The selection of child `index`; the bound itself is row 0.
pub(crate) fn child_row(index: usize) -> u64 {
    index as u64 + 1
}

/// The tag of a selection of one primitive of a geometry child, above any child row.
const POLYGON: u64 = 0x5 << 60;

/// The selection of primitive `primitive` of child `child`, which a click in the viewport makes.
pub(crate) fn polygon_row(child: usize, primitive: usize) -> u64 {
    POLYGON | ((child as u64 & 0x0FFF_FFFF) << 32) | (primitive as u64 & 0xFFFF_FFFF)
}

/// The child and the primitive a polygon selection names.
fn polygon_of(selection: u64) -> Option<(usize, usize)> {
    (selection & (0xF << 60) == POLYGON).then(|| {
        let child = (selection >> 32) & 0x0FFF_FFFF;
        (child as usize, (selection & 0xFFFF_FFFF) as usize)
    })
}

/// The child a selection names or holds the primitive of; `None` for the bound itself or a child it does not hold.
pub(crate) fn selected_child(b: &Bound, selection: Option<u64>) -> Option<(usize, &BoundChild)> {
    let selection = selection?;
    let index = match polygon_of(selection) {
        Some((child, _)) => child,
        None => usize::try_from(selection.checked_sub(1)?).ok()?,
    };
    Some((index, b.children.get(index)?))
}

/// The index, shape and material key of the primitive a polygon selection names in `child`; none for any other selection.
pub(crate) fn polygon_rows(child: &BoundChild, selection: Option<u64>) -> Vec<(String, String)> {
    let Some((_, index)) = selection.and_then(polygon_of) else {
        return Vec::new();
    };
    let Some(Ok(ChildBound::Geometry(g))) = &child.bound else {
        return Vec::new();
    };
    let Some(p) = g.primitives.get(index) else {
        return Vec::new();
    };
    let shape = match p {
        Primitive::Triangle { .. } => tr!("triangle"),
        Primitive::Sphere { .. } => tr!("sphere"),
        Primitive::Capsule { .. } => tr!("capsule"),
        Primitive::Box(_) => tr!("box"),
        Primitive::Cylinder { .. } => tr!("cylinder"),
    };
    let mut rows = vec![(tr!("Primitive").to_string(), format!("{index} ({shape})"))];
    if let Some(key) = g.primitive_key(index) {
        rows.push((tr!("Material key").to_string(), key_text(key)));
    }
    rows
}

/// A child's row in the Outline: its name and what it holds.
pub(crate) fn child_label(index: usize, child: &BoundChild) -> (String, String) {
    match &child.bound {
        None => (tr!("Child {index}: null", index = index), String::new()),
        Some(Err(e)) => (tr!("Child {index}: not read", index = index), e.to_string()),
        Some(Ok(shape)) => {
            let name = super::gta6_resource::shape_name(shape).unwrap_or(tr!("geometry"));
            let detail = match (shape, shape.extent()) {
                (ChildBound::Geometry(g), _) => tr!(
                    "{vertices} vertices, {primitives} primitives",
                    vertices = g.vertices.len(),
                    primitives = g.primitives.len(),
                ),
                (_, Some((_, radius))) => tr!("radius {radius} m", radius = format!("{radius:.3}")),
                (_, None) => String::new(),
            };
            (
                tr!("Child {index} ({shape})", index = index, shape = name),
                detail,
            )
        }
    }
}

fn point([x, y, z]: [f32; 3]) -> String {
    format!("({x:.3}, {y:.3}, {z:.3})")
}

fn bvh_value(bvh: Option<Bvh>) -> String {
    bvh.map_or(tr!("none").to_string(), |h| {
        tr!(
            "{nodes} nodes, {leaves} leaves",
            nodes = h.nodes,
            leaves = h.leaves
        )
    })
}

/// The grid the inspector shows of the bound itself.
pub(crate) fn bound_rows(b: &Bound) -> Vec<(String, String)> {
    vec![
        (tr!("Children").to_string(), b.children.len().to_string()),
        (
            tr!("Box").to_string(),
            format!("{} - {}", point(b.box_min), point(b.box_max)),
        ),
        (tr!("Radius").to_string(), format!("{:.4}", b.radius)),
        (tr!("Centroid").to_string(), point(b.centroid)),
        (tr!("BVH").to_string(), bvh_value(b.bvh)),
    ]
}

/// The grid the inspector shows of one child: where its matrix places it, then its own fields.
pub(crate) fn child_rows(child: &BoundChild) -> Vec<(String, String)> {
    let [.., origin] = child.matrix;
    let mut rows = vec![(
        tr!("Origin").to_string(),
        point([origin[0], origin[1], origin[2]]),
    )];
    if let Some([a, b]) = child.flags {
        rows.push((tr!("Flags").to_string(), format!("{a:#X} {b:#X}")));
    }
    if let Some(id) = child.material_id {
        rows.push((tr!("Material key").to_string(), key_text(material_key(id))));
    }
    let Some(Ok(shape)) = &child.bound else {
        return rows;
    };
    let name = super::gta6_resource::shape_name(shape).unwrap_or(tr!("geometry"));
    rows.push((tr!("Type").to_string(), name.to_string()));
    if let Some((half, radius)) = shape.extent() {
        rows.push((tr!("Half extents").to_string(), point(half)));
        rows.push((tr!("Radius").to_string(), format!("{radius:.4}")));
    }
    let ChildBound::Geometry(g) = shape else {
        return rows;
    };
    let mut kinds = [0usize; 5];
    for p in &g.primitives {
        kinds[match p {
            Primitive::Triangle { .. } => 0,
            Primitive::Sphere { .. } => 1,
            Primitive::Capsule { .. } => 2,
            Primitive::Box(_) => 3,
            Primitive::Cylinder { .. } => 4,
        }] += 1;
    }
    let ids: Vec<String> = g
        .material_ids
        .iter()
        .map(|id| format!("{id:016X}"))
        .collect();
    let mut keys = std::collections::BTreeMap::new();
    for key in (0..g.primitives.len()).filter_map(|p| g.primitive_key(p)) {
        *keys.entry(key).or_insert(0usize) += 1;
    }
    let keys: Vec<String> = (keys.iter())
        .map(|(key, count)| format!("{} ({count})", key_text(*key)))
        .collect();
    rows.extend([
        (
            tr!("Box").to_string(),
            format!("{} - {}", point(g.box_min), point(g.box_max)),
        ),
        (tr!("Vertices").to_string(), g.vertices.len().to_string()),
        (
            tr!("Primitives").to_string(),
            tr!(
                "{triangles} triangles, {spheres} spheres, {capsules} capsules, {boxes} boxes, {cylinders} cylinders",
                triangles = kinds[0],
                spheres = kinds[1],
                capsules = kinds[2],
                boxes = kinds[3],
                cylinders = kinds[4],
            ),
        ),
        (tr!("Materials").to_string(), ids.join(" ")),
        (
            tr!("Material indices").to_string(),
            g.primitive_materials.len().to_string(),
        ),
        (tr!("Material keys").to_string(), keys.join(", ")),
        (
            tr!("Shapes not known").to_string(),
            g.unknown_shapes.len().to_string(),
        ),
        (tr!("BVH").to_string(), bvh_value(g.bvh)),
    ]);
    rows
}

/// A material key as the inspector names it.
fn key_text(key: u16) -> String {
    format!("{key:#06X}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::BoundGeometry;

    fn child(bound: Option<ChildBound>) -> BoundChild {
        let mut matrix = [[0.0; 4]; 4];
        matrix[3] = [4.0, -2.5, 7.0, 0.0];
        BoundChild {
            matrix,
            flags: Some([0x30, 0x41]),
            material_id: match bound {
                Some(ChildBound::Geometry(_)) | None => None,
                Some(_) => Some(0x42_0003_7646),
            },
            bound: bound.map(Ok),
        }
    }

    fn bound() -> Bound {
        let geometry = BoundGeometry {
            box_min: [-1.0, -2.0, -3.0],
            box_max: [1.0, 2.0, 3.0],
            vertices: vec![[0.0; 3]; 4],
            primitives: vec![
                Primitive::Triangle {
                    vertices: [0, 1, 2],
                    neighbours: [0xFFFF; 3],
                    edges: [0xFF; 3],
                },
                Primitive::Capsule {
                    ends: [0, 3],
                    radius: 0.25,
                },
                Primitive::Box([0, 1, 2, 3]),
            ],
            material_ids: vec![0x0123_4567_89AB_4DEF, 7],
            primitive_materials: vec![0, 1, 0],
            bvh: Some(Bvh {
                box_min: [0.0; 3],
                box_max: [1.0; 3],
                nodes: 5,
                leaves: 3,
            }),
            unknown_shapes: Vec::new(),
        };
        Bound {
            radius: 2.0,
            box_min: [-1.0; 3],
            box_max: [1.0; 3],
            centroid: [0.0; 3],
            children: vec![
                child(Some(ChildBound::Geometry(geometry))),
                child(Some(ChildBound::Sphere {
                    centre: [0.5, 0.0, 0.0],
                    radius: 0.75,
                })),
                child(None),
            ],
            bvh: None,
        }
    }

    #[test]
    fn each_child_of_a_bound_is_a_row_whose_selection_shows_its_own_fields() {
        let b = bound();
        let rows: Vec<_> = (b.children.iter().enumerate())
            .map(|(i, c)| child_label(i, c))
            .collect();
        assert_eq!(rows[0].0, "Child 0 (geometry)");
        assert_eq!(rows[0].1, "4 vertices, 3 primitives");
        assert_eq!(
            rows[1],
            ("Child 1 (sphere)".into(), "radius 0.750 m".into())
        );
        assert_eq!(rows[2].0, "Child 2: null");

        assert!(selected_child(&b, Some(0)).is_none(), "row 0 is the bound");
        assert!(selected_child(&b, Some(child_row(3))).is_none());
        let (index, picked) = selected_child(&b, Some(child_row(0))).expect("child 0");
        assert_eq!(index, 0);
        let grid = child_rows(picked);
        let value = |label: &str| {
            (grid.iter())
                .find(|(l, _)| l == label)
                .map(|(_, v)| v.as_str())
                .unwrap_or_else(|| panic!("no {label} in {grid:?}"))
        };
        assert_eq!(value("Origin"), "(4.000, -2.500, 7.000)");
        assert_eq!(value("Flags"), "0x30 0x41");
        assert_eq!(
            value("Primitives"),
            "1 triangles, 0 spheres, 1 capsules, 1 boxes, 0 cylinders"
        );
        assert_eq!(value("Materials"), "0123456789AB4DEF 0000000000000007");
        assert_eq!(value("Material keys"), "0x0007 (1), 0x4DEF (2)");
        assert!(grid.iter().all(|(l, _)| l != "Material key"));
        assert_eq!(value("BVH"), "5 nodes, 3 leaves");
        let sphere = child_rows(&b.children[1]);
        assert!(sphere.contains(&("Half extents".into(), "(0.750, 0.750, 0.750)".into())));
        assert!(sphere.contains(&("Material key".into(), "0x7646".into())));
        assert!(bound_rows(&b).contains(&("Children".into(), "3".into())));
    }
}

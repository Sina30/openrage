use super::*;

#[test]
fn a_composite_reads_to_its_child_s_vertices_primitives_and_bvh() {
    let (img, _) = bound();
    let bound = read_bound(&img.bytes).expect("the bound reads");
    assert_eq!(bound.box_max, [1.0, 1.0, 0.0]);
    assert!(bound.bvh.is_none());
    let [child] = bound.children.as_slice() else {
        panic!("one child, not {:?}", bound.children);
    };
    assert_eq!(child.matrix[3], [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(child.flags, Some([0x13, 0]));
    let Some(Ok(ChildBound::Geometry(geometry))) = &child.bound else {
        panic!("a geometry child, not {:?}", child.bound);
    };
    assert_eq!(geometry.vertices, TRIANGLE_AT.to_vec());
    assert_eq!(
        geometry.primitives,
        [
            Primitive::Triangle {
                vertices: [0, 1, 2],
                neighbours: [0xFFFF; 3],
                edges: [0; 3],
            },
            Primitive::Sphere {
                centre: 0,
                radius: 0.0
            }
        ]
    );
    assert!(geometry.unknown_shapes.is_empty());
    assert!(geometry.material_ids.is_empty() && geometry.primitive_materials.is_empty());
    let bvh = geometry.bvh.expect("a BVH");
    assert_eq!((bvh.nodes, bvh.leaves), (2, 1));
}

#[test]
fn a_bound_of_another_type_byte_is_refused() {
    let (mut img, parts) = bound();
    img.bytes[parts.root + 0x10] = 10;
    assert!(matches!(
        read_bound(&img.bytes),
        Err(HeaderlessError::BoundType {
            found: 10,
            expected: COMPOSITE,
            ..
        })
    ));
}

/// The one child of a bound that reads, as what it is or why it is refused.
fn only_child(bytes: &[u8]) -> Result<ChildBound> {
    let mut bound = read_bound(bytes).expect("the composite reads");
    assert_eq!(bound.children.len(), 1);
    bound.children.remove(0).bound.expect("a child pointer")
}

fn only_geometry(bytes: &[u8]) -> Result<BoundGeometry> {
    match only_child(bytes)? {
        ChildBound::Geometry(g) => Ok(g),
        other => panic!("a geometry child, not {other:?}"),
    }
}

/// Where the built bound's one child lies.
fn child_of(img: &Image, parts: &BoundParts) -> usize {
    let children = u64_at(&img.bytes, parts.root + 0x70) as usize & 0x0FFF_FFFF;
    u64_at(&img.bytes, children) as usize & 0x0FFF_FFFF
}

const CENTRE: [f32; 3] = [0.5, -1.25, 2.0];

/// A primitive's type, its fields by offset, and its box's half extents, radius and margin.
type Case = (u8, Vec<(usize, f32)>, [f32; 3], f32, f32);

/// A primitive of `kind` written over the built child: its fields, then the box about [`CENTRE`],
/// the radius and the margin given.
fn put_primitive(img: &mut Image, child: usize, (kind, fields, half, radius, margin): &Case) {
    img.bytes[child + 0x50..child + BOUND_CHILD].fill(0);
    img.bytes[child + 0x10] = *kind;
    for (o, x) in fields {
        img.put(child + o, &x.to_le_bytes());
    }
    img.vec3(child + 0x20, std::array::from_fn(|a| CENTRE[a] + half[a]));
    img.vec3(child + 0x30, std::array::from_fn(|a| CENTRE[a] - half[a]));
    img.vec3(child + 0x40, CENTRE);
    img.put(child + 0x14, &radius.to_le_bytes());
    img.put(child + 0x2C, &margin.to_le_bytes());
}

#[test]
fn each_primitive_s_box_computed_from_its_fields_equals_the_box_written() {
    let length = |v: [f32; 3]| v.iter().map(|a| a * a).sum::<f32>().sqrt();
    let (a, b, l) = (0.0303f32, 0.038f32, 0.2317f32);
    let corners: [[f32; 3]; 8] = [
        [0.2921, 0.2796, 0.3036],
        [-0.2921, 0.2796, 0.3036],
        [-0.2921, 0.2796, -0.3036],
        [0.2921, 0.2796, -0.3036],
        [0.2921, -0.2796, 0.3298],
        [-0.2921, -0.2796, 0.3298],
        [-0.2921, -0.2796, -0.3298],
        [0.2921, -0.2796, -0.3298],
    ];
    let hm = 0.0399f32;
    let hull_fields = (corners.iter().enumerate())
        .flat_map(|(i, c)| (0..3).map(move |k| (0x70 + 16 * i + 4 * k, c[k])))
        .collect();
    let hull_far = corners.iter().map(|c| length(*c)).fold(0f32, f32::max);
    let (fa, fb, h, ka, fm) = (0.3082f32, 0.3122f32, 0.0355f32, 0.8618f32, 0.0044f32);
    let kb = 1.0 / ka;
    let fr = (fa + fm * ka).max(fb + fm * kb);
    let cases = [
        (0, vec![], [0.4; 3], 0.4, 0.4),
        (1, vec![], [0.3, 1.1, 0.3], 1.1, 0.3),
        (
            2,
            vec![(0x70, a), (0x74, b), (0x78, l)],
            [b, l / 2.0 + b, b],
            l / 2.0 + b,
            a,
        ),
        (
            3,
            vec![],
            [0.5, 0.75, 1.25],
            length([0.5, 0.75, 1.25]),
            0.04,
        ),
        (8, vec![], [0.0246, 0.2456, 0.2456], 0.2456, 0.0246),
        (
            9,
            vec![],
            [0.019, 0.0052, 0.019],
            length([0.019, 0.0052, 0.0]),
            0.001,
        ),
        (
            13,
            hull_fields,
            [0.2921 + hm, 0.2796 + hm, 0.3298 + hm],
            hull_far + hm,
            hm,
        ),
        (
            14,
            vec![(0x70, fa), (0x74, fb), (0x78, h), (0x7C, ka), (0x80, kb)],
            [fr, h / 2.0, fr],
            length([fr, h, 0.0]),
            fm,
        ),
    ];
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    for case in &cases {
        put_primitive(&mut img, child, case);
        let shape = only_child(&img.bytes).unwrap_or_else(|e| panic!("type {}: {e}", case.0));
        let (half, radius) = shape.extent().expect("a primitive's extent");
        for (got, want) in half
            .iter()
            .chain([&radius])
            .zip(case.2.iter().chain([&case.3]))
        {
            assert!((got - want).abs() < 1e-6, "type {}: {shape:?}", case.0);
        }
        // A radius 1 % off the fields' is refused.
        let mut off = case.clone();
        off.3 *= 1.01;
        put_primitive(&mut img, child, &off);
        assert!(
            matches!(only_child(&img.bytes), Err(HeaderlessError::PrimitiveBox { found, .. }) if found == case.0),
            "type {}",
            case.0
        );
    }
    put_primitive(&mut img, child, &cases[7]);
    let ChildBound::Frustum {
        centre,
        radii,
        factors,
        height,
        margin,
    } = only_child(&img.bytes).expect("a frustum")
    else {
        panic!("a frustum");
    };
    assert_eq!(
        (centre, radii, factors, height, margin),
        (CENTRE, [fa, fb], [ka, kb], h, fm)
    );
    // The factors swapped give another box, which is refused.
    img.put(child + 0x7C, &kb.to_le_bytes());
    img.put(child + 0x80, &ka.to_le_bytes());
    assert!(only_child(&img.bytes).is_err());
    put_primitive(&mut img, child, &cases[6]);
    let Ok(ChildBound::Hull { corners: read, .. }) = only_child(&img.bytes) else {
        panic!("a hull");
    };
    assert_eq!(read, corners);
    put_primitive(&mut img, child, &cases[2]);
    assert!(matches!(
        only_child(&img.bytes),
        Ok(ChildBound::TaperedCapsule { radii, length, .. }) if radii == [a, b] && length == l
    ));
    img.bytes[child + 0x8F] = 1;
    assert!(matches!(
        only_child(&img.bytes),
        Err(HeaderlessError::Constant { found: 1, .. })
    ));
}

#[test]
fn a_child_of_a_type_with_no_layout_is_refused_by_name_and_the_composite_still_reads() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    for kind in [7, 10, 15] {
        img.bytes[child + 0x10] = kind;
        let refused = only_child(&img.bytes).expect_err("no layout");
        assert!(
            matches!(refused, HeaderlessError::BoundNotLaidOut { found, .. } if found == kind),
            "{refused}"
        );
    }
}

#[test]
fn a_primitive_of_an_unknown_type_is_refused() {
    let (mut img, parts) = bound();
    img.bytes[parts.primitives + 16 + 3] = 5;
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::PrimitiveType { found: 5, .. })
    ));
}

#[test]
fn a_triangle_index_past_the_vertices_is_refused() {
    let (mut img, parts) = bound();
    img.u16(parts.primitives + 8, 3);
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::IndexPastCount {
            index: 3,
            count: 3,
            ..
        })
    ));
}

#[test]
fn a_bvh_whose_leaves_miss_a_primitive_is_refused() {
    let (mut img, parts) = bound();
    img.u16(parts.nodes + 16 + 0xE, 1);
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::BvhCover {
            covered: 1,
            primitives: 2
        })
    ));
}

#[test]
fn a_triangle_s_edge_bytes_are_read_and_an_open_mark_on_a_linked_edge_is_refused() {
    let (mut img, parts) = bound();
    img.u16(parts.primitives + 0xA, 1);
    img.put(parts.primitives, &[2, 0xFF, 0xFF]);
    let geometry = only_geometry(&img.bytes).expect("the child reads");
    assert!(matches!(
        geometry.primitives[0],
        Primitive::Triangle {
            edges: [2, 0xFF, 0xFF],
            neighbours: [1, 0xFFFF, 0xFFFF],
            ..
        }
    ));
    img.bytes[parts.primitives] = 0xFF;
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::OpenEdgeLinked { edge: 0, .. })
    ));
}

#[test]
fn the_material_ids_and_each_primitive_s_material_read_and_an_index_past_them_is_refused() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    let ids = img.alloc(16);
    img.u64(ids, 0x11);
    img.u64(ids + 8, 0x4200_0003_7646);
    let list = img.alloc(2);
    img.put(list, &[1, 0]);
    img.bytes[child + 0x120] = 2;
    img.ptr(child + 0xF0, ids);
    img.ptr(child + 0x118, list);
    let geometry = only_geometry(&img.bytes).expect("the child reads");
    assert_eq!(geometry.material_ids, [0x11, 0x4200_0003_7646]);
    assert_eq!(geometry.primitive_materials, [1, 0]);
    assert_eq!(
        (0..3)
            .map(|p| geometry.primitive_key(p))
            .collect::<Vec<_>>(),
        [Some(0x7646), Some(0x11), None]
    );
    img.u64(ids + 8, 0x4200_0003_F646);
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::MaterialBit15 {
            id: 0x4200_0003_F646,
            ..
        })
    ));
    img.u64(ids + 8, 0x22);
    img.bytes[list] = 2;
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::IndexPastCount {
            index: 2,
            count: 2,
            ..
        })
    ));
}

#[test]
fn a_geometry_with_no_bvh_reads_triangles_only_and_never_its_bytes_past_0x130() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    img.bytes[child + 0x10] = PLAIN_GEOMETRY;
    // Its hull counts every vertex, as a type-4 geometry's does.
    img.u16(child + 0x84, 3);
    assert!(matches!(
        only_child(&img.bytes),
        Err(HeaderlessError::NotATriangle { found: 1, at }) if at == (parts.primitives + 16) as u64
    ));
    img.u32(child + 0xD4, 1);
    // The next object's first word, which is no pointer.
    img.u64(child + 0x130, 1);
    let geometry = only_geometry(&img.bytes).expect("a geometry with no BVH");
    assert!(geometry.bvh.is_none());
    assert_eq!(geometry.vertices, TRIANGLE_AT.to_vec());
    assert!(matches!(
        geometry.primitives[..],
        [Primitive::Triangle {
            vertices: [0, 1, 2],
            ..
        }]
    ));
    img.bytes[child + 0x10] = GEOMETRY;
    assert!(matches!(
        only_child(&img.bytes),
        Err(HeaderlessError::NotAPointer { .. })
    ));
}

#[test]
fn a_sphere_capsule_or_cylinder_reads_its_radius_and_a_radius_that_is_no_length_is_refused() {
    let (mut img, parts) = bound();
    let at = parts.primitives + 16;
    img.put(at + 4, &0.25f32.to_le_bytes());
    img.u16(at, 2);
    let read = |img: &Image| only_geometry(&img.bytes).map(|g| g.primitives[1]);
    assert_eq!(
        read(&img).expect("a sphere"),
        Primitive::Sphere {
            centre: 2,
            radius: 0.25
        }
    );
    img.u16(at + 8, 1);
    img.bytes[at + 3] = 2;
    assert_eq!(
        read(&img).expect("a capsule"),
        Primitive::Capsule {
            ends: [2, 1],
            radius: 0.25
        }
    );
    img.bytes[at + 3] = 4;
    assert_eq!(
        read(&img).expect("a cylinder"),
        Primitive::Cylinder {
            ends: [2, 1],
            radius: 0.25
        }
    );
    for bad in [-0.5f32, f32::NAN, f32::INFINITY] {
        img.put(at + 4, &bad.to_le_bytes());
        assert!(
            matches!(read(&img), Err(HeaderlessError::Radius { .. })),
            "{bad}"
        );
    }
    img.put(at + 4, &0.25f32.to_le_bytes());
    img.bytes[at + 2] = 1;
    assert!(matches!(
        read(&img),
        Err(HeaderlessError::Constant { found: 1, .. })
    ));
}

#[test]
fn a_cylinder_on_one_vertex_is_named_unknown_a_capsule_there_is_a_sphere_and_both_read() {
    let (mut img, parts) = bound();
    // No BVH, whose leaf the cylinder would be judged against.
    img.u64(child_of(&img, &parts) + 0x130, 0);
    let at = parts.primitives + 16;
    for (kind, named) in [(2, &[][..]), (4, &[(1, UnknownShape::EndsOnOneVertex)][..])] {
        img.bytes[at + 3] = kind;
        img.u16(at, 1);
        img.u16(at + 8, 1);
        let geometry = only_geometry(&img.bytes).expect("the child reads");
        assert_eq!(geometry.unknown_shapes, named, "type {kind}");
        img.u16(at + 8, 2);
        let geometry = only_geometry(&img.bytes).expect("the child reads");
        assert!(geometry.unknown_shapes.is_empty(), "type {kind}");
    }
}

#[test]
fn a_cylinder_whose_box_from_its_fields_misses_its_bvh_leaf_is_named_unknown() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    let bvh = u64_at(&img.bytes, child + 0x130) as usize & 0x0FFF_FFFF;
    // A quantum of a millimetre about the origin; leaf 1 holds the triangle and a cylinder of
    // radius 0.25 along x from vertex 0 to vertex 1.
    img.vec3(bvh + 0x40, [0.001; 3]);
    let leaf = |img: &mut Image, node: usize, low: [i16; 3], high: [i16; 3], first, count| {
        let at = parts.nodes + 16 * node;
        for a in 0..3 {
            img.u16(at + 2 * a, low[a] as u16);
            img.u16(at + 6 + 2 * a, high[a] as u16);
        }
        img.u16(at + 0xC, first);
        img.u16(at + 0xE, count);
    };
    leaf(&mut img, 1, [0, -250, -250], [1000, 1000, 250], 0, 2);
    let at = parts.primitives + 16;
    img.bytes[at + 3] = 4;
    img.u16(at + 8, 1);
    let cylinder = |img: &mut Image, radius: f32| {
        img.put(at + 4, &radius.to_le_bytes());
        only_geometry(&img.bytes)
            .expect("the child reads")
            .unknown_shapes
    };
    assert_eq!(cylinder(&mut img, 0.25), []);
    assert_eq!(
        cylinder(&mut img, 0.3),
        [(1, UnknownShape::CylinderOffItsLeaf)],
        "past the leaf"
    );
    assert_eq!(cylinder(&mut img, 0.1), [], "inside a leaf it shares");
    leaf(&mut img, 0, [0, -250, -250], [1000, 250, 250], 1, 1);
    leaf(&mut img, 1, [0, 0, 0], [1000, 1000, 0], 0, 1);
    assert_eq!(cylinder(&mut img, 0.25), []);
    assert_eq!(
        cylinder(&mut img, 0.1),
        [(1, UnknownShape::CylinderOffItsLeaf)],
        "short of a leaf it holds alone"
    );
    // A tree past half the section: read twice from the budget, it would be refused as aliased.
    let nodes = img.alloc(16 * 64);
    let leaves = img.bytes[parts.nodes..parts.nodes + 32].to_vec();
    img.put(nodes, &leaves);
    img.ptr(bvh + 0x50, nodes);
    img.u32(bvh + 0x58, 64);
    assert!(16 * 64 > img.bytes.len() / 2);
    assert_eq!(
        cylinder(&mut img, 0.1),
        [(1, UnknownShape::CylinderOffItsLeaf)]
    );
}

#[test]
fn a_primitive_child_s_material_id_reads_from_its_two_lanes_and_bit_15_is_refused() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    let half = [0.5f32, 0.75, 1.25];
    let radius = half.iter().map(|h| h * h).sum::<f32>().sqrt();
    put_primitive(&mut img, child, &(3, vec![], half, radius, 0.04));
    img.u32(child + 0x4C, 0x0003_7646);
    img.u32(child + 0x5C, 0x42);
    let read = read_bound(&img.bytes).expect("the composite reads");
    assert_eq!(read.children[0].material_id, Some(0x42_0003_7646));
    assert_eq!(material_key(0x42_0003_7646), 0x7646);
    assert!(matches!(
        read.children[0].bound,
        Some(Ok(ChildBound::Box { .. }))
    ));
    img.u32(child + 0x4C, 0x0003_F646);
    let read = read_bound(&img.bytes).expect("the composite reads");
    assert_eq!(read.children[0].material_id, None);
    assert!(matches!(
        read.children[0].bound,
        Some(Err(HeaderlessError::MaterialBit15 { .. }))
    ));
}

#[test]
fn the_composite_s_bvh_covers_its_children_once() {
    let (mut img, parts) = bound();
    let bvh = img.alloc(0x70);
    img.vec3(bvh + 0x10, [1.0, 1.0, 0.0]);
    let node = img.alloc(16);
    img.u16(node + 0xE, 1);
    img.ptr(bvh + 0x50, node);
    img.u32(bvh + 0x58, 1);
    img.ptr(parts.root + 0xA0, bvh);
    let read = read_bound(&img.bytes).expect("the bound reads");
    let tree = read.bvh.expect("a composite BVH");
    assert_eq!(
        (tree.nodes, tree.leaves, tree.box_max),
        (1, 1, [1.0, 1.0, 0.0])
    );
    img.u16(node + 0xE, 2);
    assert!(matches!(
        read_bound(&img.bytes),
        Err(HeaderlessError::IndexPastCount {
            index: 2,
            count: 1,
            ..
        })
    ));
}

#[test]
fn a_geometry_child_of_type_4_reads_without_a_bvh_and_states_its_vertices_as_its_hull() {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    img.bytes[child + 0x10] = super::super::bound::PLAIN_GEOMETRY;
    img.u16(child + 0x84, 3);
    // Its triangle alone: a type-4 geometry holds triangles only.
    img.u32(child + 0xD4, 1);
    let geometry = only_geometry(&img.bytes).expect("a type-4 geometry reads");
    assert_eq!(geometry.vertices, TRIANGLE_AT.to_vec());
    assert!(
        geometry.bvh.is_none(),
        "the word at +0x130 is past its record"
    );
    img.u16(child + 0x84, 4);
    assert!(matches!(
        only_geometry(&img.bytes),
        Err(HeaderlessError::Disagree {
            first: 4,
            second: 3,
            ..
        })
    ));
}

#[test]
fn a_child_s_material_and_shape_and_a_polygon_s_material_write_in_place() {
    // A geometry of two materials, its triangle naming the second.
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    let ids = img.alloc(16);
    img.u64(ids, 0x11);
    img.u64(ids + 8, 0x4200_0003_7646);
    let list = img.alloc(2);
    img.put(list, &[1, 0]);
    img.bytes[child + 0x120] = 2;
    img.ptr(child + 0xF0, ids);
    img.ptr(child + 0x118, list);
    let original = img.bytes.clone();
    let mut bytes = original.clone();
    let unchanged = read_bound(&original).expect("the bound reads");
    write_bound(&mut bytes, &unchanged).expect("nothing edited writes");
    assert_eq!(bytes, original);
    let mut edited = read_bound(&original).expect("the bound reads");
    let Some(Ok(ChildBound::Geometry(g))) = edited.children[0].bound.as_mut() else {
        panic!("a geometry child");
    };
    g.primitive_materials = vec![0, 1];
    g.material_ids[1] = 0x4200_0003_0022;
    write_bound(&mut bytes, &edited).expect("the materials write in place");
    assert_eq!(bytes.len(), original.len());
    let read = only_geometry(&bytes).expect("it reads back");
    assert_eq!(read.primitive_materials, [0, 1]);
    assert_eq!(
        (read.primitive_key(0), read.primitive_key(1)),
        (Some(0x11), Some(0x22))
    );

    // A sphere child whose radius and material key change.
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    put_primitive(&mut img, child, &(0, vec![], [0.4; 3], 0.4, 0.4));
    img.u32(child + 0x4C, 0x0001_1234);
    img.u32(child + 0x5C, 0x42);
    let mut bytes = img.bytes.clone();
    let mut edited = read_bound(&bytes).expect("the sphere reads");
    edited.children[0].bound = Some(Ok(ChildBound::Sphere {
        centre: CENTRE,
        radius: 0.6,
    }));
    edited.children[0].material_id = Some(0x42_0001_0055);
    write_bound(&mut bytes, &edited).expect("the sphere writes in place");
    let read = read_bound(&bytes).expect("it reads back");
    assert_eq!(read.children[0].material_id, Some(0x42_0001_0055));
    let Some(Ok(ChildBound::Sphere { centre, radius })) = &read.children[0].bound else {
        panic!("a sphere, not {:?}", read.children[0].bound);
    };
    assert_eq!((*centre, *radius), (CENTRE, 0.6));
    // Another type is another record.
    edited.children[0].bound = Some(Ok(ChildBound::Box {
        box_min: [0.0; 3],
        box_max: [1.0; 3],
    }));
    assert!(matches!(
        write_bound(&mut bytes, &edited),
        Err(HeaderlessError::NotInPlace(_))
    ));
}

#[path = "write_bound_vertex.rs"]
mod write_bound_vertex;

#[path = "write_bound_reshape.rs"]
mod write_bound_reshape;

#[path = "write_bound_grow.rs"]
mod write_bound_grow;

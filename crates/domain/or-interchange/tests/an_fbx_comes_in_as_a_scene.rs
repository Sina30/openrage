//! A binary FBX read into a scene, on documents the suite writes node by node as a modelling tool does.

use std::io::Cursor;

use fbxcel::low::FbxVersion;
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};
use or_interchange::{fbx, LodSlot, Scene};

enum V {
    Id(i64),
    I(i32),
    D(f64),
    S(String),
    Ds(Vec<f64>),
    Is(Vec<i32>),
}

struct Node {
    name: &'static str,
    values: Vec<V>,
    children: Vec<Node>,
}

fn n(name: &'static str, values: Vec<V>, children: Vec<Node>) -> Node {
    Node {
        name,
        values,
        children,
    }
}

fn s(text: &str) -> V {
    V::S(text.to_string())
}

fn write_node(writer: &mut Writer<Cursor<Vec<u8>>>, node: &Node) {
    {
        let mut attributes = writer.new_node(node.name).unwrap();
        for value in &node.values {
            match value {
                V::Id(v) => attributes.append_i64(*v).unwrap(),
                V::I(v) => attributes.append_i32(*v).unwrap(),
                V::D(v) => attributes.append_f64(*v).unwrap(),
                V::S(v) => attributes.append_string_direct(v).unwrap(),
                V::Ds(v) => attributes
                    .append_arr_f64_from_iter(None, v.iter().copied())
                    .unwrap(),
                V::Is(v) => attributes
                    .append_arr_i32_from_iter(None, v.iter().copied())
                    .unwrap(),
            }
        }
    }
    for child in &node.children {
        write_node(writer, child);
    }
    writer.close_node().unwrap();
}

fn document(nodes: &[Node]) -> Vec<u8> {
    let mut writer = Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_4).unwrap();
    for node in nodes {
        write_node(&mut writer, node);
    }
    writer.finalize(&FbxFooter::default()).unwrap().into_inner()
}

/// A `Properties70` of `P` nodes, each a name, its three descriptors and its numbers.
fn properties(rows: &[(&str, &[f64])]) -> Node {
    n(
        "Properties70",
        vec![],
        rows.iter()
            .map(|(name, numbers)| {
                let mut values = vec![s(name), s("Vector3D"), s("Vector"), s("A")];
                values.extend(numbers.iter().map(|v| V::D(*v)));
                n("P", values, vec![])
            })
            .collect(),
    )
}

/// The settings of Blender's default export: Z up, a front along -Y, X across, in centimetres.
fn blender_settings() -> Node {
    settings(2, 1, 1, -1, 0, 1, 1.0)
}

fn settings(
    up: i32,
    up_sign: i32,
    front: i32,
    front_sign: i32,
    coord: i32,
    coord_sign: i32,
    unit: f64,
) -> Node {
    let int = |name: &str, v: i32| {
        n(
            "P",
            vec![s(name), s("int"), s("Integer"), s(""), V::I(v)],
            vec![],
        )
    };
    n(
        "GlobalSettings",
        vec![],
        vec![n(
            "Properties70",
            vec![],
            vec![
                int("UpAxis", up),
                int("UpAxisSign", up_sign),
                int("FrontAxis", front),
                int("FrontAxisSign", front_sign),
                int("CoordAxis", coord),
                int("CoordAxisSign", coord_sign),
                n(
                    "P",
                    vec![
                        s("UnitScaleFactor"),
                        s("double"),
                        s("Number"),
                        s(""),
                        V::D(unit),
                    ],
                    vec![],
                ),
            ],
        )],
    )
}

fn layer(name: &'static str, mapping: &str, reference: &str, data: &[(&'static str, V)]) -> Node {
    let mut children = vec![
        n("Version", vec![V::I(101)], vec![]),
        n("Name", vec![s("")], vec![]),
        n("MappingInformationType", vec![s(mapping)], vec![]),
        n("ReferenceInformationType", vec![s(reference)], vec![]),
    ];
    for (key, value) in data {
        let value = match value {
            V::Ds(v) => V::Ds(v.clone()),
            V::Is(v) => V::Is(v.clone()),
            _ => unreachable!("a layer holds arrays"),
        };
        children.push(n(key, vec![value], vec![]));
    }
    n(name, vec![V::I(0)], children)
}

fn geometry(
    id: i64,
    name: &str,
    vertices: Vec<f64>,
    polygons: Vec<i32>,
    layers: Vec<Node>,
) -> Node {
    let mut children = vec![
        n("Vertices", vec![V::Ds(vertices)], vec![]),
        n("PolygonVertexIndex", vec![V::Is(polygons)], vec![]),
    ];
    children.extend(layers);
    n(
        "Geometry",
        vec![V::Id(id), s(&format!("{name}\0\u{1}Geometry")), s("Mesh")],
        children,
    )
}

fn model(id: i64, name: &str, subclass: &str, rows: &[(&str, &[f64])]) -> Node {
    n(
        "Model",
        vec![V::Id(id), s(&format!("{name}\0\u{1}Model")), s(subclass)],
        vec![n("Version", vec![V::I(232)], vec![]), properties(rows)],
    )
}

/// `child` is connected to `parent`, which is the direction every FBX connection of an object reads.
fn connect(child: i64, parent: i64) -> Node {
    n("C", vec![s("OO"), V::Id(child), V::Id(parent)], vec![])
}

/// The six faces of a cube of half-width `half`, each a quad with its own normal and the same four coordinates.
struct Cube {
    vertices: Vec<f64>,
    polygons: Vec<i32>,
    normals: Vec<f64>,
    uvs: Vec<f64>,
    uv_index: Vec<i32>,
}

fn cube(half: f64) -> Cube {
    let mut out = Cube {
        vertices: Vec::new(),
        polygons: Vec::new(),
        normals: Vec::new(),
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        uv_index: Vec::new(),
    };
    for i in 0..8 {
        for axis in 0..3 {
            out.vertices
                .push(if i >> axis & 1 == 1 { half } else { -half });
        }
    }
    let point = |axis: usize, sign: i32, u: i32, v: i32| -> i32 {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut bits = [0; 3];
        bits[axis] = i32::from(sign > 0);
        bits[a] = i32::from(u > 0);
        bits[b] = i32::from(v > 0);
        bits[0] | bits[1] << 1 | bits[2] << 2
    };
    for axis in 0..3 {
        for sign in [1, -1] {
            // Counter-clockwise seen from outside: (u, v) in the cyclic order, reversed for the far side.
            let mut ring = [(-1, -1), (1, -1), (1, 1), (-1, 1)];
            if sign < 0 {
                ring.reverse();
            }
            for (corner, (u, v)) in ring.iter().enumerate() {
                let last = corner == 3;
                let p = point(axis, sign, *u, *v);
                out.polygons.push(if last { !p } else { p });
                let mut normal = [0.0; 3];
                normal[axis] = f64::from(sign);
                out.normals.extend(normal);
                out.uv_index.push(corner as i32);
            }
        }
    }
    out
}

/// A cube as Blender writes one: normals and coordinates mapped by polygon vertex.
fn cube_file(settings: Node, unit_half: f64) -> Vec<u8> {
    let cube = cube(unit_half);
    document(&[
        settings,
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Cube",
                    cube.vertices.clone(),
                    cube.polygons.clone(),
                    vec![
                        layer(
                            "LayerElementNormal",
                            "ByPolygonVertex",
                            "Direct",
                            &[("Normals", V::Ds(cube.normals.clone()))],
                        ),
                        layer(
                            "LayerElementUV",
                            "ByPolygonVertex",
                            "IndexToDirect",
                            &[
                                ("UV", V::Ds(cube.uvs.clone())),
                                ("UVIndex", V::Is(cube.uv_index.clone())),
                            ],
                        ),
                    ],
                ),
                model(20, "Cube", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20), connect(20, 0)]),
    ])
}

fn load(bytes: &[u8]) -> Scene {
    fbx::load(bytes).unwrap_or_else(|e| panic!("the FBX was refused: {e}"))
}

fn refusal(bytes: &[u8]) -> String {
    match fbx::load(bytes) {
        Ok(_) => panic!("the FBX was read and was expected to be refused"),
        Err(e) => e.to_string(),
    }
}

fn dropped(scene: &Scene, what: &str) -> Option<usize> {
    scene
        .dropped
        .iter()
        .find(|d| d.what.starts_with(what))
        .map(|d| d.count)
}

/// Whether a triangle's right-hand-rule facing agrees with the normals its vertices state, which is
/// what says the winding survived the transform into the drawable's space.
fn faces_agree(scene: &Scene) -> bool {
    scene.primitives().all(|p| {
        p.indices.as_chunks::<3>().0.iter().all(|t| {
            let at = |i: u32| p.positions[i as usize];
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            let edge =
                |from: [f32; 3], to: [f32; 3]| [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            let (u, v) = (edge(a, b), edge(b, c));
            let facing = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let normal = p.normals[t[0] as usize];
            facing[0] * normal[0] + facing[1] * normal[1] + facing[2] * normal[2] > 0.0
        })
    })
}

#[test]
fn a_cube_as_blender_writes_it_comes_in_as_one_mesh_in_metres() {
    // Half a metre either side, as centimetres, which is what the file's unit says.
    let scene = load(&cube_file(blender_settings(), 50.0));

    assert_eq!(scene.objects.len(), 1);
    let object = &scene.objects[0];
    assert_eq!(object.meshes.len(), 1);
    assert_eq!(object.meshes[0].name, "Cube");
    assert_eq!(object.meshes[0].primitives.len(), 1);
    let p = &object.meshes[0].primitives[0];

    // Six quads are twelve triangles, and every corner states a normal of its own, so no
    // vertex is shared: 24.
    assert_eq!(p.triangle_count(), 12);
    assert_eq!(p.vertex_count(), 24);
    assert_eq!(p.normals.len(), 24);
    for position in &p.positions {
        assert!(
            position.iter().all(|c| (c.abs() - 0.5).abs() < 1e-6),
            "{position:?}"
        );
    }
    assert!(p.normals.iter().all(|n| {
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (length - 1.0).abs() < 1e-5
    }));
    assert!(
        faces_agree(&scene),
        "the winding disagrees with the normals"
    );

    // One set of coordinates, on the channel the first one takes, with V turned over.
    assert_eq!(p.uvs.len(), 1);
    assert_eq!(p.uv_channels, vec![6]);
    assert!(p.uvs[0]
        .iter()
        .all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])));
    // The first corner states (0, 0), which is the bottom left of the image and so V = 1 from the top.
    assert_eq!(p.uvs[0][0], [0.0, 1.0]);
    assert_eq!(object.meshes[0].lod, LodSlot::Level(0));
    assert!(scene.dropped.is_empty(), "{:?}", scene.dropped);
}

#[test]
fn a_vertex_that_corners_state_the_same_values_for_is_one_vertex() {
    // A square of two triangles' worth: four points, one polygon, one normal for all.
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Plane",
                    vec![
                        0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 0.0,
                    ],
                    vec![0, 1, 2, !3],
                    vec![layer(
                        "LayerElementNormal",
                        "ByPolygonVertex",
                        "Direct",
                        &[("Normals", V::Ds([0.0, 0.0, 1.0].repeat(4)))],
                    )],
                ),
                model(20, "Plane", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let scene = load(&bytes);
    let p = scene.primitives().next().unwrap();
    assert_eq!((p.vertex_count(), p.triangle_count()), (4, 2));
    assert!(faces_agree(&scene));
}

#[test]
fn a_file_in_maya_axes_is_turned_to_z_up_with_the_front_toward_minus_y() {
    // Y up, Z toward the viewer, X across, in metres: the FBX SDK's own defaults.
    let bytes = document(&[
        settings(1, 1, 2, 1, 0, 1, 100.0),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Corner",
                    // A triangle at the origin, one unit up, one unit to the front, one across.
                    vec![0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
                    vec![0, 1, !2],
                    vec![],
                ),
                model(20, "Corner", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let p = &load(&bytes).objects[0].meshes[0].primitives[0];
    // Up is Z, the front is -Y, across is X.
    assert_eq!(p.positions[0], [0.0, 0.0, 1.0]);
    assert_eq!(p.positions[1], [0.0, -1.0, 0.0]);
    assert_eq!(p.positions[2], [1.0, 0.0, 0.0]);
}

#[test]
fn a_left_handed_file_is_mirrored_and_its_winding_is_turned_with_it() {
    // The same axes with the front reversed make the triple left-handed.
    let bytes = cube_file(settings(2, 1, 1, 1, 0, 1, 1.0), 50.0);
    let scene = load(&bytes);
    assert!(faces_agree(&scene), "a mirrored file came in inside out");
}

#[test]
fn a_models_own_transform_is_applied_in_the_order_the_file_states() {
    // Turned a quarter about X then lifted a metre: a point on +Y goes to +Z, then up.
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Pin",
                    vec![0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0],
                    vec![0, 1, !2],
                    vec![],
                ),
                model(
                    20,
                    "Pin",
                    "Mesh",
                    &[
                        ("Lcl Translation", &[0.0, 0.0, 100.0]),
                        ("Lcl Rotation", &[90.0, 0.0, 0.0]),
                    ],
                ),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let p = &load(&bytes).objects[0].meshes[0].primitives[0];
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
    assert!(
        near(p.positions[0], [0.0, 0.0, 2.0]),
        "{:?}",
        p.positions[0]
    );
    assert!(
        near(p.positions[1], [1.0, 0.0, 2.0]),
        "{:?}",
        p.positions[1]
    );
    assert!(
        near(p.positions[2], [0.0, -1.0, 2.0]),
        "{:?}",
        p.positions[2]
    );
}

#[test]
fn a_child_model_follows_its_parent() {
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Pin",
                    vec![0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
                    vec![0, 1, !2],
                    vec![],
                ),
                model(
                    20,
                    "Pin",
                    "Mesh",
                    &[("Lcl Translation", &[0.0, 0.0, 100.0])],
                ),
                model(
                    30,
                    "Holder",
                    "Null",
                    &[("Lcl Translation", &[200.0, 0.0, 0.0])],
                ),
            ],
        ),
        n(
            "Connections",
            vec![],
            vec![connect(10, 20), connect(20, 30)],
        ),
    ]);
    let p = &load(&bytes).objects[0].meshes[0].primitives[0];
    assert_eq!(p.positions[0], [2.0, 0.0, 1.0]);
}

#[test]
fn polygons_by_material_become_primitives_in_material_order() {
    // Two quads, the first of material 1 and the second of material 0.
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Pair",
                    vec![
                        0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 0.0, 200.0,
                        0.0, 0.0, 200.0, 100.0, 0.0,
                    ],
                    vec![0, 1, 2, !3, 1, 4, 5, !2],
                    vec![layer(
                        "LayerElementMaterial",
                        "ByPolygon",
                        "IndexToDirect",
                        &[("Materials", V::Is(vec![1, 0]))],
                    )],
                ),
                model(20, "Pair", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let primitives = &load(&bytes).objects[0].meshes[0].primitives;
    assert_eq!(primitives.len(), 2);
    // Material 0 first: the second quad, which reaches x = 2.
    assert!(primitives[0].positions.iter().any(|p| p[0] == 2.0));
    assert!(primitives[1].positions.iter().all(|p| p[0] <= 1.0));
    assert!(primitives.iter().all(|p| p.triangle_count() == 2));
}

#[test]
fn a_models_name_says_which_level_of_detail_it_is() {
    let level = |name: &str| {
        let bytes = document(&[
            blender_settings(),
            n(
                "Objects",
                vec![],
                vec![
                    geometry(
                        10,
                        name,
                        (0..9).map(f64::from).collect(),
                        vec![0, 1, !2],
                        vec![],
                    ),
                    model(20, name, "Mesh", &[]),
                ],
            ),
            n("Connections", vec![], vec![connect(10, 20)]),
        ]);
        load(&bytes).objects[0].meshes[0].lod
    };
    assert_eq!(level("subject_lod0_model0"), LodSlot::Level(0));
    assert_eq!(level("subject_lod1_model0"), LodSlot::Level(1));
    assert_eq!(level("subject_lod2"), LodSlot::Level(2));
    assert_eq!(level("lod3"), LodSlot::Level(3));
    assert_eq!(level("subject_container"), LodSlot::Container);
    assert_eq!(level("hold_the_lodge"), LodSlot::Level(0));
}

/// A quad of four points with two bones, as a modelling tool binds one.
fn skinned_file(influences: &[(&[i32], &[f64])]) -> Vec<u8> {
    let identity = |translation: [f64; 3]| {
        let mut m = vec![0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = translation[0];
        m[13] = translation[1];
        m[14] = translation[2];
        V::Ds(m)
    };
    let cluster = |id: i64, name: &str, indexes: &[i32], weights: &[f64], link: [f64; 3]| {
        n(
            "Deformer",
            vec![
                V::Id(id),
                s(&format!("{name}\0\u{1}SubDeformer")),
                s("Cluster"),
            ],
            vec![
                n("Indexes", vec![V::Is(indexes.to_vec())], vec![]),
                n("Weights", vec![V::Ds(weights.to_vec())], vec![]),
                n("Transform", vec![identity([0.0; 3])], vec![]),
                n("TransformLink", vec![identity(link)], vec![]),
            ],
        )
    };
    let mut objects = vec![
        geometry(
            10,
            "Quad",
            vec![
                0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 0.0,
            ],
            vec![0, 1, 2, !3],
            vec![],
        ),
        model(20, "Quad", "Mesh", &[]),
        model(100, "Hip", "LimbNode", &[]),
        model(
            101,
            "Knee",
            "LimbNode",
            &[("Lcl Translation", &[0.0, 100.0, 0.0])],
        ),
        n(
            "Deformer",
            vec![V::Id(300), s("Skin\0\u{1}Deformer"), s("Skin")],
            vec![],
        ),
    ];
    let mut connections = vec![connect(10, 20), connect(101, 100), connect(300, 10)];
    for (k, (indexes, weights)) in influences.iter().enumerate() {
        let (id, bone) = (200 + k as i64, 100 + k as i64);
        let name = if k == 0 { "Hip" } else { "Knee" };
        objects.push(cluster(
            id,
            name,
            indexes,
            weights,
            [0.0, 100.0 * k as f64, 0.0],
        ));
        connections.push(connect(id, 300));
        connections.push(connect(bone, id));
    }
    document(&[
        blender_settings(),
        n("Objects", vec![], objects),
        n("Connections", vec![], connections),
    ])
}

#[test]
fn a_skinned_mesh_brings_its_bones_and_normalised_weights() {
    // Point 0 is all Hip, 1 is all Hip, 2 and 3 are shared with the Knee.
    let bytes = skinned_file(&[
        (&[0, 1, 2, 3], &[1.0, 1.0, 0.5, 0.25]),
        (&[2, 3], &[0.5, 0.75]),
    ]);
    let scene = load(&bytes);
    let object = &scene.objects[0];

    assert_eq!(
        object
            .bones
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        ["Hip", "Knee"]
    );
    assert_eq!(object.bones[0].parent, None);
    assert_eq!(object.bones[1].parent, Some(0));
    // The knee sits a metre above the hip, in the drawable's space.
    let knee = object.bones[1].translation;
    assert!(
        knee[0].abs() < 1e-6 && (knee[1] - 1.0).abs() < 1e-6 && knee[2].abs() < 1e-6,
        "{knee:?}"
    );

    let p = &object.meshes[0].primitives[0];
    assert_eq!(p.joints.len(), 4);
    assert_eq!(p.joints[0], [0, 0, 0, 0]);
    assert_eq!(p.weights[0], [1.0, 0.0, 0.0, 0.0]);
    // Vertex 2: Hip 0.5 and Knee 0.5 of 1.0.
    assert_eq!(&p.joints[2][..2], &[0, 1]);
    assert!((p.weights[2][0] - 0.5).abs() < 1e-6 && (p.weights[2][1] - 0.5).abs() < 1e-6);
    // Vertex 3: Hip 0.25 and Knee 0.75, heaviest first.
    assert_eq!(&p.joints[3][..2], &[1, 0]);
    assert!((p.weights[3][0] - 0.75).abs() < 1e-6);
    for weights in &p.weights {
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }
    assert!(scene.dropped.is_empty(), "{:?}", scene.dropped);
}

#[test]
fn a_vertex_with_more_than_four_influences_keeps_the_heaviest_four_and_says_so() {
    // Six bones' worth of weights on one point, as 6 clusters: build by hand.
    let identity = {
        let mut m = vec![0.0; 16];
        for i in [0, 5, 10, 15] {
            m[i] = 1.0;
        }
        V::Ds(m)
    };
    let mut objects = vec![
        geometry(
            10,
            "Tri",
            vec![0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
            vec![0, 1, !2],
            vec![],
        ),
        model(20, "Tri", "Mesh", &[]),
        n(
            "Deformer",
            vec![V::Id(300), s("Skin\0\u{1}Deformer"), s("Skin")],
            vec![],
        ),
    ];
    let mut connections = vec![connect(10, 20), connect(300, 10)];
    let weights = [0.30, 0.25, 0.20, 0.15, 0.06, 0.04];
    for (k, weight) in weights.iter().enumerate() {
        let (cluster, bone) = (200 + k as i64, 100 + k as i64);
        objects.push(model(bone, &format!("Bone{k}"), "LimbNode", &[]));
        objects.push(n(
            "Deformer",
            vec![V::Id(cluster), s("C\0\u{1}SubDeformer"), s("Cluster")],
            vec![
                n("Indexes", vec![V::Is(vec![0])], vec![]),
                n("Weights", vec![V::Ds(vec![*weight])], vec![]),
                n("Transform", vec![identity.clone_v()], vec![]),
                n("TransformLink", vec![identity.clone_v()], vec![]),
            ],
        ));
        connections.push(connect(cluster, 300));
        connections.push(connect(bone, cluster));
    }
    let bytes = document(&[
        blender_settings(),
        n("Objects", vec![], objects),
        n("Connections", vec![], connections),
    ]);
    let scene = load(&bytes);
    let p = &scene.objects[0].meshes[0].primitives[0];
    assert_eq!(p.joints[0], [0, 1, 2, 3]);
    assert!((p.weights[0].iter().sum::<f32>() - 1.0).abs() < 1e-6);
    assert_eq!(
        dropped(&scene, "vertices with more than four influences"),
        Some(1)
    );
    // The two vertices no cluster names are bound whole to the first joint, and counted.
    assert_eq!(dropped(&scene, "skinned vertices with no weight"), Some(2));
}

trait CloneV {
    fn clone_v(&self) -> V;
}

impl CloneV for V {
    fn clone_v(&self) -> V {
        match self {
            V::Ds(v) => V::Ds(v.clone()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn what_the_file_holds_that_a_drawable_has_no_place_for_is_counted() {
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Tri",
                    vec![0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
                    vec![0, 1, !2],
                    vec![],
                ),
                model(20, "Tri", "Mesh", &[]),
                model(21, "Eye", "Camera", &[]),
                model(22, "Lamp", "Light", &[]),
                n(
                    "Material",
                    vec![V::Id(30), s("Skin\0\u{1}Material"), s("")],
                    vec![],
                ),
                n(
                    "Material",
                    vec![V::Id(31), s("Cloth\0\u{1}Material"), s("")],
                    vec![],
                ),
                n(
                    "Texture",
                    vec![V::Id(32), s("Colour\0\u{1}Texture"), s("")],
                    vec![],
                ),
                n(
                    "AnimationStack",
                    vec![V::Id(33), s("Take 001\0\u{1}AnimStack"), s("")],
                    vec![],
                ),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let scene = load(&bytes);
    assert_eq!(dropped(&scene, "materials"), Some(2));
    assert_eq!(dropped(&scene, "textures"), Some(1));
    assert_eq!(dropped(&scene, "animation stacks"), Some(1));
    assert_eq!(dropped(&scene, "cameras"), Some(1));
    assert_eq!(dropped(&scene, "lights"), Some(1));
}

#[test]
fn a_polygon_of_more_than_four_corners_is_fanned_and_counted() {
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Hex",
                    vec![
                        1.0, 0.0, 0.0, 0.5, 0.9, 0.0, -0.5, 0.9, 0.0, -1.0, 0.0, 0.0, -0.5, -0.9,
                        0.0, 0.5, -0.9, 0.0,
                    ],
                    vec![0, 1, 2, 3, 4, !5],
                    vec![],
                ),
                model(20, "Hex", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let scene = load(&bytes);
    assert_eq!(scene.primitives().next().unwrap().triangle_count(), 4);
    assert_eq!(
        dropped(&scene, "polygons of more than four corners"),
        Some(1)
    );
}

#[test]
fn a_layer_mapped_by_edge_is_dropped_and_named() {
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Tri",
                    vec![0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
                    vec![0, 1, !2],
                    vec![layer(
                        "LayerElementNormal",
                        "ByEdge",
                        "Direct",
                        &[("Normals", V::Ds(vec![0.0, 0.0, 1.0]))],
                    )],
                ),
                model(20, "Tri", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let scene = load(&bytes);
    assert!(scene.primitives().next().unwrap().normals.is_empty());
    assert_eq!(
        dropped(&scene, "layers (LayerElementNormal mapped \"ByEdge\")"),
        Some(1)
    );
}

#[test]
fn a_file_that_is_not_a_binary_fbx_of_a_version_it_reads_is_refused_by_name() {
    assert!(refusal(b"; FBX 7.3.0 project file\n").contains("ASCII"));
    assert!(refusal(b"GIF89a").contains("not a binary FBX"));
    assert!(refusal(b"").contains("not a binary FBX"));

    let mut old = cube_file(blender_settings(), 50.0);
    // The version word follows the 23-byte header.
    old[23..27].copy_from_slice(&6100u32.to_le_bytes());
    let why = refusal(&old);
    assert!(why.contains("6.1") && why.contains("7.1"), "{why}");

    // Cut short, it does not parse and does not panic.
    let whole = cube_file(blender_settings(), 50.0);
    assert!(refusal(&whole[..whole.len() / 2]).contains("does not parse"));
}

#[test]
fn a_file_of_version_seven_three_is_read_as_the_layout_it_shares_with_seven_four() {
    let mut older = cube_file(blender_settings(), 50.0);
    older[23..27].copy_from_slice(&7300u32.to_le_bytes());
    let (a, b) = (load(&older), load(&cube_file(blender_settings(), 50.0)));
    let (a, b) = (
        a.primitives().next().unwrap(),
        b.primitives().next().unwrap(),
    );
    assert_eq!(a.positions, b.positions);
    assert_eq!(a.indices, b.indices);
}

#[test]
fn a_document_that_places_no_mesh_is_refused() {
    let bytes = document(&[
        blender_settings(),
        n("Objects", vec![], vec![model(20, "Empty", "Null", &[])]),
        n("Connections", vec![], vec![]),
    ]);
    assert!(refusal(&bytes).contains("no mesh"));
}

#[test]
fn a_polygon_naming_a_point_the_mesh_does_not_have_is_refused() {
    let bytes = document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(10, "Bad", vec![0.0; 9], vec![0, 1, !7], vec![]),
                model(20, "Bad", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ]);
    let why = refusal(&bytes);
    assert!(why.contains("control point 7 of 3"), "{why}");
}

#[test]
fn a_cluster_whose_lists_disagree_is_refused() {
    let bytes = skinned_file(&[(&[0, 1, 2], &[1.0, 1.0])]);
    let why = refusal(&bytes);
    assert!(why.contains("3 control points and 2 weights"), "{why}");
}

/// A square of one polygon whose layers are `layers`, in Blender's axes.
fn square_file(layers: Vec<Node>) -> Vec<u8> {
    document(&[
        blender_settings(),
        n(
            "Objects",
            vec![],
            vec![
                geometry(
                    10,
                    "Square",
                    vec![
                        0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 0.0,
                    ],
                    vec![0, 1, 2, !3],
                    layers,
                ),
                model(20, "Square", "Mesh", &[]),
            ],
        ),
        n("Connections", vec![], vec![connect(10, 20)]),
    ])
}

#[test]
fn normals_mapped_by_vertex_or_for_all_are_looked_up_as_the_file_maps_them() {
    // Smooth shading: one normal for each of the four points, so the corners share vertices.
    let smooth = load(&square_file(vec![layer(
        "LayerElementNormal",
        "ByVertice",
        "Direct",
        &[(
            "Normals",
            V::Ds(vec![
                0.0, 0.0, 1.0, 0.0, 0.6, 0.8, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
            ]),
        )],
    )]));
    let p = smooth.primitives().next().unwrap();
    assert_eq!((p.vertex_count(), p.triangle_count()), (4, 2));
    assert!((p.normals[1][1] - 0.6).abs() < 1e-6 && (p.normals[1][2] - 0.8).abs() < 1e-6);

    // Indexed and for all: one normal, named by an index.
    let flat = load(&square_file(vec![layer(
        "LayerElementNormal",
        "AllSame",
        "IndexToDirect",
        &[
            ("Normals", V::Ds(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0])),
            ("NormalsIndex", V::Is(vec![1])),
        ],
    )]));
    let p = flat.primitives().next().unwrap();
    assert!(
        p.normals.iter().all(|n| *n == [0.0, 0.0, 1.0]),
        "{:?}",
        p.normals
    );
}

#[test]
fn colours_come_in_on_the_drawables_colour_channels_in_the_order_the_file_lists_them() {
    let colours = |first: [f64; 4], second: [f64; 4]| {
        let layer_of = |c: [f64; 4]| {
            layer(
                "LayerElementColor",
                "ByPolygonVertex",
                "IndexToDirect",
                &[
                    ("Colors", V::Ds(c.to_vec())),
                    ("ColorIndex", V::Is(vec![0, 0, 0, 0])),
                ],
            )
        };
        load(&square_file(vec![layer_of(first), layer_of(second)]))
    };
    let scene = colours([1.0, 0.0, 0.0, 1.0], [0.0, 0.5, 0.0, 0.25]);
    let p = scene.primitives().next().unwrap();
    // Diffuse, then specular.
    assert_eq!(p.colour_channels, vec![4, 5]);
    assert_eq!(p.colours[0][0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(p.colours[1][0], [0.0, 0.5, 0.0, 0.25]);
}

#[test]
fn a_tangent_and_its_binormal_make_a_tangent_with_the_side_the_bitangent_lies_on() {
    let tangents = |binormal: [f64; 3]| {
        let scene = load(&square_file(vec![
            layer(
                "LayerElementNormal",
                "ByPolygonVertex",
                "Direct",
                &[("Normals", V::Ds([0.0, 0.0, 1.0].repeat(4)))],
            ),
            layer(
                "LayerElementTangent",
                "ByPolygonVertex",
                "Direct",
                &[("Tangents", V::Ds([1.0, 0.0, 0.0].repeat(4)))],
            ),
            layer(
                "LayerElementBinormal",
                "ByPolygonVertex",
                "Direct",
                &[("Binormals", V::Ds(binormal.repeat(4)))],
            ),
        ]));
        let tangents = scene.primitives().next().unwrap().tangents.clone();
        tangents
    };
    // The cross of the normal and the tangent is +Y: a binormal along it is a right-handed frame.
    assert!(tangents([0.0, 1.0, 0.0])
        .iter()
        .all(|t| *t == [1.0, 0.0, 0.0, 1.0]));
    assert!(tangents([0.0, -1.0, 0.0])
        .iter()
        .all(|t| *t == [1.0, 0.0, 0.0, -1.0]));
}

#[test]
fn a_cycle_of_parents_and_a_very_deep_chain_are_read_without_overflowing() {
    let mesh = |extra_models: Vec<Node>, links: Vec<Node>| {
        let mut objects = vec![
            geometry(
                10,
                "Pin",
                vec![0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
                vec![0, 1, !2],
                vec![],
            ),
            model(
                20,
                "Pin",
                "Mesh",
                &[("Lcl Translation", &[0.0, 0.0, 100.0])],
            ),
        ];
        objects.extend(extra_models);
        let mut connections = vec![connect(10, 20)];
        connections.extend(links);
        load(&document(&[
            blender_settings(),
            n("Objects", vec![], objects),
            n("Connections", vec![], connections),
        ]))
    };

    // The mesh hangs from a model that hangs from the mesh.
    let cycle = mesh(
        vec![model(
            30,
            "Loop",
            "Null",
            &[("Lcl Translation", &[100.0, 0.0, 0.0])],
        )],
        vec![connect(20, 30), connect(30, 20)],
    );
    assert_eq!(cycle.primitives().count(), 1);

    // Twenty thousand models deep: far past any rig, and a recursion that would not come back.
    let chain: Vec<Node> = (0..20_000)
        .map(|i| {
            model(
                100 + i,
                "Link",
                "Null",
                &[("Lcl Translation", &[1.0, 0.0, 0.0])],
            )
        })
        .collect();
    let mut links: Vec<Node> = (0..19_999).map(|i| connect(100 + i, 101 + i)).collect();
    links.push(connect(20, 100));
    assert_eq!(mesh(chain, links).primitives().count(), 1);
}

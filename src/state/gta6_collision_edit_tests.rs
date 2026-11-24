use std::path::PathBuf;

use or_io::gta6::headerless::{
    Bone, Bound, BoundChild, BoundGeometry, Bvh, ChildBound, Fragment, FragmentChild,
    FragmentGroup, Primitive, Skeleton,
};

use super::*;
use crate::state::document::Document;

const OPEN: u16 = 0xFFFF;
const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 0.0],
];
/// Where the second bone lies: a metre up from the first, not turned.
const UP: [f32; 3] = [0.0, 0.0, 1.0];

/// A state holding `resource` as its one document, at `path`.
fn holding(resource: Resource, path: &str) -> AppState {
    let mut state = AppState::new();
    let payload = DocPayload::Gta6Drawable(Arc::new(resource));
    state
        .documents
        .push(Document::new(1, Some(PathBuf::from(path)), payload));
    state.active_document = Some(1);
    state
}

fn resource(state: &AppState) -> &Resource {
    match state.document(1).map(|d| &d.payload) {
        Some(DocPayload::Gta6Drawable(r)) => r,
        _ => panic!("a GTA VI body"),
    }
}

fn bound_of(state: &AppState) -> &Bound {
    match resource(state) {
        Resource::Bound(b) => b,
        Resource::DrawableWithBound(r) => r.bound.as_ref().expect("a composite"),
        other => panic!("a bound, not {other:?}"),
    }
}

fn fragment_of(state: &AppState) -> &Fragment {
    match resource(state) {
        Resource::DrawableWithBound(r) => &r.fragment,
        other => panic!("a fragment, not {other:?}"),
    }
}

fn primitives(state: &AppState) -> Vec<Primitive> {
    match &bound_of(state).children[0].bound {
        Some(Ok(ChildBound::Geometry(g))) => g.primitives.clone(),
        other => panic!("a geometry, not {other:?}"),
    }
}

/// A composite of one geometry child over a right triangle and a fourth vertex below its first
/// edge: triangle 0 over 0, 1, 2 and triangle 1 over 1, 3, 2, linked across their edge from 1 to 2.
fn two_triangles(bvh: bool) -> Resource {
    let geometry = BoundGeometry {
        box_min: [-1.0; 3],
        box_max: [1.0; 3],
        vertices: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, -1.0, -0.5],
        ],
        primitives: vec![
            Primitive::Triangle {
                vertices: [0, 1, 2],
                neighbours: [OPEN, 1, OPEN],
                edges: [0xFF, 9, 0xFF],
            },
            Primitive::Triangle {
                vertices: [1, 3, 2],
                neighbours: [OPEN, OPEN, 0],
                edges: [0xFF, 0xFF, 9],
            },
        ],
        material_ids: Vec::new(),
        primitive_materials: Vec::new(),
        bvh: bvh.then_some(Bvh {
            box_min: [-1.0; 3],
            box_max: [1.0; 3],
            nodes: 3,
            leaves: 2,
        }),
        unknown_shapes: Vec::new(),
    };
    Resource::Bound(Bound {
        radius: 2.0,
        box_min: [-1.0; 3],
        box_max: [1.0; 3],
        centroid: [0.0; 3],
        children: vec![BoundChild {
            matrix: IDENTITY,
            flags: Some([0x13, 0]),
            bound: Some(Ok(ChildBound::Geometry(geometry))),
            material_id: None,
        }],
        bvh: None,
    })
}

#[test]
fn a_rewired_triangle_links_across_its_new_edge_and_undoes_whole() {
    let mut state = holding(two_triangles(true), "0x1010.bound");
    let read = primitives(&state);
    let rewire = CollisionEdit::Rewire {
        child: 0,
        primitive: 1,
        vertices: [1, 0, 3],
    };
    state
        .edit_gta6_collision(1, rewire)
        .expect("a triangle of a geometry with a BVH");
    let now = primitives(&state);
    assert!(
        matches!(
            now[1],
            Primitive::Triangle {
                vertices: [1, 0, 3],
                neighbours: [0, OPEN, OPEN],
                ..
            }
        ),
        "{now:?}"
    );
    assert!(
        matches!(
            now[0],
            Primitive::Triangle {
                neighbours: [1, OPEN, OPEN],
                ..
            }
        ),
        "the first triangle now links across 0 to 1 and no longer across 1 to 2: {now:?}"
    );
    let twice = CollisionEdit::Rewire {
        child: 0,
        primitive: 1,
        vertices: [1, 1, 3],
    };
    assert!(
        state.edit_gta6_collision(1, twice).is_err(),
        "a vertex twice"
    );
    assert_eq!(state.undo().as_deref(), Some("Rewired a triangle"));
    assert_eq!(primitives(&state), read);
    assert!(state.redo().is_some());
    assert_eq!(primitives(&state), now);

    let mut plain = holding(two_triangles(false), "0x1010.bound");
    assert!(
        plain.edit_gta6_collision(1, rewire).is_err(),
        "no BVH: shrunk vertices"
    );
    assert!(plain.undo().is_none(), "a refused edit made no step");
}

#[test]
fn a_bound_child_s_type_and_include_entry_is_a_step_and_a_composite_with_none_refuses_one() {
    let mut state = holding(two_triangles(true), "0x1010.bound");
    let edit = CollisionEdit::ChildFlags {
        child: 0,
        flags: [0x55, 0x66],
    };
    state
        .edit_gta6_collision(1, edit)
        .expect("an entry the composite states");
    assert_eq!(bound_of(&state).children[0].flags, Some([0x55, 0x66]));
    assert!(state.undo().is_some());
    assert_eq!(bound_of(&state).children[0].flags, Some([0x13, 0]));
    let past = CollisionEdit::ChildFlags {
        child: 1,
        flags: [0; 2],
    };
    assert!(
        state.edit_gta6_collision(1, past).is_err(),
        "no second child"
    );

    let mut none = two_triangles(true);
    if let Resource::Bound(b) = &mut none {
        b.children[0].flags = None;
    }
    let mut state = holding(none, "0x1010.bound");
    assert!(state.edit_gta6_collision(1, edit).is_err());
    assert!(state.undo().is_none());
}

fn bone(name: &str, parent: Option<u16>, translation: [f32; 3]) -> Bone {
    Bone {
        name: name.to_string(),
        hash: or_io::gta6::hash::name_hash32(name.as_bytes()),
        parent,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation,
        scale: [1.0; 3],
        inverse_bind: [[0.0; 3]; 4],
    }
}

fn bones() -> Vec<Bone> {
    vec![bone("root", None, [0.0; 3]), bone("lid", Some(0), UP)]
}

/// A matrix of the composite's: the identity's axes at `at`.
fn at(at: [f32; 3]) -> [[f32; 4]; 4] {
    let mut m = IDENTITY;
    m[3][..3].copy_from_slice(&at);
    m
}

/// A fragment of two groups, one a bone, each holding one child the composite lists: a sphere of
/// half a metre at its bone.
fn two_groups() -> Resource {
    let bones = bones();
    let group = |b: u16, children: std::ops::Range<u16>| FragmentGroup {
        bone: b,
        bone_hash: bones[usize::from(b)].hash,
        parent: None,
        child_groups: 0..0,
        second_set: children.end..children.end,
        children,
    };
    let child = |g: u16, place: [f32; 3]| FragmentChild {
        group: g,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.0; 3],
        angular_inertia: [1.0; 3],
        mass: 1.0,
        composite: Some(g),
        bound: None,
        flags: [0x13 + u64::from(g), 0],
        bound_word: 0x5000_1000 + 0x100 * u64::from(g),
        matrix: at(place),
    };
    let sphere = |g: u16, place: [f32; 3]| BoundChild {
        matrix: at(place),
        flags: Some([0x13 + u64::from(g), 0]),
        bound: Some(Ok(ChildBound::Sphere {
            centre: [0.0; 3],
            radius: 0.5,
        })),
        material_id: Some(0x7646),
    };
    Resource::DrawableWithBound(or_io::gta6::headerless::DrawableWithBound {
        name: "lidded".to_string(),
        drawable: crate::state::opening_a_ttpf::drawable_keyed(1),
        bound: Some(Bound {
            radius: 2.0,
            box_min: [-0.5, -0.5, -0.5],
            box_max: [0.5, 0.5, 1.5],
            centroid: [0.0, 0.0, 0.5],
            children: vec![sphere(0, [0.0; 3]), sphere(1, UP)],
            bvh: None,
        }),
        fragment: Box::new(Fragment {
            groups: vec![group(0, 0..1), group(1, 1..2)],
            children: vec![child(0, [0.0; 3]), child(1, UP)],
            ..Fragment::default()
        }),
    })
}

/// A fragment state whose skeleton the scan kept for `path`.
fn fragment_state(path: &str) -> AppState {
    let skeleton = Skeleton { bones: bones() };
    crate::state::rpf9_scan::keep_scanned_skeleton(std::path::Path::new(path), skeleton);
    holding(two_groups(), path)
}

#[test]
fn a_moved_fragment_child_places_its_composite_matrix_and_undoes_whole() {
    let mut state = fragment_state("0x2010.fragment");
    let step = [0.25, 0.0, 0.0];
    let edit = CollisionEdit::ChildTransform {
        child: 0,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: step,
    };
    state
        .edit_gta6_collision(1, edit)
        .expect("a unit turn and a step");
    assert_eq!(fragment_of(&state).children[0].translation, step);
    let matrix = bound_of(&state).children[0].matrix;
    assert_eq!(matrix[3][..3], step, "the first bone is the identity");
    assert_eq!(
        bound_of(&state).box_max[0],
        0.75,
        "the box reaches the moved sphere"
    );
    let half = CollisionEdit::ChildTransform {
        child: 0,
        rotation: [0.0, 0.0, 0.0, 0.5],
        translation: step,
    };
    assert!(
        state.edit_gta6_collision(1, half).is_err(),
        "not a unit turn"
    );
    assert_eq!(state.undo().as_deref(), Some("Moved a fragment's child"));
    assert_eq!(fragment_of(&state).children[0].translation, [0.0; 3]);
    assert_eq!(bound_of(&state).children[0].matrix, IDENTITY);
    assert_eq!(bound_of(&state).box_max, [0.5, 0.5, 1.5]);

    let mut unscanned = holding(two_groups(), "0x2010.unscanned");
    assert!(
        unscanned.edit_gta6_collision(1, edit).is_err(),
        "no skeleton was kept"
    );
}

#[test]
fn a_child_given_another_group_keeps_where_it_lies_and_undoes_to_its_place() {
    let mut state = fragment_state("0x3010.fragment");
    let words: Vec<u64> = fragment_of(&state)
        .children
        .iter()
        .map(|c| c.bound_word)
        .collect();
    let edit = CollisionEdit::ChildGroup { child: 0, group: 1 };
    state
        .edit_gta6_collision(1, edit)
        .expect("a group on a bone");
    let f = fragment_of(&state);
    let groups: Vec<_> = (f.groups.iter())
        .map(|g| (g.children.clone(), g.second_set.clone()))
        .collect();
    assert_eq!(groups, [(0..0, 0..0), (0..2, 2..2)]);
    let order: Vec<_> = (f.children.iter())
        .map(|c| (c.bound_word, c.group, c.composite))
        .collect();
    assert_eq!(order, [(words[1], 1, Some(0)), (words[0], 1, Some(1))]);
    // Off the bone a metre up, it lies a metre below that bone, where it lay.
    assert_eq!(f.children[1].translation, [0.0, 0.0, -1.0]);
    let placed: Vec<[f32; 4]> = bound_of(&state)
        .children
        .iter()
        .map(|c| c.matrix[3])
        .collect();
    assert_eq!(
        placed,
        [at(UP)[3], IDENTITY[3]],
        "the composite follows its children's order"
    );

    assert_eq!(
        state.undo().as_deref(),
        Some("Gave a fragment's child another group")
    );
    let f = fragment_of(&state);
    let order: Vec<_> = (f.children.iter())
        .map(|c| (c.bound_word, c.group, c.composite, c.translation))
        .collect();
    assert_eq!(
        order,
        [
            (words[0], 0, Some(0), [0.0; 3]),
            (words[1], 1, Some(1), [0.0; 3])
        ]
    );
    assert_eq!(bound_of(&state).children[0].matrix, IDENTITY);
    assert!(state.redo().is_some());
    assert_eq!(fragment_of(&state).children[1].bound_word, words[0]);

    let mut state = fragment_state("0x3010.fragment");
    let nowhere = CollisionEdit::ChildGroup { child: 0, group: 2 };
    assert!(
        state.edit_gta6_collision(1, nowhere).is_err(),
        "no third group"
    );
    assert!(state.undo().is_none());
}

#[test]
fn a_fragment_child_s_entry_sets_its_copy_and_the_composite_s_together() {
    let mut state = fragment_state("0x4010.fragment");
    let edit = CollisionEdit::ChildFlags {
        child: 1,
        flags: [0x77, 0x1],
    };
    state
        .edit_gta6_collision(1, edit)
        .expect("a listed child's entry");
    assert_eq!(fragment_of(&state).children[1].flags, [0x77, 0x1]);
    assert_eq!(bound_of(&state).children[1].flags, Some([0x77, 0x1]));
    assert!(state.undo().is_some());
    assert_eq!(fragment_of(&state).children[1].flags, [0x14, 0]);
    assert_eq!(bound_of(&state).children[1].flags, Some([0x14, 0]));
}

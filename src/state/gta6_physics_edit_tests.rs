use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::rpf::{read_rpf9_body, Rpf9Sections};
use or_io::gta6::headerless::{
    read_bound, read_navmesh, Archetype, Fragment, FragmentChild, SMALL, WATER,
};

use super::*;
use crate::state::opening_a_gta_vi_body::bound_body;
use crate::state::opening_an_rpf9_archive::{
    navmesh_cell, row_at, rows, rpf9_head, scanned, settle, written, zlib,
};

const V: u64 = 0x5000_0000;
/// The sphere child's centre, and where its record lies in the body.
const CENTRE: [f32; 3] = [0.25, -0.5, 1.0];
const CHILD_AT: usize = 0x180;
/// The sphere child's material id, its low word at +0x4C and its high word at +0x5C.
const MATERIAL: u64 = 0x7_0002_1234;

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn floats(v: &mut [u8], at: usize, xs: &[f32]) {
    for (i, x) in xs.iter().enumerate() {
        put(v, at + 4 * i, &x.to_le_bytes());
    }
}

/// The bound body whose one child is a sphere of radius 0.5 about [`CENTRE`].
fn sphere_body() -> Vec<u8> {
    let mut v = bound_body();
    // Up to the block map at 0x3E0, which stays.
    v[CHILD_AT..0x3E0].fill(0);
    let at = CHILD_AT;
    floats(&mut v, at + 0x14, &[0.5]);
    floats(&mut v, at + 0x20, &CENTRE.map(|c| c + 0.5));
    floats(&mut v, at + 0x2C, &[0.5]);
    floats(&mut v, at + 0x30, &CENTRE.map(|c| c - 0.5));
    floats(&mut v, at + 0x40, &CENTRE);
    put(&mut v, at + 0x4C, &(MATERIAL as u32).to_le_bytes());
    put(&mut v, at + 0x5C, &((MATERIAL >> 32) as u32).to_le_bytes());
    v
}

/// The bound body whose geometry child holds two materials, its first primitive naming the second.
fn geometry_body() -> Vec<u8> {
    let mut v = bound_body();
    let (ids, list) = (0x340, 0x360);
    put(&mut v, ids, &0x11u64.to_le_bytes());
    put(&mut v, ids + 8, &0x4200_0003_7646u64.to_le_bytes());
    put(&mut v, list, &[1, 0]);
    put(&mut v, CHILD_AT + 0xF0, &(V + ids as u64).to_le_bytes());
    put(&mut v, CHILD_AT + 0x118, &(V + list as u64).to_le_bytes());
    v[CHILD_AT + 0x120] = 2;
    v
}

/// The body opened from a scanned archive that holds it at 0x1010.
fn opened_body(dir: &Path, body: &[u8]) -> (AppState, DocumentId) {
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1010, 0);
    bytes.extend(zlib(body));
    let mut state = AppState::new();
    let archive = scanned(&mut state, written(dir, &bytes));
    state.open_scanned_body(archive, row_at(&rows(&state, archive), 0x1010));
    settle(&mut state);
    let doc = (state.active_document)
        .filter(|&d| d != archive)
        .unwrap_or_else(|| panic!("the body: {}", state.error_dialog_message));
    (state, doc)
}

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.extract_jobs.is_running() {
        state.poll_extract_job();
        assert!(Instant::now() < deadline, "the save never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The virtual section a read at offset 0 of the file at `path` returns.
fn read_back(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (find, read) = read_rpf9_body(Cursor::new(bytes), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &read)
        .expect("a resource")
        .virtual_section
}

/// `doc` saved as `name` in `dir`, once the save has ended.
fn saved(state: &mut AppState, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    assert!(state.save_active_document_as(path.clone()));
    finished(state);
    let said: Vec<_> = state.toasts.iter().map(|t| t.message.clone()).collect();
    assert!(path.exists(), "{said:?} {}", state.error_dialog_message);
    path
}

fn bound_of(state: &AppState, doc: DocumentId) -> &Bound {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Gta6Drawable(r)) => match &**r {
            Resource::Bound(b) => b,
            other => panic!("a bound, not {other:?}"),
        },
        _ => panic!("a GTA VI body"),
    }
}

/// The sphere child's material id and radius.
fn sphere(state: &AppState, doc: DocumentId) -> (Option<u64>, f32) {
    let child = &bound_of(state, doc).children[0];
    match child.bound {
        Some(Ok(ChildBound::Sphere { radius, .. })) => (child.material_id, radius),
        ref other => panic!("a sphere, not {other:?}"),
    }
}

#[test]
fn a_primitive_child_s_key_and_radius_undo_and_save_in_place() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = opened_body(dir.path(), &sphere_body());
    assert_eq!(sphere(&state, doc), (Some(MATERIAL), 0.5));
    let copy = read_back(&saved(&mut state, dir.path(), "copy.rsc"));

    let key = 0x0055;
    let keyed = (MATERIAL & !0x7FFF) | key;
    let edit = BoundEdit::ChildKey {
        child: 0,
        key: key as u16,
    };
    state
        .edit_gta6_bound(doc, edit)
        .expect("a primitive child's key");
    let radius = BoundEdit::Dimensions {
        child: 0,
        values: vec![0.75],
    };
    state
        .edit_gta6_bound(doc, radius)
        .expect("a sphere's radius");
    // Refused, changing nothing: a sphere has one dimension, a length is not negative, and a key
    // is fifteen bits.
    for refused in [
        BoundEdit::Dimensions {
            child: 0,
            values: vec![0.75, 1.0],
        },
        BoundEdit::Dimensions {
            child: 0,
            values: vec![-1.0],
        },
        BoundEdit::ChildKey {
            child: 0,
            key: 0x8000,
        },
    ] {
        assert!(state.edit_gta6_bound(doc, refused).is_err());
    }
    assert_eq!(sphere(&state, doc), (Some(keyed), 0.75));
    assert!(state.undo().is_some());
    assert_eq!(sphere(&state, doc), (Some(keyed), 0.5));
    assert!(state.redo().is_some());

    let written = read_back(&saved(&mut state, dir.path(), "saved.rsc"));
    assert_eq!(written.len(), copy.len());
    let read = read_bound(&written).expect("the saved bound reads");
    assert_eq!(read.children[0].material_id, Some(keyed));
    assert!(
        matches!(read.children[0].bound, Some(Ok(ChildBound::Sphere { centre, radius })) if centre == CENTRE && radius == 0.75),
        "{:?}",
        read.children[0].bound
    );
    // Only the sphere's record moved.
    let record = CHILD_AT..CHILD_AT + 0x70;
    let moved = (0..copy.len()).filter(|i| copy[*i] != written[*i]);
    assert!(
        moved.clone().all(|i| record.contains(&i)),
        "{:?}",
        moved.collect::<Vec<_>>()
    );
}

#[test]
fn a_polygon_takes_a_material_its_geometry_holds_and_saves_in_place() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = opened_body(dir.path(), &geometry_body());
    let key_of = |state: &AppState, p: usize| match &bound_of(state, doc).children[0].bound {
        Some(Ok(ChildBound::Geometry(g))) => g.primitive_key(p),
        other => panic!("a geometry, not {other:?}"),
    };
    assert_eq!(key_of(&state, 0), Some(0x7646));
    let copy = read_back(&saved(&mut state, dir.path(), "copy.rsc"));

    let edit = |primitive, key| BoundEdit::PolygonKey {
        child: 0,
        primitive,
        key,
    };
    state
        .edit_gta6_bound(doc, edit(0, 0x11))
        .expect("a key the geometry holds");
    assert!(
        state.edit_gta6_bound(doc, edit(0, 0x99)).is_err(),
        "a key it does not"
    );
    assert!(
        state.edit_gta6_bound(doc, edit(7, 0x11)).is_err(),
        "a primitive it does not"
    );
    // A geometry's key is its primitives': it has none of its own.
    let own = BoundEdit::ChildKey { child: 0, key: 1 };
    assert!(state.edit_gta6_bound(doc, own).is_err());
    assert_eq!(key_of(&state, 0), Some(0x11));

    let written = read_back(&saved(&mut state, dir.path(), "saved.rsc"));
    let read = read_bound(&written).expect("the saved bound reads");
    let Some(Ok(ChildBound::Geometry(g))) = &read.children[0].bound else {
        panic!("a geometry");
    };
    assert_eq!(g.primitive_materials, [0, 0]);
    let moved: Vec<usize> = (0..copy.len())
        .filter(|i| copy[*i] != written[*i])
        .collect();
    assert_eq!(moved.len(), 1, "one index byte: {moved:?}");
}

#[test]
fn a_cell_polygon_s_flags_undo_and_save_in_place() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = opened_body(dir.path(), &navmesh_cell());
    let flags = |state: &AppState| match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Resource(or_io::Asset::NavMesh(mesh))) => (mesh.polys_array.as_deref())
            .map(|l| l.iter().map(TNavMeshPoly::as_integer1).collect::<Vec<_>>()),
        _ => panic!("a navigation mesh"),
    };
    let read = flags(&state);
    assert_eq!(read, Some(vec![WATER]));
    let copy = read_back(&saved(&mut state, dir.path(), "copy.rsc"));
    let cell = read_navmesh(&copy).expect("the copy reads");
    assert_eq!(
        cell.polygons[0].flags, WATER,
        "nothing edited saves the cell as read"
    );

    state
        .set_gta6_polygon_flags(doc, 0, WATER | SMALL)
        .expect("a polygon of the cell");
    assert!(
        state.set_gta6_polygon_flags(doc, 1, 0).is_err(),
        "a polygon it does not hold"
    );
    assert_eq!(flags(&state), Some(vec![WATER | SMALL]));
    assert!(state.undo().is_some());
    assert_eq!(flags(&state), read);
    assert!(state.redo().is_some());

    let written = read_back(&saved(&mut state, dir.path(), "saved.rsc"));
    let cell = read_navmesh(&written).expect("the saved cell reads");
    assert_eq!(cell.polygons[0].flags, WATER | SMALL);
    let moved: Vec<usize> = (0..copy.len())
        .filter(|i| copy[*i] != written[*i])
        .collect();
    assert_eq!(moved.len(), 1, "one flag byte: {moved:?}");
}

fn child(composite: Option<u16>, mass: f32) -> FragmentChild {
    FragmentChild {
        group: 0,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.0; 3],
        angular_inertia: [1.0; 3],
        mass,
        composite,
        bound: None,
        flags: [0; 2],
        bound_word: 0,
        matrix: [[0.0; 4]; 4],
    }
}

#[test]
fn an_archetype_s_mass_is_its_listed_children_s_sum_each_way() {
    let archetype = Archetype {
        mass: 5.0,
        angular_inertia: [2.0, 4.0, 8.0],
        type_flags: 0x3F,
    };
    let mut f = Fragment {
        archetype: Some(archetype),
        children: vec![child(Some(0), 2.0), child(Some(1), 3.0), child(None, 1.0)],
        ..Fragment::default()
    };
    let masses = |f: &Fragment| f.children.iter().map(|c| c.mass).collect::<Vec<_>>();
    apply_fragment(&mut f, FragmentEdit::Mass(10.0)).expect("a mass to share");
    assert_eq!(f.archetype.map(|a| a.mass), Some(10.0));
    assert_eq!(masses(&f), [4.0, 6.0, 1.0], "the second set keeps its own");
    let edit = FragmentEdit::ChildMass {
        child: 0,
        mass: 1.5,
    };
    apply_fragment(&mut f, edit).expect("a child's mass");
    assert_eq!(f.archetype.map(|a| a.mass), Some(7.5));
    let second = FragmentEdit::ChildMass {
        child: 2,
        mass: 9.0,
    };
    apply_fragment(&mut f, second).expect("a second-set child's mass");
    assert_eq!(f.archetype.map(|a| a.mass), Some(7.5));
    assert!(apply_fragment(
        &mut f,
        FragmentEdit::ChildMass {
            child: 3,
            mass: 1.0
        }
    )
    .is_err());

    let mut weightless = Fragment {
        archetype: Some(Archetype {
            mass: 0.0,
            ..archetype
        }),
        ..Fragment::default()
    };
    assert!(apply_fragment(&mut weightless, FragmentEdit::Mass(1.0)).is_err());
    assert!(apply_fragment(&mut Fragment::default(), FragmentEdit::Mass(1.0)).is_err());
}

#[test]
fn a_child_s_matrix_and_the_bound_s_box_undo_and_save_in_place() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (mut state, doc) = opened_body(dir.path(), &sphere_body());
    let root = BoundEdit::Root {
        radius: 20.0,
        box_min: [-1.0; 3],
        box_max: [11.0, 1.0, 2.0],
        centroid: [5.0, 0.5, 0.5],
    };
    state
        .edit_gta6_bound(doc, root)
        .expect("a box that holds the child");
    let mut matrix = bound_of(&state, doc).children[0].matrix;
    matrix[3][0] = 9.5;
    let moved = BoundEdit::Matrix { child: 0, matrix };
    state
        .edit_gta6_bound(doc, moved)
        .expect("a move inside the box");
    let mut far = matrix;
    far[3][0] = 20.0;
    let past = BoundEdit::Matrix {
        child: 0,
        matrix: far,
    };
    assert!(state.edit_gta6_bound(doc, past).is_err(), "past the box");
    assert_eq!(bound_of(&state, doc).children[0].matrix, matrix);
    assert!(state.undo().is_some());
    assert_eq!(bound_of(&state, doc).children[0].matrix[3][0], 10.0);
    assert!(state.redo().is_some());

    let written = read_back(&saved(&mut state, dir.path(), "placed.rsc"));
    let read = read_bound(&written).expect("the saved bound reads");
    assert_eq!(read.children[0].matrix, matrix);
    assert_eq!((read.box_max, read.radius), ([11.0, 1.0, 2.0], 20.0));
}

#[test]
fn a_drawable_s_lone_bound_refuses_a_matrix_or_a_box_when_edited_and_takes_its_dimensions() {
    let identity: [[f32; 4]; 4] =
        std::array::from_fn(|r| std::array::from_fn(|c| f32::from(u8::from(r == c))));
    let shape = || ChildBound::Box {
        box_min: [-0.5; 3],
        box_max: [0.5; 3],
    };
    let mut drawable = crate::state::opening_a_ttpf::drawable_keyed(1);
    drawable.bound = Some(Bound {
        radius: 0.75f32.sqrt(),
        box_min: [-0.5; 3],
        box_max: [0.5; 3],
        centroid: [0.0; 3],
        children: vec![or_io::gta6::headerless::BoundChild {
            matrix: identity,
            flags: None,
            bound: Some(Ok(shape())),
            material_id: Some(0x7646),
        }],
        bvh: None,
    });
    let mut state = AppState::new();
    let payload = DocPayload::Gta6Drawable(Arc::new(Resource::Drawable(drawable)));
    let path = Some(PathBuf::from("0x1010.drawable"));
    state.documents.push(Document::new(1, path, payload));
    state.active_document = Some(1);

    let mut matrix = identity;
    matrix[3][0] = 1.0;
    let moved = state.edit_gta6_bound(1, BoundEdit::Matrix { child: 0, matrix });
    assert!(moved.is_err_and(|why| why.contains("lone bound")));
    let root = BoundEdit::Root {
        radius: 1.0,
        box_min: [-1.0; 3],
        box_max: [1.0; 3],
        centroid: [0.0; 3],
    };
    assert!(state.edit_gta6_bound(1, root).is_err());
    assert!(state.undo().is_none(), "a refused edit made no step");
    let sized = BoundEdit::Dimensions {
        child: 0,
        values: vec![2.0, 2.0, 2.0],
    };
    state
        .edit_gta6_bound(1, sized)
        .expect("its shape's dimensions");
    assert_eq!(state.undo().as_deref(), Some("Set a bound's dimensions"));
}

#[test]
fn an_inertia_is_its_own_value_for_the_archetype_and_each_child() {
    let archetype = Archetype {
        mass: 5.0,
        angular_inertia: [2.0, 4.0, 8.0],
        type_flags: 0x3F,
    };
    let mut f = Fragment {
        archetype: Some(archetype),
        children: vec![child(Some(0), 2.0), child(Some(1), 3.0)],
        ..Fragment::default()
    };
    apply_fragment(&mut f, FragmentEdit::Inertia([0.5, 1.5, 2.5])).expect("the archetype's");
    assert_eq!(
        f.archetype.map(|a| a.angular_inertia),
        Some([0.5, 1.5, 2.5])
    );
    let edit = FragmentEdit::ChildInertia {
        child: 1,
        inertia: [0.25; 3],
    };
    apply_fragment(&mut f, edit).expect("a child's");
    assert_eq!(f.children[1].angular_inertia, [0.25; 3]);
    assert_eq!(f.children[0].angular_inertia, [1.0; 3]);
    // Neither moves the masses, and the archetype's stays its own, not the children's sum.
    assert_eq!(
        f.archetype.map(|a| (a.mass, a.angular_inertia)),
        Some((5.0, [0.5, 1.5, 2.5]))
    );
    let past = FragmentEdit::ChildInertia {
        child: 2,
        inertia: [1.0; 3],
    };
    assert!(apply_fragment(&mut f, past).is_err());
}

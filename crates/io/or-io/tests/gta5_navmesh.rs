//! A `.ynv` authored as a plain Rust value, with invented content: triangles
//! over three vertices and a quadtree of two leaves, reopened, edited so each
//! figure the mesh derives would go stale, and saved. Needs no game file.

use std::mem::size_of;

use or_io::decl::{Element, File, Param, Plain, Raw, View};
use or_io::gta5::navmesh::{items, PolygonError, CHUNK_BYTES, NO_NEIGHBOUR};
use or_io::gta5::*;

/// `items` as a split array, every sub-array but the last full.
fn split<T: Owned + Clone>(items: Vec<T>) -> Option<Box<AiSplitArray<T>>>
where
    Raw<T>: Plain + Element + Param,
{
    let capacity = CHUNK_BYTES / size_of::<Raw<T>>();
    Some(Box::new(AiSplitArray {
        sub_arrays: items
            .chunks(capacity)
            .map(|c| SubArray {
                elements: c.to_vec(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }))
}

fn capacity() -> usize {
    CHUNK_BYTES / size_of::<Raw<TNavMeshPoly>>()
}

fn polygon(corners: usize) -> TNavMeshPoly {
    let mut p = TNavMeshPoly::default();
    p.set_num_vertices(corners).unwrap();
    p
}

fn leaf(cover_points: usize) -> Option<Box<CNavMeshQuadTree>> {
    Some(Box::new(CNavMeshQuadTree {
        leaf_data: Some(Box::new(TNavMeshQuadTreeLeafData {
            polys: vec![0],
            cover_points: vec![CNavMeshCoverPoint::default(); cover_points],
            ..Default::default()
        })),
        ..Default::default()
    }))
}

/// Gives `mesh` one index and one edge per corner its polygons have.
fn close(mesh: &mut CNavMesh) {
    let corners: usize = mesh
        .polys_array
        .as_deref()
        .map_or(0, |l| l.iter().map(TNavMeshPoly::num_vertices).sum());
    mesh.vertex_index_array = split((0..corners).map(|i| (i % 3) as u16).collect());
    mesh.adjacent_polys_array = split(vec![TAdjPoly::default(); corners]);
}

/// `polygons` triangles over three vertices, and a quadtree of two leaves.
fn authored(polygons: usize) -> CNavMesh {
    let mut mesh = CNavMesh {
        first_node: Some(Box::new(PgBasePageMap::default())),
        compressed_vertex_array: split(vec![CNavMeshCompressedVertex::default(); 3]),
        polys_array: split(vec![polygon(3); polygons]),
        quad_tree: Some(Box::new(CNavMeshQuadTree {
            children: [leaf(2), leaf(1), None, None],
            ..Default::default()
        })),
        ..Default::default()
    };
    close(&mut mesh);
    mesh
}

/// The mesh `authored` saves, reopened: what an edit starts from.
fn reopened(polygons: usize) -> CNavMesh {
    CNavMesh::from_bytes(&authored(polygons).to_bytes().unwrap()).unwrap()
}

/// Every polygon of a saved mesh, as the file states it.
fn saved_polygons(file: &[u8]) -> Vec<Raw<TNavMeshPoly>> {
    let peek = CNavMesh::peek(file).unwrap();
    let list = peek.root().unwrap().polys_array().unwrap().unwrap();
    items::<TNavMeshPoly>(&list, "polygon")
        .unwrap()
        .into_iter()
        .copied()
        .collect()
}

#[test]
fn a_split_array_saves_the_starts_its_sub_arrays_imply() {
    let n = capacity();
    let mut mesh = reopened(n + 1);
    let polys = mesh.polys_array.as_deref_mut().unwrap();
    polys.sub_arrays.insert(
        0,
        SubArray {
            elements: vec![polygon(3); n],
            ..Default::default()
        },
    );
    close(&mut mesh);
    let saved = mesh.to_bytes().unwrap();
    let peek = CNavMesh::peek(&saved).unwrap();
    let list = peek.root().unwrap().polys_array().unwrap().unwrap();
    let starts: Vec<u32> = list.start_indices().unwrap().iter().copied().collect();
    assert_eq!(starts, [0, n as u32, 2 * n as u32]);
}

/// The second polygon's first corner, read as 3, goes stale when the first gains a corner.
#[test]
fn a_polygon_saves_the_corners_before_it_as_its_first() {
    let mut mesh = reopened(2);
    let first = mesh.polys_array.as_deref_mut().unwrap();
    let first = &mut first.sub_arrays[0].elements[0];
    first.set_num_vertices(4).unwrap();
    first.set_as_integer1(0x1234);
    first.set_as_integer2(0x0005_0000);
    close(&mut mesh);
    let polygons = saved_polygons(&mesh.to_bytes().unwrap());
    let corners: Vec<u32> = polygons.iter().map(|p| p.as_integer2 & 0xffff).collect();
    assert_eq!(corners, [0, 4]);
    assert_eq!(
        polygons[0].as_integer1,
        0x1234 | (4 << 21),
        "the flags and the count"
    );
    assert_eq!(
        polygons[0].as_integer2, 0x0005_0000,
        "the mesh index is kept"
    );
}

/// The second polygon's first special link, read as 0, goes stale when the first gains one;
/// the polygon a full sub-array moves along states the sub-array it now sits in.
#[test]
fn a_polygon_saves_its_sub_array_and_the_special_links_before_it() {
    let n = capacity();
    let mut mesh = reopened(n);
    let polys = mesh.polys_array.as_deref_mut().unwrap();
    polys.sub_arrays[0].elements[0]
        .set_num_special_links(2)
        .unwrap();
    polys.sub_arrays[0].elements[0].set_as_integer4(0x7);
    polys.sub_arrays[0].elements.insert(0, polygon(3));
    let moved = polys.sub_arrays[0].elements.pop().unwrap();
    polys.sub_arrays.push(SubArray {
        elements: vec![moved],
        ..Default::default()
    });
    close(&mut mesh);
    mesh.special_links_indices = vec![0, 0];
    let polygons = saved_polygons(&mesh.to_bytes().unwrap());
    let starts: Vec<u32> = polygons[..3]
        .iter()
        .map(|p| (p.as_integer4 >> 15) & 0x1ff)
        .collect();
    assert_eq!(starts, [0, 0, 2]);
    assert_eq!(
        polygons[1].as_integer4 & 0xf,
        0x7,
        "the parent exit edge is kept"
    );
    let sub_array = |p: &Raw<TNavMeshPoly>| (p.as_integer4 >> 4) & 0xff;
    assert_eq!(
        (sub_array(&polygons[n - 1]), sub_array(&polygons[n])),
        (0, 1)
    );
}

#[test]
fn a_mesh_whose_special_link_counts_do_not_close_on_its_indices_is_refused() {
    let mut mesh = reopened(2);
    let polys = mesh.polys_array.as_deref_mut().unwrap();
    polys.sub_arrays[0].elements[0]
        .set_num_special_links(2)
        .unwrap();
    let why = mesh.to_bytes().unwrap_err().to_string();
    assert!(
        why.contains(
            "the polygons' special link counts sum to 2 and the mesh holds 0 special link indices"
        ),
        "{why}"
    );
}

/// The second leaf's first cover point, read as 2, goes stale when the first gains one.
#[test]
fn a_leaf_saves_the_cover_points_before_it_as_its_first() {
    let mut mesh = reopened(1);
    let tree = mesh.quad_tree.as_deref_mut().unwrap();
    let first = tree.children[0].as_deref_mut().unwrap();
    let first = first.leaf_data.as_deref_mut().unwrap();
    first.cover_points.push(CNavMeshCoverPoint::default());
    let saved = mesh.to_bytes().unwrap();
    let peek = CNavMesh::peek(&saved).unwrap();
    let image = View::image(&peek.root().unwrap());
    let root = peek.root().unwrap().quad_tree().unwrap().unwrap();
    let children = { View::get(&root).children };
    let starts: Vec<u16> = children[..2]
        .iter()
        .map(|slot| {
            let node = View::<Raw<CNavMeshQuadTree>>::at_address(image, slot.raw()).unwrap();
            let leaf = node.leaf_data().unwrap().unwrap();
            leaf.index_of_first_cover_point_in_list
        })
        .collect();
    assert_eq!(starts, [0, 3]);
}

/// The total, read with one special link, goes stale when a second is added.
#[test]
fn a_mesh_saves_the_memory_its_items_take() {
    let total = |file: &[u8]| {
        CNavMesh::peek(file)
            .unwrap()
            .root()
            .unwrap()
            .total_memory_used
    };
    let mut mesh = reopened(1);
    mesh.special_links = vec![CSpecialLinkInfo::default()];
    let before = total(&mesh.to_bytes().unwrap());
    mesh.special_links.push(CSpecialLinkInfo::default());
    let after = total(&mesh.to_bytes().unwrap());
    assert_eq!(
        (after - before) as usize,
        size_of::<Raw<CSpecialLinkInfo>>()
    );
    assert!(before > 0);
}

#[test]
fn a_polygon_s_counts_refuse_what_their_bits_cannot_hold() {
    let mut p = polygon(15);
    let why = p.set_num_vertices(16).unwrap_err().to_string();
    assert!(why.contains("4 bits wide"), "{why}");
    let why = p.set_num_special_links(8).unwrap_err().to_string();
    assert!(why.contains("3 bits wide"), "{why}");
    assert_eq!((p.num_vertices(), p.num_special_links()), (15, 0));
}

const CELL: u32 = 7;
const NEXT: u32 = 8;

/// `polygons` triangles in a strip, in cell `CELL`: corner 0 of polygon i leads to i + 1 and corner 1
/// to i - 1, both through slot 0, which names the cell itself; polygon 0's corner 2 leads to polygon 5
/// of `NEXT` through slot 1 when `next` lists it, and every other edge leads nowhere.
fn strip(polygons: usize, next: bool) -> CNavMesh {
    let mut mesh = authored(polygons);
    mesh.index_of_mesh = CELL;
    mesh.adjacent_meshes.navmesh_indices[0] = CELL;
    if next {
        mesh.adjacent_meshes.navmesh_indices[1] = NEXT;
    }
    let to = |slot, polygon| TAdjPoly::leading_to(slot, polygon).unwrap();
    let nowhere = to(1, NO_NEIGHBOUR);
    let mut edges = Vec::new();
    for i in 0..polygons {
        edges.push(if i + 1 < polygons {
            to(0, i + 1)
        } else {
            nowhere.clone()
        });
        edges.push(if i > 0 { to(0, i - 1) } else { nowhere.clone() });
        edges.push(if i == 0 && next {
            to(1, 5)
        } else {
            nowhere.clone()
        });
    }
    mesh.adjacent_polys_array = split(edges);
    let tree = mesh.quad_tree.as_deref_mut().unwrap();
    for leaf in tree.children.iter_mut().flatten() {
        leaf.leaf_data.as_deref_mut().unwrap().polys = (0..polygons as u16).collect();
    }
    mesh
}

/// A mesh as a file holds it: saved, and opened again.
fn reopen(mesh: CNavMesh) -> CNavMesh {
    CNavMesh::from_bytes(&mesh.to_bytes().unwrap()).unwrap()
}

/// Every edge of `mesh`, one per corner, as the slot and the polygon it leads to.
fn leads(mesh: &CNavMesh) -> Vec<Option<(usize, usize)>> {
    let edges = mesh.adjacent_polys_array.as_deref().unwrap();
    edges.iter().map(TAdjPoly::neighbour).collect()
}

fn polygons_of(mesh: &CNavMesh) -> usize {
    mesh.polys_array.as_deref().map_or(0, |l| l.len())
}

fn leaf_polys(mesh: &CNavMesh) -> Vec<Vec<u16>> {
    let tree = mesh.quad_tree.as_deref().unwrap();
    let leaves = tree.children.iter().flatten();
    leaves
        .map(|l| l.leaf_data.as_deref().unwrap().polys.clone())
        .collect()
}

/// Gives polygons `at` one special link each, a single link from the first to the second.
fn link(mesh: &mut CNavMesh, at: [usize; 2]) {
    mesh.special_links = vec![CSpecialLinkInfo {
        original_link_from_poly: at[0] as u16,
        link_from_poly: at[0] as u16,
        original_link_to_poly: at[1] as u16,
        link_to_poly: at[1] as u16,
        ..Default::default()
    }];
    let polys = mesh.polys_array.as_deref_mut().unwrap();
    for polygon in at {
        polys.sub_arrays[0].elements[polygon]
            .set_num_special_links(1)
            .unwrap();
    }
    mesh.special_links_indices = vec![0, 0];
}

#[test]
fn a_disabled_polygon_saves_and_reopens_disabled() {
    let mut mesh = reopen(strip(4, true));
    assert_eq!(mesh.disabled_polygons(), [false; 4]);
    let before = leads(&mesh);
    // Polygon 1's edges to 2 and 0, its third, and the edges of 0 and 2 that lead into it.
    assert_eq!(mesh.disable_polygon(1).unwrap(), 4);

    let saved = mesh.to_bytes().unwrap();
    let reopened = CNavMesh::from_bytes(&saved).unwrap();
    assert_eq!(reopened.disabled_polygons(), [false, true, false, false]);
    let changed = [0, 3, 4, 5, 7];
    for (at, (was, now)) in before.iter().zip(leads(&reopened)).enumerate() {
        if changed.contains(&at) {
            assert_eq!(now, None, "edge {at} still leads somewhere");
        } else {
            assert_eq!(*was, now, "edge {at} changed");
        }
    }
    assert_eq!(
        reopened.to_bytes().unwrap(),
        saved,
        "a second save is the first"
    );
    assert_eq!(mesh.disable_polygon(1).unwrap(), 0, "already disabled");
}

#[test]
fn a_deleted_polygon_is_refused_naming_what_leads_into_it() {
    let mesh = reopen(strip(4, true));
    let mut edited = mesh.clone();
    let refused = edited.delete_polygon(1).unwrap_err();
    let PolygonError::Referenced {
        polygon,
        references,
        ..
    } = &refused
    else {
        panic!("{refused}");
    };
    assert_eq!(*polygon, 1);
    assert_eq!(references.edges, [(0, 0), (2, 1)]);
    assert_eq!(references.cells, [NEXT]);
    let said = refused.to_string();
    assert!(
        said.contains("polygon 0 corner 0") && said.contains("cell 8"),
        "{said}"
    );
    assert_eq!(edited, mesh, "a refused deletion changes nothing");
}

#[test]
fn a_mesh_that_lists_another_cell_refuses_every_deletion_naming_it() {
    let mut mesh = reopen(strip(4, true));
    mesh.disable_polygon(3).unwrap();
    let refused = mesh.delete_polygon(3).unwrap_err().to_string();
    assert!(refused.contains("cell 8"), "{refused}");
}

#[test]
fn a_deleted_polygon_renumbers_every_reference_to_the_ones_after_it() {
    let mut mesh = strip(5, false);
    link(&mut mesh, [0, 3]);
    let mut mesh = reopen(mesh);
    mesh.disable_polygon(1).unwrap();
    let before = mesh.clone();
    mesh.delete_polygon(1).unwrap();

    let renumber = |polygon: usize| polygon - usize::from(polygon > 1);
    let after = reopen(mesh);
    assert_eq!(polygons_of(&after), 4);
    let (was, now) = (leads(&before), leads(&after));
    assert_eq!(now.len(), 12, "polygon 1's three corners are gone");
    for (new, old) in [0usize, 2, 3, 4].into_iter().enumerate() {
        for corner in 0..3 {
            let was = was[old * 3 + corner].map(|(slot, polygon)| (slot, renumber(polygon)));
            assert_eq!(now[new * 3 + corner], was, "polygon {old} corner {corner}");
        }
    }
    assert_eq!(leaf_polys(&after), vec![vec![0, 1, 2, 3]; 2]);
    let link = &after.special_links[0];
    assert_eq!(
        (
            link.link_from_poly,
            link.original_link_from_poly,
            link.link_to_poly,
            link.original_link_to_poly
        ),
        (0, 0, 2, 2)
    );
}

#[test]
fn a_polygon_a_special_link_names_is_refused_naming_the_link() {
    let mut mesh = strip(4, false);
    link(&mut mesh, [0, 2]);
    let mut mesh = reopen(mesh);
    mesh.disable_polygon(2).unwrap();
    let before = mesh.clone();
    let refused = mesh.delete_polygon(2).unwrap_err();
    let PolygonError::Referenced { references, .. } = &refused else {
        panic!("{refused}");
    };
    assert_eq!(references.special_links, [0]);
    assert!(refused.to_string().contains("special link 0"), "{refused}");
    assert_eq!(mesh, before);
}

#[test]
fn a_deletion_refills_the_sub_array_it_left_short() {
    let n = capacity();
    for (polygons, sub_arrays) in [(n + 2, vec![n, 1]), (n + 1, vec![n])] {
        let mut mesh = reopen(strip(polygons, false));
        mesh.disable_polygon(0).unwrap();
        mesh.delete_polygon(0).unwrap();
        let saved = mesh.to_bytes().unwrap();
        let held: Vec<usize> = CNavMesh::from_bytes(&saved)
            .unwrap()
            .polys_array
            .as_deref()
            .unwrap()
            .sub_arrays
            .iter()
            .map(|s| s.elements.len())
            .collect();
        assert_eq!(held, sub_arrays);
        assert_eq!(saved_polygons(&saved).len(), polygons - 1);
    }
}

#[test]
fn a_polygon_past_the_end_is_named_and_nothing_changes() {
    let mut mesh = reopen(strip(5, false));
    let before = mesh.clone();
    let wanted = PolygonError::Missing {
        polygon: 9,
        polygons: 5,
    };
    assert_eq!(mesh.disable_polygon(9).unwrap_err(), wanted);
    assert_eq!(mesh.delete_polygon(9).unwrap_err(), wanted);
    assert_eq!(mesh, before);
}

/// A vehicle mesh's one table entry is 16383, which no cell has: nothing there leads out of the mesh.
#[test]
fn a_table_entry_that_names_no_cell_blocks_no_deletion() {
    let mut mesh = strip(3, false);
    mesh.index_of_mesh = 10000;
    mesh.adjacent_meshes.navmesh_indices = [0; 32];
    mesh.adjacent_meshes.navmesh_indices[0] = 0x3fff;
    let nowhere = TAdjPoly::leading_to(0, NO_NEIGHBOUR).unwrap();
    mesh.adjacent_polys_array = split(vec![nowhere; 9]);
    let mut mesh = reopen(mesh);
    assert_eq!(mesh.neighbours(), Vec::<u32>::new());
    mesh.delete_polygon(0).unwrap();
    assert_eq!(polygons_of(&reopen(mesh)), 2);
}

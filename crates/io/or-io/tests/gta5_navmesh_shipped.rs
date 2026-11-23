//! Disabling and deleting a polygon over shipped `.ynv` cells.
//!
//! ⚠ Corpus-gated: a stride of the staged Legacy cells, each edited and saved. A skipped suite has not passed.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use or_io::decl::File;
use or_io::gta5::navmesh::PolygonError;
use or_io::gta5::*;

/// Every `STRIDE`th staged cell, which keeps the vehicle meshes and the large cells both.
const STRIDE: usize = 12;

/// The cell a vehicle mesh's one table entry holds, which names no cell.
const NO_CELL: u32 = 0x3fff;

fn every(what: &str) -> Option<Vec<PathBuf>> {
    let root = or_test_support::fixtures_root(env!("CARGO_MANIFEST_DIR"));
    let all = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ynv").find();
    if all.is_empty() {
        or_test_support::skip(what, &root, "ynv");
        return None;
    }
    Some(all)
}

/// Every `STRIDE`th staged cell.
fn staged(what: &str) -> Option<Vec<PathBuf>> {
    every(what).map(|all| all.into_iter().step_by(STRIDE).collect())
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn edges(mesh: &CNavMesh) -> Vec<TAdjPoly> {
    let list = mesh.adjacent_polys_array.as_deref();
    list.map_or(Vec::new(), |l| l.iter().cloned().collect())
}

fn leads(mesh: &CNavMesh) -> Vec<Option<(usize, usize)>> {
    edges(mesh).iter().map(TAdjPoly::neighbour).collect()
}

/// Each polygon's corners as a range of the edge list.
fn runs(mesh: &CNavMesh) -> Vec<Range<usize>> {
    let mut end = 0;
    let polygons = mesh
        .polys_array
        .as_deref()
        .into_iter()
        .flat_map(|l| l.iter());
    polygons
        .map(|p| {
            let run = end..end + p.num_vertices();
            end = run.end;
            run
        })
        .collect()
}

/// The polygon with the most edges that lead somewhere, which is where an edit has the most to change.
fn busiest(mesh: &CNavMesh) -> Option<usize> {
    let (leads, runs) = (leads(mesh), runs(mesh));
    (0..runs.len()).max_by_key(|p| runs[*p].clone().filter(|at| leads[*at].is_some()).count())
}

/// Each leaf's polygon list, depth first.
fn leaf_polys(mesh: &CNavMesh) -> Vec<Vec<u16>> {
    fn walk(node: &CNavMeshQuadTree, out: &mut Vec<Vec<u16>>) {
        out.extend(node.leaf_data.as_deref().map(|l| l.polys.clone()));
        node.children.iter().flatten().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    mesh.quad_tree
        .as_deref()
        .into_iter()
        .for_each(|t| walk(t, &mut out));
    out
}

/// The first slot of `mesh`'s table that does not name the cell itself.
fn first_slot_not_its_own(mesh: &CNavMesh) -> usize {
    let table = &mesh.adjacent_meshes.navmesh_indices;
    let filled = table.iter().position(|c| *c == 0).unwrap_or(table.len());
    (0..table.len())
        .find(|slot| table[..filled].get(*slot) != Some(&mesh.index_of_mesh))
        .expect("a table of 32 holds the cell once at most")
}

/// The slot an edge that leads nowhere states, as the low five bits of its first word.
#[test]
fn every_shipped_edge_that_leads_nowhere_names_the_first_slot_that_is_not_its_cell() {
    let Some(files) = every("the slot of a navmesh edge that leads nowhere") else {
        return;
    };
    let (mut edges_seen, mut cells) = (0, 0);
    for path in &files {
        let mesh = CNavMesh::open(path).unwrap();
        let first = first_slot_not_its_own(&mesh) as u32;
        for edge in edges(&mesh).iter().filter(|e| e.neighbour().is_none()) {
            assert_eq!(edge.as_integer1 & 0x1f, first, "{}", name(path));
            edges_seen += 1;
        }
        cells += 1;
    }
    println!("{edges_seen} edges that lead nowhere over {cells} cells, each in that slot");
    assert!(edges_seen > 0);
}

/// A table entry of all ones names no cell: no edge that leads somewhere states it, and the cells that list nothing else are vehicle meshes.
#[test]
fn the_table_entry_of_all_ones_is_one_no_shipped_edge_leads_into() {
    let Some(files) = every("the table entry that names no cell") else {
        return;
    };
    let (mut named, mut alone) = (0, 0);
    for path in &files {
        let mesh = CNavMesh::open(path).unwrap();
        let table = &mesh.adjacent_meshes.navmesh_indices;
        for (slot, _) in edges(&mesh).iter().filter_map(TAdjPoly::neighbour) {
            assert_ne!(table[slot], NO_CELL, "{}: an edge into no cell", name(path));
            named += 1;
        }
        if mesh.neighbours().is_empty() {
            let filled = table.iter().position(|c| *c == 0).unwrap_or(table.len());
            let itself_or_none = |c: &u32| *c == mesh.index_of_mesh || *c == NO_CELL;
            assert!(table[..filled].iter().all(itself_or_none), "{}", name(path));
            assert_eq!(mesh.index_of_mesh, 10000, "{}", name(path));
            alone += 1;
        }
    }
    println!("{named} edges lead somewhere, none into 16383; {alone} cells list no other cell");
    assert!(named > 0);
}

/// What a disabled polygon changes: its own edges that lead somewhere and every edge that led into it.
#[test]
fn disabling_a_shipped_polygon_changes_exactly_the_edges_that_touch_it() {
    let Some(files) = staged("disabling a shipped navmesh polygon") else {
        return;
    };
    let (mut cells, mut changed_edges) = (0, 0);
    for path in &files {
        let mesh = CNavMesh::open(path).unwrap();
        let Some(polygon) = busiest(&mesh) else {
            continue;
        };
        let (before, runs) = (leads(&mesh), runs(&mesh));
        let referenced = mesh.references(polygon).unwrap();
        let leading_in =
            (referenced.edges.iter()).map(|(other, corner)| runs[*other].start + corner);
        let own = runs[polygon].clone().filter(|at| before[*at].is_some());
        let expected: BTreeSet<usize> = own.chain(leading_in).collect();

        let mut edited = mesh.clone();
        assert_eq!(edited.disable_polygon(polygon).unwrap(), expected.len());
        let saved = edited.to_bytes().unwrap();
        let reopened = CNavMesh::from_bytes(&saved).unwrap();
        let now = leads(&reopened);
        let changed: BTreeSet<usize> = (0..before.len())
            .filter(|at| before[*at] != now[*at])
            .collect();
        assert_eq!(changed, expected, "{}: polygon {polygon}", name(path));
        assert!(
            reopened.disabled_polygons()[polygon],
            "{}: polygon {polygon} reopens enabled",
            name(path)
        );
        assert_eq!(
            reopened.to_bytes().unwrap(),
            saved,
            "{}: a second save is not the first",
            name(path)
        );
        cells += 1;
        changed_edges += expected.len();
    }
    println!(
        "{cells} cells: a disabled polygon changed {changed_edges} edges, each one it touches"
    );
    assert!(cells > 0);
}

/// A severed edge states what a shipped edge that leads nowhere states: the same slot, the second word its twin, the bits above kept.
#[test]
fn a_severed_edge_states_what_a_shipped_edge_that_leads_nowhere_states() {
    let Some(files) = staged("a severed navmesh edge") else {
        return;
    };
    let (mut cells, mut severed) = (0, 0);
    for path in &files {
        let mesh = CNavMesh::open(path).unwrap();
        let Some(polygon) = busiest(&mesh) else {
            continue;
        };
        let was = edges(&mesh);
        let low = |word: u32| word & 0xf_ffff;
        let shipped: BTreeSet<u32> = (was.iter())
            .filter(|e| e.neighbour().is_none())
            .map(|e| low(e.as_integer1))
            .collect();
        if shipped.is_empty() {
            continue;
        }
        let mut edited = mesh.clone();
        edited.disable_polygon(polygon).unwrap();
        for (at, (before, now)) in was.iter().zip(edges(&edited)).enumerate() {
            if *before == now {
                continue;
            }
            let at = format!("{}: edge {at}", name(path));
            assert!(
                shipped.contains(&low(now.as_integer1)),
                "{at} was cut to {:#x}, which no shipped edge in the cell states",
                low(now.as_integer1)
            );
            assert_eq!(low(now.as_integer1), low(now.as_integer2), "{at}: its twin");
            assert_eq!(
                (before.as_integer1 >> 20, before.as_integer2 >> 20),
                (now.as_integer1 >> 20, now.as_integer2 >> 20),
                "{at}: its upper bits"
            );
            severed += 1;
        }
        cells += 1;
    }
    println!("{cells} cells, {severed} severed edges, each stating a slot a shipped edge states");
    assert!(cells > 0 && severed > 0);
}

/// A shipped cell lists the cells around it, and a deletion that could break their edges is refused naming them.
#[test]
fn deleting_from_a_shipped_cell_that_lists_others_is_refused_naming_them() {
    let Some(files) = staged("deleting from a shipped navmesh cell") else {
        return;
    };
    let (mut refused, mut free) = (0, 0);
    for path in &files {
        let mesh = CNavMesh::open(path).unwrap();
        let Some(polygon) = busiest(&mesh) else {
            continue;
        };
        let neighbours = mesh.neighbours();
        let mut edited = mesh.clone();
        edited.disable_polygon(polygon).unwrap();
        let before = edited.clone();
        match edited.delete_polygon(polygon) {
            Err(PolygonError::Referenced { references, .. }) => {
                assert_eq!(references.cells, neighbours, "{}", name(path));
                assert!(!neighbours.is_empty(), "{}", name(path));
                // ⚠ Saved bytes, not `==`: a navmesh's floats can be NaN, which is unequal to itself.
                let unchanged = edited.to_bytes().unwrap() == before.to_bytes().unwrap();
                assert!(unchanged, "{}: a refusal changed the mesh", name(path));
                refused += 1;
            }
            Ok(()) => {
                assert!(neighbours.is_empty(), "{}", name(path));
                free += 1;
            }
            Err(other) => panic!("{}: {other}", name(path)),
        }
    }
    println!("{refused} cells refused naming the cells around them; {free} had none to name");
    assert!(refused > 0);
}

/// With the cells around it struck from its table, a shipped cell deletes a polygon and renumbers every reference to a later one.
#[test]
fn a_shipped_cell_deletes_a_polygon_and_renumbers_what_names_the_ones_after_it() {
    let Some(files) = staged("deleting from a shipped navmesh cell") else {
        return;
    };
    let (mut cells, mut renumbered) = (0, 0);
    for path in &files {
        let mut mesh = CNavMesh::open(path).unwrap();
        let own = mesh.index_of_mesh;
        let table = &mut mesh.adjacent_meshes.navmesh_indices;
        let filled = table.iter().position(|c| *c == 0).unwrap_or(table.len());
        let Some(own_slot) = table[..filled].iter().position(|c| *c == own) else {
            continue;
        };
        // The table ends at its first zero, so a cell struck from it is replaced by the entry that names none.
        for cell in table[..filled].iter_mut().filter(|c| **c != own) {
            *cell = NO_CELL;
        }
        let count = runs(&mesh).len();
        let clear = |p: &usize| (mesh.references(*p)).is_ok_and(|r| r.special_links.is_empty());
        let Some(polygon) = (count / 2..count).find(clear) else {
            continue;
        };
        mesh.disable_polygon(polygon).unwrap();
        let before = mesh.clone();
        mesh.delete_polygon(polygon).unwrap();
        let saved = mesh.to_bytes().unwrap();
        let after = CNavMesh::from_bytes(&saved).unwrap();

        let renumber = |q: usize| q - usize::from(q > polygon);
        let (old_runs, new_runs) = (runs(&before), runs(&after));
        assert_eq!(new_runs.len(), count - 1, "{}", name(path));
        let (was, now) = (leads(&before), leads(&after));
        let kept = (0..count).filter(|q| *q != polygon);
        for (new, old) in kept.enumerate() {
            assert_eq!(old_runs[old].len(), new_runs[new].len(), "{}", name(path));
            for k in 0..old_runs[old].len() {
                let then = was[old_runs[old].start + k];
                // A slot that names the cell renumbers; any other names a cell this test struck out.
                let want =
                    then.map(|(slot, q)| (slot, if slot == own_slot { renumber(q) } else { q }));
                let at = format!("{}: polygon {old} corner {k}", name(path));
                assert_eq!(now[new_runs[new].start + k], want, "{at}");
                renumbered += usize::from(want != then);
            }
        }
        let want_leaves: Vec<Vec<u16>> = leaf_polys(&before)
            .into_iter()
            .map(|l| {
                (l.into_iter().filter(|p| usize::from(*p) != polygon))
                    .map(|p| renumber(usize::from(p)) as u16)
                    .collect()
            })
            .collect();
        assert_eq!(
            leaf_polys(&after),
            want_leaves,
            "{}: the leaves",
            name(path)
        );
        for (was, now) in before.special_links.iter().zip(&after.special_links) {
            let ends = |l: &CSpecialLinkInfo| [l.link_from_poly, l.link_to_poly];
            let want = ends(was).map(|e| renumber(usize::from(e)) as u16);
            assert_eq!(ends(now), want, "{}: a special link", name(path));
        }
        cells += 1;
    }
    println!("{cells} cells deleted a polygon; {renumbered} edges were renumbered on the way");
    assert!(cells > 0);
}

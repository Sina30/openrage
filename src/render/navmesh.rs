//! A `.ynv`'s polygons in world space and its quadtree's leaf sectors.

use glam::Vec3;
use or_io::gta5::{CNavMesh, CNavMeshQuadTree, Vector3};

fn vec3(v: &Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Each polygon's corners in order, dequantised into the box the quadtree's root states.
///
/// ⚠ A corner whose index names no vertex is dropped rather than refused: the
/// view draws what it can of a mesh the save would refuse.
pub(crate) fn polygons(mesh: &CNavMesh) -> Vec<Vec<Vec3>> {
    let (Some(root), Some(polys)) = (mesh.quad_tree.as_deref(), mesh.polys_array.as_deref()) else {
        return Vec::new();
    };
    let min = vec3(&root.mins);
    let size = vec3(&root.maxs) - min;
    let vertices: Vec<Vec3> = mesh
        .compressed_vertex_array
        .as_deref()
        .map(|a| {
            a.iter()
                .map(|v| min + Vec3::from(v.data.map(|q| f32::from(q) / 65535.0)) * size)
                .collect()
        })
        .unwrap_or_default();
    let indices: Vec<u16> = mesh
        .vertex_index_array
        .as_deref()
        .map(|a| a.iter().copied().collect())
        .unwrap_or_default();
    let mut first = 0;
    polys
        .iter()
        .map(|p| {
            let run = indices.get(first..first + p.num_vertices()).unwrap_or(&[]);
            first += p.num_vertices();
            run.iter()
                .filter_map(|i| vertices.get(usize::from(*i)).copied())
                .collect()
        })
        .collect()
}

/// The box of every quadtree node with no children, depth first.
pub(crate) fn sectors(mesh: &CNavMesh) -> Vec<(Vec3, Vec3)> {
    fn walk(node: &CNavMeshQuadTree, out: &mut Vec<(Vec3, Vec3)>) {
        if node.children.iter().all(Option::is_none) {
            out.push((vec3(&node.mins), vec3(&node.maxs)));
        }
        for child in node.children.iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = mesh.quad_tree.as_deref() {
        walk(root, &mut out);
    }
    out
}

/// The quadtree root's box, which bounds every vertex.
pub(crate) fn extent(mesh: &CNavMesh) -> Option<(Vec3, Vec3)> {
    let root = mesh.quad_tree.as_deref()?;
    Some((vec3(&root.mins), vec3(&root.maxs)))
}

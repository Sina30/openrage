//! A polygonal bound built from a mesh, and the BVH a concave bound is traversed through.
//!
//! ⚠ A geometry's vertices are 16-bit and its quantum comes from its box, so
//! the whole mesh is known before one vertex is written.

use std::collections::HashMap;

use super::{
    BoundType, CompressedVertex, PhBVHSubtreeInfo, PhBound, PhBoundAny, PhBoundBVH,
    PhBoundGeometry, PhMaterialMgrId, PhOptimizedBvh, PhOptimizedBvhNode, PhPolygon, PhPrimBox,
    PhPrimCapsule, PhPrimSphere, NO_NEIGHBOUR, PRIMITIVE_TYPE_MASK, VERTEX_INDEX_MASK,
};
use crate::decl::{Open, Plain, Raw};
use crate::error::Error;
use crate::gta5::shared::{Vec3V, Vec4V, Vector3};

/// The collision margin a built bound states in its bounding box's `w`.
pub const CONVEX_DISTANCE_MARGIN: f32 = 0.04;

/// A node holding at most this many polygons is a leaf of a geometry's tree.
pub const GEOMETRY_ITEM_THRESHOLD: usize = 4;

/// The same for a composite, whose leaves hold one child bound each.
pub const COMPOSITE_ITEM_THRESHOLD: usize = 1;

/// A subtree header is stated once a subtree grows past this many nodes.
pub const MAX_TREE_NODE_COUNT: usize = 127;

/// The `i16` range a node's quantized box spans.
const QUANTIZED_LIMIT: f32 = 32767.0;

/// ⚠ Slot `k` of `neighboring_polygons` is the triangle across the edge from
/// vertex `k` to vertex `k + 1`, wrapping. Nothing in the file states it, and
/// a wrong rotation cross-links the adjacency with every index still valid;
/// `a_neighbour_slot_names_the_edge_this_workspace_says_it_does` measures it.
pub const EDGE_VERTICES: [(usize, usize); 3] = [(0, 1), (1, 2), (2, 0)];

/// The mesh a polygonal bound is built from.
#[derive(Debug, Clone, Default)]
pub struct BoundMesh {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
    /// The materials the bound holds; empty is legal and common.
    pub materials: Vec<PhMaterialMgrId>,
    /// One index into `materials` per triangle, or empty for none.
    pub triangle_materials: Vec<u8>,
}

impl BoundMesh {
    pub fn new(positions: Vec<[f32; 3]>, triangles: Vec<[u16; 3]>) -> Self {
        Self {
            positions,
            triangles,
            ..Default::default()
        }
    }
}

fn invalid(context: String) -> Error {
    Error::InvalidData { context }
}

/// A `phBoundGeometry` over a mesh.
pub fn geometry(mesh: &BoundMesh) -> Result<PhBoundGeometry, Error> {
    if mesh.positions.is_empty() {
        return Err(invalid(
            "a geometry bound over no vertices has no quantum to store them at".into(),
        ));
    }
    if mesh.positions.len() > usize::from(VERTEX_INDEX_MASK) + 1 {
        return Err(invalid(format!(
            "a polygon addresses a vertex in fifteen bits; this mesh holds {}",
            mesh.positions.len()
        )));
    }
    if !mesh.triangle_materials.is_empty() && mesh.triangle_materials.len() != mesh.triangles.len()
    {
        return Err(invalid(format!(
            "{} material index/indices for {} triangle(s); both runs are written at one count",
            mesh.triangle_materials.len(),
            mesh.triangles.len()
        )));
    }
    if let Some(worst) = mesh
        .triangle_materials
        .iter()
        .copied()
        .filter(|i| usize::from(*i) >= mesh.materials.len())
        .max()
    {
        return Err(invalid(format!(
            "a triangle names material {worst} of {}",
            mesh.materials.len()
        )));
    }
    for (i, tri) in mesh.triangles.iter().enumerate() {
        if let Some(v) = tri
            .iter()
            .find(|v| usize::from(**v) >= mesh.positions.len())
        {
            return Err(invalid(format!(
                "triangle {i} names vertex {v} of {}",
                mesh.positions.len()
            )));
        }
    }

    let (min, max) = union(mesh.positions.iter().map(|p| (*p, *p)));
    let centre: [f32; 3] = std::array::from_fn(|a| (min[a] + max[a]) * 0.5);
    // A zero extent stores every vertex on that axis at zero, which is what a flat bound means.
    let unquantize: [f32; 3] = std::array::from_fn(|a| {
        let extent = max[a] - min[a];
        if extent > 0.0 {
            extent * 0.5 / QUANTIZED_LIMIT
        } else {
            1.0
        }
    });
    let quantize = unquantize.map(|u| 1.0 / u);
    let compressed_vertices = mesh
        .positions
        .iter()
        .map(|p| {
            let [x, y, z] = std::array::from_fn(|a| {
                ((p[a] - centre[a]) * quantize[a]).clamp(i16::MIN.into(), i16::MAX.into()) as i16
            });
            CompressedVertex {
                x,
                y,
                z,
                ..Default::default()
            }
        })
        .collect();

    let neighbours = adjacency(&mesh.triangles);
    let polygons = mesh
        .triangles
        .iter()
        .zip(neighbours)
        .map(|(tri, neighboring_polygons)| PhPolygon {
            area: f32::from_bits(triangle_area(mesh, tri).to_bits() & !PRIMITIVE_TYPE_MASK),
            vertex_indices: *tri,
            neighboring_polygons,
            ..Default::default()
        })
        .collect();

    let radius_around_centroid = mesh
        .positions
        .iter()
        .map(|p| {
            (0..3)
                .map(|a| (p[a] - centre[a]).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(0.0f32, f32::max);
    let vec4 = |[x, y, z]: [f32; 3], w: f32| Vec4V {
        x,
        y,
        z,
        w,
        ..Default::default()
    };
    let vec3 = |[x, y, z]: [f32; 3]| Vec3V {
        x,
        y,
        z,
        ..Default::default()
    };
    let mut header = PhBound {
        r#type: Open::new(BoundType::Geometry),
        radius_around_centroid,
        bounding_box_max_xyz_margin_w: vec4(max, CONVEX_DISTANCE_MARGIN),
        centroid_offset_xyz_material_id0_w: vec4(centre, 0.0),
        cg_offset_xyz_material_id1_w: vec4(centre, 0.0),
        volume_distribution: vec4([1.0; 3], 0.0),
        ..Default::default()
    };
    header.set_bounding_box_min(min);

    Ok(PhBoundGeometry {
        base: header,
        polygons,
        un_quantize_factor: vec3(unquantize),
        bounding_box_center: vec3(centre),
        compressed_vertices,
        material_ids: mesh.materials.clone(),
        poly_mat_index_list: mesh.triangle_materials.clone(),
        ..Default::default()
    })
}

/// A `phBoundBVH` over a mesh, tree included.
///
/// ⚠ The tree reorders the polygons, so they are not in the order the triangles were given.
pub fn bvh(mesh: &BoundMesh) -> Result<PhBoundBVH, Error> {
    let mut base = geometry(mesh)?;
    base.base.r#type = Open::new(BoundType::Bvh);
    let mut bound = PhBoundBVH {
        base,
        ..Default::default()
    };
    rebuild(&mut bound)?;
    Ok(bound)
}

/// Rebuilds a BVH bound's tree over the polygons it holds.
///
/// ⚠ A leaf states a contiguous range, so the polygons are permuted, and two
/// things index them: `poly_mat_index_list` moves with them and every
/// triangle's `neighboring_polygons` is renumbered. Moving one without the
/// others repaints the surface and cross-links the adjacency, and a round trip
/// sees neither.
pub fn rebuild(bound: &mut PhBoundBVH) -> Result<(), Error> {
    let g = &mut bound.base;
    let items = g
        .polygons
        .iter()
        .enumerate()
        .map(|(i, p)| {
            g.polygon_box(p).ok_or_else(|| {
                invalid(format!(
                    "polygon {i} names a vertex this bound does not hold, so it has no box to \
                     build a tree over"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let built = build(&items, GEOMETRY_ITEM_THRESHOLD)?;

    let mut position = vec![NO_NEIGHBOUR; g.polygons.len()];
    for (new, old) in built.order.iter().enumerate() {
        position[*old] = new as u16;
    }
    let polygons = built
        .order
        .iter()
        .map(|old| {
            let mut p = g.polygons[*old].clone();
            if p.primitive() == 0 {
                for n in p.neighboring_polygons.iter_mut() {
                    if *n != NO_NEIGHBOUR {
                        *n = position
                            .get(usize::from(*n))
                            .copied()
                            .unwrap_or(NO_NEIGHBOUR);
                    }
                }
            }
            p
        })
        .collect();
    // An absent index run stays absent: a run of zeroes would add an allocation the bound did not have.
    if !g.poly_mat_index_list.is_empty() {
        g.poly_mat_index_list = built
            .order
            .iter()
            .map(|old| g.poly_mat_index_list.get(*old).copied().unwrap_or(0))
            .collect();
    }
    g.polygons = polygons;
    bound.bvh = Some(Box::new(built.bvh));
    Ok(())
}

/// Rebuilds every BVH bound under this one, through composites; returns how many.
pub fn rebuild_all(bound: &mut PhBoundAny) -> Result<usize, Error> {
    match bound {
        PhBoundAny::PhBoundBVH(b) => rebuild(b).map(|()| 1),
        PhBoundAny::PhBoundComposite(c) => c
            .bounds
            .iter_mut()
            .flatten()
            .map(|child| rebuild_all(child))
            .sum(),
        _ => Ok(0),
    }
}

/// A built tree, and the order its leaves assume the items are in.
#[derive(Debug, Clone)]
pub struct BvhBuild {
    pub bvh: PhOptimizedBvh,
    /// `order[new] = old`; empty at the composite threshold, where a leaf names its one item.
    pub order: Vec<usize>,
}

/// A tree over a list of `(min, max)` boxes.
///
/// ⚠ The tree holds only for items in [`BvhBuild::order`] above the composite
/// threshold: a leaf states `(first, count)`, so the items it covers are adjacent.
pub fn build(items: &[([f32; 3], [f32; 3])], item_threshold: usize) -> Result<BvhBuild, Error> {
    if items.is_empty() {
        return Err(invalid(
            "a BVH over no items is not a tree, it is a null pointer".into(),
        ));
    }
    if items.len() > usize::from(u16::MAX) {
        return Err(invalid(format!(
            "a leaf states its first item in sixteen bits; this tree is over {}",
            items.len()
        )));
    }

    let (min, max) = with_headroom(union(items.iter().copied()));
    let centre: [f32; 3] = std::array::from_fn(|a| (min[a] + max[a]) * 0.5);
    const RANGE: f32 = 65534.0;
    let extent: [f32; 3] = std::array::from_fn(|a| max[a] - min[a]);
    let step: [f32; 3] = std::array::from_fn(|a| {
        if extent[a] > 0.0 {
            extent[a] / RANGE
        } else {
            1.0
        }
    });
    let scale: [f32; 3] = std::array::from_fn(|a| {
        if extent[a] > 0.0 {
            RANGE / extent[a]
        } else {
            1.0
        }
    });

    let mut root = Node::over((0..items.len()).collect());
    root.split(items, item_threshold.max(1));
    root.update_box(items);
    root.number(&mut 0);
    let mut flat = Vec::new();
    root.flatten(&mut flat);
    let mut trees = Vec::new();
    root.gather_trees(&mut trees);

    let order: Vec<usize> = flat.iter().flat_map(|n| n.items.iter().copied()).collect();
    let mut position = vec![0usize; items.len()];
    for (new, old) in order.iter().enumerate() {
        position[*old] = new;
    }

    let nodes = flat
        .iter()
        .map(|node| {
            let leaf = node.total_nodes <= 1;
            let polygon_count = if leaf { node.total_items } else { 0 };
            Ok(PhOptimizedBvhNode {
                aabb_min: quantize_floor(node.min, centre, step),
                aabb_max: quantize_ceil(node.max, centre, step),
                node_data: if leaf {
                    node.items
                        .first()
                        .map_or(0, |first| position[*first] as u16)
                } else {
                    u16::try_from(node.total_nodes).map_err(|_| {
                        invalid(format!(
                            "a node's escape count is sixteen bits; it spans {} nodes",
                            node.total_nodes
                        ))
                    })?
                },
                polygon_count: u8::try_from(polygon_count).map_err(|_| {
                    invalid(format!(
                        "a leaf counts eight bits of items; it holds {polygon_count}"
                    ))
                })?,
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let subtree_headers = trees
        .iter()
        .map(|tree| PhBVHSubtreeInfo {
            aabb_min: quantize_floor(tree.min, centre, step),
            aabb_max: quantize_ceil(tree.max, centre, step),
            root_node_index: tree.index as u16,
            last_index: (tree.index + tree.total_nodes) as u16,
            ..Default::default()
        })
        .collect();

    let vector = |[x, y, z]: [f32; 3]| Vector3 {
        x,
        y,
        z,
        w: f32::NAN,
        ..Default::default()
    };
    Ok(BvhBuild {
        bvh: PhOptimizedBvh {
            contiguous_nodes: nodes,
            aabb_min: vector(min),
            aabb_max: vector(max),
            aabb_center: vector(centre),
            quantize: vector(scale),
            inv_quantize: vector(step),
            subtree_headers,
            ..Default::default()
        },
        order: if item_threshold <= COMPOSITE_ITEM_THRESHOLD {
            Vec::new()
        } else {
            order
        },
    })
}

impl PhOptimizedBvh {
    /// A node's box, decompressed the way the traversal does: `i16 * inv_quantize + aabb_center`.
    pub fn node_box(&self, node: &PhOptimizedBvhNode) -> ([f32; 3], [f32; 3]) {
        let (c, q) = (&self.aabb_center, &self.inv_quantize);
        let at = |v: [i16; 3]| {
            [
                f32::from(v[0]) * q.x + c.x,
                f32::from(v[1]) * q.y + c.y,
                f32::from(v[2]) * q.z + c.z,
            ]
        };
        (at(node.aabb_min), at(node.aabb_max))
    }
}

impl PhBoundGeometry {
    /// One vertex in the bound's own space.
    pub fn vertex_position(&self, index: usize) -> Option<[f32; 3]> {
        let v = self.compressed_vertices.get(index)?;
        let (f, c) = (&self.un_quantize_factor, &self.bounding_box_center);
        Some([
            f32::from(v.x) * f.x + c.x,
            f32::from(v.y) * f.y + c.y,
            f32::from(v.z) * f.z + c.z,
        ])
    }

    /// The vertices a polygon names, and its radius.
    ///
    /// ⚠ Each primitive keeps its indices at offsets of its own, so reading a
    /// triangle's three from a sphere reads its radius as a vertex.
    pub fn polygon_vertices(&self, p: &PhPolygon) -> Option<(Vec<usize>, f32)> {
        let mut raw = <Raw<PhPolygon> as Plain>::zeroed();
        raw.area = p.area;
        raw.vertex_indices = p.vertex_indices;
        raw.neighboring_polygons = p.neighboring_polygons;
        let bytes = Plain::as_bytes(&raw);
        let masked = |v: u16| usize::from(v & VERTEX_INDEX_MASK);
        Some(match p.primitive() {
            0 => (p.vertex_indices.iter().map(|v| masked(*v)).collect(), 0.0),
            1 => {
                let s = <Raw<PhPrimSphere> as Plain>::read_from(bytes)?;
                (vec![usize::from(s.center_index)], { s.radius })
            }
            2 | 4 => {
                let c = <Raw<PhPrimCapsule> as Plain>::read_from(bytes)?;
                (
                    vec![usize::from(c.end_index0), usize::from(c.end_index1)],
                    { c.radius },
                )
            }
            3 => {
                let b = <Raw<PhPrimBox> as Plain>::read_from(bytes)?;
                (
                    { b.vertex_indices }.iter().map(|v| masked(*v)).collect(),
                    0.0,
                )
            }
            _ => return None,
        })
    }

    /// A polygon's box, its radius included; `None` where it names a vertex this bound does not hold.
    pub fn polygon_box(&self, p: &PhPolygon) -> Option<([f32; 3], [f32; 3])> {
        let (indices, radius) = self.polygon_vertices(p)?;
        let corners = indices
            .into_iter()
            .map(|i| self.vertex_position(i).map(|v| (v, v)))
            .collect::<Option<Vec<_>>>()?;
        let (min, max) = union(corners.into_iter());
        Some((min.map(|v| v - radius), max.map(|v| v + radius)))
    }
}

fn union(boxes: impl Iterator<Item = ([f32; 3], [f32; 3])>) -> ([f32; 3], [f32; 3]) {
    boxes.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), (min, max)| {
        (
            std::array::from_fn(|a| lo[a].min(min[a])),
            std::array::from_fn(|a| hi[a].max(max[a])),
        )
    })
}

fn triangle_area(mesh: &BoundMesh, tri: &[u16; 3]) -> f32 {
    let p = |i: u16| mesh.positions[usize::from(i)];
    let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
    let ab: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let ac: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    0.5 * cross.iter().map(|v| v * v).sum::<f32>().sqrt()
}

/// Which triangle sits across each edge, or [`NO_NEIGHBOUR`].
///
/// ⚠ An edge three or more triangles share keeps the first pairing found: a
/// slot holds one index, so a non-manifold seam loses something, deterministically.
fn adjacency(triangles: &[[u16; 3]]) -> Vec<[u16; 3]> {
    let mut owner: HashMap<(u16, u16), (usize, usize)> = HashMap::new();
    let mut out = vec![[NO_NEIGHBOUR; 3]; triangles.len()];
    for (i, tri) in triangles.iter().enumerate() {
        for (slot, (a, b)) in EDGE_VERTICES.iter().enumerate() {
            let key = (tri[*a].min(tri[*b]), tri[*a].max(tri[*b]));
            match owner.get(&key) {
                Some(&(other, other_slot)) => {
                    if out[i][slot] == NO_NEIGHBOUR && out[other][other_slot] == NO_NEIGHBOUR {
                        out[i][slot] = other as u16;
                        out[other][other_slot] = i as u16;
                    }
                }
                None => {
                    owner.insert(key, (i, slot));
                }
            }
        }
    }
    out
}

/// The tree's own box, grown so no node face lands on the `i16` clamp.
///
/// ⚠ A face on the tree's extreme quantizes to ±32767 with no room left to
/// round outward, which is where a leaf stops containing its own polygon
/// (thirteen `.ybn` bounds, `cs1_15b_1.ybn` last). And two quanta are not
/// enough: at y ≈ 5315 with an extent of 5.2 one `f32` step is about four
/// quanta, so the pad is the larger of two quanta and eight `f32` steps at the
/// coordinate's magnitude.
fn with_headroom((min, max): ([f32; 3], [f32; 3])) -> ([f32; 3], [f32; 3]) {
    let pad: [f32; 3] = std::array::from_fn(|a| {
        ((max[a] - min[a]) * (2.0 / 65534.0))
            .max(min[a].abs().max(max[a].abs()) * 8.0 * f32::EPSILON)
    });
    (
        std::array::from_fn(|a| min[a] - pad[a]),
        std::array::from_fn(|a| max[a] + pad[a]),
    )
}

/// ⚠ Floor the min and ceil the max, never truncate both: the traversal
/// rejects a ray that misses a node's box, so a face rounded inward loses what
/// sits in the sliver. And each step is checked against the decompression,
/// because `quantize` and `inv_quantize` are two separately rounded floats
/// whose product is not one (two `cs1_15b_1.ybn` leaves missed their own
/// polygons by one quantum).
fn quantize_floor(v: [f32; 3], centre: [f32; 3], q: [f32; 3]) -> [i16; 3] {
    std::array::from_fn(|a| {
        let mut i = ((v[a] - centre[a]) / q[a]).floor();
        while i > -QUANTIZED_LIMIT && i * q[a] + centre[a] > v[a] {
            i -= 1.0;
        }
        i.clamp(-QUANTIZED_LIMIT, QUANTIZED_LIMIT) as i16
    })
}

fn quantize_ceil(v: [f32; 3], centre: [f32; 3], q: [f32; 3]) -> [i16; 3] {
    std::array::from_fn(|a| {
        let mut i = ((v[a] - centre[a]) / q[a]).ceil();
        while i < QUANTIZED_LIMIT && i * q[a] + centre[a] < v[a] {
            i += 1.0;
        }
        i.clamp(-QUANTIZED_LIMIT, QUANTIZED_LIMIT) as i16
    })
}

/// A node while the tree is built, holding item indices rather than boxes.
struct Node {
    items: Vec<usize>,
    children: Vec<Node>,
    min: [f32; 3],
    max: [f32; 3],
    index: usize,
}

/// A node once the tree stopped changing shape.
struct FlatNode {
    items: Vec<usize>,
    min: [f32; 3],
    max: [f32; 3],
    index: usize,
    total_nodes: usize,
    total_items: usize,
}

impl Node {
    fn over(items: Vec<usize>) -> Self {
        Self {
            items,
            children: Vec::new(),
            min: [0.0; 3],
            max: [0.0; 3],
            index: 0,
        }
    }

    fn total_nodes(&self) -> usize {
        1 + self.children.iter().map(Node::total_nodes).sum::<usize>()
    }

    fn total_items(&self) -> usize {
        self.items.len() + self.children.iter().map(Node::total_items).sum::<usize>()
    }

    fn update_box(&mut self, items: &[([f32; 3], [f32; 3])]) {
        for child in self.children.iter_mut() {
            child.update_box(items);
        }
        (self.min, self.max) = union(
            self.items
                .iter()
                .map(|i| items[*i])
                .chain(self.children.iter().map(|c| (c.min, c.max))),
        );
    }

    /// Splits on the axis whose centroid average divides the items most evenly.
    fn split(&mut self, items: &[([f32; 3], [f32; 3])], threshold: usize) {
        if self.items.len() <= threshold {
            return;
        }
        let centre = |i: usize| -> [f32; 3] {
            std::array::from_fn(|a| (items[i].0[a] + items[i].1[a]) * 0.5)
        };

        let mut sum = [0.0f64; 3];
        for i in &self.items {
            for (a, slot) in sum.iter_mut().enumerate() {
                *slot += f64::from(items[*i].0[a]) + f64::from(items[*i].1[a]);
            }
        }
        let avg: [f32; 3] =
            std::array::from_fn(|a| (sum[a] * (0.5 / self.items.len() as f64)) as f32);
        let mut below = [0usize; 3];
        for i in &self.items {
            let c = centre(*i);
            for a in 0..3 {
                if c[a] < avg[a] {
                    below[a] += 1;
                }
            }
        }
        let target = self.items.len() as f32 / 2.0;
        let skew: [f32; 3] = std::array::from_fn(|a| (target - below[a] as f32).abs());
        let axis = if skew[0] <= skew[1] && skew[0] <= skew[2] {
            0
        } else if skew[1] <= skew[2] {
            1
        } else {
            2
        };

        let (mut high, mut low): (Vec<usize>, Vec<usize>) = self
            .items
            .iter()
            .partition(|i| centre(**i)[axis] > avg[axis]);

        // Every centroid on one side of its own average happens; splitting by position recovers.
        if high.is_empty() || low.is_empty() {
            let mut all: Vec<usize> = high.drain(..).chain(low.drain(..)).collect();
            all.sort_by(|a, b| {
                items[*a]
                    .0
                    .partial_cmp(&items[*b].0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        items[*a]
                            .1
                            .partial_cmp(&items[*b].1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });
            let half = all.len() / 2;
            if half == 0 {
                self.items = all;
                return;
            }
            low = all.split_off(half);
            high = all;
        }

        self.items = Vec::new();
        for list in [high, low] {
            let mut child = Node::over(list);
            child.split(items, threshold);
            self.children.push(child);
        }
        self.children
            .sort_by_key(|c| std::cmp::Reverse(c.total_items()));
    }

    /// Depth-first, parent before children: an internal node's `node_data` is
    /// the count of nodes to skip to its next sibling, a jump only if its whole
    /// subtree sits contiguously behind it.
    fn number(&mut self, counter: &mut usize) {
        self.index = *counter;
        *counter += 1;
        for child in self.children.iter_mut() {
            child.number(counter);
        }
    }

    fn flat(&self) -> FlatNode {
        FlatNode {
            items: self.items.clone(),
            min: self.min,
            max: self.max,
            index: self.index,
            total_nodes: self.total_nodes(),
            total_items: self.total_items(),
        }
    }

    fn flatten(&self, out: &mut Vec<FlatNode>) {
        out.push(self.flat());
        for child in &self.children {
            child.flatten(out);
        }
    }

    fn gather_trees(&self, out: &mut Vec<FlatNode>) {
        if self.total_nodes() > MAX_TREE_NODE_COUNT && !self.children.is_empty() {
            for child in &self.children {
                child.gather_trees(out);
            }
        } else {
            out.push(self.flat());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::File;

    fn grid(n: usize) -> Vec<([f32; 3], [f32; 3])> {
        (0..n)
            .map(|i| {
                let x = i as f32;
                ([x, 0.0, 0.0], [x + 0.5, 1.0, 1.0])
            })
            .collect()
    }

    fn leaves(bvh: &PhOptimizedBvh) -> impl Iterator<Item = &PhOptimizedBvhNode> {
        bvh.contiguous_nodes.iter().filter(|n| n.polygon_count > 0)
    }

    /// Two triangles sharing one edge: a unit square.
    fn quad() -> BoundMesh {
        BoundMesh::new(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    }

    #[test]
    fn the_leaves_tile_the_reordered_items() {
        let items = grid(64);
        let built = build(&items, GEOMETRY_ITEM_THRESHOLD).unwrap();
        let mut covered = vec![0usize; items.len()];
        let mut next = 0usize;
        for node in leaves(&built.bvh) {
            assert_eq!(
                usize::from(node.node_data),
                next,
                "leaf ranges run in node order"
            );
            for slot in 0..usize::from(node.polygon_count) {
                covered[next + slot] += 1;
            }
            next += usize::from(node.polygon_count);
        }
        assert_eq!(next, items.len());
        assert!(covered.iter().all(|c| *c == 1), "{covered:?}");
        assert_eq!(built.order.len(), items.len());
    }

    /// ⚠ An internal node's `node_data` counts the nodes of its own subtree, so
    /// `index + node_data` is its next sibling; a wrong count skips branches.
    #[test]
    fn an_internal_nodes_escape_count_reaches_its_next_sibling() {
        let built = build(&grid(200), GEOMETRY_ITEM_THRESHOLD).unwrap();
        let nodes = &built.bvh.contiguous_nodes;
        let mut internal = 0usize;
        for (i, node) in nodes.iter().enumerate() {
            if node.polygon_count > 0 {
                continue;
            }
            internal += 1;
            let skip = usize::from(node.node_data);
            assert!(skip >= 3, "node {i}: an internal node and two children");
            assert!(i + skip <= nodes.len(), "node {i} skips past the end");
        }
        assert!(internal > 1);
    }

    #[test]
    fn a_nodes_quantized_box_contains_every_item_under_it() {
        let items = grid(40);
        let built = build(&items, GEOMETRY_ITEM_THRESHOLD).unwrap();
        let mut cursor = 0usize;
        for node in leaves(&built.bvh) {
            let (lo, hi) = built.bvh.node_box(node);
            for slot in 0..usize::from(node.polygon_count) {
                let (min, max) = items[built.order[cursor + slot]];
                assert!((0..3).all(|a| min[a] >= lo[a] && max[a] <= hi[a]));
            }
            cursor += usize::from(node.polygon_count);
        }
        assert_eq!(cursor, items.len());
    }

    #[test]
    fn a_one_item_leaf_names_its_item_and_nothing_moves() {
        assert!(build(&grid(9), COMPOSITE_ITEM_THRESHOLD)
            .unwrap()
            .order
            .is_empty());
    }

    #[test]
    fn a_tree_over_nothing_is_refused() {
        assert!(build(&[], GEOMETRY_ITEM_THRESHOLD).is_err());
    }

    #[test]
    fn a_built_bound_stores_the_positions_it_was_given() {
        let mesh = quad();
        let g = geometry(&mesh).unwrap();
        for (i, want) in mesh.positions.iter().enumerate() {
            let got = g.vertex_position(i).unwrap();
            assert!(
                (0..3).all(|a| (got[a] - want[a]).abs() < 1e-4),
                "vertex {i}: stored {got:?} for {want:?}"
            );
        }
    }

    /// ⚠ The tag is three bits of the area's own mantissa.
    #[test]
    fn every_built_polygon_carries_its_type() {
        let g = geometry(&quad()).unwrap();
        assert!(g.polygons.iter().all(|p| p.primitive() == 0));
        assert!(
            (g.polygons[0].area - 0.5).abs() < 1e-3,
            "half a unit square, to 21 bits of mantissa: {}",
            g.polygons[0].area
        );
    }

    #[test]
    fn the_shared_edge_is_the_only_one_with_a_neighbour() {
        let g = geometry(&quad()).unwrap();
        let named: Vec<usize> = g
            .polygons
            .iter()
            .map(|p| {
                p.neighboring_polygons
                    .iter()
                    .filter(|n| **n != NO_NEIGHBOUR)
                    .count()
            })
            .collect();
        assert_eq!(named, vec![1, 1], "one shared edge, named from both sides");
        // Triangle 0's shared edge (0, 2) is its slot 2; triangle 1's is its slot 0.
        assert_eq!(g.polygons[0].neighboring_polygons[2], 1);
        assert_eq!(g.polygons[1].neighboring_polygons[0], 0);
    }

    #[test]
    fn a_built_bvh_bound_is_a_bvh_bound_and_saves() {
        let bound = bvh(&quad()).unwrap();
        assert_eq!(bound.base.base.r#type.get(), BoundType::Bvh);
        assert!(bound.bvh.is_some());
        PhBoundAny::PhBoundBVH(bound).to_bytes().unwrap();
    }

    #[test]
    fn a_mesh_naming_a_material_it_does_not_hold_is_refused() {
        let mut mesh = quad();
        mesh.triangle_materials = vec![0, 0];
        assert!(geometry(&mesh).is_err());
        mesh.materials = vec![PhMaterialMgrId::default()];
        assert!(geometry(&mesh).is_ok());
    }
}

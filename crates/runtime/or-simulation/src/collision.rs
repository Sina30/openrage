// Median-split BVH; ray hits use Möller–Trumbore.
use glam::Vec3;
use or_io::gta5::bounds::VERTEX_INDEX_MASK;
use or_io::gta5::{
    CompressedVertex, Mat34V, PhBound, PhBoundAny, PhBoundComposite, PhBoundGeometry,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub pos: Vec3,
    pub normal: Vec3,
    pub mtl_id: u8,
}

#[derive(Debug, Clone, Copy)]
struct Tri {
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    normal: Vec3,
    mtl_id: u8,
}

impl Tri {
    fn new(v0: Vec3, v1: Vec3, v2: Vec3, mtl_id: u8) -> Self {
        let n = (v1 - v0).cross(v2 - v0).normalize_or_zero();
        Self {
            v0,
            v1,
            v2,
            normal: n,
            mtl_id,
        }
    }

    fn centroid(&self) -> Vec3 {
        (self.v0 + self.v1 + self.v2) / 3.0
    }

    fn aabb(&self) -> Aabb {
        let min = self.v0.min(self.v1).min(self.v2);
        let max = self.v0.max(self.v1).max(self.v2);
        Aabb { min, max }
    }
}

#[derive(Debug, Clone, Copy)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    fn ray_t_min(&self, origin: Vec3, inv_dir: Vec3, max_t: f32) -> f32 {
        let t1 = (self.min - origin) * inv_dir;
        let t2 = (self.max - origin) * inv_dir;
        let tmin = t1.min(t2);
        let tmax = t1.max(t2);
        let entry = tmin.x.max(tmin.y).max(tmin.z).max(0.0);
        let exit = tmax.x.min(tmax.y).min(tmax.z).min(max_t);
        if entry <= exit {
            entry
        } else {
            f32::INFINITY
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    aabb: Aabb,
    left: u32,
    right: u32,
    tri_count: u32,
}

const BVH_MAX_LEAF: usize = 4;
#[derive(Debug, Clone, Default)]
pub struct BvhCollider {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

impl BvhCollider {
    pub fn from_phbound(bound: &PhBoundAny) -> Self {
        let mut tris: Vec<Tri> = Vec::new();
        collect_tris(bound, glam::Mat4::IDENTITY, &mut tris);
        Self::from_tris(tris)
    }

    #[doc(hidden)]
    pub fn from_world_triangles(triangles: &[(Vec3, Vec3, Vec3)], mtl_ids: &[u8]) -> Self {
        let mut tris = Vec::with_capacity(triangles.len());
        for (i, (a, b, c)) in triangles.iter().enumerate() {
            let mtl = mtl_ids.get(i).copied().unwrap_or(0);
            tris.push(Tri::new(*a, *b, *c, mtl));
        }
        Self::from_tris(tris)
    }

    fn from_tris(mut tris: Vec<Tri>) -> Self {
        if tris.is_empty() {
            return Self::default();
        }
        let mut nodes: Vec<Node> = Vec::with_capacity(2 * tris.len());
        let mut indices: Vec<u32> = (0..tris.len() as u32).collect();
        let tris_len = tris.len();
        build(&mut tris, &mut indices, 0, tris_len, &mut nodes);
        let reordered: Vec<Tri> = indices.iter().map(|&i| tris[i as usize]).collect();
        Self {
            tris: reordered,
            nodes,
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    pub fn ray_cast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        if self.nodes.is_empty() || self.tris.is_empty() || max_t <= 0.0 {
            return None;
        }
        let dir_n = dir.normalize_or_zero();
        if dir_n == Vec3::ZERO {
            return None;
        }
        let inv_dir = Vec3::new(
            if dir_n.x.abs() > 1e-20 {
                1.0 / dir_n.x
            } else {
                f32::INFINITY
            },
            if dir_n.y.abs() > 1e-20 {
                1.0 / dir_n.y
            } else {
                f32::INFINITY
            },
            if dir_n.z.abs() > 1e-20 {
                1.0 / dir_n.z
            } else {
                f32::INFINITY
            },
        );

        let mut best: Option<Hit> = None;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(idx) = stack.pop() {
            let node = self.nodes[idx as usize];
            let cur_max = best.map(|h| h.t).unwrap_or(max_t);
            let entry = node.aabb.ray_t_min(origin, inv_dir, cur_max);
            if entry == f32::INFINITY {
                continue;
            }
            if node.tri_count > 0 {
                let start = node.left as usize;
                let end = start + node.tri_count as usize;
                for tri in &self.tris[start..end] {
                    if let Some(h) = ray_triangle(origin, dir_n, max_t, tri) {
                        if best.map(|b| h.t < b.t).unwrap_or(true) {
                            best = Some(h);
                        }
                    }
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        best
    }
}

// Möller–Trumbore ray/triangle test; normal is flipped to face the ray.
fn ray_triangle(origin: Vec3, dir: Vec3, max_t: f32, tri: &Tri) -> Option<Hit> {
    const EPS: f32 = 1e-7;
    let edge1 = tri.v1 - tri.v0;
    let edge2 = tri.v2 - tri.v0;
    let h = dir.cross(edge2);
    let a = edge1.dot(h);
    if a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = origin - tri.v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = f * dir.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * edge2.dot(q);
    if t < 0.0 || t > max_t {
        return None;
    }
    let mut n = tri.normal;
    if n.dot(dir) > 0.0 {
        n = -n;
    }
    Some(Hit {
        t,
        pos: origin + dir * t,
        normal: n,
        mtl_id: tri.mtl_id,
    })
}

fn build(
    tris: &mut [Tri],
    indices: &mut [u32],
    node_offset: usize,
    count: usize,
    nodes: &mut Vec<Node>,
) -> u32 {
    let mut aabb = Aabb::empty();
    for &i in &indices[node_offset..node_offset + count] {
        aabb = aabb.union(tris[i as usize].aabb());
    }
    if count <= BVH_MAX_LEAF {
        let leaf_idx = nodes.len() as u32;
        nodes.push(Node {
            aabb,
            left: node_offset as u32,
            right: 0,
            tri_count: count as u32,
        });
        return leaf_idx;
    }
    let extent = aabb.extent();
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let slice = &mut indices[node_offset..node_offset + count];
    slice.sort_by(|&a, &b| {
        let ca = component(tris[a as usize].centroid(), axis);
        let cb = component(tris[b as usize].centroid(), axis);
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mid = count / 2;
    let my_idx = nodes.len() as u32;
    nodes.push(Node {
        aabb,
        left: 0,
        right: 0,
        tri_count: 0,
    });
    let left_idx = build(tris, indices, node_offset, mid, nodes);
    let right_idx = build(tris, indices, node_offset + mid, count - mid, nodes);
    nodes[my_idx as usize].left = left_idx;
    nodes[my_idx as usize].right = right_idx;
    my_idx
}

fn component(v: Vec3, axis: u8) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

fn collect_tris(bound: &PhBoundAny, world: glam::Mat4, out: &mut Vec<Tri>) {
    match bound {
        PhBoundAny::PhBoundGeometry(g) => collect_geometry_tris(g, world, out),
        PhBoundAny::PhBoundBVH(b) => collect_geometry_tris(&b.base, world, out),
        PhBoundAny::PhBoundComposite(c) => collect_composite_tris(c, world, out),
        PhBoundAny::PhBoundBox(b) => collect_box_tris(&b.base, world, out),
        PhBoundAny::PhBoundSphere(b) => collect_sphere_tris(&b.base, world, out),
        PhBoundAny::PhBoundCapsule(b) => collect_capsule_tris(&b.base, world, out),
        PhBoundAny::PhBoundCylinder(b) => collect_cylinder_tris(&b.base, world, out),
        PhBoundAny::PhBoundDisc(b) => collect_disc_tris(&b.base, world, out),
        // ⚠ No shipped file holds a plane, and its record states nothing past the header.
        PhBoundAny::PhBoundPlane(b) => collect_box_tris(&b.base, world, out),
        PhBoundAny::Base(_) => {}
    }
}

fn centroid(header: &PhBound) -> Vec3 {
    let c = &header.centroid_offset_xyz_material_id0_w;
    Vec3::new(c.x, c.y, c.z)
}

fn bb(header: &PhBound) -> (Vec3, Vec3) {
    let max = &header.bounding_box_max_xyz_margin_w;
    (
        Vec3::from(header.bounding_box_min()),
        Vec3::new(max.x, max.y, max.z),
    )
}

/// The margin, in the `w` of the bounding box's maximum.
fn margin(header: &PhBound) -> f32 {
    header.bounding_box_max_xyz_margin_w.w
}

fn collect_sphere_tris(header: &PhBound, world: glam::Mat4, out: &mut Vec<Tri>) {
    let r = header.radius_around_centroid;
    if r <= 0.0 {
        return;
    }
    tessellate_sphere(centroid(header), r, world, out);
}

fn tessellate_sphere(center: Vec3, r: f32, world: glam::Mat4, out: &mut Vec<Tri>) {
    const STACKS: usize = 8;
    const SLICES: usize = 16;
    use std::f32::consts::PI;
    let mut grid: Vec<Vec<Vec3>> = Vec::with_capacity(STACKS + 1);
    for s in 0..=STACKS {
        let theta = (s as f32 / STACKS as f32) * PI;
        let y = theta.cos() * r;
        let r_ring = theta.sin() * r;
        let mut ring = Vec::with_capacity(SLICES);
        for i in 0..SLICES {
            let phi = (i as f32 / SLICES as f32) * 2.0 * PI;
            let p = Vec3::new(
                center.x + r_ring * phi.cos(),
                center.y + y,
                center.z + r_ring * phi.sin(),
            );
            ring.push(world.transform_point3(p));
        }
        grid.push(ring);
    }
    for s in 0..STACKS {
        for i in 0..SLICES {
            let i_next = (i + 1) % SLICES;
            let a = grid[s][i];
            let b = grid[s][i_next];
            let c = grid[s + 1][i];
            let d = grid[s + 1][i_next];
            out.push(Tri::new(a, c, b, 0));
            out.push(Tri::new(b, c, d, 0));
        }
    }
}

/// ⚠ A capsule's radius is its margin, and half its length the rest of its radius around the centroid.
fn collect_capsule_tris(header: &PhBound, world: glam::Mat4, out: &mut Vec<Tri>) {
    let r = margin(header);
    let half_len = header.radius_around_centroid - r;
    if r <= 0.0 {
        return;
    }
    let center = centroid(header);
    tessellate_cylinder(center, r, half_len, world, out, false);
    tessellate_hemisphere(center + Vec3::new(0.0, half_len, 0.0), r, world, out, true);
    tessellate_hemisphere(
        center + Vec3::new(0.0, -half_len, 0.0),
        r,
        world,
        out,
        false,
    );
}

fn tessellate_hemisphere(center: Vec3, r: f32, world: glam::Mat4, out: &mut Vec<Tri>, top: bool) {
    const STACKS: usize = 4;
    const SLICES: usize = 16;
    use std::f32::consts::PI;
    let mut grid: Vec<Vec<Vec3>> = Vec::with_capacity(STACKS + 1);
    for s in 0..=STACKS {
        let theta = (s as f32 / STACKS as f32) * (PI * 0.5);
        let dy = theta.sin() * r * if top { 1.0 } else { -1.0 };
        let r_ring = theta.cos() * r;
        let mut ring = Vec::with_capacity(SLICES);
        for i in 0..SLICES {
            let phi = (i as f32 / SLICES as f32) * 2.0 * PI;
            let p = Vec3::new(
                center.x + r_ring * phi.cos(),
                center.y + dy,
                center.z + r_ring * phi.sin(),
            );
            ring.push(world.transform_point3(p));
        }
        grid.push(ring);
    }
    for s in 0..STACKS {
        for i in 0..SLICES {
            let i_next = (i + 1) % SLICES;
            let a = grid[s][i];
            let b = grid[s][i_next];
            let c = grid[s + 1][i];
            let d = grid[s + 1][i_next];
            if top {
                out.push(Tri::new(a, c, b, 0));
                out.push(Tri::new(b, c, d, 0));
            } else {
                out.push(Tri::new(a, b, c, 0));
                out.push(Tri::new(b, d, c, 0));
            }
        }
    }
}

/// ⚠ A cylinder's radius and half-height are read off its box: the fields it declares for them are zero in every one shipped.
fn collect_cylinder_tris(header: &PhBound, world: glam::Mat4, out: &mut Vec<Tri>) {
    let (mn, mx) = bb(header);
    let r = (mx.x - mn.x).max(mx.z - mn.z) * 0.5;
    let half_h = (mx.y - mn.y) * 0.5;
    if r <= 0.0 {
        return;
    }
    tessellate_cylinder(centroid(header), r, half_h, world, out, true);
}

fn tessellate_cylinder(
    center: Vec3,
    r: f32,
    half_h: f32,
    world: glam::Mat4,
    out: &mut Vec<Tri>,
    capped: bool,
) {
    const SLICES: usize = 24;
    use std::f32::consts::PI;
    let top_y = center.y + half_h;
    let bot_y = center.y - half_h;
    for i in 0..SLICES {
        let i_next = (i + 1) % SLICES;
        let phi0 = (i as f32 / SLICES as f32) * 2.0 * PI;
        let phi1 = (i_next as f32 / SLICES as f32) * 2.0 * PI;
        let p0 = Vec3::new(center.x + r * phi0.cos(), 0.0, center.z + r * phi0.sin());
        let p1 = Vec3::new(center.x + r * phi1.cos(), 0.0, center.z + r * phi1.sin());
        let a = world.transform_point3(Vec3::new(p0.x, top_y, p0.z));
        let b = world.transform_point3(Vec3::new(p1.x, top_y, p1.z));
        let c = world.transform_point3(Vec3::new(p0.x, bot_y, p0.z));
        let d = world.transform_point3(Vec3::new(p1.x, bot_y, p1.z));
        out.push(Tri::new(a, c, b, 0));
        out.push(Tri::new(b, c, d, 0));
    }
    if capped {
        let top_centre = world.transform_point3(Vec3::new(center.x, top_y, center.z));
        let bot_centre = world.transform_point3(Vec3::new(center.x, bot_y, center.z));
        for i in 0..SLICES {
            let i_next = (i + 1) % SLICES;
            let phi0 = (i as f32 / SLICES as f32) * 2.0 * PI;
            let phi1 = (i_next as f32 / SLICES as f32) * 2.0 * PI;
            let a = world.transform_point3(Vec3::new(
                center.x + r * phi0.cos(),
                top_y,
                center.z + r * phi0.sin(),
            ));
            let b = world.transform_point3(Vec3::new(
                center.x + r * phi1.cos(),
                top_y,
                center.z + r * phi1.sin(),
            ));
            let c = world.transform_point3(Vec3::new(
                center.x + r * phi0.cos(),
                bot_y,
                center.z + r * phi0.sin(),
            ));
            let d = world.transform_point3(Vec3::new(
                center.x + r * phi1.cos(),
                bot_y,
                center.z + r * phi1.sin(),
            ));
            out.push(Tri::new(top_centre, b, a, 0));
            out.push(Tri::new(bot_centre, c, d, 0));
        }
    }
}

/// ⚠ Half a disc's thickness is its margin, or half its box along X: the field it declares for it is zero in every one shipped.
fn collect_disc_tris(header: &PhBound, world: glam::Mat4, out: &mut Vec<Tri>) {
    let r = header.radius_around_centroid;
    let m = match margin(header) {
        m if m > 0.0 => m,
        _ => {
            let (mn, mx) = bb(header);
            (mx.x - mn.x) * 0.5
        }
    }
    .max(0.0);
    if r <= 0.0 {
        return;
    }
    let center = centroid(header);
    const SLICES: usize = 24;
    use std::f32::consts::PI;
    let pos_x_centre = world.transform_point3(Vec3::new(center.x + m, center.y, center.z));
    let neg_x_centre = world.transform_point3(Vec3::new(center.x - m, center.y, center.z));
    for i in 0..SLICES {
        let i_next = (i + 1) % SLICES;
        let phi0 = (i as f32 / SLICES as f32) * 2.0 * PI;
        let phi1 = (i_next as f32 / SLICES as f32) * 2.0 * PI;
        let a = world.transform_point3(Vec3::new(
            center.x + m,
            center.y + r * phi0.cos(),
            center.z + r * phi0.sin(),
        ));
        let b = world.transform_point3(Vec3::new(
            center.x + m,
            center.y + r * phi1.cos(),
            center.z + r * phi1.sin(),
        ));
        let c2 = world.transform_point3(Vec3::new(
            center.x - m,
            center.y + r * phi0.cos(),
            center.z + r * phi0.sin(),
        ));
        let d = world.transform_point3(Vec3::new(
            center.x - m,
            center.y + r * phi1.cos(),
            center.z + r * phi1.sin(),
        ));
        out.push(Tri::new(pos_x_centre, a, b, 0));
        out.push(Tri::new(neg_x_centre, d, c2, 0));
        if m > 0.0 {
            out.push(Tri::new(a, c2, b, 0));
            out.push(Tri::new(b, c2, d, 0));
        }
    }
}

fn collect_box_tris(header: &PhBound, world: glam::Mat4, out: &mut Vec<Tri>) {
    let (lo, hi) = bb(header);
    let xform = |p: Vec3| -> Vec3 { world.transform_point3(p) };
    let v = [
        xform(Vec3::new(lo.x, lo.y, lo.z)),
        xform(Vec3::new(hi.x, lo.y, lo.z)),
        xform(Vec3::new(hi.x, hi.y, lo.z)),
        xform(Vec3::new(lo.x, hi.y, lo.z)),
        xform(Vec3::new(lo.x, lo.y, hi.z)),
        xform(Vec3::new(hi.x, lo.y, hi.z)),
        xform(Vec3::new(hi.x, hi.y, hi.z)),
        xform(Vec3::new(lo.x, hi.y, hi.z)),
    ];
    let faces = [
        (0, 2, 1),
        (0, 3, 2),
        (4, 5, 6),
        (4, 6, 7),
        (0, 1, 5),
        (0, 5, 4),
        (3, 7, 6),
        (3, 6, 2),
        (0, 4, 7),
        (0, 7, 3),
        (1, 2, 6),
        (1, 6, 5),
    ];
    for (a, b, c) in faces {
        out.push(Tri::new(v[a], v[b], v[c], 0));
    }
}

fn collect_composite_tris(c: &PhBoundComposite, world: glam::Mat4, out: &mut Vec<Tri>) {
    for (i, child) in c.bounds.iter().enumerate() {
        let Some(child) = child else { continue };
        let local = c
            .current_matrices
            .get(i)
            .map_or(glam::Mat4::IDENTITY, mat34v_to_mat4);
        collect_tris(child, world * local, out);
    }
}

fn collect_geometry_tris(g: &PhBoundGeometry, world: glam::Mat4, out: &mut Vec<Tri>) {
    let unquantize = Vec3::new(
        g.un_quantize_factor.x,
        g.un_quantize_factor.y,
        g.un_quantize_factor.z,
    );
    let center = Vec3::new(
        g.bounding_box_center.x,
        g.bounding_box_center.y,
        g.bounding_box_center.z,
    );
    let vertex = |i: u16| {
        g.compressed_vertices
            .get(usize::from(i & VERTEX_INDEX_MASK))
            .map(|v| decompress(v, unquantize, center, world))
    };
    for (poly_idx, poly) in g.polygons.iter().enumerate() {
        if poly.primitive() != 0 {
            continue;
        }
        let [a, b, c] = poly.vertex_indices;
        let (Some(p0), Some(p1), Some(p2)) = (vertex(a), vertex(b), vertex(c)) else {
            continue;
        };
        let mtl_id = g.poly_mat_index_list.get(poly_idx).copied().unwrap_or(0);
        out.push(Tri::new(p0, p1, p2, mtl_id));
    }
}

fn decompress(v: &CompressedVertex, unq: Vec3, ctr: Vec3, world: glam::Mat4) -> Vec3 {
    let p = Vec3::new(f32::from(v.x), f32::from(v.y), f32::from(v.z)) * unq + ctr;
    world.transform_point3(p)
}

fn mat34v_to_mat4(m: &Mat34V) -> glam::Mat4 {
    glam::Mat4::from_cols(
        glam::Vec4::new(m.col0.x, m.col0.y, m.col0.z, 0.0),
        glam::Vec4::new(m.col1.x, m.col1.y, m.col1.z, 0.0),
        glam::Vec4::new(m.col2.x, m.col2.y, m.col2.z, 0.0),
        glam::Vec4::new(m.col3.x, m.col3.y, m.col3.z, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bvh_ray_cast_hits_known_triangle() {
        let tris = [(
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )];
        let bvh = BvhCollider::from_world_triangles(&tris, &[7]);
        assert_eq!(bvh.triangle_count(), 1);
        let hit = bvh
            .ray_cast(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0), 10.0)
            .expect("ray should hit triangle");
        assert!((hit.t - 1.0).abs() < 1e-4);
        assert!((hit.pos.z - 0.0).abs() < 1e-4);
        assert!(hit.normal.z > 0.99);
        assert_eq!(hit.mtl_id, 7);
    }

    #[test]
    fn bvh_ray_misses_when_outside_aabb() {
        let tris = [(
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )];
        let bvh = BvhCollider::from_world_triangles(&tris, &[0]);
        let miss = bvh.ray_cast(Vec3::new(10.0, 10.0, 5.0), Vec3::new(0.0, 0.0, -1.0), 100.0);
        assert!(miss.is_none());
    }

    #[test]
    fn bvh_returns_nearest_hit_across_layered_triangles() {
        let tris = [
            (
                Vec3::new(-1.0, -1.0, 2.0),
                Vec3::new(1.0, -1.0, 2.0),
                Vec3::new(0.0, 1.0, 2.0),
            ),
            (
                Vec3::new(-1.0, -1.0, 5.0),
                Vec3::new(1.0, -1.0, 5.0),
                Vec3::new(0.0, 1.0, 5.0),
            ),
        ];
        let bvh = BvhCollider::from_world_triangles(&tris, &[1, 2]);
        let hit = bvh
            .ray_cast(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 10.0)
            .unwrap();
        assert!((hit.t - 2.0).abs() < 1e-4);
        assert_eq!(hit.mtl_id, 1);
    }
}

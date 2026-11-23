use super::super::super::bound::cylinder_box;
use super::*;

/// The built child's vertex quantum: its box is 2 m either side of the origin.
const STEP: f32 = 2.0 / 32767.0;
/// The BVH's quanta a metre over the box's 4 m.
const PER_METRE: f32 = 65534.0 / 4.0;
const MARGIN: f32 = 0.04;
/// The sphere's leaf, which no move here reaches.
const SPHERE_LEAF: [i16; 6] = [-11, -12, -13, 14, 15, 16];

/// A built bound whose one child a vertex move may change, and where its parts lie.
struct Movable {
    img: Image,
    child: usize,
    vertices: usize,
    nodes: usize,
}

fn quantum(x: f32) -> i16 {
    (f64::from(x) / f64::from(STEP)).round() as i16
}

fn decoded(q: i16) -> f32 {
    (f64::from(q) * f64::from(STEP)) as f32
}

fn i16_at(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}

fn node(b: &[u8], nodes: usize, i: usize) -> [i16; 6] {
    std::array::from_fn(|k| i16_at(b, nodes + 16 * i + 2 * k))
}

/// The built bound with its child's box 2 m either side of the origin, its triangle over vertices 0
/// to 2 and a sphere of 0.25 m at vertex 2, and a BVH of a parent over the triangle's leaf and the
/// sphere's.
fn movable() -> Movable {
    let (img, parts) = bound();
    movable_in(img, &parts)
}

/// [`movable`]'s child, in the bound `parts` built into `img`.
fn movable_in(mut img: Image, parts: &BoundParts) -> Movable {
    let child = child_of(&img, parts);
    img.vec3(child + 0x20, [2.0; 3]);
    img.vec3(child + 0x30, [-2.0; 3]);
    img.vec3(child + 0x90, [STEP; 3]);
    img.vec3(child + 0xA0, [0.0; 3]);
    img.put(child + 0x2C, &MARGIN.to_le_bytes());
    img.put(child + 0x14, &1.5f32.to_le_bytes());
    let vertices = u64_at(&img.bytes, child + 0xB0) as usize & 0x0FFF_FFFF;
    for (i, v) in TRIANGLE_AT.iter().enumerate() {
        for (a, x) in v.iter().enumerate() {
            img.u16(vertices + 6 * i + 2 * a, quantum(*x) as u16);
        }
    }
    img.u16(parts.primitives + 16, 2);
    img.put(parts.primitives + 16 + 4, &0.25f32.to_le_bytes());
    let bvh = u64_at(&img.bytes, child + 0x130) as usize & 0x0FFF_FFFF;
    img.vec3(bvh + 0x30, [PER_METRE; 3]);
    let nodes = img.alloc(48);
    img.u16(nodes + 0xC, 3);
    img.u16(nodes + 16 + 0xE, 1);
    img.u16(nodes + 32 + 0xC, 1);
    img.u16(nodes + 32 + 0xE, 1);
    for (k, q) in SPHERE_LEAF.iter().enumerate() {
        img.u16(nodes + 32 + 2 * k, *q as u16);
    }
    img.ptr(bvh + 0x50, nodes);
    img.u32(bvh + 0x58, 3);
    Movable {
        img,
        child,
        vertices,
        nodes,
    }
}

/// The bound in `bytes`, its child's vertex `vertex` at `to`.
fn moved(bytes: &[u8], vertex: usize, to: [f32; 3]) -> Bound {
    let mut edited = read_bound(bytes).expect("the bound reads");
    let Some(Ok(ChildBound::Geometry(g))) = &mut edited.children[0].bound else {
        panic!("a geometry child");
    };
    g.vertices[vertex] = to;
    edited
}

#[test]
fn a_moved_vertex_is_quantised_again_and_its_triangle_s_leaf_and_their_parent_fit_it() {
    let mut m = movable();
    let to = [1.5, 0.25, -0.5];
    let g = only_geometry(&m.img.bytes).expect("the child reads");
    vertex_move_holds(&g, 1, to).expect("inside the box");
    let edited = moved(&m.img.bytes, 1, to);
    write_bound_vertices(&mut m.img.bytes, &edited).expect("in place");

    let b = &m.img.bytes;
    let q = to.map(quantum);
    assert_eq!([0, 2, 4].map(|o| i16_at(b, m.vertices + 6 + o)), q);
    let read = only_geometry(b).expect("it reads back").vertices[1];
    assert!((0..3).all(|a| (read[a] - to[a]).abs() <= STEP), "{read:?}");

    // The triangle's leaf: its corners grown by the margin, rounded out to whole quanta.
    let corners = [[0i16; 3], q, TRIANGLE_AT[2].map(quantum)].map(|c| c.map(decoded));
    let lo: [f32; 3] =
        std::array::from_fn(|a| corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min) - MARGIN);
    let hi: [f32; 3] =
        std::array::from_fn(|a| corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max) + MARGIN);
    let leaf: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => (lo[k] * PER_METRE).floor() as i16,
        _ => (hi[k - 3] * PER_METRE).ceil() as i16,
    });
    assert_eq!(node(b, m.nodes, 1), leaf);
    assert_eq!(
        node(b, m.nodes, 2),
        SPHERE_LEAF,
        "the sphere's leaf is not touched"
    );
    let root: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => leaf[k].min(SPHERE_LEAF[k]),
        _ => leaf[k].max(SPHERE_LEAF[k]),
    });
    assert_eq!(
        node(b, m.nodes, 0),
        root,
        "the parent is the union of its two"
    );

    let far = q.map(decoded).iter().map(|x| x * x).sum::<f32>().sqrt() + MARGIN;
    assert!(far > 1.5);
    let radius = f32::from_bits(u32_at(b, m.child + 0x14));
    assert!((radius - far).abs() < 1e-5, "{radius} for {far}");
}

fn vec3_at(b: &[u8], at: usize) -> [f32; 3] {
    std::array::from_fn(|a| f32::from_bits(u32_at(b, at + 4 * a)))
}

#[test]
fn a_move_out_of_the_box_grows_it_and_every_vertex_leaf_and_the_composite_follow() {
    let mut m = movable();
    let to = [2.5, 0.0, 0.0];
    let g = only_geometry(&m.img.bytes).expect("the child reads");
    vertex_move_holds(&g, 1, to).expect("a move past the box grows it");
    let edited = moved(&m.img.bytes, 1, to);
    write_bound_vertices(&mut m.img.bytes, &edited).expect("the box grows");
    let b = &m.img.bytes;

    // Only +x grows: a quarter margin and two quanta of the grown box past the vertex.
    let held_x: f32 = 2.5 - -2.0;
    let max_x = 2.5 + (0.25 * MARGIN + 2.0 * held_x / 65534.0);
    let read = only_geometry(b).expect("it reads back");
    assert_eq!((read.box_min, read.box_max), ([-2.0; 3], [max_x, 2.0, 2.0]));
    // The quantisation follows the box: its centre, and GTA V's scale of its half width.
    let per = f32::from_bits(0x3800_0100);
    let centre: [f32; 3] = [(-2.0 + max_x) / 2.0, 0.0, 0.0];
    let scale: [f32; 3] = [(max_x - -2.0) / 2.0 * per, 2.0 * per, 2.0 * per];
    assert_eq!(vec3_at(b, m.child + 0xA0), centre);
    assert_eq!(vec3_at(b, m.child + 0x90), scale);

    // Every vertex is stored again in that frame: the moved one, and the others as they decoded.
    let again = |p: [f32; 3]| -> [i16; 3] {
        std::array::from_fn(|a| {
            ((f64::from(p[a]) - f64::from(centre[a])) / f64::from(scale[a])).round() as i16
        })
    };
    let given = [
        TRIANGLE_AT[0].map(quantum).map(decoded),
        to,
        TRIANGLE_AT[2].map(quantum).map(decoded),
    ];
    let mut now = Vec::new();
    for (i, p) in given.iter().enumerate() {
        let q = again(*p);
        assert_eq!(
            [0, 2, 4].map(|o| i16_at(b, m.vertices + 6 * i + o)),
            q,
            "vertex {i}"
        );
        let at: [f32; 3] = std::array::from_fn(|a| {
            (f64::from(centre[a]) + f64::from(q[a]) * f64::from(scale[a])) as f32
        });
        now.push(at);
    }

    // The BVH's frame is the grown box's, and both leaves are fitted again in it.
    let bvh = u64_at(b, m.child + 0x130) as usize & 0x0FFF_FFFF;
    let per_metre: [f32; 3] = [65534.0 / (max_x - -2.0), 65534.0 / 4.0, 65534.0 / 4.0];
    assert_eq!(vec3_at(b, bvh), [-2.0; 3]);
    assert_eq!(vec3_at(b, bvh + 0x10), [max_x, 2.0, 2.0]);
    assert_eq!(vec3_at(b, bvh + 0x20), centre);
    assert_eq!(vec3_at(b, bvh + 0x30), per_metre);
    let leaf = |lo: [f32; 3], hi: [f32; 3]| -> [i16; 6] {
        std::array::from_fn(|k| match k {
            0..3 => ((lo[k] - centre[k]) * per_metre[k]).floor() as i16,
            _ => ((hi[k - 3] - centre[k - 3]) * per_metre[k - 3]).ceil() as i16,
        })
    };
    let lo: [f32; 3] =
        std::array::from_fn(|a| now.iter().map(|c| c[a]).fold(f32::MAX, f32::min) - MARGIN);
    let hi: [f32; 3] =
        std::array::from_fn(|a| now.iter().map(|c| c[a]).fold(f32::MIN, f32::max) + MARGIN);
    let triangle = leaf(lo, hi);
    let sphere = leaf(now[2].map(|x| x - 0.25), now[2].map(|x| x + 0.25));
    assert_eq!(node(b, m.nodes, 1), triangle);
    assert_eq!(node(b, m.nodes, 2), sphere);
    let root: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => triangle[k].min(sphere[k]),
        _ => triangle[k].max(sphere[k]),
    });
    assert_eq!(node(b, m.nodes, 0), root);

    // The radius reaches every vertex stored again, with the margin.
    let reach = |p: &[f32; 3]| p.iter().map(|x| x.powi(2)).sum::<f32>().sqrt() + MARGIN;
    let radius = now.iter().map(reach).fold(1.5, f32::max);
    assert_eq!(f32::from_bits(u32_at(b, m.child + 0x14)), radius);

    // The composite's box holds the grown child, and its radius reaches the child's far corners.
    let composite = read_bound(b).expect("the bound reads back");
    let far = [max_x, 2.0, 2.0]
        .iter()
        .map(|x: &f32| x.powi(2))
        .sum::<f32>()
        .sqrt();
    assert_eq!(
        (composite.box_min, composite.box_max, composite.radius),
        ([-2.0; 3], [max_x, 2.0, 2.0], far)
    );
}

#[test]
fn a_vertex_past_the_vertices_or_not_finite_or_a_tree_out_of_order_is_refused() {
    let m = movable();
    let g = only_geometry(&m.img.bytes).expect("the child reads");
    assert!(matches!(
        vertex_move_holds(&g, 3, [0.0; 3]),
        Err(HeaderlessError::IndexPastCount { index: 3, .. })
    ));
    assert!(vertex_move_holds(&g, 1, [f32::NAN, 0.0, 0.0]).is_err());
    let mut plain = only_geometry(&m.img.bytes).expect("the child reads");
    plain.bvh = None;
    assert!(vertex_move_holds(&plain, 1, [0.5; 3]).is_err(), "no BVH");

    // A parent whose subtree count is not its subtree's.
    let mut bad = movable();
    bad.img.u16(bad.nodes + 0xC, 2);
    let before = bad.img.bytes.clone();
    let edited = moved(&before, 1, [0.5, 0.5, 0.0]);
    assert!(write_bound_vertices(&mut bad.img.bytes, &edited).is_err());
    assert_eq!(bad.img.bytes, before);
}

#[test]
fn a_vertex_of_a_drawable_s_bound_moves_through_its_root_and_grows_the_composite_there() {
    let (mut img, phys, d) = drawable([1.0, 1.0, 0.0]);
    let parts = bound_into(&mut img);
    img.ptr(d.root + 0xC8, parts.root);
    let mut m = movable_in(img, &parts);
    let embedded =
        |b: &[u8]| (read_drawable(b, Some(&phys)).expect("it reads").bound).expect("its composite");
    let moved_to = |b: &[u8], to: [f32; 3]| {
        let mut edited = embedded(b);
        let Some(Ok(ChildBound::Geometry(g))) = &mut edited.children[0].bound else {
            panic!("a geometry child");
        };
        g.vertices[1] = to;
        edited
    };

    let to = [1.5, 0.25, -0.5];
    let edited = moved_to(&m.img.bytes, to);
    write_drawable_bound(&mut m.img.bytes, &phys, &edited).expect("in place");
    let q = to.map(quantum);
    assert_eq!(
        [0, 2, 4].map(|o| i16_at(&m.img.bytes, m.vertices + 6 + o)),
        q
    );
    let read = embedded(&m.img.bytes);
    let Some(Ok(ChildBound::Geometry(g))) = &read.children[0].bound else {
        panic!("a geometry child");
    };
    assert!(
        (0..3).all(|a| (g.vertices[1][a] - to[a]).abs() <= STEP),
        "{g:?}"
    );

    // Past the child's box: the composite the root names grows to hold the grown child.
    let edited = moved_to(&m.img.bytes, [2.5, 0.0, 0.0]);
    write_drawable_bound(&mut m.img.bytes, &phys, &edited).expect("the box grows");
    let read = embedded(&m.img.bytes);
    let Some(Ok(ChildBound::Geometry(g))) = &read.children[0].bound else {
        panic!("a geometry child");
    };
    assert!(g.box_max[0] > 2.5, "{:?}", g.box_max);
    assert_eq!(
        read.box_max[0], g.box_max[0],
        "the composite holds its child"
    );
    assert!(read.radius > 2.5, "{}", read.radius);
}

#[test]
fn a_cylinder_s_box_grows_its_ends_by_the_radius_across_its_axis_and_a_zero_axis_not_at_all() {
    assert_eq!(
        cylinder_box([0.0; 3], [2.0, 0.0, 0.0], 0.5),
        ([0.0, -0.5, -0.5], [2.0, 0.5, 0.5])
    );
    let at = [1.0, 2.0, 3.0];
    assert_eq!(cylinder_box(at, at, 0.5), (at, at));
    // Along (0.6, 0.8, 0) the sines across x, y and z are 0.8, 0.6 and 1.
    let (lo, hi) = cylinder_box([0.0; 3], [3.0, 4.0, 0.0], 1.0);
    let (want_lo, want_hi) = ([-0.8, -0.6, -1.0], [3.8, 4.6, 1.0]);
    for a in 0..3 {
        assert!((lo[a] - want_lo[a]).abs() < 1e-6, "{lo:?}");
        assert!((hi[a] - want_hi[a]).abs() < 1e-6, "{hi:?}");
    }
}

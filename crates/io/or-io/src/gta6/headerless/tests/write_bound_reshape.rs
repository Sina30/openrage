use super::*;

/// The built child's vertex quantum: its box is 2 m either side of the origin.
const STEP: f32 = 2.0 / 32767.0;
/// The BVH's quanta a metre over the box's 4 m.
const PER_METRE: f32 = 65534.0 / 4.0;
const MARGIN: f32 = 0.04;
/// A unit right triangle on z = 0, and a fourth vertex below and before its first edge.
const AT: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.5, -1.0, -0.5],
];
/// The first triangle's leaf, which no rewire of the second reaches.
const FIRST_LEAF: [i16; 6] = [-700, -700, -700, 17000, 17000, 700];
/// A shared edge's byte as built, which the angle does not give.
const BUILT_BYTE: u8 = 9;

fn quantum(x: f32) -> i16 {
    (f64::from(x) / f64::from(STEP)).round() as i16
}

fn decoded(q: i16) -> f32 {
    (f64::from(q) * f64::from(STEP)) as f32
}

fn node(b: &[u8], nodes: usize, i: usize) -> [i16; 6] {
    std::array::from_fn(|k| {
        let at = nodes + 16 * i + 2 * k;
        i16::from_le_bytes([b[at], b[at + 1]])
    })
}

/// A triangle record over `vertices`, its neighbours and its edge bytes.
fn triangle(img: &mut Image, at: usize, vertices: [u16; 3], neighbours: [u16; 3], edges: [u8; 3]) {
    img.put(at, &[edges[0], edges[1], edges[2], 0]);
    for (k, v) in vertices.iter().chain(&neighbours).enumerate() {
        img.u16(at + 4 + 2 * k, *v);
    }
}

/// The built bound with its child over [`AT`]: triangle 0 over vertices 0, 1, 2 and triangle 1
/// over 1, 3, 2, linked across their edge from 1 to 2, under a BVH of a parent over a leaf each.
fn two_triangles() -> (Image, usize, usize) {
    let (mut img, parts) = bound();
    let child = child_of(&img, &parts);
    img.vec3(child + 0x20, [2.0; 3]);
    img.vec3(child + 0x30, [-2.0; 3]);
    img.vec3(child + 0x90, [STEP; 3]);
    img.vec3(child + 0xA0, [0.0; 3]);
    img.put(child + 0x2C, &MARGIN.to_le_bytes());
    img.put(child + 0x14, &3.5f32.to_le_bytes());
    let vertices = img.alloc(6 * AT.len());
    for (i, v) in AT.iter().enumerate() {
        for (a, x) in v.iter().enumerate() {
            img.u16(vertices + 6 * i + 2 * a, quantum(*x) as u16);
        }
    }
    img.ptr(child + 0xB0, vertices);
    img.u32(child + 0xD0, AT.len() as u32);
    let open = 0xFFFF;
    triangle(
        &mut img,
        parts.primitives,
        [0, 1, 2],
        [open, 1, open],
        [0xFF, BUILT_BYTE, 0xFF],
    );
    triangle(
        &mut img,
        parts.primitives + 16,
        [1, 3, 2],
        [open, open, 0],
        [0xFF, 0xFF, BUILT_BYTE],
    );
    let bvh = u64_at(&img.bytes, child + 0x130) as usize & 0x0FFF_FFFF;
    img.vec3(bvh + 0x30, [PER_METRE; 3]);
    let nodes = img.alloc(48);
    img.u16(nodes + 0xC, 3);
    for (k, q) in FIRST_LEAF.iter().enumerate() {
        img.u16(nodes + 16 + 2 * k, *q as u16);
    }
    img.u16(nodes + 16 + 0xE, 1);
    img.u16(nodes + 32 + 0xC, 1);
    img.u16(nodes + 32 + 0xE, 1);
    img.ptr(bvh + 0x50, nodes);
    img.u32(bvh + 0x58, 3);
    (img, parts.primitives, nodes)
}

fn geometry_of(b: &mut Bound) -> &mut BoundGeometry {
    match &mut b.children[0].bound {
        Some(Ok(ChildBound::Geometry(g))) => g,
        other => panic!("a geometry child, not {other:?}"),
    }
}

/// The byte of the edge between triangles `t` and `u` of [`AT`] stored to the quantum: their
/// normals' angle in degrees, 255 a half turn, rounded down.
fn expected_byte(t: [usize; 3], u: [usize; 3]) -> u8 {
    let p = |i: usize| AT[i].map(|x| f64::from(decoded(quantum(x))));
    let normal = |t: [usize; 3]| {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let (x, y): ([f64; 3], [f64; 3]) = (
            std::array::from_fn(|k| b[k] - a[k]),
            std::array::from_fn(|k| c[k] - a[k]),
        );
        let n = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        let l = n.iter().map(|v| v * v).sum::<f64>().sqrt();
        n.map(|v| v / l)
    };
    let (n, m) = (normal(t), normal(u));
    let cos: f64 = (0..3).map(|k| n[k] * m[k]).sum();
    (255.0 * cos.acos().to_degrees() / 180.0).floor() as u8
}

#[test]
fn an_edge_s_byte_is_its_convex_angle_in_steps_of_a_half_turn_over_255() {
    assert_eq!(edge_byte(0.0), 0, "flat");
    assert_eq!(edge_byte(-12.0), 0, "concave");
    assert_eq!(edge_byte(f64::NAN), 0);
    assert_eq!(edge_byte(0.1), 1, "a convex edge holds at least 1");
    assert_eq!(edge_byte(90.0), 127);
    assert_eq!(edge_byte(180.0), 254, "0xFF is an open edge's");
}

#[test]
fn a_rewired_triangle_links_across_the_edge_it_now_shares_and_its_leaf_fits_it() {
    let (img, primitives, nodes) = two_triangles();
    let mut bytes = img.bytes.clone();
    let mut edited = read_bound(&bytes).expect("the bound reads");
    rewire_triangle(geometry_of(&mut edited), 1, [1, 0, 3])
        .expect("a triangle of a geometry with a BVH");
    let byte = expected_byte([0, 1, 2], [1, 0, 3]);
    assert!(
        byte > 0 && byte < 0xFF,
        "the edge from 0 to 1 is convex: {byte}"
    );
    let open = 0xFFFF;
    assert_eq!(
        geometry_of(&mut edited).primitives,
        [
            Primitive::Triangle {
                vertices: [0, 1, 2],
                neighbours: [1, open, open],
                edges: [byte, 0xFF, 0xFF],
            },
            Primitive::Triangle {
                vertices: [1, 0, 3],
                neighbours: [0, open, open],
                edges: [byte, 0xFF, 0xFF],
            },
        ]
    );
    write_bound(&mut bytes, &edited).expect("a rewire writes in place");
    assert_eq!(bytes.len(), img.bytes.len());
    let read = only_geometry(&bytes).expect("it reads back");
    assert_eq!(read.primitives, geometry_of(&mut edited).primitives);
    assert_eq!(
        bytes[primitives + 16..primitives + 20],
        [byte, 0xFF, 0xFF, 0]
    );

    // The second triangle's leaf: its corners grown by the margin, rounded out to whole quanta.
    let corners = [1, 0, 3].map(|i| AT[i].map(|x| decoded(quantum(x))));
    let lo: [f32; 3] =
        std::array::from_fn(|a| corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min) - MARGIN);
    let hi: [f32; 3] =
        std::array::from_fn(|a| corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max) + MARGIN);
    let leaf: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => (lo[k] * PER_METRE).floor() as i16,
        _ => (hi[k - 3] * PER_METRE).ceil() as i16,
    });
    assert_eq!(node(&bytes, nodes, 2), leaf);
    assert_eq!(
        node(&bytes, nodes, 1),
        FIRST_LEAF,
        "the first triangle's leaf is not touched"
    );
    let root: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => leaf[k].min(FIRST_LEAF[k]),
        _ => leaf[k].max(FIRST_LEAF[k]),
    });
    assert_eq!(node(&bytes, nodes, 0), root);

    // Written again unchanged, nothing moves.
    let again = bytes.clone();
    let unchanged = read_bound(&bytes).expect("it reads back");
    write_bound(&mut bytes, &unchanged).expect("nothing edited writes");
    assert_eq!(bytes, again);
}

#[test]
fn a_rewire_that_makes_no_triangle_or_whose_links_do_not_answer_is_refused() {
    let (img, ..) = two_triangles();
    let mut edited = read_bound(&img.bytes).expect("the bound reads");
    let g = geometry_of(&mut edited);
    assert!(rewire_triangle(g, 1, [1, 1, 3]).is_err(), "a vertex twice");
    assert!(
        rewire_triangle(g, 1, [1, 0, 4]).is_err(),
        "a vertex past the four"
    );
    assert!(
        rewire_triangle(g, 2, [1, 0, 3]).is_err(),
        "no third primitive"
    );
    // Vertices 0, 1 and the point halfway between lie on one line.
    g.vertices[3] = [0.5, 0.0, 0.0];
    assert!(rewire_triangle(g, 1, [0, 1, 3]).is_err(), "no triangle");
    g.bvh = None;
    assert!(
        rewire_triangle(g, 1, [1, 0, 2]).is_err(),
        "no BVH: shrunk vertices"
    );

    let mut edited = read_bound(&img.bytes).expect("the bound reads");
    if let Primitive::Triangle { neighbours, .. } = &mut geometry_of(&mut edited).primitives[0] {
        // A link the second triangle does not answer.
        neighbours[0] = 1;
    }
    let mut bytes = img.bytes.clone();
    assert!(matches!(
        write_bound(&mut bytes, &edited),
        Err(HeaderlessError::NotInPlace(_))
    ));
}

#[test]
fn a_child_s_type_and_include_entry_writes_as_two_numbers() {
    let (img, parts) = bound();
    let mut bytes = img.bytes.clone();
    let mut edited = read_bound(&bytes).expect("the bound reads");
    assert_eq!(edited.children[0].flags, Some([0x13, 0]));
    edited.children[0].flags = Some([0x55, 0x6600_0000_0000_0077]);
    write_bound(&mut bytes, &edited).expect("the entry writes in place");
    let read = read_bound(&bytes).expect("it reads back");
    assert_eq!(read.children[0].flags, Some([0x55, 0x6600_0000_0000_0077]));
    let entries = u64_at(&bytes, parts.root + 0x88) as usize & 0x0FFF_FFFF;
    assert_eq!(u64_at(&bytes, entries + 8), 0x6600_0000_0000_0077);
    edited.children[0].flags = None;
    assert!(
        write_bound(&mut bytes, &edited).is_err(),
        "an entry is not taken away"
    );
}

//! A bound's collision reshaped in place: a triangle of a geometry with a BVH wired to other
//! vertices, its links made again with the triangles it shares an edge with, each shared edge's
//! byte given by the angle between the two triangles and its BVH leaf fitted again; and a
//! composite child's type and include entry written as two numbers.

use super::bound::GEOMETRY;
use super::bytes::sections;
use super::write_bound_vertex::item_box;
use super::write_bvh::{tree, union, Span};
use super::write_physics::{put, refuse};
use super::{BoundGeometry, HeaderlessError, Primitive, Result};

/// No neighbour across an edge.
const OPEN: u16 = 0xFFFF;
/// The byte of an edge with no neighbour.
const OPEN_EDGE: u8 = 0xFF;
const NO_BVH: &str = "only a geometry with a BVH is reshaped: one with none carries a shrunk copy of each vertex, and how far a vertex is shrunk is not known";

/// The byte of a convex edge between two triangles whose normals lie `degrees` apart.
// 255 a half turn, rounded down, between 1 and 254; 0 for none.
pub fn edge_byte(degrees: f64) -> u8 {
    if degrees.is_nan() || degrees <= 0.0 {
        return 0;
    }
    (255.0 * degrees / 180.0).floor().clamp(1.0, 254.0) as u8
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| a[k] * b[k]).sum()
}

/// The unit normal of triangle `t` over `at`, and its distance from the origin; `None` where its
/// vertices lie on one line or past `at`.
fn plane(at: &[[f32; 3]], t: [u16; 3]) -> Option<([f64; 3], f64)> {
    let p = |i: u16| at.get(usize::from(i)).map(|v| v.map(f64::from));
    let (a, b, c) = (p(t[0])?, p(t[1])?, p(t[2])?);
    let u: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
    let w: [f64; 3] = std::array::from_fn(|k| c[k] - a[k]);
    let n = [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ];
    let length = dot(n, n).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let n = n.map(|x| x / length);
    Some((n, dot(n, a)))
}

/// The byte of the edge triangle `t` shares with `other`: their normals' angle where the vertex
/// of `other` that `t` lacks lies behind `t`, else 0.
fn shared_byte(at: &[[f32; 3]], t: [u16; 3], other: [u16; 3]) -> u8 {
    let (Some((n, d)), Some((m, _))) = (plane(at, t), plane(at, other)) else {
        return 0;
    };
    let far = (other.iter().find(|v| !t.contains(v))).and_then(|v| at.get(usize::from(*v)));
    let Some(far) = far else {
        return 0;
    };
    if dot(n, far.map(f64::from)) - d >= -1e-6 {
        return 0;
    }
    edge_byte(dot(n, m).clamp(-1.0, 1.0).acos().to_degrees())
}

/// Edge `k` of a triangle: from its vertex `k` to the next.
fn edge(t: [u16; 3], k: usize) -> [u16; 2] {
    [t[k], t[(k + 1) % 3]]
}

fn same_edge(a: [u16; 2], b: [u16; 2]) -> bool {
    a == b || a == [b[1], b[0]]
}

/// Triangle `primitive` of `g` wired to `vertices`, its edges linked again.
// The links of its old edges are cut, each new edge is linked to the first other triangle that has
// that edge open, and the byte of every edge it holds is given by the angle between the two
// triangles, 0xFF where it has no neighbour.
pub fn rewire_triangle(g: &mut BoundGeometry, primitive: usize, vertices: [u16; 3]) -> Result<()> {
    if g.bvh.is_none() {
        return Err(refuse(NO_BVH));
    }
    if let Some(v) = vertices
        .iter()
        .find(|v| usize::from(**v) >= g.vertices.len())
    {
        return Err(HeaderlessError::IndexPastCount {
            what: "triangle's vertex",
            index: u64::from(*v),
            count: g.vertices.len() as u64,
        });
    }
    let [a, b, c] = vertices;
    if a == b || b == c || a == c {
        return Err(refuse("a triangle names three distinct vertices"));
    }
    if plane(&g.vertices, vertices).is_none() {
        return Err(refuse(
            "the three vertices lie on one line, so they make no triangle",
        ));
    }
    let Some(Primitive::Triangle {
        neighbours: old, ..
    }) = g.primitives.get(primitive).copied()
    else {
        return Err(refuse("only a triangle is wired to other vertices"));
    };
    let Ok(named) = u16::try_from(primitive) else {
        return Err(refuse(
            "a triangle past the 65,535th is named by no neighbour",
        ));
    };
    if named == OPEN {
        return Err(refuse(
            "a triangle past the 65,535th is named by no neighbour",
        ));
    }
    for n in old.iter().filter(|n| **n != OPEN) {
        if let Some(Primitive::Triangle {
            neighbours, edges, ..
        }) = g.primitives.get_mut(usize::from(*n))
        {
            for (n, e) in neighbours.iter_mut().zip(edges.iter_mut()) {
                if *n == named {
                    (*n, *e) = (OPEN, OPEN_EDGE);
                }
            }
        }
    }
    let mut links = [OPEN; 3];
    for (k, link) in links.iter_mut().enumerate() {
        let wanted = edge(vertices, k);
        let found = g.primitives.iter().enumerate().find_map(|(i, p)| match p {
            Primitive::Triangle {
                vertices: v,
                neighbours: n,
                ..
            } if i != primitive => (0..3)
                .find(|&j| n[j] == OPEN && same_edge(edge(*v, j), wanted))
                .map(|j| (i, j, *v)),
            _ => None,
        });
        let Some((i, j, other)) = found else {
            continue;
        };
        let Some(to) = u16::try_from(i).ok().filter(|to| *to != OPEN) else {
            continue;
        };
        let byte = shared_byte(&g.vertices, other, vertices);
        if let Some(Primitive::Triangle {
            neighbours, edges, ..
        }) = g.primitives.get_mut(i)
        {
            (neighbours[j], edges[j]) = (named, byte);
        }
        *link = to;
    }
    let edges: [u8; 3] = std::array::from_fn(|k| match g.primitives.get(usize::from(links[k])) {
        Some(Primitive::Triangle { vertices: v, .. }) if links[k] != OPEN => {
            shared_byte(&g.vertices, vertices, *v)
        }
        _ => OPEN_EDGE,
    });
    if let Some(p) = g.primitives.get_mut(primitive) {
        *p = Primitive::Triangle {
            vertices,
            neighbours: links,
            edges,
        };
    }
    Ok(())
}

/// What a rewired triangle's leaf is fitted from: the geometry's primitives, its quantised
/// vertices decoded, its margin and its BVH.
struct Held {
    primitives: usize,
    vertices: Vec<[f32; 3]>,
    margin: f32,
    bvh: Option<usize>,
}

fn held(section: &[u8], at: usize) -> Result<Held> {
    sections(section, Some(&[]), |v, _| {
        let count = v.u32("vertex count", at + 0xD0)? as usize;
        let scale = v.vec3("quantisation scale", at + 0x90)?;
        let centre = v.vec3("quantisation centre", at + 0xA0)?;
        let quantised = v.array("vertices", v.to("vertices", at + 0xB0)?, count, 6)?;
        let vertices = (quantised.as_chunks::<6>().0.iter())
            .map(|q| {
                std::array::from_fn(|a| {
                    let lane = i16::from_le_bytes([q[2 * a], q[2 * a + 1]]);
                    (f64::from(centre[a]) + f64::from(lane) * f64::from(scale[a])) as f32
                })
            })
            .collect();
        // A geometry of the type with no BVH ends before +0x130: the bytes there are another object's.
        let bvh = match v.u8("bound type", at + 0x10)? {
            GEOMETRY => v.ptr("BVH", at + 0x130)?,
            _ => None,
        };
        Ok(Held {
            primitives: v.to("primitives", at + 0x88)?,
            vertices,
            margin: v.f32("margin", at + 0x2C)?,
            bvh,
        })
    })
}

/// The triangles `edited` wires otherwise than `read`, written into the geometry child at `at`:
/// each changed record's edge bytes, vertices and neighbours, every link checked to name its
/// triangle back across an edge both hold, and the BVH leaves that hold a triangle whose
/// vertices changed fitted again.
pub(super) fn write_rewired(
    section: &mut [u8],
    at: usize,
    read: &BoundGeometry,
    edited: &BoundGeometry,
) -> Result<()> {
    if read.primitives == edited.primitives {
        return Ok(());
    }
    if read.primitives.len() != edited.primitives.len() {
        return Err(refuse("it changes the number of a geometry's primitives"));
    }
    let changed: Vec<usize> = (0..read.primitives.len())
        .filter(|&i| read.primitives[i] != edited.primitives[i])
        .collect();
    let mut moved = Vec::new();
    for &i in &changed {
        let (
            Primitive::Triangle { vertices: was, .. },
            Primitive::Triangle {
                vertices,
                neighbours,
                edges,
            },
        ) = (read.primitives[i], edited.primitives[i])
        else {
            return Err(refuse(
                "only a triangle's vertices, neighbours and edge bytes are reshaped",
            ));
        };
        if vertices
            .iter()
            .any(|v| usize::from(*v) >= edited.vertices.len())
        {
            return Err(refuse("a triangle names a vertex past the vertices"));
        }
        for k in (0..3).filter(|&k| neighbours[k] != OPEN) {
            if edges[k] == OPEN_EDGE {
                return Err(refuse("an edge that names a neighbour is marked open"));
            }
            let Some(Primitive::Triangle {
                vertices: w,
                neighbours: m,
                ..
            }) = edited.primitives.get(usize::from(neighbours[k]))
            else {
                return Err(refuse(
                    "a triangle's neighbour is not a triangle of the geometry",
                ));
            };
            let back =
                (0..3).any(|j| usize::from(m[j]) == i && same_edge(edge(*w, j), edge(vertices, k)));
            if !back {
                return Err(refuse(
                    "a triangle's neighbour does not name it back across the edge they share",
                ));
            }
        }
        if was != vertices {
            moved.push(i);
        }
    }
    let h = held(section, at)?;
    if !moved.is_empty() && h.bvh.is_none() {
        return Err(refuse(NO_BVH));
    }
    for &i in &changed {
        let Primitive::Triangle {
            vertices,
            neighbours,
            edges,
        } = edited.primitives[i]
        else {
            continue;
        };
        let mut record = [0u8; 16];
        record[..3].copy_from_slice(&edges);
        for (k, word) in vertices.iter().chain(&neighbours).enumerate() {
            record[4 + 2 * k..6 + 2 * k].copy_from_slice(&word.to_le_bytes());
        }
        put(section, h.primitives + 16 * i, &record)?;
    }
    let Some(bvh) = h.bvh.filter(|_| !moved.is_empty()) else {
        return Ok(());
    };
    let tree = tree(section, bvh)?;
    let at_vertex = |i: u16| h.vertices.get(usize::from(i)).copied();
    let boxes = tree.refit(|first, items| {
        let leaf = first..first + items;
        if !moved.iter().any(|m| leaf.contains(m)) {
            return Ok(None);
        }
        let held = (edited.primitives.get(leaf))
            .ok_or(refuse("a BVH leaf names a primitive past the primitives"))?;
        let mut span: Option<Span> = None;
        for p in held {
            let b = item_box(p, &at_vertex, h.margin)
                .ok_or(refuse("a primitive names a vertex past the vertices"))?;
            span = Some(span.map_or(b, |s| union(s, b)));
        }
        Ok(span)
    })?;
    tree.write(section, &boxes)
}

/// Child `child`'s 16-byte type and include entry of the composite at `at` written as `flags`.
pub(super) fn put_child_flags(
    section: &mut [u8],
    at: usize,
    child: usize,
    flags: Option<[u64; 2]>,
) -> Result<()> {
    let flags = flags.ok_or(refuse("a child's type and include entry is not taken away"))?;
    let entries = sections(section, Some(&[]), |v, _| {
        v.ptr("child type and include entries", at + 0x88)
    })?
    .ok_or(refuse("the composite has no type and include entries"))?;
    put(section, entries + 16 * child, &flags[0].to_le_bytes())?;
    put(section, entries + 16 * child + 8, &flags[1].to_le_bytes())
}

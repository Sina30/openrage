//! A vertex of a bound's geometry child moved in place: the vertex quantised again, the BVH leaves
//! holding a primitive that names it fitted again, every parent the union of its children, and the
//! child's radius raised to reach it. A move past the child's box grows the box, the quantisation
//! and the BVH's frame with it, so every vertex is quantised again and every leaf fitted; the
//! composite's box, radius and BVH follow. A triangle's edge bytes are kept.

use super::bound::{cylinder_box, COMPOSITE, GEOMETRY};
use super::bytes::sections;
use super::write_bvh::{corners, refit_composite, tree, union, Span};
use super::write_physics::{put, put_vec3, refuse};
use super::{read_bound, Bound, BoundGeometry, ChildBound, HeaderlessError, Primitive, Result};

/// GTA V's quantisation scale of a geometry over its box's half width, about 1 / 32767.
const PER_HALF_WIDTH: u32 = 0x3800_0100;

/// The vertices a primitive names.
fn named(p: &Primitive) -> &[u16] {
    match p {
        Primitive::Triangle { vertices, .. } => vertices,
        Primitive::Sphere { centre, .. } => std::slice::from_ref(centre),
        Primitive::Capsule { ends, .. } | Primitive::Cylinder { ends, .. } => ends,
        Primitive::Box(corners) => corners,
    }
}

fn names_any(p: &Primitive, moved: &[usize]) -> bool {
    named(p).iter().any(|v| moved.contains(&usize::from(*v)))
}

/// The box of `points`, each face moved out by `grow`.
fn span(points: &[[f32; 3]], grow: f32) -> Span {
    let lo = std::array::from_fn(|a| points.iter().fold(f32::MAX, |m, p| m.min(p[a])) - grow);
    let hi = std::array::from_fn(|a| points.iter().fold(f32::MIN, |m, p| m.max(p[a])) + grow);
    (lo, hi)
}

/// Primitive `p`'s box over the vertices `at` gives, a triangle grown by `margin`, as its BVH
/// leaf holds it; `None` where it names a vertex `at` does not give.
fn item_box(p: &Primitive, at: &dyn Fn(u16) -> Option<[f32; 3]>, margin: f32) -> Option<Span> {
    Some(match *p {
        Primitive::Triangle { vertices, .. } => {
            let [a, b, c] = vertices;
            span(&[at(a)?, at(b)?, at(c)?], margin)
        }
        Primitive::Sphere { centre, radius } => span(&[at(centre)?], radius),
        Primitive::Capsule { ends, radius } => span(&[at(ends[0])?, at(ends[1])?], radius),
        // Ends on one vertex state no axis: GTA V's box of it is the point.
        Primitive::Cylinder { ends, .. } if ends[0] == ends[1] => span(&[at(ends[0])?], 0.0),
        Primitive::Cylinder { ends, radius } => cylinder_box(at(ends[0])?, at(ends[1])?, radius),
        Primitive::Box(corners) => {
            let [a, b, c, d] = corners;
            let stated = [at(a)?, at(b)?, at(c)?, at(d)?];
            // The other four corners are half the four's sum less each.
            let half: [f64; 3] =
                std::array::from_fn(|k| stated.iter().map(|p| f64::from(p[k])).sum::<f64>() / 2.0);
            let other = |p: &[f32; 3]| -> [f32; 3] {
                std::array::from_fn(|k| (half[k] - f64::from(p[k])) as f32)
            };
            let mut all = stated.to_vec();
            all.extend(stated.iter().map(other));
            span(&all, 0.0)
        }
    })
}

/// Refused where a primitive naming one of `moved` leaves the box `lo` to `hi`.
fn inside(
    primitives: &[Primitive],
    moved: &[usize],
    at: &dyn Fn(u16) -> Option<[f32; 3]>,
    (lo, hi): Span,
) -> Result<()> {
    for p in primitives.iter().filter(|p| names_any(p, moved)) {
        let (min, max) =
            item_box(p, at, 0.0).ok_or(refuse("a primitive names a vertex past the vertices"))?;
        if (0..3).any(|a| min[a] < lo[a] || max[a] > hi[a]) {
            return Err(refuse(
                "a primitive stored to the nearest quantum lies past its child's box",
            ));
        }
    }
    Ok(())
}

/// Whether vertex `vertex` of `g` may move to `position`.
// `g` has a BVH to fit again; a move past the child's box grows the box at the save.
pub fn vertex_move_holds(g: &BoundGeometry, vertex: usize, position: [f32; 3]) -> Result<()> {
    if g.bvh.is_none() {
        return Err(refuse(
            "only a geometry with a BVH has a vertex moved: one with none carries a shrunk copy of each vertex, and how far a vertex is shrunk is not known",
        ));
    }
    if vertex >= g.vertices.len() {
        return Err(HeaderlessError::IndexPastCount {
            what: "vertex to move",
            index: vertex as u64,
            count: g.vertices.len() as u64,
        });
    }
    if position.iter().any(|x| !x.is_finite()) {
        return Err(refuse("a position is three finite numbers"));
    }
    Ok(())
}

/// The vertices `edited` moved, each written by the rules [`vertex_move_holds`] checks.
// Nothing else of `edited` is written.
pub fn write_bound_vertices(virtual_section: &mut [u8], edited: &Bound) -> Result<()> {
    write_bound_vertices_at(virtual_section, 0, edited, &|v| read_bound(v))
}

/// [`write_bound_vertices`] for the bound at `at`, which `reread` reads from the section.
pub(super) fn write_bound_vertices_at(
    virtual_section: &mut [u8],
    at: usize,
    edited: &Bound,
    reread: &dyn Fn(&[u8]) -> Result<Bound>,
) -> Result<()> {
    let read = reread(virtual_section)?;
    if read.children.len() != edited.children.len() {
        return Err(refuse("the bound holds another number of children"));
    }
    let composite = sections(virtual_section, Some(&[]), |v, _| {
        Ok(v.u8("bound type", at + 0x10)? == COMPOSITE)
    })?;
    let records = match composite {
        true => sections(virtual_section, Some(&[]), |v, _| {
            let pointers = v.to("child pointers", at + 0x70)?;
            (0..read.children.len())
                .map(|i| v.ptr("child", pointers + 8 * i))
                .collect::<Result<Vec<_>>>()
        })?,
        // A lone bound is its own child, and its box is its geometry's.
        false => vec![Some(at)],
    };
    let mut wrote = false;
    let mut grown = Vec::new();
    let pairs = read.children.iter().zip(&edited.children).zip(records);
    for (i, ((r, e), at)) in pairs.enumerate() {
        let (Some(Ok(ChildBound::Geometry(rg))), Some(Ok(ChildBound::Geometry(eg))), Some(at)) =
            (&r.bound, &e.bound, at)
        else {
            continue;
        };
        if rg.vertices.len() != eg.vertices.len() {
            return Err(refuse("it changes the number of a geometry's vertices"));
        }
        let moved: Vec<usize> = (0..rg.vertices.len())
            .filter(|&i| rg.vertices[i] != eg.vertices[i])
            .collect();
        if !moved.is_empty() {
            if let Some(span) = move_vertices(virtual_section, at, rg, &moved, &eg.vertices)? {
                grown.push((i, span));
            }
            wrote = true;
        }
    }
    if composite && !grown.is_empty() {
        follow_grown(virtual_section, at, &read, &grown, reread)?;
    }
    if wrote {
        reread(virtual_section)?;
    }
    Ok(())
}

/// The composite at `at`, read as `read`, given the children's boxes `grown`: its box grown to hold
/// each placed by its child's matrix, its radius raised to reach their corners, and its BVH fitted
/// again over them.
fn follow_grown(
    section: &mut [u8],
    at: usize,
    read: &Bound,
    grown: &[(usize, Span)],
    reread: &dyn Fn(&[u8]) -> Result<Bound>,
) -> Result<()> {
    let mut held = (read.box_min, read.box_max);
    let mut radius = read.radius;
    for (i, span) in grown {
        let child =
            (read.children.get(*i)).ok_or(refuse("the bound holds another number of children"))?;
        for p in corners(&child.matrix, *span) {
            held = union(held, (p, p));
            let d: f32 = (0..3).map(|a| (p[a] - read.centroid[a]).powi(2)).sum();
            radius = radius.max(d.sqrt());
        }
    }
    let reframe = held != (read.box_min, read.box_max);
    if reframe {
        put_vec3(section, at + 0x20, held.1)?;
        put_vec3(section, at + 0x30, held.0)?;
    }
    if radius > read.radius {
        put(section, at + 0x14, &radius.to_le_bytes())?;
    }
    let after = reread(section)?;
    refit_composite(section, at, read, &after, reframe)
}

/// What a vertex move reads of the geometry child at `at`.
struct Frame {
    box_min: [f32; 3],
    box_max: [f32; 3],
    radius: f32,
    margin: f32,
    centroid: [f32; 3],
    scale: [f32; 3],
    centre: [f32; 3],
    vertices_at: usize,
    quantised: Vec<[i16; 3]>,
    bvh: usize,
}

fn frame(section: &[u8], at: usize) -> Result<Frame> {
    sections(section, Some(&[]), |v, _| {
        if v.u8("bound type", at + 0x10)? != GEOMETRY {
            return Err(refuse(
                "only a geometry with a BVH has a vertex moved: one with none carries a shrunk copy of each vertex, and how far a vertex is shrunk is not known",
            ));
        }
        let bvh = (v.ptr("BVH", at + 0x130)?).ok_or(refuse("the geometry has no BVH"))?;
        let count = v.u32("vertex count", at + 0xD0)? as usize;
        let vertices_at = v.to("vertices", at + 0xB0)?;
        let quantised = (v
            .array("vertices", vertices_at, count, 6)?
            .as_chunks::<6>()
            .0
            .iter())
        .map(|q| std::array::from_fn(|a| i16::from_le_bytes([q[2 * a], q[2 * a + 1]])))
        .collect();
        Ok(Frame {
            box_min: v.vec3("box minimum", at + 0x30)?,
            box_max: v.vec3("box maximum", at + 0x20)?,
            radius: v.f32("radius", at + 0x14)?,
            margin: v.f32("margin", at + 0x2C)?,
            centroid: v.vec3("centroid", at + 0x40)?,
            scale: v.vec3("quantisation scale", at + 0x90)?,
            centre: v.vec3("quantisation centre", at + 0xA0)?,
            vertices_at,
            quantised,
            bvh,
        })
    })
}

/// A quantised vertex decoded as the engine's f32 multiply-add.
fn decode(q: &[i16; 3], centre: [f32; 3], scale: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|a| (f64::from(centre[a]) + f64::from(q[a]) * f64::from(scale[a])) as f32)
}

/// `box_` grown to hold `held` where it does not: each grown face a quarter margin and two quanta
/// of the grown box past what it holds, as no measured rule says how far a shipped box reaches.
fn grown_box(box_: Span, held: Span, margin: f32) -> Span {
    let pad: [f32; 3] =
        std::array::from_fn(|a| 0.25 * margin + 2.0 * (held.1[a] - held.0[a]) / 65534.0);
    (
        std::array::from_fn(|a| match held.0[a] < box_.0[a] {
            true => held.0[a] - pad[a],
            false => box_.0[a],
        }),
        std::array::from_fn(|a| match held.1[a] > box_.1[a] {
            true => held.1[a] + pad[a],
            false => box_.1[a],
        }),
    )
}

/// The child at `at`, read as `g`, given `positions` for its vertices `moved`; the child's box
/// where the move grew it.
fn move_vertices(
    section: &mut [u8],
    at: usize,
    g: &BoundGeometry,
    moved: &[usize],
    positions: &[[f32; 3]],
) -> Result<Option<Span>> {
    let f = frame(section, at)?;
    let mut tree = tree(section, f.bvh)?;
    let count = f.quantised.len();
    if count != positions.len() || count != g.vertices.len() {
        return Err(refuse("the geometry holds another number of vertices"));
    }
    let mut is_moved = vec![false; count];
    for &i in moved {
        if let Some(m) = is_moved.get_mut(i) {
            *m = true;
        }
    }
    let given: Vec<[f32; 3]> = (0..count)
        .map(|i| match is_moved[i] {
            true => positions[i],
            false => decode(&f.quantised[i], f.centre, f.scale),
        })
        .collect();
    let given_at = |i: u16| given.get(usize::from(i)).copied();

    // The box holds every moved vertex and every primitive that names one.
    let read_box = (f.box_min, f.box_max);
    let mut held = read_box;
    for &i in moved {
        let p = *given
            .get(i)
            .ok_or(refuse("a vertex past the vertices moved"))?;
        held = union(held, (p, p));
    }
    for p in g.primitives.iter().filter(|p| names_any(p, moved)) {
        let b = item_box(p, &given_at, 0.0)
            .ok_or(refuse("a primitive names a vertex past the vertices"))?;
        held = union(held, b);
    }
    let grows = held != read_box;
    let (box_, centre, scale) = match grows {
        false => (read_box, f.centre, f.scale),
        true => {
            let (lo, hi) = grown_box(read_box, held, f.margin);
            let centre: [f32; 3] = std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0);
            let per = f32::from_bits(PER_HALF_WIDTH);
            let scale: [f32; 3] = std::array::from_fn(|a| (hi[a] - lo[a]) / 2.0 * per);
            let fits = centre.iter().all(|c| c.is_finite())
                && scale.iter().all(|s| s.is_finite() && *s > 0.0);
            if !fits {
                return Err(refuse(
                    "the grown box has no extent on an axis, so it has no quantisation",
                ));
            }
            ((lo, hi), centre, scale)
        }
    };

    let mut quantised = f.quantised.clone();
    for i in (0..count).filter(|&i| grows || is_moved[i]) {
        let p = given[i];
        let lanes: [f64; 3] = std::array::from_fn(|a| {
            ((f64::from(p[a]) - f64::from(centre[a])) / f64::from(scale[a])).round()
        });
        if lanes.iter().any(|l| !(-32767.0..=32767.0).contains(l)) {
            return Err(refuse(
                "a vertex lies past the quantisation of its child's box",
            ));
        }
        quantised[i] = lanes.map(|l| l as i16);
    }
    let decoded: Vec<[f32; 3]> = (quantised.iter())
        .map(|q| decode(q, centre, scale))
        .collect();
    let at_vertex = |i: u16| decoded.get(usize::from(i)).copied();
    inside(&g.primitives, moved, &at_vertex, box_)?;

    if grows {
        tree.reframe(box_)?;
    }
    let boxes = tree.refit(|first, items| {
        let held = (g.primitives.get(first..first + items))
            .ok_or(refuse("a BVH leaf names a primitive past the primitives"))?;
        if !grows && !held.iter().any(|p| names_any(p, moved)) {
            return Ok(None);
        }
        let mut span: Option<Span> = None;
        for p in held {
            let b = item_box(p, &at_vertex, f.margin)
                .ok_or(refuse("a primitive names a vertex past the vertices"))?;
            span = Some(span.map_or(b, |s| union(s, b)));
        }
        Ok(span)
    })?;

    for (i, (now, was)) in quantised.iter().zip(&f.quantised).enumerate() {
        if now != was {
            let bytes: Vec<u8> = now.iter().flat_map(|q| q.to_le_bytes()).collect();
            put(section, f.vertices_at + 6 * i, &bytes)?;
        }
    }
    tree.write(section, &boxes)?;
    if grows {
        // The vectors' fourth lanes are the margin and words not known: only x, y, z move.
        put_vec3(section, at + 0x20, box_.1)?;
        put_vec3(section, at + 0x30, box_.0)?;
        put_vec3(section, at + 0x90, scale)?;
        put_vec3(section, at + 0xA0, centre)?;
    }
    // The radius reaches every vertex stored again from the centroid with the margin to spare.
    let reach = (0..count)
        .filter(|&i| grows || is_moved[i])
        .map(|i| {
            let d: f32 = (0..3)
                .map(|a| (decoded[i][a] - f.centroid[a]).powi(2))
                .sum();
            d.sqrt() + f.margin
        })
        .fold(f.radius, f32::max);
    if reach > f.radius {
        put(section, at + 0x14, &reach.to_le_bytes())?;
    }
    Ok(grows.then_some(box_))
}

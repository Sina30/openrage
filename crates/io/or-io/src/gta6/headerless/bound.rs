//! The composite bound, its geometry children and the primitive bounds a composite may hold.

use super::bytes::{f32_at, sections, u32_at, u64_at, Bytes};
use super::{HeaderlessError, Result};

/// The bound type byte a composite carries.
pub(super) const COMPOSITE: u8 = 6;
/// The bound type byte a composite's geometry child carries.
pub(super) const GEOMETRY: u8 = 5;
pub(super) const BOUND_CHILD: usize = 0x150;
/// The bound type byte of a geometry with no BVH, whose record ends at +0x130: the bytes there are
/// the next object's.
pub(super) const PLAIN_GEOMETRY: u8 = 4;
const PLAIN_GEOMETRY_RECORD: usize = 0x130;
/// The type bytes of the primitive bounds: 0, 1 and 3 keep GTA V's codes and sizes, the rest do not.
const SPHERE: u8 = 0;
const CAPSULE: u8 = 1;
const TAPERED_CAPSULE: u8 = 2;
const BOX: u8 = 3;
const DISC: u8 = 8;
const CYLINDER: u8 = 9;
const HULL: u8 = 13;
const FRUSTUM: u8 = 14;

/// A composite of bounds, each placed by its own matrix.
#[derive(Debug)]
pub struct Bound {
    pub radius: f32,
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub centroid: [f32; 3],
    pub children: Vec<BoundChild>,
    /// A BVH over the children, whose leaves name each child once.
    pub bvh: Option<Bvh>,
}

#[derive(Debug)]
pub struct BoundChild {
    /// Four rows of four floats; each of the first three rows was measured at unit length.
    pub matrix: [[f32; 4]; 4],
    /// The child's 16-byte type and include entry, whose bits are not known; `None` where the
    /// composite has no entries.
    pub flags: Option<[u64; 2]>,
    /// `None` for a null pointer; a child of a type with no measured layout is refused by name.
    pub bound: Option<Result<ChildBound>>,
    /// A primitive child's material id, its +0x4C word low and its +0x5C word high; `None` for a
    /// geometry, whose materials are its primitives', or a child not read.
    pub material_id: Option<u64>,
}

/// The material key of a material id: its 15 low bits, one key a material in every body.
pub fn material_key(id: u64) -> u16 {
    (id & 0x7FFF) as u16
}

/// A composite's child, by its type byte.
#[derive(Debug)]
pub enum ChildBound {
    Geometry(BoundGeometry),
    /// Type 0: its box is a cube, and its margin and radius are half its edge.
    Sphere {
        centre: [f32; 3],
        radius: f32,
    },
    /// Type 1: the capsule inside the box, its radius half the box's smallest extent.
    Capsule {
        box_min: [f32; 3],
        box_max: [f32; 3],
        radius: f32,
    },
    /// Type 3.
    Box {
        box_min: [f32; 3],
        box_max: [f32; 3],
    },
    /// Type 2: spheres of radii `radii` whose centres lie `length` apart along y, and the cone
    /// between them; which radius lies at +y is not known.
    TaperedCapsule {
        centre: [f32; 3],
        radii: [f32; 2],
        length: f32,
    },
    /// Type 8: a disc in the yz plane, `margin` thick either side of it.
    Disc {
        centre: [f32; 3],
        radius: f32,
        margin: f32,
    },
    /// Type 9: along y.
    Cylinder {
        centre: [f32; 3],
        radius: f32,
        half_height: f32,
    },
    /// Type 13: the hull of eight corners about the centre, grown by `margin`.
    Hull {
        centre: [f32; 3],
        corners: [[f32; 3]; 8],
        margin: f32,
    },
    /// Type 14: end radii `radii`, each grown by `margin` times its factor, `height` apart along y;
    /// which end lies at +y, and what the factors are, is not known.
    Frustum {
        centre: [f32; 3],
        radii: [f32; 2],
        factors: [f32; 2],
        height: f32,
        margin: f32,
    },
}

impl ChildBound {
    /// The half extents about its centre and the radius around it a primitive's fields give.
    pub fn extent(&self) -> Option<([f32; 3], f32)> {
        let length = |v: [f32; 3]| v.iter().map(|a| a * a).sum::<f32>().sqrt();
        Some(match *self {
            Self::Geometry(_) => return None,
            Self::Sphere { radius, .. } => ([radius; 3], radius),
            Self::Capsule {
                box_min,
                box_max,
                radius,
            } => {
                let half_y = (box_max[1] - box_min[1]) / 2.0;
                ([radius, half_y, radius], half_y)
            }
            Self::Box { box_min, box_max } => {
                let half = std::array::from_fn(|a| (box_max[a] - box_min[a]) / 2.0);
                (half, length(half))
            }
            Self::TaperedCapsule { radii, length, .. } => {
                let r = radii[0].max(radii[1]);
                ([r, length / 2.0 + r, r], length / 2.0 + r)
            }
            Self::Disc { radius, margin, .. } => ([margin, radius, radius], radius),
            Self::Cylinder {
                radius,
                half_height,
                ..
            } => (
                [radius, half_height, radius],
                length([radius, half_height, 0.0]),
            ),
            Self::Hull {
                corners, margin, ..
            } => {
                let half = std::array::from_fn(|a| {
                    corners.iter().fold(0f32, |m, c| m.max(c[a].abs())) + margin
                });
                let far = corners.iter().map(|c| length(*c)).fold(0f32, f32::max);
                (half, far + margin)
            }
            Self::Frustum {
                radii,
                factors,
                height,
                margin,
                ..
            } => {
                let r = (radii[0] + margin * factors[0]).max(radii[1] + margin * factors[1]);
                ([r, height / 2.0, r], length([r, height, 0.0]))
            }
        })
    }
}

#[derive(Debug)]
pub struct BoundGeometry {
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub primitives: Vec<Primitive>,
    /// Eight bytes a material, of a material table that is not in the package.
    pub material_ids: Vec<u64>,
    /// One per primitive, each below the material count; empty where the list is null.
    pub primitive_materials: Vec<u8>,
    /// `None` for a geometry of the type with no BVH.
    pub bvh: Option<Bvh>,
    /// The primitives whose fields read but whose shape is not known, by index, ascending.
    pub unknown_shapes: Vec<(usize, UnknownShape)>,
}

impl BoundGeometry {
    /// The material key of primitive `index`; `None` where the geometry names no material for it.
    pub fn primitive_key(&self, index: usize) -> Option<u16> {
        let m = self.primitive_materials.get(index)?;
        self.material_ids
            .get(usize::from(*m))
            .copied()
            .map(material_key)
    }
}

/// Why a primitive whose fields read has no known shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownShape {
    /// A cylinder whose two ends are one vertex: it states no axis, and its BVH box is the point.
    EndsOnOneVertex,
    /// A cylinder whose box from its ends and radius is not its BVH leaf's; its axis is its ends'.
    CylinderOffItsLeaf,
}

/// A bound primitive; every index names a vertex of its geometry, and no margin grows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    /// `neighbours` name the primitives across each edge, 0xFFFF for none.
    Triangle {
        vertices: [u16; 3],
        neighbours: [u16; 3],
        /// Bytes 0 to 2, one an edge: 0xFF only where it has no neighbour, other values not known.
        edges: [u8; 3],
    },
    Sphere {
        centre: u16,
        radius: f32,
    },
    /// Hemispheres of `radius` at both ends, and the tube between them.
    Capsule {
        ends: [u16; 2],
        radius: f32,
    },
    /// Alternate corners of the box: the other four are half their sum less each.
    Box([u16; 4]),
    /// Flat caps at both ends, `radius` about the axis between them.
    Cylinder {
        ends: [u16; 2],
        radius: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bvh {
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub nodes: u32,
    /// Nodes with an item count; together they cover every item once.
    pub leaves: u32,
}

pub fn read_bound(virtual_section: &[u8]) -> Result<Bound> {
    sections(virtual_section, Some(&[]), |v, _| bound_at(v, 0))
}

pub(super) fn bound_at(v: Bytes, at: usize) -> Result<Bound> {
    let found = v.u8("bound type", at + 0x10)?;
    if found != COMPOSITE {
        return Err(HeaderlessError::BoundType {
            at: at as u64,
            found,
            expected: COMPOSITE,
        });
    }
    let count = v.u16("child count", at + 0x98)?;
    let again = v.u16("child count", at + 0x9A)?;
    if count != again {
        return Err(HeaderlessError::Disagree {
            what: "composite's two child counts",
            first: count.into(),
            second: again.into(),
        });
    }
    let count = usize::from(count);
    let pointers = v.to("child pointers", at + 0x70)?;
    let matrices = v.to("child matrices", at + 0x78)?;
    v.array("child pointers", pointers, count, 8)?;
    let matrix_bytes = v.array("child matrices", matrices, count, 64)?;
    let flags = match v.ptr("child type and include entries", at + 0x88)? {
        Some(flags) => Some(v.array("child type and include entries", flags, count, 16)?),
        None => None,
    };
    let mut children = Vec::with_capacity(count);
    for (i, m) in matrix_bytes.as_chunks::<64>().0.iter().enumerate() {
        let row = |r: usize| -> [f32; 4] { std::array::from_fn(|c| f32_at(m, 16 * r + 4 * c)) };
        let read = v
            .ptr("child", pointers + 8 * i)?
            .map(|child| child_bound(v, child));
        children.push(BoundChild {
            matrix: std::array::from_fn(row),
            flags: flags.map(|f| [u64_at(f, 16 * i), u64_at(f, 16 * i + 8)]),
            material_id: read.as_ref().and_then(|r| r.as_ref().ok()?.1),
            bound: read.map(|r| r.map(|(shape, _)| shape)),
        });
    }
    let bvh = match v.ptr("composite BVH", at + 0xA0)? {
        Some(h) => Some(bvh(v, h, count)?),
        None => None,
    };
    Ok(Bound {
        radius: v.f32("radius", at + 0x14)?,
        box_max: v.vec3("box maximum", at + 0x20)?,
        box_min: v.vec3("box minimum", at + 0x30)?,
        centroid: v.vec3("centroid", at + 0x40)?,
        children,
        bvh,
    })
}

/// The bound a drawable's 0xC8 points at: a composite, or a lone bound of another measured type
/// read as a composite of it alone, placed where it is.
pub(super) fn drawable_bound_at(v: Bytes, at: usize) -> Result<Bound> {
    if v.u8("bound type", at + 0x10)? == COMPOSITE {
        return bound_at(v, at);
    }
    let (shape, material_id) = child_bound(v, at)?;
    Ok(Bound {
        radius: v.f32("radius", at + 0x14)?,
        box_max: v.vec3("box maximum", at + 0x20)?,
        box_min: v.vec3("box minimum", at + 0x30)?,
        centroid: v.vec3("centroid", at + 0x40)?,
        children: vec![BoundChild {
            matrix: std::array::from_fn(|r| std::array::from_fn(|c| f32::from(u8::from(r == c)))),
            flags: None,
            bound: Some(Ok(shape)),
            material_id,
        }],
        bvh: None,
    })
}

/// A child, and a primitive child's material id.
pub(super) fn child_bound(v: Bytes, at: usize) -> Result<(ChildBound, Option<u64>)> {
    let kind = v.u8("bound type", at + 0x10)?;
    // The record's size, and where the zero bytes after its fields start.
    let (size, fields_end) = match kind {
        GEOMETRY | PLAIN_GEOMETRY => {
            return geometry_bound(v, at, kind).map(|g| (ChildBound::Geometry(g), None))
        }
        SPHERE | BOX => (0x70, 0x70),
        CAPSULE | DISC | CYLINDER => (0x80, 0x70),
        TAPERED_CAPSULE => (0x90, 0x7C),
        FRUSTUM => (0x90, 0x84),
        HULL => (0x100, 0x100),
        found => {
            return Err(HeaderlessError::BoundNotLaidOut {
                at: at as u64,
                found,
            })
        }
    };
    let r = v.get("primitive bound", at, size)?;
    let f = |o: usize| f32_at(r, o);
    let vec3 = |o: usize| -> [f32; 3] { std::array::from_fn(|a| f(o + 4 * a)) };
    let (radius, margin) = (f(0x14), f(0x2C));
    let (box_max, box_min, centre) = (vec3(0x20), vec3(0x30), vec3(0x40));
    let half: [f32; 3] = std::array::from_fn(|a| (box_max[a] - box_min[a]) / 2.0);
    let shape = match kind {
        SPHERE => ChildBound::Sphere { centre, radius },
        CAPSULE => ChildBound::Capsule {
            box_min,
            box_max,
            radius: margin,
        },
        BOX => ChildBound::Box { box_min, box_max },
        TAPERED_CAPSULE => ChildBound::TaperedCapsule {
            centre,
            radii: [f(0x70), f(0x74)],
            length: f(0x78),
        },
        DISC => ChildBound::Disc {
            centre,
            radius: half[1],
            margin,
        },
        CYLINDER => ChildBound::Cylinder {
            centre,
            radius: half[0],
            half_height: half[1],
        },
        HULL => ChildBound::Hull {
            centre,
            corners: std::array::from_fn(|i| vec3(0x70 + 16 * i)),
            margin,
        },
        _ => ChildBound::Frustum {
            centre,
            radii: [f(0x70), f(0x74)],
            factors: [f(0x7C), f(0x80)],
            height: f(0x78),
            margin,
        },
    };
    if let Some(i) = r[fields_end..].iter().position(|b| *b != 0) {
        return Err(HeaderlessError::Constant {
            what: "primitive bound's bytes after its fields",
            at: (at + fields_end + i) as u64,
            found: r[fields_end + i].into(),
        });
    }
    // Every measured primitive reproduces its box and radius within this.
    let tolerance = 1e-4 + 2e-3 * half.iter().fold(0f32, |m, h| m.max(*h));
    let near = |a: f32, b: f32| (a - b).abs() <= tolerance;
    let (want_half, want_radius) = shape.extent().unwrap_or_default();
    let stated_margin = match shape {
        ChildBound::Sphere { radius, .. } => near(margin, radius),
        ChildBound::TaperedCapsule { radii, .. } => near(margin, radii[0].min(radii[1])),
        ChildBound::Frustum { factors, .. } => (factors[0] * factors[1] - 1.0).abs() <= 1e-3,
        _ => true,
    };
    let holds = (0..3)
        .all(|a| near(want_half[a], half[a]) && near(centre[a], (box_max[a] + box_min[a]) / 2.0))
        && near(want_radius, radius)
        && stated_margin;
    if !holds {
        return Err(HeaderlessError::PrimitiveBox {
            at: at as u64,
            found: kind,
        });
    }
    let material = u64::from(u32_at(r, 0x4C)) | u64::from(u32_at(r, 0x5C)) << 32;
    Ok((shape, Some(unset_bit_15(material, at + 0x4C)?)))
}

/// `id`, refused where bit 15, which no measured material id sets, is set.
fn unset_bit_15(id: u64, at: usize) -> Result<u64> {
    match id & 0x8000 {
        0 => Ok(id),
        _ => Err(HeaderlessError::MaterialBit15 { at: at as u64, id }),
    }
}

fn geometry_bound(v: Bytes, at: usize, kind: u8) -> Result<BoundGeometry> {
    let record = match kind {
        GEOMETRY => BOUND_CHILD,
        _ => PLAIN_GEOMETRY_RECORD,
    };
    v.get("bound child", at, record)?;
    let vertex_count = v.u32("vertex count", at + 0xD0)? as usize;
    let hull = v.u16("convex hull vertex count", at + 0x84)?;
    if kind == PLAIN_GEOMETRY && usize::from(hull) != vertex_count {
        return Err(HeaderlessError::Disagree {
            what: "geometry's hull vertex count and its vertex count",
            first: hull.into(),
            second: vertex_count as u64,
        });
    }
    let primitive_count = v.u32("primitive count", at + 0xD4)? as usize;
    let scale = v.vec3("quantisation scale", at + 0x90)?;
    let centre = v.vec3("quantisation centre", at + 0xA0)?;
    let vertex_bytes = v.array("vertices", v.to("vertices", at + 0xB0)?, vertex_count, 6)?;
    let vertices: Vec<[f32; 3]> = vertex_bytes
        .as_chunks::<6>()
        .0
        .iter()
        .map(|q| {
            std::array::from_fn(|a| {
                centre[a] + f32::from(i16::from_le_bytes([q[2 * a], q[2 * a + 1]])) * scale[a]
            })
        })
        .collect();
    let primitives_at = v.to("primitives", at + 0x88)?;
    let primitive_bytes = v.array("primitives", primitives_at, primitive_count, 16)?;
    let primitives: Vec<Primitive> = primitive_bytes
        .as_chunks::<16>()
        .0
        .iter()
        .enumerate()
        .map(|(i, r)| primitive(r, primitives_at + 16 * i, vertex_count, primitive_count))
        .collect::<Result<_>>()?;
    let not_triangle = (primitives.iter()).position(|p| !matches!(p, Primitive::Triangle { .. }));
    if let (PLAIN_GEOMETRY, Some(i)) = (kind, not_triangle) {
        return Err(HeaderlessError::NotATriangle {
            at: (primitives_at + 16 * i) as u64,
            found: primitive_bytes[16 * i + 3],
        });
    }
    let materials = usize::from(v.u8("material count", at + 0x120)?);
    let material_ids = match v.ptr("material ids", at + 0xF0)? {
        Some(ids) => (v
            .array("material ids", ids, materials, 8)?
            .as_chunks::<8>()
            .0)
            .iter()
            .enumerate()
            .map(|(i, id)| unset_bit_15(u64::from_le_bytes(*id), ids + 8 * i))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    let primitive_materials = match v.ptr("per-primitive material indices", at + 0x118)? {
        Some(list) => {
            let list = v.array("per-primitive material indices", list, primitive_count, 1)?;
            if let Some(i) = list.iter().find(|i| usize::from(**i) >= materials.max(1)) {
                return Err(HeaderlessError::IndexPastCount {
                    what: "primitive's material index",
                    index: (*i).into(),
                    count: materials as u64,
                });
            }
            list.to_vec()
        }
        None => Vec::new(),
    };
    // A capsule whose ends are one vertex is the sphere of its radius there.
    let mut unknown_shapes: Vec<(usize, UnknownShape)> = (primitives.iter().enumerate())
        .filter(|(_, p)| matches!(p, Primitive::Cylinder { ends, .. } if ends[0] == ends[1]))
        .map(|(i, _)| (i, UnknownShape::EndsOnOneVertex))
        .collect();
    let tree = match kind {
        GEOMETRY => v.ptr("BVH", at + 0x130)?,
        _ => None,
    };
    let bvh = match tree {
        Some(h) => {
            let read = bvh(v, h, primitive_count)?;
            let margin = v.f32("margin", at + 0x2C)?;
            let quantised = (vertex_bytes, scale, centre);
            for i in cylinders_off_their_leaf(v, h, margin, quantised, &primitives)? {
                unknown_shapes.push((i, UnknownShape::CylinderOffItsLeaf));
            }
            unknown_shapes.sort_unstable_by_key(|(i, _)| *i);
            Some(read)
        }
        None => None,
    };
    Ok(BoundGeometry {
        box_max: v.vec3("box maximum", at + 0x20)?,
        box_min: v.vec3("box minimum", at + 0x30)?,
        vertices,
        primitives,
        material_ids,
        primitive_materials,
        bvh,
        unknown_shapes,
    })
}

/// The cylinders of a geometry whose box, from its fields, misses a face of the box of the BVH
/// leaf at `at` that holds it by more than 0.2 margins and two quanta: past the face, or, where the
/// leaf holds the cylinder alone, short of it. A cylinder whose ends are one vertex is not judged.
///
/// ⚠ The box is GTA V's, every step in f32 ([`cylinder_box`]): in f64 a nearly axis-aligned
/// cylinder of a large radius is short of its leaf by several quanta.
fn cylinders_off_their_leaf(
    v: Bytes,
    at: usize,
    margin: f32,
    (vertices, scale, origin): (&[u8], [f32; 3], [f32; 3]),
    primitives: &[Primitive],
) -> Result<Vec<usize>> {
    let decode = |q: [i16; 3], origin: [f32; 3], scale: [f32; 3]| -> [f64; 3] {
        std::array::from_fn(|a| f64::from(origin[a]) + f64::from(q[a]) * f64::from(scale[a]))
    };
    let word = |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
    let vertex = |i: u16| {
        let q = vertices.get(6 * usize::from(i)..6 * usize::from(i) + 6)?;
        Some(decode([0, 2, 4].map(|o| word(q, o)), origin, scale).map(|x| x as f32))
    };
    let centre = v.vec3("BVH centre", at + 0x20)?;
    let quantum = v.vec3("BVH quantum", at + 0x40)?;
    let count = v.u32("BVH node count", at + 0x58)? as usize;
    // The nodes `bvh` read and spent from the budget, read again without spending it.
    let nodes = v.get("BVH nodes", v.to("BVH nodes", at + 0x50)?, 16 * count)?;
    let mut off = Vec::new();
    for node in nodes.as_chunks::<16>().0 {
        let corner = |o: usize| decode([0, 2, 4].map(|a| word(node, o + a)), centre, quantum);
        let (low, high) = (corner(0), corner(6));
        let (first, items) = (word(node, 0xC) as u16, word(node, 0xE) as u16);
        for i in usize::from(first)..usize::from(first) + usize::from(items) {
            let Some(&Primitive::Cylinder { ends, radius }) = primitives.get(i) else {
                continue;
            };
            let (Some(p), Some(q)) = (vertex(ends[0]), vertex(ends[1])) else {
                continue;
            };
            if ends[0] == ends[1] {
                continue;
            }
            let (lo, hi) = cylinder_box(p, q, radius);
            let misses = (0..3).any(|a| {
                let (lo, hi) = (f64::from(lo[a]), f64::from(hi[a]));
                let slack = 0.2 * f64::from(margin) + 2.0 * f64::from(quantum[a]).abs() + 1e-6;
                lo < low[a] - slack
                    || hi > high[a] + slack
                    || (items == 1 && (lo > low[a] + slack || hi < high[a] - slack))
            });
            if misses {
                off.push(i);
            }
        }
    }
    Ok(off)
}

/// A cylinder's box from its ends and radius as GTA V's BVH boxes it, every step in f32: each
/// end grown by the radius times the sine of the axis to the lane; a zero axis grows nothing.
pub(super) fn cylinder_box(p: [f32; 3], q: [f32; 3], radius: f32) -> ([f32; 3], [f32; 3]) {
    let d: [f32; 3] = std::array::from_fn(|a| q[a] - p[a]);
    let n2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let grow: [f32; 3] = if n2 == 0.0 {
        [0.0; 3]
    } else {
        let inv = 1.0 / n2.sqrt();
        std::array::from_fn(|a| {
            let u = d[a] * inv;
            (1.0 - u * u).max(0.0).sqrt() * radius
        })
    };
    (
        std::array::from_fn(|a| p[a].min(q[a]) - grow[a]),
        std::array::from_fn(|a| p[a].max(q[a]) + grow[a]),
    )
}

/// A 16-byte primitive, whose type is its byte 3 (GTA V's low bits of byte 0 are not it).
fn primitive(r: &[u8; 16], at: usize, vertices: usize, primitives: usize) -> Result<Primitive> {
    let u = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
    if r[3] != 0 && r[2] != 0 {
        return Err(HeaderlessError::Constant {
            what: "byte 2 of a primitive other than a triangle",
            at: at as u64 + 2,
            found: r[2].into(),
        });
    }
    let radius = || match f32_at(r, 4) {
        radius if radius.is_finite() && radius >= 0.0 => Ok(radius),
        radius => Err(HeaderlessError::Radius {
            at: at as u64,
            radius,
        }),
    };
    let primitive = match r[3] {
        0 => Primitive::Triangle {
            vertices: [u(4), u(6), u(8)],
            neighbours: [u(0xA), u(0xC), u(0xE)],
            edges: [r[0], r[1], r[2]],
        },
        1 => Primitive::Sphere {
            centre: u(0),
            radius: radius()?,
        },
        2 => Primitive::Capsule {
            ends: [u(0), u(8)],
            radius: radius()?,
        },
        // Bit 15 of a box index was masked where its indices were measured in range.
        3 => Primitive::Box([u(4), u(6), u(8), u(0xA)].map(|i| i & 0x7FFF)),
        4 => Primitive::Cylinder {
            ends: [u(0), u(8)],
            radius: radius()?,
        },
        found => {
            return Err(HeaderlessError::PrimitiveType {
                at: at as u64,
                found,
            })
        }
    };
    let below = |what, index: u16, count: usize| {
        if usize::from(index) < count {
            Ok(())
        } else {
            Err(HeaderlessError::IndexPastCount {
                what,
                index: index.into(),
                count: count as u64,
            })
        }
    };
    let indices: &[u16] = match &primitive {
        Primitive::Triangle {
            vertices,
            neighbours,
            edges,
        } => {
            for (edge, n) in neighbours.iter().enumerate() {
                if *n == 0xFFFF {
                    continue;
                }
                below("triangle's neighbour", *n, primitives)?;
                if edges[edge] == 0xFF {
                    return Err(HeaderlessError::OpenEdgeLinked {
                        at: at as u64,
                        edge,
                    });
                }
            }
            vertices
        }
        Primitive::Sphere { centre, .. } => std::slice::from_ref(centre),
        Primitive::Capsule { ends, .. } | Primitive::Cylinder { ends, .. } => ends,
        Primitive::Box(i) => i,
    };
    for i in indices {
        below("primitive's vertex index", *i, vertices)?;
    }
    Ok(primitive)
}

/// The 0x70-byte BVH record at `at`, whose leaves cover `items` once.
fn bvh(v: Bytes, at: usize, items: usize) -> Result<Bvh> {
    let nodes = v.u32("BVH node count", at + 0x58)?;
    let node_bytes = v.array(
        "BVH nodes",
        v.to("BVH nodes", at + 0x50)?,
        nodes as usize,
        16,
    )?;
    let (mut leaves, mut covered) = (0u32, 0u64);
    for node in node_bytes.as_chunks::<16>().0 {
        let first = u64::from(u16::from_le_bytes([node[0xC], node[0xD]]));
        let count = u64::from(u16::from_le_bytes([node[0xE], node[0xF]]));
        if count == 0 {
            continue;
        }
        if first + count > items as u64 {
            return Err(HeaderlessError::IndexPastCount {
                what: "BVH leaf's end",
                index: first + count,
                count: items as u64,
            });
        }
        leaves += 1;
        covered += count;
    }
    if covered != items as u64 {
        return Err(HeaderlessError::BvhCover {
            covered,
            primitives: items as u64,
        });
    }
    Ok(Bvh {
        box_min: v.vec3("BVH box minimum", at)?,
        box_max: v.vec3("BVH box maximum", at + 0x10)?,
        nodes,
        leaves,
    })
}

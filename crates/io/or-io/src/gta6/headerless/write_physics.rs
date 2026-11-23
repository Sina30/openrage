//! Edits of a bound, a fragment's physics level and a navigation cell written back into a
//! resource's sections where they move nothing: the body is read before and after, and an edit
//! the reader would not read back is refused.

use super::bound::COMPOSITE;
use super::bytes::{sections, u64_at, Bytes};
use super::write_bound_reshape::{put_child_flags, write_rewired};
use super::write_bound_vertex::write_bound_vertices_at;
use super::write_bvh::refit_composite;
use super::{
    read_bound, read_drawable, read_drawable_with_bound, read_navmesh, Bound, BoundChild,
    BoundGeometry, ChildBound, Fragment, HeaderlessError, Result,
};

/// A polygon record's bytes.
const POLYGON: usize = 44;
/// A fragment child record's bytes.
const CHILD: usize = 0x50;

pub(super) fn refuse(why: &'static str) -> HeaderlessError {
    HeaderlessError::NotInPlace(why)
}

/// `bytes` over `section` at `at`.
pub(super) fn put(section: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let len = section.len() as u64;
    let to = (at.checked_add(bytes.len()))
        .and_then(|end| section.get_mut(at..end))
        .ok_or(HeaderlessError::PastEnd {
            what: "edited value",
            at: at as u64,
            section: "virtual",
            len,
        })?;
    to.copy_from_slice(bytes);
    Ok(())
}

fn put_f32(section: &mut [u8], at: usize, value: f32) -> Result<()> {
    put(section, at, &value.to_le_bytes())
}

pub(super) fn put_vec3(section: &mut [u8], at: usize, value: [f32; 3]) -> Result<()> {
    for (a, x) in value.iter().enumerate() {
        put_f32(section, at + 4 * a, *x)?;
    }
    Ok(())
}

/// The inverse a record keeps beside a mass or an inertia: zero for zero.
fn inverse(x: f32) -> f32 {
    if x == 0.0 {
        0.0
    } else {
        1.0 / x
    }
}

/// A primitive child's type byte and the numbers its shape holds; `None` for a geometry.
fn numbers(shape: &ChildBound) -> Option<(u8, Vec<f32>)> {
    let mut n = Vec::new();
    let kind = match shape {
        ChildBound::Geometry(_) => return None,
        ChildBound::Sphere { centre, radius } => {
            n.extend(centre);
            n.push(*radius);
            0
        }
        ChildBound::Capsule {
            box_min,
            box_max,
            radius,
        } => {
            n.extend(box_min);
            n.extend(box_max);
            n.push(*radius);
            1
        }
        ChildBound::TaperedCapsule {
            centre,
            radii,
            length,
        } => {
            n.extend(centre);
            n.extend(radii);
            n.push(*length);
            2
        }
        ChildBound::Box { box_min, box_max } => {
            n.extend(box_min);
            n.extend(box_max);
            3
        }
        ChildBound::Disc {
            centre,
            radius,
            margin,
        } => {
            n.extend(centre);
            n.extend([*radius, *margin]);
            8
        }
        ChildBound::Cylinder {
            centre,
            radius,
            half_height,
        } => {
            n.extend(centre);
            n.extend([*radius, *half_height]);
            9
        }
        ChildBound::Hull {
            centre,
            corners,
            margin,
        } => {
            n.extend(centre);
            for c in corners {
                n.extend(c);
            }
            n.push(*margin);
            13
        }
        ChildBound::Frustum {
            centre,
            radii,
            factors,
            height,
            margin,
        } => {
            n.extend(centre);
            n.extend(radii);
            n.extend(factors);
            n.extend([*height, *margin]);
            14
        }
    };
    Some((kind, n))
}

/// The primitive record at `at` written from `shape`: its box about its centre, its radius and
/// margin as the reader derives them, and its own fields.
fn put_shape(section: &mut [u8], at: usize, shape: &ChildBound) -> Result<()> {
    let mid = |lo: &[f32; 3], hi: &[f32; 3]| -> [f32; 3] {
        std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0)
    };
    let (centre, margin) = match shape {
        ChildBound::Geometry(_) => return Err(refuse("a geometry child has no dimensions")),
        ChildBound::Sphere { centre, radius } => (*centre, Some(*radius)),
        ChildBound::Capsule {
            box_min,
            box_max,
            radius,
        } => (mid(box_min, box_max), Some(*radius)),
        ChildBound::Box { box_min, box_max } => (mid(box_min, box_max), None),
        ChildBound::TaperedCapsule { centre, radii, .. } => (*centre, Some(radii[0].min(radii[1]))),
        ChildBound::Disc { centre, margin, .. }
        | ChildBound::Hull { centre, margin, .. }
        | ChildBound::Frustum { centre, margin, .. } => (*centre, Some(*margin)),
        ChildBound::Cylinder { centre, .. } => (*centre, None),
    };
    let (half, radius) = shape
        .extent()
        .ok_or(refuse("a geometry child has no dimensions"))?;
    // The vectors' fourth lanes are the margin and the material id's low word: only x, y, z move.
    put_f32(section, at + 0x14, radius)?;
    put_vec3(
        section,
        at + 0x20,
        std::array::from_fn(|a| centre[a] + half[a]),
    )?;
    put_vec3(
        section,
        at + 0x30,
        std::array::from_fn(|a| centre[a] - half[a]),
    )?;
    put_vec3(section, at + 0x40, centre)?;
    if let Some(margin) = margin {
        put_f32(section, at + 0x2C, margin)?;
    }
    match shape {
        ChildBound::TaperedCapsule { radii, length, .. } => {
            put_f32(section, at + 0x70, radii[0])?;
            put_f32(section, at + 0x74, radii[1])?;
            put_f32(section, at + 0x78, *length)?;
        }
        ChildBound::Hull { corners, .. } => {
            for (i, c) in corners.iter().enumerate() {
                put_vec3(section, at + 0x70 + 16 * i, *c)?;
            }
        }
        ChildBound::Frustum {
            radii,
            factors,
            height,
            ..
        } => {
            put_f32(section, at + 0x70, radii[0])?;
            put_f32(section, at + 0x74, radii[1])?;
            put_f32(section, at + 0x78, *height)?;
            put_f32(section, at + 0x7C, factors[0])?;
            put_f32(section, at + 0x80, factors[1])?;
        }
        _ => {}
    }
    Ok(())
}

/// The material ids, shapes, child matrices and box that `edited` changed, written into the
/// composite of a bound's section.
// A primitive child's material id and shape; a geometry child's material ids and its primitives'
// materials. Nothing is written that `edited` holds as read.
pub fn write_bound(virtual_section: &mut [u8], edited: &Bound) -> Result<()> {
    let read = read_bound(virtual_section)?;
    write_bound_at(virtual_section, 0, &read, edited, true)?;
    read_bound(virtual_section).map(|_| ())
}

/// [`write_bound`] for the bound a drawable's root points at from 0xC8.
pub fn write_drawable_bound(
    virtual_section: &mut [u8],
    physical: &[u8],
    edited: &Bound,
) -> Result<()> {
    let read = read_drawable(virtual_section, Some(physical))?;
    let read = read.bound.ok_or(refuse("the drawable holds no bound"))?;
    let at = sections(virtual_section, Some(&[]), |v, _| v.to("bound", 0xC8))?;
    write_bound_at(virtual_section, at, &read, edited, true)?;
    let reread = |v: &[u8]| {
        (read_drawable(v, Some(physical))?.bound).ok_or(refuse("the drawable holds no bound"))
    };
    write_bound_vertices_at(virtual_section, at, edited, &reread)?;
    read_drawable(virtual_section, Some(physical)).map(|_| ())
}

/// [`write_bound`] for the composite a fragment's record at 0x10 holds.
// Its children stay where their bones and their own transforms place them.
pub fn write_fragment_bound(
    virtual_section: &mut [u8],
    physical: &[u8],
    edited: &Bound,
) -> Result<()> {
    let read = read_drawable_with_bound(virtual_section, Some(physical))?;
    let read = read.bound.ok_or(refuse("the fragment holds no bound"))?;
    let at = sections(virtual_section, Some(&[]), |v, _| {
        v.to("bound", v.to("record that holds the bound", 0x10)? + 0x20)
    })?;
    write_bound_at(virtual_section, at, &read, edited, false)?;
    let reread = |v: &[u8]| {
        (read_drawable_with_bound(v, Some(physical))?.bound)
            .ok_or(refuse("the fragment holds no bound"))
    };
    write_bound_vertices_at(virtual_section, at, edited, &reread)?;
    read_drawable_with_bound(virtual_section, Some(physical)).map(|_| ())
}

/// The bound at `at`, read as `read`, written from `edited`; a child moves only where `may_move`.
pub(super) fn write_bound_at(
    section: &mut [u8],
    at: usize,
    read: &Bound,
    edited: &Bound,
    may_move: bool,
) -> Result<()> {
    if read.children.len() != edited.children.len() {
        return Err(refuse("the bound holds another number of children"));
    }
    let root = |b: &Bound| (b.radius, b.box_min, b.box_max, b.centroid);
    let root_changed = root(read) != root(edited);
    let moved = (read.children.iter().zip(&edited.children)).any(|(r, e)| r.matrix != e.matrix);
    let composite = sections(section, Some(&[]), |v, _| {
        Ok(v.u8("bound type", at + 0x10)? == COMPOSITE)
    })?;
    if !composite {
        // A lone bound is its own child, placed where it is: its box is its shape's.
        if root_changed || moved {
            return Err(refuse(
                "a lone bound has no matrix, and its box is its shape's",
            ));
        }
        let (Some(r), Some(e)) = (read.children.first(), edited.children.first()) else {
            return Err(refuse("the bound holds another number of children"));
        };
        return write_child(section, Some(at), r, e);
    }
    if moved && !may_move {
        return Err(refuse(
            "it moves a fragment's child, which its bone and its own transform place",
        ));
    }
    if root_changed || moved {
        let pairs = read.children.iter().zip(&edited.children);
        for (r, e) in pairs.filter(|(r, e)| r.matrix != e.matrix) {
            unit_axes(&r.matrix, &e.matrix)?;
        }
        placed_inside(edited)?;
    }
    let (records, matrices) = sections(section, Some(&[]), |v, _| {
        let pointers = v.to("child pointers", at + 0x70)?;
        let records = (0..read.children.len())
            .map(|i| v.ptr("child", pointers + 8 * i))
            .collect::<Result<Vec<_>>>()?;
        Ok((records, v.to("child matrices", at + 0x78)?))
    })?;
    let pairs = read.children.iter().zip(&edited.children);
    for (i, ((r, e), record)) in pairs.zip(records).enumerate() {
        write_child(section, record, r, e)?;
        if r.flags != e.flags {
            put_child_flags(section, at, i, e.flags)?;
        }
        if r.matrix != e.matrix {
            for (k, row) in e.matrix.iter().enumerate() {
                put_vec3(
                    section,
                    matrices + 64 * i + 16 * k,
                    [row[0], row[1], row[2]],
                )?;
            }
        }
    }
    if root_changed {
        // The vectors' fourth lanes are the margin, a count and a word: only x, y, z move.
        put_f32(section, at + 0x14, edited.radius)?;
        put_vec3(section, at + 0x20, edited.box_max)?;
        put_vec3(section, at + 0x30, edited.box_min)?;
        put_vec3(section, at + 0x40, edited.centroid)?;
    }
    // A BVH over the children holds their boxes as placed, in the frame of the composite's box.
    let reframe = (read.box_min, read.box_max) != (edited.box_min, edited.box_max);
    refit_composite(section, at, read, edited, reframe)
}

/// A child's material id and shape, or a geometry's materials, where `e` changed them from `r`.
fn write_child(
    section: &mut [u8],
    at: Option<usize>,
    r: &BoundChild,
    e: &BoundChild,
) -> Result<()> {
    let other = || refuse("the edit's children are not the bound's");
    let (Some(Ok(rs)), Some(Ok(es)), Some(at)) = (&r.bound, &e.bound, at) else {
        let ok = |b: &Option<Result<ChildBound>>| b.as_ref().map(|b| b.is_ok());
        if ok(&r.bound) != ok(&e.bound) {
            return Err(other());
        }
        return Ok(());
    };
    match (rs, es) {
        (ChildBound::Geometry(rg), ChildBound::Geometry(eg)) => {
            geometry(section, at, rg, eg)?;
            write_rewired(section, at, rg, eg)
        }
        (ChildBound::Geometry(_), _) | (_, ChildBound::Geometry(_)) => Err(other()),
        _ => {
            let (was, now) = (numbers(rs), numbers(es));
            if was.as_ref().map(|n| n.0) != now.as_ref().map(|n| n.0) {
                return Err(refuse(
                    "it changes a child's type, which sets the size of its record",
                ));
            }
            if was != now {
                put_shape(section, at, es)?;
            }
            if e.material_id != r.material_id {
                let keeps = refuse("a primitive child keeps a material id");
                let id = e.material_id.ok_or(keeps)?;
                put(section, at + 0x4C, &(id as u32).to_le_bytes())?;
                put(section, at + 0x5C, &((id >> 32) as u32).to_le_bytes())?;
            }
            Ok(())
        }
    }
}

/// Whether `b`'s box, radius, centroid and child matrices hold what [`write_bound`] checks.
pub fn bound_placement_holds(b: &Bound) -> Result<()> {
    for child in &b.children {
        unit_axes(&child.matrix, &child.matrix)?;
    }
    placed_inside(b)
}

/// A child's matrix `edited` as every measured one is: finite, its first three rows of unit
/// length; each row's fourth lane, whose meaning is not known, kept as `read`.
fn unit_axes(read: &[[f32; 4]; 4], edited: &[[f32; 4]; 4]) -> Result<()> {
    if (0..4).any(|r| read[r][3] != edited[r][3]) {
        return Err(refuse(
            "it changes a matrix row's fourth lane, whose meaning is not known",
        ));
    }
    if edited.iter().flatten().any(|x| !x.is_finite()) {
        return Err(refuse("a child's matrix holds a number not finite"));
    }
    let length = |r: &[f32; 4]| (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    if edited[..3].iter().any(|r| (length(r) - 1.0).abs() > 1e-3) {
        return Err(refuse(
            "a child's matrix has an axis row that is not of unit length",
        ));
    }
    Ok(())
}

/// A box about a child in its own frame: a geometry's, or a primitive's about its centre.
pub(super) fn local_box(shape: &ChildBound) -> Option<([f32; 3], [f32; 3])> {
    let centre = match shape {
        ChildBound::Geometry(g) => return Some((g.box_min, g.box_max)),
        ChildBound::Capsule {
            box_min, box_max, ..
        }
        | ChildBound::Box { box_min, box_max } => {
            std::array::from_fn(|a| (box_min[a] + box_max[a]) / 2.0)
        }
        ChildBound::Sphere { centre, .. }
        | ChildBound::TaperedCapsule { centre, .. }
        | ChildBound::Disc { centre, .. }
        | ChildBound::Cylinder { centre, .. }
        | ChildBound::Hull { centre, .. }
        | ChildBound::Frustum { centre, .. } => *centre,
    };
    let (half, _) = shape.extent()?;
    Some((
        std::array::from_fn(|a| centre[a] - half[a]),
        std::array::from_fn(|a| centre[a] + half[a]),
    ))
}

/// The composite's own box, radius and centroid as every measured body holds them: each number
/// finite, the radius not below zero, the centroid inside the box, and each child's box, placed by
/// its matrix, inside it.
fn placed_inside(b: &Bound) -> Result<()> {
    let (lo, hi, c) = (b.box_min, b.box_max, b.centroid);
    let numbers = lo.iter().chain(&hi).chain(&c).chain([&b.radius]);
    if numbers.clone().any(|x| !x.is_finite()) {
        return Err(refuse(
            "the bound's box, radius or centroid holds a number that is not finite",
        ));
    }
    if (0..3).any(|a| !(lo[a]..=hi[a]).contains(&c[a])) {
        return Err(refuse(
            "the bound's box is inverted, or its centroid lies outside it",
        ));
    }
    if b.radius < 0.0 {
        return Err(refuse("the bound's radius is below zero"));
    }
    let reach = numbers.fold(1f32, |m, x| m.max(x.abs()));
    let slack = 1e-3 + 1e-5 * reach;
    for child in &b.children {
        let Some((min, max)) = (child.bound.as_ref())
            .and_then(|s| s.as_ref().ok())
            .and_then(local_box)
        else {
            continue;
        };
        let m = &child.matrix;
        for k in 0..8 {
            let p = [
                if k & 1 == 0 { min[0] } else { max[0] },
                if k & 2 == 0 { min[1] } else { max[1] },
                if k & 4 == 0 { min[2] } else { max[2] },
            ];
            let at: [f32; 3] =
                std::array::from_fn(|a| p[0] * m[0][a] + p[1] * m[1][a] + p[2] * m[2][a] + m[3][a]);
            if (0..3).any(|a| !(lo[a] - slack..=hi[a] + slack).contains(&at[a])) {
                return Err(refuse(
                    "a child's box, placed by its matrix, lies outside the bound's box",
                ));
            }
        }
    }
    Ok(())
}

/// The geometry child at `at` written from `edited`: its material ids and each primitive's
/// material index, the lists keeping their lengths.
fn geometry(
    section: &mut [u8],
    at: usize,
    read: &BoundGeometry,
    edited: &BoundGeometry,
) -> Result<()> {
    if edited.material_ids.len() != read.material_ids.len()
        || edited.primitive_materials.len() != read.primitive_materials.len()
    {
        return Err(refuse(
            "it changes the length of a geometry's material lists",
        ));
    }
    let count = edited.material_ids.len();
    if let Some(m) = (edited.primitive_materials.iter()).find(|m| usize::from(**m) >= count.max(1))
    {
        return Err(HeaderlessError::IndexPastCount {
            what: "primitive's material index",
            index: (*m).into(),
            count: count as u64,
        });
    }
    let (ids, list) = sections(section, Some(&[]), |v, _| {
        Ok((
            v.ptr("material ids", at + 0xF0)?,
            v.ptr("per-primitive material indices", at + 0x118)?,
        ))
    })?;
    let none = || refuse("the geometry's list is null");
    for (k, (r, e)) in read
        .material_ids
        .iter()
        .zip(&edited.material_ids)
        .enumerate()
    {
        if r != e {
            put(section, ids.ok_or_else(none)? + 8 * k, &e.to_le_bytes())?;
        }
    }
    let materials = read
        .primitive_materials
        .iter()
        .zip(&edited.primitive_materials);
    for (k, (r, e)) in materials.enumerate() {
        if r != e {
            put(section, list.ok_or_else(none)? + k, &[*e])?;
        }
    }
    Ok(())
}

/// The masses and angular inertias `edited` changed, written into a fragment's archetype.
// Its children's too, and the archetype's inverses. A child that moves or changes its group is refused, as
// its composite's matrix and lists follow from those.
pub fn write_fragment(
    virtual_section: &mut [u8],
    physical: &[u8],
    edited: &Fragment,
) -> Result<()> {
    let read = read_drawable_with_bound(virtual_section, Some(physical))?.fragment;
    if read.children.len() != edited.children.len() {
        return Err(refuse("the fragment holds another number of children"));
    }
    let (archetype, children) = sections(virtual_section, None, |v, _| {
        Ok((v.ptr("archetype", 0x10)?, v.ptr("children", 0x28)?))
    })?;
    match (read.archetype, edited.archetype, archetype) {
        (None, None, _) => {}
        (Some(r), Some(e), Some(at)) => {
            if r.mass != e.mass {
                put_f32(virtual_section, at + 0x4C, e.mass)?;
                put_f32(virtual_section, at + 0x5C, inverse(e.mass))?;
            }
            let inertia = r.angular_inertia.iter().zip(&e.angular_inertia);
            for (k, (was, now)) in inertia.enumerate() {
                if was != now {
                    put_f32(virtual_section, at + 0x40 + 4 * k, *now)?;
                    put_f32(virtual_section, at + 0x50 + 4 * k, inverse(*now))?;
                }
            }
        }
        _ => return Err(refuse("the edit's archetype is not the fragment's")),
    }
    for (i, (r, e)) in read.children.iter().zip(&edited.children).enumerate() {
        if (r.group, r.composite, r.rotation, r.translation)
            != (e.group, e.composite, e.rotation, e.translation)
        {
            return Err(refuse(
                "it moves a child or gives it another group, which its composite's matrix and lists follow",
            ));
        }
        let at = children.ok_or(refuse("the fragment has no children"))? + CHILD * i;
        if r.mass != e.mass {
            put_f32(virtual_section, at + 0x2C, e.mass)?;
        }
        let inertia = r.angular_inertia.iter().zip(&e.angular_inertia);
        for (k, (was, now)) in inertia.enumerate() {
            if was != now {
                put_f32(virtual_section, at + 0x20 + 4 * k, *now)?;
            }
        }
    }
    read_drawable_with_bound(virtual_section, Some(physical)).map(|_| ())
}

/// A navigation cell's polygon flags at +0x08, one per polygon in read order, but for `keep`'s bits.
pub fn write_polygon_flags(virtual_section: &mut [u8], flags: &[u32], keep: u32) -> Result<()> {
    let read = read_navmesh(virtual_section)?;
    let places = sections(virtual_section, Some(&[]), |v, _| items(v, 0x98, POLYGON))?;
    if read.polygons.len() != flags.len() || places.len() != flags.len() {
        return Err(refuse("the cell holds another number of polygons"));
    }
    for ((at, p), f) in places.into_iter().zip(&read.polygons).zip(flags) {
        let word = (p.flags & keep) | (f & !keep);
        if word != p.flags {
            put(virtual_section, at + 8, &word.to_le_bytes())?;
        }
    }
    read_navmesh(virtual_section).map(|_| ())
}

/// Where each item of the split array the root's slot `slot` points at lies, chunk by chunk.
pub(super) fn items(v: Bytes, slot: usize, stride: usize) -> Result<Vec<usize>> {
    let what = "polygon list";
    let at = v.to(what, slot)?;
    let table = v.to(what, at + 0x10)?;
    let chunks = usize::try_from(v.u64(what, at + 0x20)?).unwrap_or(usize::MAX);
    let rows = v.array(what, table, chunks, 16)?;
    let mut places = Vec::new();
    for (i, row) in rows.as_chunks::<16>().0.iter().enumerate() {
        let n = usize::try_from(u64_at(row, 8)).unwrap_or(usize::MAX);
        let chunk = v.to(what, table + 16 * i)?;
        v.array(what, chunk, n, stride)?;
        places.extend((0..n).map(|k| chunk + stride * k));
    }
    Ok(places)
}

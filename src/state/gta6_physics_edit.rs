//! Edits of a scanned GTA VI bound, fragment and navigation cell that move nothing, each a step of
//! undo: a child's or a polygon's material key and a primitive child's dimensions, the archetype's
//! and the children's masses, a polygon's flags. A save reads the body again on its worker and
//! writes the values shown over its sections in place.

use std::sync::Arc;

use or_io::gta5::TNavMeshPoly;
use or_io::gta6::headerless::write_bound_vertices;
use or_io::gta6::headerless::{
    bound_placement_holds, material_key, write_bound, write_drawable_bound, write_fragment_level,
    write_polygon_flags, Bound, BoundGeometry, ChildBound, Fragment, Resource,
};

use super::document::Document;
use super::edit_history::Change;
use super::gta6_edit::EditSections;
use super::gta6_resource_save::EditedSave;
use super::{AppState, DocPayload, DocumentId};

/// What an edit of a GTA VI bound sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoundEdit {
    /// The material key of primitive child `child`.
    ChildKey { child: usize, key: u16 },
    /// Primitive `primitive` of geometry child `child` given the geometry's material of key `key`.
    PolygonKey {
        child: usize,
        primitive: usize,
        key: u16,
    },
    /// Primitive child `child`'s dimensions, in the order [`dimensions`] names them.
    Dimensions { child: usize, values: Vec<f32> },
    /// Child `child`'s matrix, rows as the composite stores them.
    Matrix { child: usize, matrix: [[f32; 4]; 4] },
    /// The bound's own radius, box and centroid.
    Root {
        radius: f32,
        box_min: [f32; 3],
        box_max: [f32; 3],
        centroid: [f32; 3],
    },
}

/// What an edit of a GTA VI fragment sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FragmentEdit {
    /// The archetype's mass in kilograms; each child the composite lists keeps its share of it.
    Mass(f32),
    /// Child `child`'s mass; the archetype's is the sum of the composite's children.
    ChildMass { child: usize, mass: f32 },
    /// The archetype's angular inertia about x, y and z, a value of its own.
    Inertia([f32; 3]),
    /// Child `child`'s angular inertia about x, y and z.
    ChildInertia { child: usize, inertia: [f32; 3] },
}

/// The bound a body is, or the one a drawable or a fragment embeds.
pub(super) fn bound_mut(payload: &mut DocPayload) -> Option<&mut Bound> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    match Arc::get_mut(resource)? {
        Resource::Bound(b) => Some(b),
        Resource::Drawable(d) => d.bound.as_mut(),
        Resource::DrawableWithBound(r) => r.bound.as_mut(),
        _ => None,
    }
}

fn shape_mut(payload: &mut DocPayload, child: usize) -> Option<&mut ChildBound> {
    (bound_mut(payload)?
        .children
        .get_mut(child)?
        .bound
        .as_mut()?)
    .as_mut()
    .ok()
}

pub(super) fn geometry_mut(payload: &mut DocPayload, child: usize) -> Option<&mut BoundGeometry> {
    match shape_mut(payload, child)? {
        ChildBound::Geometry(g) => Some(g),
        _ => None,
    }
}

fn fragment_mut(payload: &mut DocPayload) -> Option<&mut Fragment> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    match Arc::get_mut(resource)? {
        Resource::DrawableWithBound(r) => Some(&mut *r.fragment),
        _ => None,
    }
}

fn polygon_mut(payload: &mut DocPayload, polygon: usize) -> Option<&mut TNavMeshPoly> {
    let DocPayload::Resource(or_io::Asset::NavMesh(mesh)) = payload else {
        return None;
    };
    (mesh.polys_array.as_deref_mut()?.sub_arrays.iter_mut())
        .flat_map(|s| s.elements.iter_mut())
        .nth(polygon)
}

/// A primitive child's dimensions an edit sets, with their names; none for a geometry.
pub(crate) fn dimensions(shape: &ChildBound) -> Vec<(&'static str, f32)> {
    match *shape {
        ChildBound::Geometry(_) => Vec::new(),
        ChildBound::Sphere { radius, .. } => vec![(tr!("Radius"), radius)],
        ChildBound::Capsule {
            box_min,
            box_max,
            radius,
        } => vec![
            (tr!("Radius"), radius),
            (tr!("Half height"), (box_max[1] - box_min[1]) / 2.0),
        ],
        ChildBound::Box { box_min, box_max } => vec![
            (tr!("Size x"), box_max[0] - box_min[0]),
            (tr!("Size y"), box_max[1] - box_min[1]),
            (tr!("Size z"), box_max[2] - box_min[2]),
        ],
        ChildBound::TaperedCapsule { radii, length, .. } => vec![
            (tr!("First radius"), radii[0]),
            (tr!("Second radius"), radii[1]),
            (tr!("Length"), length),
        ],
        ChildBound::Disc { radius, margin, .. } => {
            vec![(tr!("Radius"), radius), (tr!("Margin"), margin)]
        }
        ChildBound::Cylinder {
            radius,
            half_height,
            ..
        } => vec![(tr!("Radius"), radius), (tr!("Half height"), half_height)],
        ChildBound::Hull { margin, .. } => vec![(tr!("Margin"), margin)],
        ChildBound::Frustum {
            radii,
            height,
            margin,
            ..
        } => vec![
            (tr!("First radius"), radii[0]),
            (tr!("Second radius"), radii[1]),
            (tr!("Height"), height),
            (tr!("Margin"), margin),
        ],
    }
}

/// `shape` about the same centre with `values` as the dimensions [`dimensions`] names.
fn with_dimensions(shape: &ChildBound, values: &[f32]) -> Option<ChildBound> {
    let centre_of = |lo: &[f32; 3], hi: &[f32; 3]| -> [f32; 3] {
        std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0)
    };
    Some(match (shape, values) {
        (ChildBound::Sphere { centre, .. }, [r]) => ChildBound::Sphere {
            centre: *centre,
            radius: *r,
        },
        (
            ChildBound::Capsule {
                box_min, box_max, ..
            },
            [r, h],
        ) => {
            let c = centre_of(box_min, box_max);
            let half = [*r, *h, *r];
            ChildBound::Capsule {
                box_min: std::array::from_fn(|a| c[a] - half[a]),
                box_max: std::array::from_fn(|a| c[a] + half[a]),
                radius: *r,
            }
        }
        (ChildBound::Box { box_min, box_max }, [x, y, z]) => {
            let c = centre_of(box_min, box_max);
            let half = [*x / 2.0, *y / 2.0, *z / 2.0];
            ChildBound::Box {
                box_min: std::array::from_fn(|a| c[a] - half[a]),
                box_max: std::array::from_fn(|a| c[a] + half[a]),
            }
        }
        (ChildBound::TaperedCapsule { centre, .. }, [a, b, l]) => ChildBound::TaperedCapsule {
            centre: *centre,
            radii: [*a, *b],
            length: *l,
        },
        (ChildBound::Disc { centre, .. }, [r, m]) => ChildBound::Disc {
            centre: *centre,
            radius: *r,
            margin: *m,
        },
        (ChildBound::Cylinder { centre, .. }, [r, h]) => ChildBound::Cylinder {
            centre: *centre,
            radius: *r,
            half_height: *h,
        },
        (
            ChildBound::Hull {
                centre, corners, ..
            },
            [m],
        ) => ChildBound::Hull {
            centre: *centre,
            corners: *corners,
            margin: *m,
        },
        (
            ChildBound::Frustum {
                centre, factors, ..
            },
            [a, b, h, m],
        ) => ChildBound::Frustum {
            centre: *centre,
            radii: [*a, *b],
            factors: *factors,
            height: *h,
            margin: *m,
        },
        _ => return None,
    })
}

/// `edit` made to `f`'s masses, its archetype's kept the sum of the children its composite lists.
fn apply_fragment(f: &mut Fragment, edit: FragmentEdit) -> Result<(), String> {
    let mut archetype = f.archetype.ok_or_else(|| {
        tr!("It has no physics level: its archetype, groups and children are null.")
    })?;
    match edit {
        FragmentEdit::Mass(mass) => {
            if archetype.mass <= 0.0 {
                return Err(tr!("The archetype's mass is zero, so its children's shares of another are not known.").to_string());
            }
            let scale = mass / archetype.mass;
            for c in f.children.iter_mut().filter(|c| c.composite.is_some()) {
                c.mass *= scale;
            }
            archetype.mass = mass;
        }
        FragmentEdit::ChildMass { child, mass } => {
            let c = (f.children.get_mut(child))
                .ok_or_else(|| tr!("The fragment holds no such child."))?;
            c.mass = mass;
            archetype.mass = (f.children.iter())
                .filter(|c| c.composite.is_some())
                .map(|c| c.mass)
                .sum();
        }
        FragmentEdit::Inertia(inertia) => archetype.angular_inertia = inertia,
        FragmentEdit::ChildInertia { child, inertia } => {
            let c = (f.children.get_mut(child))
                .ok_or_else(|| tr!("The fragment holds no such child."))?;
            c.angular_inertia = inertia;
        }
    }
    f.archetype = Some(archetype);
    Ok(())
}

/// Refused for a drawable's lone bound, which is read as a composite of one placed where it is.
// Every measured composite states its children's type and include entries; a lone bound has none.
fn placed_by_itself(payload: &DocPayload) -> Result<(), String> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return Ok(());
    };
    let Resource::Drawable(d) = &**resource else {
        return Ok(());
    };
    match d.bound.as_ref().map(|b| &b.children[..]) {
        Some([c]) if c.flags.is_none() => Err(tr!("A drawable's lone bound has no matrix of its own, and its box is its shape's: change its dimensions instead.").to_string()),
        _ => Ok(()),
    }
}

/// The bits of a polygon's flags GTA V's polygon holds as its flags; the rest hold its corners.
fn model_flag_bits() -> u32 {
    let mut p = TNavMeshPoly::default();
    p.set_as_integer1(u32::MAX);
    p.as_integer1()
}

impl AppState {
    /// `edit` made to the GTA VI bound `doc` shows, as a step of its history.
    pub(crate) fn edit_gta6_bound(
        &mut self,
        doc: DocumentId,
        edit: BoundEdit,
    ) -> Result<(), String> {
        let busy =
            || tr!("The bound is not edited while a task reads the body; try again once it ends.");
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let payload = &mut document.payload;
        let (label, change) = match edit {
            BoundEdit::ChildKey { child, key } => {
                if key > 0x7FFF {
                    return Err(tr!("A material key is fifteen bits: 0 to 0x7FFF.").to_string());
                }
                let held = (bound_mut(payload).and_then(|b| b.children.get_mut(child)))
                    .ok_or_else(busy)?;
                let id = held.material_id.ok_or_else(|| {
                    tr!("Only a primitive child holds a material key of its own: a geometry's are its primitives'.")
                })?;
                let before = held.material_id;
                held.material_id = Some((id & !0x7FFF) | u64::from(key));
                let change = Change::part(format!("gta6 bound key {child}"), before, move |p| {
                    bound_mut(p)?
                        .children
                        .get_mut(child)
                        .map(|c| &mut c.material_id)
                });
                (tr!("Set a material key"), change)
            }
            BoundEdit::PolygonKey {
                child,
                primitive,
                key,
            } => {
                let g = geometry_mut(payload, child).ok_or_else(busy)?;
                let index = (g.material_ids.iter())
                    .position(|id| material_key(*id) == key)
                    .and_then(|i| u8::try_from(i).ok())
                    .ok_or_else(|| tr!("No material of this geometry has that key: another would lengthen its material list, which would move the sections."))?;
                let held = (g.primitive_materials.get_mut(primitive))
                    .ok_or_else(|| tr!("This geometry names no material for that primitive."))?;
                let before = *held;
                *held = index;
                let key = format!("gta6 bound polygon {child} {primitive}");
                let change = Change::part(key, before, move |p| {
                    geometry_mut(p, child)?
                        .primitive_materials
                        .get_mut(primitive)
                });
                (tr!("Set a material key"), change)
            }
            BoundEdit::Dimensions { child, values } => {
                if values.iter().any(|x| !x.is_finite() || *x < 0.0) {
                    return Err(
                        tr!("A dimension is a length: a number of metres, zero or more.")
                            .to_string(),
                    );
                }
                let held = shape_mut(payload, child).ok_or_else(busy)?;
                let shaped = with_dimensions(held, &values)
                    .ok_or_else(|| tr!("Only a primitive child's dimensions are edited."))?;
                let before = std::mem::replace(held, shaped);
                let change = Change::part(format!("gta6 bound shape {child}"), before, move |p| {
                    shape_mut(p, child)
                });
                (tr!("Set a bound's dimensions"), change)
            }
            BoundEdit::Matrix { child, matrix } => {
                placed_by_itself(payload)?;
                if let DocPayload::Gta6Drawable(r) = payload {
                    if matches!(**r, Resource::DrawableWithBound(_)) {
                        return Err(tr!("A fragment's child is placed by its bone and its own transform, so its composite's matrix is not edited.").to_string());
                    }
                }
                let b = bound_mut(payload).ok_or_else(busy)?;
                let held = (b.children.get_mut(child))
                    .ok_or_else(|| tr!("The bound holds no such child."))?;
                let before = std::mem::replace(&mut held.matrix, matrix);
                if let Err(e) = bound_placement_holds(b) {
                    if let Some(c) = b.children.get_mut(child) {
                        c.matrix = before;
                    }
                    return Err(e.to_string());
                }
                let change = Change::part(format!("gta6 bound matrix {child}"), before, move |p| {
                    bound_mut(p)?.children.get_mut(child).map(|c| &mut c.matrix)
                });
                (tr!("Placed a bound's child"), change)
            }
            BoundEdit::Root {
                radius,
                box_min,
                box_max,
                centroid,
            } => {
                placed_by_itself(payload)?;
                let b = bound_mut(payload).ok_or_else(busy)?;
                let before = (b.radius, b.box_min, b.box_max, b.centroid);
                (b.radius, b.box_min, b.box_max, b.centroid) = (radius, box_min, box_max, centroid);
                if let Err(e) = bound_placement_holds(b) {
                    (b.radius, b.box_min, b.box_max, b.centroid) = before;
                    return Err(e.to_string());
                }
                let change = Change::part("gta6 bound root", before.0, |p| {
                    bound_mut(p).map(|b| &mut b.radius)
                })
                .and_part(before.1, |p| bound_mut(p).map(|b| &mut b.box_min))
                .and_part(before.2, |p| bound_mut(p).map(|b| &mut b.box_max))
                .and_part(before.3, |p| bound_mut(p).map(|b| &mut b.centroid));
                (tr!("Set a bound's box"), change)
            }
        };
        self.record(doc, label, change);
        Ok(())
    }

    /// `edit` made to the GTA VI fragment `doc` shows, as a step of its history.
    pub(crate) fn edit_gta6_fragment(
        &mut self,
        doc: DocumentId,
        edit: FragmentEdit,
    ) -> Result<(), String> {
        let not_negative = |x: &f32| x.is_finite() && *x >= 0.0;
        match edit {
            FragmentEdit::Mass(mass) | FragmentEdit::ChildMass { mass, .. } => {
                if !not_negative(&mass) {
                    return Err(tr!("A mass is a number of kilograms, zero or more.").to_string());
                }
            }
            FragmentEdit::Inertia(inertia) | FragmentEdit::ChildInertia { inertia, .. } => {
                if !inertia.iter().all(not_negative) {
                    return Err(
                        tr!("An angular inertia is three numbers, each zero or more.").to_string(),
                    );
                }
            }
        }
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let f = fragment_mut(&mut document.payload).ok_or_else(|| {
            tr!("The fragment is not edited while a task reads the body; try again once it ends.")
        })?;
        let archetype = f.archetype;
        let masses: Vec<f32> = f.children.iter().map(|c| c.mass).collect();
        let inertias: Vec<[f32; 3]> = f.children.iter().map(|c| c.angular_inertia).collect();
        apply_fragment(f, edit)?;
        let (key, label) = match edit {
            FragmentEdit::Mass(_) => (
                "gta6 fragment mass".to_string(),
                tr!("Set the archetype's mass"),
            ),
            FragmentEdit::ChildMass { child, .. } => (
                format!("gta6 fragment child {child} mass"),
                tr!("Set a child's mass"),
            ),
            FragmentEdit::Inertia(_) => (
                "gta6 fragment inertia".to_string(),
                tr!("Set the archetype's angular inertia"),
            ),
            FragmentEdit::ChildInertia { child, .. } => (
                format!("gta6 fragment child {child} inertia"),
                tr!("Set a child's angular inertia"),
            ),
        };
        let mut change = Change::part(key, archetype, |p| {
            fragment_mut(p).map(|f| &mut f.archetype)
        });
        for (i, m) in masses.into_iter().enumerate() {
            change = change.and_part(m, move |p| {
                fragment_mut(p)?.children.get_mut(i).map(|c| &mut c.mass)
            });
        }
        for (i, inertia) in inertias.into_iter().enumerate() {
            change = change.and_part(inertia, move |p| {
                fragment_mut(p)?
                    .children
                    .get_mut(i)
                    .map(|c| &mut c.angular_inertia)
            });
        }
        self.record(doc, label, change);
        Ok(())
    }

    /// Polygon `polygon` of the scanned GTA VI cell `doc` given the flags `flags`, as a step of its
    /// history; the bits that hold GTA V's corner count keep the cell's own.
    pub(crate) fn set_gta6_polygon_flags(
        &mut self,
        doc: DocumentId,
        polygon: usize,
        flags: u32,
    ) -> Result<(), String> {
        let path = self.document(doc).and_then(|d| d.path.clone());
        if path
            .as_deref()
            .and_then(super::gta6_navmesh::scanned_cell)
            .is_none()
        {
            return Err(
                tr!("Only a scanned GTA VI cell's polygon flags are written in place.").to_string(),
            );
        }
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let held = polygon_mut(&mut document.payload, polygon)
            .ok_or_else(|| tr!("The cell holds no such polygon."))?;
        let before = held.clone();
        held.set_as_integer1(flags);
        let change = Change::part(format!("gta6 polygon flags {polygon}"), before, move |p| {
            polygon_mut(p, polygon)
        });
        self.record(doc, tr!("Set a polygon's flags"), change);
        Ok(())
    }
}

/// The GTA VI drawable row's `edited_sections`: the materials' edit, then a bound's or a
/// fragment's values written over the body read again; `None` where it holds no edit.
pub(super) fn edited_save(document: &Document) -> Option<Result<EditedSave, String>> {
    let save = super::gta6_edit::edited_save(document)?;
    let DocPayload::Gta6Drawable(resource) = &document.payload else {
        return Some(save);
    };
    if !matches!(
        **resource,
        Resource::Bound(_) | Resource::Drawable(_) | Resource::DrawableWithBound(_)
    ) {
        return Some(save);
    }
    // ⚠ The worker holds the body until the save ends, so an edit made meanwhile is refused.
    let resource = Arc::clone(resource);
    let reshaped = match bound_reshape::saved(document) {
        Ok(reshaped) => reshaped,
        Err(why) => return Some(Err(why)),
    };
    Some(save.map(|save| match save {
        EditedSave::Body {
            archive,
            offset,
            edit,
        } => {
            let edit: Arc<EditSections> = Arc::new(move |role, sections| {
                edit(role, sections)?;
                bound_reshape::write_saved(sections, &resource, &reshaped)?;
                let (v, p) = (&mut sections.virtual_section, &sections.physical);
                let written = match &*resource {
                    Resource::Bound(b) => {
                        write_bound(v, b).and_then(|()| write_bound_vertices(v, b))
                    }
                    Resource::Drawable(d) => {
                        (d.bound.as_ref()).map_or(Ok(()), |b| write_drawable_bound(v, p, b))
                    }
                    Resource::DrawableWithBound(r) => write_fragment_level(v, p, r),
                    _ => Ok(()),
                };
                written.map_err(|e| e.to_string())
            });
            EditedSave::Body {
                archive,
                offset,
                edit,
            }
        }
        held => held,
    }))
}

/// The navigation mesh row's `edited_sections` where a scanned GTA VI cell holds an edit: its body
/// read again on the worker, its deletions made and its polygons' flags written over it; `None`
/// for any other.
pub(super) fn edited_cell(document: &Document) -> Option<Result<EditedSave, String>> {
    if !document.history.can_undo() {
        return None;
    }
    let path = document.path.as_deref()?;
    let cell = super::gta6_navmesh::scanned_cell(path)?;
    let (archive, offset) = super::rpf9_scan::scanned_body(path)?;
    let DocPayload::Resource(or_io::Asset::NavMesh(mesh)) = &document.payload else {
        return None;
    };
    let flags: Vec<u32> = (mesh.polys_array.as_deref()).map_or(Vec::new(), |l| {
        l.iter().map(TNavMeshPoly::as_integer1).collect()
    });
    let keep = !model_flag_bits();
    let open = super::gta6_navmesh_edit::open_edges(mesh);
    let (made, others) = super::gta6_navmesh_delete::saved_with(path, &cell);
    let edit: Arc<EditSections> = Arc::new(move |_, sections| {
        super::gta6_navmesh_delete::write_deletions(sections, &made, &others)?;
        write_polygon_flags(&mut sections.virtual_section, &flags, keep)
            .map_err(|e| e.to_string())?;
        super::gta6_navmesh_edit::write_cuts(&mut sections.virtual_section, &open)
    });
    Some(Ok(EditedSave::Body {
        archive: archive.to_path_buf(),
        offset,
        edit,
    }))
}

#[path = "gta6_bound_reshape.rs"]
pub(crate) mod bound_reshape;

#[cfg(test)]
#[path = "gta6_physics_edit_tests.rs"]
mod tests;

//! A scanned GTA VI bound's collision reshaped where a count changes, one step of undo each: a
//! geometry's primitive or vertex added or deleted, a child of a lone bound's composite copied or
//! deleted, a primitive child given another type. A save makes each count change again on the
//! body read again, before the values the document shows are written over it in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use or_io::container::rpf::Rpf9Sections;
use or_io::gta6::headerless::{
    add_bound_primitive, add_bound_vertex, copy_bound_child, delete_bound_child,
    delete_bound_primitive, delete_bound_vertex, fit_composite_box, reshape_bound_body,
    retyped_bound_child, write_retyped, Bound, BoundChild, BoundGeometry, BoundHolder,
    BoundReshape, ChildBound, Primitive, Resource, UnknownShape,
};

use super::{bound_mut, geometry_mut, shape_mut};
use crate::state::document::Document;
use crate::state::edit_history::{Change, Swap};
use crate::state::{AppState, DocPayload, DocumentId};

/// What a reshape of a GTA VI bound that changes a count, or a child's type, sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ReshapeEdit {
    /// Primitive `primitive` of geometry child `child` deleted.
    DeletePrimitive { child: usize, primitive: usize },
    /// A triangle over `vertices` added to geometry child `child`, of material index `material`.
    AddTriangle {
        child: usize,
        vertices: [u16; 3],
        material: u8,
    },
    /// Vertex `vertex` of geometry child `child` deleted.
    DeleteVertex { child: usize, vertex: usize },
    /// A vertex at `position` added to geometry child `child`.
    AddVertex { child: usize, position: [f32; 3] },
    /// Child `child` of a lone bound's composite deleted.
    DeleteChild { child: usize },
    /// Primitive child `child` of a lone bound's composite copied as its last.
    CopyChild { child: usize },
    /// Primitive child `child` given the type `kind`, filling its box.
    ChildType { child: usize, kind: u8 },
}

/// A count change as a save makes it again, `None` for a type change, which a save finds itself;
/// and the counts of the bound it began from.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Made {
    op: Option<BoundReshape>,
    began: Vec<Option<(usize, usize)>>,
}

type Key = (PathBuf, DocumentId);

// ponytail: one entry per document a bound was reshaped in, never emptied; an id is not reused.
static MADE: Mutex<BTreeMap<Key, Vec<Made>>> = Mutex::new(BTreeMap::new());

// The changes a save over the document's own file wrote into it, which the body read again holds.
static SAVED: Mutex<BTreeMap<Key, Vec<Made>>> = Mutex::new(BTreeMap::new());

fn made() -> MutexGuard<'static, BTreeMap<Key, Vec<Made>>> {
    MADE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn saved_made() -> MutexGuard<'static, BTreeMap<Key, Vec<Made>>> {
    SAVED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The changes of `made` a body that holds `held` holds not yet, or why none are made.
fn unsaved<'a>(made: &'a [Made], held: &[Made]) -> Result<&'a [Made], String> {
    made.strip_prefix(held).ok_or_else(|| {
        tr!("The file holds a collision change that was undone after it was saved, and a save does not take it out again: open the file again to edit it.").to_string()
    })
}

/// `made` less the changes the body at `key` already holds.
fn pending(key: &Key, made: &[Made]) -> Result<Vec<Made>, String> {
    let held = saved_made().get(key).cloned().unwrap_or_default();
    unsaved(made, &held).map(<[Made]>::to_vec)
}

/// Document `document`'s file now holds every change its collision made.
pub(crate) fn saved_over(document: &Document) {
    let Some(path) = document.path.clone() else {
        return;
    };
    let key = (path, document.id);
    if let Some(made) = made().get(&key).cloned() {
        saved_made().insert(key, made);
    }
}

/// Each child's vertex and primitive counts, `None` for a child that is no geometry.
fn counts(b: &Bound) -> Vec<Option<(usize, usize)>> {
    (b.children.iter())
        .map(|c| match &c.bound {
            Some(Ok(ChildBound::Geometry(g))) => Some((g.vertices.len(), g.primitives.len())),
            _ => None,
        })
        .collect()
}

fn holder_of(resource: &Resource) -> Option<BoundHolder> {
    match resource {
        Resource::Bound(_) => Some(BoundHolder::Bound),
        Resource::Drawable(_) => Some(BoundHolder::Drawable),
        Resource::DrawableWithBound(_) => Some(BoundHolder::Fragment),
        _ => None,
    }
}

fn bound_of(resource: &Resource) -> Option<&Bound> {
    match resource {
        Resource::Bound(b) => Some(b),
        Resource::Drawable(d) => d.bound.as_ref(),
        Resource::DrawableWithBound(r) => r.bound.as_ref(),
        _ => None,
    }
}

/// The changes a save makes, which an undo puts back.
struct MadePart {
    key: Key,
    made: Vec<Made>,
}

impl Swap for MadePart {
    fn swap(&mut self, _: &mut DocPayload) -> bool {
        std::mem::swap(made().entry(self.key.clone()).or_default(), &mut self.made);
        true
    }
}

/// A child of the bound taken out or put back at `child`: once swapped, the other.
struct ChildSlot {
    child: usize,
    held: Option<BoundChild>,
}

impl Swap for ChildSlot {
    fn swap(&mut self, payload: &mut DocPayload) -> bool {
        let Some(b) = bound_mut(payload) else {
            return false;
        };
        match self.held.take() {
            Some(c) if self.child <= b.children.len() => {
                b.children.insert(self.child, c);
                true
            }
            Some(c) => {
                self.held = Some(c);
                false
            }
            None if self.child < b.children.len() => {
                self.held = Some(b.children.remove(self.child));
                true
            }
            None => false,
        }
    }
}

type Held = (
    Vec<Primitive>,
    Vec<u8>,
    Vec<[f32; 3]>,
    Vec<(usize, UnknownShape)>,
);

fn held(g: &BoundGeometry) -> Held {
    (
        g.primitives.clone(),
        g.primitive_materials.clone(),
        g.vertices.clone(),
        g.unknown_shapes.clone(),
    )
}

/// A step putting back geometry child `child`'s lists as `was` holds them.
fn geometry_change(child: usize, (p, m, v, u): Held) -> Change {
    Change::Part {
        key: None,
        parts: Vec::new(),
    }
    .and_part(p, move |d| {
        geometry_mut(d, child).map(|g| &mut g.primitives)
    })
    .and_part(m, move |d| {
        geometry_mut(d, child).map(|g| &mut g.primitive_materials)
    })
    .and_part(v, move |d| geometry_mut(d, child).map(|g| &mut g.vertices))
    .and_part(u, move |d| {
        geometry_mut(d, child).map(|g| &mut g.unknown_shapes)
    })
}

type Root = (f32, [f32; 3], [f32; 3], [f32; 3]);

fn root_of(b: &Bound) -> Root {
    (b.radius, b.box_min, b.box_max, b.centroid)
}

fn with_root(change: Change, (radius, lo, hi, centroid): Root) -> Change {
    change
        .and_part(radius, |p| bound_mut(p).map(|b| &mut b.radius))
        .and_part(lo, |p| bound_mut(p).map(|b| &mut b.box_min))
        .and_part(hi, |p| bound_mut(p).map(|b| &mut b.box_max))
        .and_part(centroid, |p| bound_mut(p).map(|b| &mut b.centroid))
}

fn shape_in(b: &mut Bound, child: usize) -> Result<&mut ChildBound, String> {
    (b.children.get_mut(child))
        .and_then(|c| c.bound.as_mut())
        .and_then(|s| s.as_mut().ok())
        .ok_or_else(|| tr!("The bound holds no such child.").to_string())
}

fn geometry_in(b: &mut Bound, child: usize) -> Result<&mut BoundGeometry, String> {
    match shape_in(b, child)? {
        ChildBound::Geometry(g) => Ok(g),
        _ => Err(
            tr!("Only a geometry child's primitives and vertices are added or deleted.")
                .to_string(),
        ),
    }
}

/// `edit` made to `b`, which `holder` holds: the change a save makes again, the step's label and
/// the step that puts `b` back.
fn changed(
    b: &mut Bound,
    holder: BoundHolder,
    edit: ReshapeEdit,
) -> Result<(Option<BoundReshape>, String, Change), String> {
    let lone = || {
        match holder {
        BoundHolder::Bound => Ok(()),
        _ => Err(tr!("Only a lone bound's composite has a child added or deleted: in a drawable or a fragment each child is also a fragment child.").to_string()),
    }
    };
    Ok(match edit {
        ReshapeEdit::DeletePrimitive { child, primitive } => {
            let g = geometry_in(b, child)?;
            let was = held(g);
            delete_bound_primitive(g, primitive).map_err(|e| e.to_string())?;
            (
                Some(BoundReshape::DeletePrimitive { child, primitive }),
                tr!("Delete primitive {primitive}", primitive = primitive),
                geometry_change(child, was),
            )
        }
        ReshapeEdit::AddTriangle {
            child,
            vertices,
            material,
        } => {
            let g = geometry_in(b, child)?;
            let was = held(g);
            let triangle = Primitive::Triangle {
                vertices,
                neighbours: [0xFFFF; 3],
                edges: [0xFF; 3],
            };
            let at = add_bound_primitive(g, triangle, material).map_err(|e| e.to_string())?;
            let record = g.primitives.get(at).copied().unwrap_or(triangle);
            (
                Some(BoundReshape::AddPrimitive {
                    child,
                    record,
                    material,
                }),
                tr!("Add triangle {primitive}", primitive = at),
                geometry_change(child, was),
            )
        }
        ReshapeEdit::DeleteVertex { child, vertex } => {
            let g = geometry_in(b, child)?;
            let was = held(g);
            delete_bound_vertex(g, vertex).map_err(|e| e.to_string())?;
            (
                Some(BoundReshape::DeleteVertex { child, vertex }),
                tr!("Delete vertex {vertex}", vertex = vertex),
                geometry_change(child, was),
            )
        }
        ReshapeEdit::AddVertex { child, position } => {
            let g = geometry_in(b, child)?;
            let was = held(g);
            let at = add_bound_vertex(g, position).map_err(|e| e.to_string())?;
            (
                Some(BoundReshape::AddVertex { child }),
                tr!("Add vertex {vertex}", vertex = at),
                geometry_change(child, was),
            )
        }
        ReshapeEdit::DeleteChild { child } => {
            lone()?;
            let root = root_of(b);
            let taken = delete_bound_child(b, child).map_err(|e| e.to_string())?;
            let change = Change::Part {
                key: None,
                parts: vec![Box::new(ChildSlot {
                    child,
                    held: Some(taken),
                })],
            };
            (
                Some(BoundReshape::DeleteChild { child }),
                tr!("Delete child {child}", child = child),
                with_root(change, root),
            )
        }
        ReshapeEdit::CopyChild { child } => {
            lone()?;
            let at = copy_bound_child(b, child).map_err(|e| e.to_string())?;
            let change = Change::Part {
                key: None,
                parts: vec![Box::new(ChildSlot {
                    child: at,
                    held: None,
                })],
            };
            (
                Some(BoundReshape::CopyChild { child }),
                tr!("Copy child {child}", child = child),
                change,
            )
        }
        ReshapeEdit::ChildType { child, kind } => {
            let lone_shape = b.children.len() == 1 && b.children[0].flags.is_none();
            if holder == BoundHolder::Drawable && lone_shape {
                return Err(tr!(
                    "A drawable's lone bound keeps its type: the drawable points at its record."
                )
                .to_string());
            }
            let root = root_of(b);
            let shape = shape_in(b, child)?;
            let now = retyped_bound_child(shape, kind).map_err(|e| e.to_string())?;
            let was = std::mem::replace(shape, now);
            if let Err(e) = fit_composite_box(b) {
                if let Ok(shape) = shape_in(b, child) {
                    *shape = was;
                }
                return Err(e.to_string());
            }
            let change = Change::Part {
                key: None,
                parts: Vec::new(),
            }
            .and_part(was, move |p| shape_mut(p, child));
            (
                None,
                tr!("Give child {child} another type", child = child),
                with_root(change, root),
            )
        }
    })
}

impl AppState {
    /// `edit` made to the GTA VI bound `doc` shows, as a step of its history a save makes again.
    pub(crate) fn reshape_gta6_bound(
        &mut self,
        doc: DocumentId,
        edit: ReshapeEdit,
    ) -> Result<(), String> {
        let busy =
            || tr!("The bound is not edited while a task reads the body; try again once it ends.");
        let path = (self.document(doc).and_then(|d| d.path.clone()))
            .ok_or_else(|| tr!("This document has no file, so its collision is not reshaped."))?;
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let payload = &mut document.payload;
        let holder = match &*payload {
            DocPayload::Gta6Drawable(resource) => holder_of(resource),
            _ => None,
        }
        .ok_or_else(busy)?;
        let b = bound_mut(payload).ok_or_else(busy)?;
        let began = counts(b);
        let (op, label, mut change) = changed(b, holder, edit)?;
        let last = counts(b);
        let key = (path.clone(), doc);
        let earlier = made().get(&key).cloned().unwrap_or_default();
        let mut planned = earlier.clone();
        planned.push(Made { op, began });
        let outcome = pending(&key, &planned).and_then(|p| tried(&path, holder, p, last));
        if let Err(why) = outcome {
            if let Change::Part { parts, .. } = &mut change {
                for part in parts.iter_mut().rev() {
                    part.swap(payload);
                }
            }
            return Err(why);
        }
        made().insert(key.clone(), planned);
        if let Change::Part { parts, .. } = &mut change {
            parts.push(Box::new(MadePart { key, made: earlier }));
        }
        self.record(doc, label, change);
        // ⚠ Never folded into the next edit: indices change under a reshape.
        if let Some(document) = self.document_mut(doc) {
            document.history.seal();
        }
        Ok(())
    }
}

/// `made` made on the body at `path` read again, as a save makes them, so a change only its bytes
/// refuse is refused when it is made rather than at every save after it.
// ponytail: the body is read on the UI thread, as a cell's deletion is; a job if bounds grow.
fn tried(
    path: &Path,
    holder: BoundHolder,
    made: Vec<Made>,
    last: Vec<Option<(usize, usize)>>,
) -> Result<(), String> {
    let Some((archive, offset)) = crate::state::rpf9_scan::scanned_body(path) else {
        return Ok(());
    };
    let edit = move |_, s: &mut Rpf9Sections| write_made(s, holder, &made, &last);
    crate::state::gta6_edit::written(archive, offset, &edit).map(drop)
}

/// Where the changes a body of counts `now` holds not yet begin: the first that began from them,
/// `None` where it holds the counts they end at, `last`.
fn start(
    made: &[Made],
    now: &[Option<(usize, usize)>],
    last: &[Option<(usize, usize)>],
) -> Result<Option<usize>, String> {
    match made.iter().position(|m| m.began == now) {
        Some(from) => Ok(Some(from)),
        None if now == last => Ok(None),
        None => Err(tr!("The body read again holds other numbers of children, vertices or primitives than the collision's changes began from.").to_string()),
    }
}

/// The changes of `made` the body `s` holds not yet, made again in order.
fn write_made(
    s: &mut Rpf9Sections,
    holder: BoundHolder,
    made: &[Made],
    last: &[Option<(usize, usize)>],
) -> Result<(), String> {
    if made.is_empty() {
        return Ok(());
    }
    let now = counts(&holder.read(s).map_err(|e| e.to_string())?);
    let Some(from) = start(made, &now, last)? else {
        return Ok(());
    };
    for op in made[from..].iter().filter_map(|m| m.op.as_ref()) {
        reshape_bound_body(s, holder, op).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The changes a save of `document` makes again before its values are written in place.
pub(super) fn saved(document: &Document) -> Result<Vec<Made>, String> {
    let Some(path) = document.path.clone() else {
        return Ok(Vec::new());
    };
    let key = (path, document.id);
    let made = made().get(&key).cloned().unwrap_or_default();
    pending(&key, &made)
}

/// `made` made again on the bound `resource` holds in `s`, then each child it gives another type
/// laid out anew.
pub(super) fn write_saved(
    s: &mut Rpf9Sections,
    resource: &Resource,
    made: &[Made],
) -> Result<(), String> {
    if made.is_empty() {
        return Ok(());
    }
    let (Some(holder), Some(b)) = (holder_of(resource), bound_of(resource)) else {
        return Ok(());
    };
    write_made(s, holder, made, &counts(b))?;
    write_retyped(s, holder, b).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn step(began: Vec<Option<(usize, usize)>>) -> Made {
        Made { op: None, began }
    }

    #[test]
    fn a_save_makes_the_changes_from_the_first_that_began_from_the_body_s_counts() {
        let (a, b, c) = (vec![Some((4, 2))], vec![Some((4, 3))], vec![Some((5, 3))]);
        let list = [step(a.clone()), step(b.clone())];
        assert_eq!(start(&list, &a, &c), Ok(Some(0)));
        assert_eq!(start(&list, &b, &c), Ok(Some(1)));
        assert_eq!(start(&list, &c, &c), Ok(None), "a body saved with them");
        assert!(start(&list, &[None], &c).is_err(), "a body of other counts");
    }

    #[test]
    fn a_save_over_the_file_leaves_only_the_later_changes_to_make_again() {
        let (a, b) = (vec![Some((4, 2))], vec![Some((4, 1))]);
        let delete = Made {
            op: Some(BoundReshape::DeletePrimitive {
                child: 0,
                primitive: 0,
            }),
            began: a.clone(),
        };
        let add = Made {
            op: Some(BoundReshape::AddVertex { child: 0 }),
            began: b,
        };
        let both = [delete.clone(), add.clone()];
        assert_eq!(
            unsaved(&both, &[]),
            Ok(&both[..]),
            "a file never saved over"
        );
        assert_eq!(unsaved(&both, &both), Ok(&[][..]), "a file that holds both");
        assert_eq!(unsaved(&both, &both[..1]), Ok(&both[1..]));
        assert!(
            unsaved(&both[..1], &both).is_err(),
            "an undone change the file holds"
        );
    }

    #[test]
    fn a_child_slot_takes_the_child_out_and_puts_it_back() {
        let child = |x: f32| BoundChild {
            matrix: [[x; 4]; 4],
            flags: None,
            bound: None,
            material_id: None,
        };
        let bound = Bound {
            radius: 1.0,
            box_min: [0.0; 3],
            box_max: [1.0; 3],
            centroid: [0.5; 3],
            children: vec![child(1.0), child(2.0)],
            bvh: None,
        };
        let mut payload = DocPayload::Gta6Drawable(Arc::new(Resource::Bound(bound)));
        let first = |p: &mut DocPayload| {
            bound_mut(p).map(|b| (b.children.len(), b.children[0].matrix[0][0]))
        };
        let mut slot = ChildSlot {
            child: 0,
            held: None,
        };
        assert!(slot.swap(&mut payload));
        assert_eq!(first(&mut payload), Some((1, 2.0)));
        assert!(slot.swap(&mut payload));
        assert_eq!(first(&mut payload), Some((2, 1.0)));
        let mut past = ChildSlot {
            child: 5,
            held: None,
        };
        assert!(!past.swap(&mut payload), "no child there");
        assert_eq!(first(&mut payload), Some((2, 1.0)), "and nothing changed");
    }
}

//! A scanned GTA VI bound's and fragment's collision reshaped, each a step of undo: a fragment
//! child moved or hung off another group, a child's type and include entry, a triangle wired to
//! other vertices. A save writes them with the physics level over the body read again, in place.

use std::sync::Arc;

use or_io::gta6::headerless::{
    fragment_child_local_in, fragment_child_place, place_fragment_child, regroup_fragment_child,
    rewire_triangle, DrawableWithBound, Resource,
};

use super::gta6_physics_edit::{bound_mut, geometry_mut};
use super::{AppState, Change, DocPayload, DocumentId, Swap};

/// What a reshape of a GTA VI bound or fragment sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CollisionEdit {
    /// Fragment child `child`'s own rotation (x, y, z, w) and translation, local to its group's bone.
    ChildTransform {
        child: usize,
        rotation: [f32; 4],
        translation: [f32; 3],
    },
    /// Fragment child `child` hung off group `group`, last of its set there, kept where it lies.
    ChildGroup { child: usize, group: u16 },
    /// The type and include entry of fragment child `child` in a fragment, else of bound child `child`.
    ChildFlags { child: usize, flags: [u64; 2] },
    /// Triangle `primitive` of geometry child `child` over `vertices`.
    Rewire {
        child: usize,
        primitive: usize,
        vertices: [u16; 3],
    },
}

fn level_mut(payload: &mut DocPayload) -> Option<&mut DrawableWithBound> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    match Arc::get_mut(resource)? {
        Resource::DrawableWithBound(r) => Some(r),
        _ => None,
    }
}

/// A fragment child to hang off another group: its index, the group, its place among that group's
/// children of its set and the own transform it takes there. Once swapped, where it came from.
struct Regrouped {
    child: usize,
    group: u16,
    place: usize,
    rotation: [f32; 4],
    translation: [f32; 3],
}

impl Swap for Regrouped {
    fn swap(&mut self, payload: &mut DocPayload) -> bool {
        let Some(level) = level_mut(payload) else {
            return false;
        };
        let f = &level.fragment;
        let (Some(c), Some(place)) = (
            f.children.get(self.child),
            fragment_child_place(f, self.child),
        ) else {
            return false;
        };
        let (group, rotation, translation) = (c.group, c.rotation, c.translation);
        match regroup_fragment_child(
            level,
            self.child,
            self.group,
            self.place,
            self.rotation,
            self.translation,
        ) {
            Ok(now) => {
                *self = Regrouped {
                    child: now,
                    group,
                    place,
                    rotation,
                    translation,
                };
                true
            }
            Err(e) => {
                tracing::warn!("a fragment child's group change did not swap: {e}");
                false
            }
        }
    }
}

impl AppState {
    /// `edit` made to the GTA VI bound or fragment `doc` shows, as a step of its history.
    pub(crate) fn edit_gta6_collision(
        &mut self,
        doc: DocumentId,
        edit: CollisionEdit,
    ) -> Result<(), String> {
        let busy_fragment = || {
            tr!("The fragment is not edited while a task reads the body; try again once it ends.")
        };
        let busy_bound =
            || tr!("The bound is not edited while a task reads the body; try again once it ends.");
        let no_child = || tr!("The fragment holds no such child.");
        let bones = match edit {
            CollisionEdit::ChildTransform { .. } | CollisionEdit::ChildGroup { .. } => {
                let path = (self.document(doc))
                    .ok_or_else(|| tr!("its document is closed"))?
                    .path
                    .clone()
                    .ok_or_else(|| {
                        tr!("This document has no file, so it holds no GTA VI skeleton.")
                    })?;
                super::rpf9_scan::scanned_skeleton(&path)?.bones
            }
            _ => Vec::new(),
        };
        let document = (self.document_mut(doc)).ok_or_else(|| tr!("its document is closed"))?;
        let payload = &mut document.payload;
        let (label, change) = match edit {
            CollisionEdit::ChildTransform {
                child,
                rotation,
                translation,
            } => {
                let level = level_mut(payload).ok_or_else(busy_fragment)?;
                let c = level.fragment.children.get(child).ok_or_else(no_child)?;
                let (was, rank) = ((c.rotation, c.translation, c.matrix), c.composite);
                let rank = rank.map(usize::from);
                let held = (level.bound.as_ref()).map(|b| {
                    let m = rank.and_then(|r| b.children.get(r)).map(|c| c.matrix);
                    (b.radius, b.box_min, b.box_max, m)
                });
                place_fragment_child(level, &bones, child, rotation, translation)
                    .map_err(|e| e.to_string())?;
                let key = format!("gta6 fragment child {child} place");
                let mut change = Change::part(key, was.0, move |p| {
                    level_mut(p)?
                        .fragment
                        .children
                        .get_mut(child)
                        .map(|c| &mut c.rotation)
                })
                .and_part(was.1, move |p| {
                    (level_mut(p)?.fragment.children.get_mut(child)).map(|c| &mut c.translation)
                })
                .and_part(was.2, move |p| {
                    level_mut(p)?
                        .fragment
                        .children
                        .get_mut(child)
                        .map(|c| &mut c.matrix)
                });
                if let Some((radius, lo, hi, m)) = held {
                    change = change
                        .and_part(radius, |p| {
                            level_mut(p)?.bound.as_mut().map(|b| &mut b.radius)
                        })
                        .and_part(lo, |p| level_mut(p)?.bound.as_mut().map(|b| &mut b.box_min))
                        .and_part(hi, |p| level_mut(p)?.bound.as_mut().map(|b| &mut b.box_max));
                    if let (Some(m), Some(rank)) = (m, rank) {
                        change = change.and_part(m, move |p| {
                            (level_mut(p)?.bound.as_mut()?.children.get_mut(rank))
                                .map(|c| &mut c.matrix)
                        });
                    }
                }
                (tr!("Moved a fragment's child"), change)
            }
            CollisionEdit::ChildGroup { child, group } => {
                let level = level_mut(payload).ok_or_else(busy_fragment)?;
                let f = &level.fragment;
                let c = f.children.get(child).ok_or_else(no_child)?;
                let g = (f.groups.get(usize::from(group)))
                    .ok_or_else(|| tr!("The fragment holds no such group."))?;
                if c.group == group {
                    return Ok(());
                }
                let place = match c.composite {
                    Some(_) => g.children.len(),
                    None => g.second_set.len(),
                };
                let (rotation, translation) =
                    fragment_child_local_in(f, &bones, child, group).map_err(|e| e.to_string())?;
                let was = Regrouped {
                    child,
                    group: c.group,
                    place: fragment_child_place(f, child).ok_or_else(no_child)?,
                    rotation: c.rotation,
                    translation: c.translation,
                };
                let now = regroup_fragment_child(level, child, group, place, rotation, translation)
                    .map_err(|e| e.to_string())?;
                let step = Regrouped { child: now, ..was };
                let change = Change::Part {
                    key: None,
                    parts: vec![Box::new(step)],
                };
                (tr!("Gave a fragment's child another group"), change)
            }
            CollisionEdit::ChildFlags { child, flags } => {
                let key = format!("gta6 collision flags {child}");
                let change = match level_mut(payload) {
                    Some(level) => {
                        let c = level
                            .fragment
                            .children
                            .get_mut(child)
                            .ok_or_else(no_child)?;
                        let (was, rank) = (std::mem::replace(&mut c.flags, flags), c.composite);
                        let mut change = Change::part(key, was, move |p| {
                            level_mut(p)?
                                .fragment
                                .children
                                .get_mut(child)
                                .map(|c| &mut c.flags)
                        });
                        let rank = rank.map(usize::from);
                        let listed = match (rank, level.bound.as_mut()) {
                            (Some(rank), Some(b)) => b.children.get_mut(rank).map(|c| (rank, c)),
                            _ => None,
                        };
                        if let Some((rank, held)) = listed {
                            let was = held.flags.replace(flags);
                            change = change.and_part(was, move |p| {
                                (level_mut(p)?.bound.as_mut()?.children.get_mut(rank))
                                    .map(|c| &mut c.flags)
                            });
                        }
                        change
                    }
                    None => {
                        let b = bound_mut(payload).ok_or_else(busy_bound)?;
                        let held = (b.children.get_mut(child))
                            .ok_or_else(|| tr!("The bound holds no such child."))?;
                        if held.flags.is_none() {
                            return Err(tr!("The composite states no type and include entries.")
                                .to_string());
                        }
                        let was = held.flags.replace(flags);
                        Change::part(key, was, move |p| {
                            bound_mut(p)?.children.get_mut(child).map(|c| &mut c.flags)
                        })
                    }
                };
                (tr!("Set a child's type and include entry"), change)
            }
            CollisionEdit::Rewire {
                child,
                primitive,
                vertices,
            } => {
                let g = geometry_mut(payload, child).ok_or_else(busy_bound)?;
                let was = g.primitives.clone();
                rewire_triangle(g, primitive, vertices).map_err(|e| e.to_string())?;
                let key = format!("gta6 bound rewire {child} {primitive}");
                let change = Change::part(key, was, move |p| {
                    geometry_mut(p, child).map(|g| &mut g.primitives)
                });
                (tr!("Rewired a triangle"), change)
            }
        };
        self.record(doc, label, change);
        Ok(())
    }
}

#[cfg(test)]
#[path = "gta6_collision_edit_tests.rs"]
mod tests;

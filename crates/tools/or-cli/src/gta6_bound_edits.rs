//! `rpf scan-bound-edits`: a scanned GTA VI bound's, drawable's or fragment's collision given a
//! vertex, a primitive and a copied child, each then deleted, in memory; whether the bound reads as
//! stored again.

use or_io::container::rpf::{Rpf9BodyKind, Rpf9Role, Rpf9Sections};
use or_io::gta6::headerless::{
    reshape_bound_body, BoundHolder, BoundReshape, ChildBound, Primitive,
};

use super::gta6_inverse::{said, sections, verdict, Outcome, READ};

const USAGE: &str = "usage: rpf scan-bound-edits <archive.rpf> <offset> [--limit <bytes>]";

/// No neighbour across a triangle's edge, and that edge's byte.
const OPEN: u16 = 0xFFFF;
const OPEN_EDGE: u8 = 0xFF;

pub(crate) fn cmd_scan_bound_edits(args: &[String]) -> Result<(), String> {
    let (find, body) = sections(args, USAGE)?;
    let role = match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    };
    let holder = match role {
        Some(Rpf9Role::Bound) => BoundHolder::Bound,
        Some(Rpf9Role::Drawable) => BoundHolder::Drawable,
        Some(Rpf9Role::DrawableWithBound) => BoundHolder::Fragment,
        _ => return Err("the body is no bound, drawable or fragment".to_string()),
    };
    let stored = holder
        .read(&body)
        .map_err(|e| format!("the bound does not read: {e}"))?;
    let shown = format!("{stored:?}");
    let geometry = (stored.children.iter().enumerate()).find_map(|(i, c)| {
        match c.bound.as_ref()?.as_ref().ok()? {
            ChildBound::Geometry(g) if g.bvh.is_some() => Some((i, g)),
            _ => None,
        }
    });
    let primitive_child = (stored.children.iter().enumerate()).find_map(|(i, c)| {
        match c.bound.as_ref()?.as_ref().ok()? {
            ChildBound::Geometry(_) => None,
            _ => Some(i),
        }
    });
    let no_geometry = || Outcome::Refused("no child is a geometry with a BVH".to_string());
    let mut held = true;

    let what = "a vertex added to a geometry, then deleted";
    let outcome = match geometry {
        Some((child, g)) => inverse(
            &body,
            holder,
            &shown,
            BoundReshape::AddVertex { child },
            BoundReshape::DeleteVertex {
                child,
                vertex: g.vertices.len(),
            },
        ),
        None => no_geometry(),
    };
    held &= said(what, outcome);

    let what = "a geometry's last primitive copied, then the copy deleted";
    let outcome = match geometry {
        Some((child, g)) => match g.primitives.last() {
            Some(last) => inverse(
                &body,
                holder,
                &shown,
                BoundReshape::AddPrimitive {
                    child,
                    record: unlinked(*last),
                    material: g.primitive_materials.last().copied().unwrap_or(0),
                },
                BoundReshape::DeletePrimitive {
                    child,
                    primitive: g.primitives.len(),
                },
            ),
            None => Outcome::Refused("the geometry holds no primitive".to_string()),
        },
        None => no_geometry(),
    };
    held &= said(what, outcome);

    let what = "a primitive child copied, then the copy deleted";
    let outcome = match primitive_child {
        Some(child) => inverse(
            &body,
            holder,
            &shown,
            BoundReshape::CopyChild { child },
            BoundReshape::DeleteChild {
                child: stored.children.len(),
            },
        ),
        None => Outcome::Refused("no child is a primitive that reads".to_string()),
    };
    held &= said(what, outcome);
    verdict(held)
}

/// `p` as a primitive added beside it is written: a triangle linked to no neighbour.
fn unlinked(p: Primitive) -> Primitive {
    match p {
        Primitive::Triangle { vertices, .. } => Primitive::Triangle {
            vertices,
            neighbours: [OPEN; 3],
            edges: [OPEN_EDGE; 3],
        },
        other => other,
    }
}

/// `edit`, then `undo`, made to the bound `holder` names in `body`; whether it reads as `shown`.
// A bound holds results of reads, so what it reads as is compared as its printed form.
fn inverse(
    body: &Rpf9Sections,
    holder: BoundHolder,
    shown: &str,
    edit: BoundReshape,
    undo: BoundReshape,
) -> Outcome {
    let mut work = body.clone();
    if let Err(e) = reshape_bound_body(&mut work, holder, &edit) {
        return Outcome::Refused(e.to_string());
    }
    if let Err(e) = reshape_bound_body(&mut work, holder, &undo) {
        return Outcome::Differs(format!("the inverse is refused: {e}"));
    }
    match holder.read(&work) {
        Ok(again) if format!("{again:?}") == shown => Outcome::Same(READ),
        Ok(_) => Outcome::Differs("the bound reads otherwise than stored".to_string()),
        Err(e) => Outcome::Differs(format!("the bound does not read: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_triangle_is_linked_to_no_neighbour_and_other_shapes_are_kept() {
        let t = Primitive::Triangle {
            vertices: [0, 1, 2],
            neighbours: [4, OPEN, 7],
            edges: [3, OPEN_EDGE, 9],
        };
        let expected = Primitive::Triangle {
            vertices: [0, 1, 2],
            neighbours: [OPEN; 3],
            edges: [OPEN_EDGE; 3],
        };
        assert_eq!(unlinked(t), expected);
        let s = Primitive::Sphere {
            centre: 3,
            radius: 0.5,
        };
        assert_eq!(unlinked(s), s);
    }
}

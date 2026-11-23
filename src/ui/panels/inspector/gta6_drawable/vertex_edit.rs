//! A scanned GTA VI bound's vertex moved: the selected geometry child's vertex chosen by number,
//! then its position in the child's frame, each change a step of undo.

use std::sync::Arc;

use or_io::gta6::headerless::{Bound, BoundChild, ChildBound, Resource};

use super::*;
use crate::state::gta6_bound::selected_child;

/// The child of an embedded bound the collision editor chose, and that child.
fn embedded<'b>(
    ui: &egui::Ui,
    doc_id: DocumentId,
    b: &'b Bound,
) -> Option<(usize, &'b BoundChild)> {
    let child = super::bound_place_edit::embedded_child(ui, doc_id);
    Some((child, b.children.get(child)?))
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    selection: Option<u64>,
) {
    let Some(DocPayload::Gta6Drawable(resource)) = state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let resource = Arc::clone(resource);
    // A drawable's or a fragment's bound edits the child its collision editor chose.
    let Some((child, c)) = (match &*resource {
        Resource::Bound(b) => selected_child(b, selection),
        Resource::Drawable(d) => d.bound.as_ref().and_then(|b| embedded(ui, doc_id, b)),
        Resource::DrawableWithBound(r) => r.bound.as_ref().and_then(|b| embedded(ui, doc_id, b)),
        _ => None,
    }) else {
        return;
    };
    let Some(Ok(ChildBound::Geometry(g))) = &c.bound else {
        return;
    };
    let Some(last) = g.vertices.len().checked_sub(1).filter(|_| g.bvh.is_some()) else {
        return;
    };
    let id = egui::Id::new(("gta6_bound_vertex", doc_id, child));
    let mut vertex = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(last);
    let mut moved = None;
    egui::CollapsingHeader::new(tr!("Move a vertex"))
        .id_salt(("gta6_bound_vertex_edit", doc_id))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr!("Save As stores a moved vertex to the nearest quantum, fits the BVH leaves of the primitives that name it and their parents again, and raises the child's radius to reach it. A move past the child's box grows the box, stores every vertex again in it, fits every leaf again and grows the composite's box to hold it. A triangle's edge bytes are kept as read, as what they hold is not known."))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            ui.horizontal(|ui| {
                ui.label(tr!("Vertex"));
                if drag_value(ui, &mut vertex, |d| d.range(0..=last)).inner {
                    ui.data_mut(|d| d.insert_temp(id, vertex));
                }
            });
            let Some(mut at) = g.vertices.get(vertex).copied() else {
                return;
            };
            ui.horizontal(|ui| {
                ui.label(tr!("Position"));
                let mut edited = false;
                for x in at.iter_mut() {
                    edited |= drag_value(ui, x, |d| d.speed(0.01).max_decimals(4)).inner;
                }
                if edited {
                    moved = Some((vertex, at));
                }
            });
        });
    // ⚠ A move needs the body alone: this panel's hold on it goes first.
    drop(resource);
    if let Some((vertex, at)) = moved {
        if let Err(why) = state.move_gta6_bound_vertex(doc_id, child, vertex, at) {
            state.show_warning_toast(why);
        }
    }
}

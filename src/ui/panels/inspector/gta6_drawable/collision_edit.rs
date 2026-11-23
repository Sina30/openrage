//! A scanned GTA VI bound's and fragment's collision reshaped: a fragment child's own transform and
//! group, a child's type and include entry, a geometry's triangle wired to other vertices; each
//! change a step of undo.

use std::sync::Arc;

use glam::{EulerRot, Quat};
use or_io::gta6::headerless::{Bound, ChildBound, Fragment, Primitive, Resource};

use super::bound_place_edit::{embedded_child, triple};
use super::*;
use crate::state::gta6_bound::selected_child;
use crate::state::gta6_collision_edit::CollisionEdit;

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
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
    let mut edits = Vec::new();
    let shown = matches!(
        &*resource,
        Resource::Bound(_) | Resource::Drawable(_) | Resource::DrawableWithBound(_)
    );
    if shown {
        egui::CollapsingHeader::new(tr!("Reshape the collision"))
            .id_salt(("gta6_collision_reshape", doc_id))
            .show(ui, |ui| {
                note(
                    ui,
                    tr!("A fragment child moves by its own transform and its composite's matrix follows; a group change keeps where it lies; a rewired triangle links to the triangles it shares an edge with. Save As writes the body as a loose resource."),
                );
                match &*resource {
                    Resource::Bound(b) => {
                        if let Some((child, _)) = selected_child(b, selection) {
                            bound_child(ui, doc_id, b, child, true, &mut edits);
                        }
                    }
                    Resource::Drawable(d) => {
                        if let Some(b) = &d.bound {
                            let child = embedded_child(ui, doc_id);
                            bound_child(ui, doc_id, b, child, true, &mut edits);
                        }
                    }
                    Resource::DrawableWithBound(r) => {
                        fragment(ui, doc_id, &r.fragment, &mut edits);
                        if let Some(b) = &r.bound {
                            let child = embedded_child(ui, doc_id);
                            bound_child(ui, doc_id, b, child, false, &mut edits);
                        }
                    }
                    _ => {}
                }
            });
    }
    // ⚠ An edit needs the body alone: this panel's hold on it goes first.
    drop(resource);
    for edit in edits {
        if let Err(why) = state.edit_gta6_collision(doc_id, edit) {
            state.show_warning_toast(why);
        }
    }
}

/// A type and include entry as two hexadecimal numbers, drafted under `salt`; the entry to set once
/// both read and Apply is clicked.
fn entry(ui: &mut egui::Ui, salt: impl std::hash::Hash, flags: [u64; 2]) -> Option<[u64; 2]> {
    // Keyed by the entry too, so a draft is dropped once the entry changes.
    let id = egui::Id::new(("gta6_collision_entry", salt, flags));
    let mut draft = (ui.data(|d| d.get_temp::<[String; 2]>(id)))
        .unwrap_or_else(|| flags.map(|w| format!("{w:016X}")));
    let mut set = None;
    ui.horizontal(|ui| {
        ui.label(tr!("Type and include entry"));
        for word in &mut draft {
            ui.add(
                egui::TextEdit::singleline(word)
                    .desired_width(140.0)
                    .font(egui::TextStyle::Monospace),
            );
        }
        let read = |w: &String| u64::from_str_radix(w.trim().trim_start_matches("0x"), 16).ok();
        let parsed = read(&draft[0]).zip(read(&draft[1])).map(|(a, b)| [a, b]);
        let changed = parsed.is_some_and(|p| p != flags);
        if ui
            .add_enabled(changed, egui::Button::new(tr!("Apply")))
            .clicked()
        {
            set = parsed;
        }
    });
    ui.data_mut(|d| d.insert_temp(id, draft));
    set
}

/// The chosen fragment child's position, turn, group and type and include entry.
fn fragment(ui: &mut egui::Ui, doc_id: DocumentId, f: &Fragment, edits: &mut Vec<CollisionEdit>) {
    let Some(last) = f.children.len().checked_sub(1) else {
        return;
    };
    let id = egui::Id::new(("gta6_reshape_fragment_child", doc_id));
    let mut child = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(last);
    ui.horizontal(|ui| {
        ui.label(tr!("Fragment child"));
        if drag_value(ui, &mut child, |d| d.range(0..=last)).inner {
            ui.data_mut(|d| d.insert_temp(id, child));
        }
    });
    let Some(c) = f.children.get(child) else {
        return;
    };
    let mut at = c.translation;
    if triple(ui, tr!("Position"), &mut at, 0.01) {
        edits.push(CollisionEdit::ChildTransform {
            child,
            rotation: c.rotation,
            translation: at,
        });
    }
    let [x, y, z, w] = c.rotation;
    let (ax, ay, az) = Quat::from_xyzw(x, y, z, w).to_euler(EulerRot::XYZ);
    let mut turn = [ax, ay, az].map(f32::to_degrees);
    if triple(ui, tr!("Rotation (degrees)"), &mut turn, 0.5) {
        let [ax, ay, az] = turn.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::XYZ, ax, ay, az).normalize();
        edits.push(CollisionEdit::ChildTransform {
            child,
            rotation: rotation.to_array(),
            translation: c.translation,
        });
    }
    if let Some(top) = (f.groups.len().checked_sub(1)).and_then(|l| u16::try_from(l).ok()) {
        ui.horizontal(|ui| {
            ui.label(tr!("Group"));
            let mut group = c.group;
            if drag_value(ui, &mut group, |d| d.range(0..=top)).inner {
                edits.push(CollisionEdit::ChildGroup { child, group });
            }
        });
    }
    if let Some(flags) = entry(ui, ("fragment", doc_id, child), c.flags) {
        edits.push(CollisionEdit::ChildFlags { child, flags });
    }
}

/// Bound child `child`'s type and include entry where `flags_too`, and for a geometry with a BVH,
/// a triangle wired to three of its vertices.
fn bound_child(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    b: &Bound,
    child: usize,
    flags_too: bool,
    edits: &mut Vec<CollisionEdit>,
) {
    let Some(c) = b.children.get(child) else {
        return;
    };
    if let Some(flags) = c.flags.filter(|_| flags_too) {
        if let Some(flags) = entry(ui, ("bound", doc_id, child), flags) {
            edits.push(CollisionEdit::ChildFlags { child, flags });
        }
    }
    let Some(Ok(ChildBound::Geometry(g))) = &c.bound else {
        return;
    };
    if g.bvh.is_none() {
        note(
            ui,
            tr!("A geometry with no BVH is not reshaped: it carries a shrunk copy of each vertex, and how far a vertex is shrunk is not known."),
        );
        return;
    }
    let Some(last) = g.primitives.len().checked_sub(1) else {
        return;
    };
    let own = |p: usize| match g.primitives.get(p) {
        Some(Primitive::Triangle { vertices, .. }) => *vertices,
        _ => [0, 1, 2],
    };
    let id = egui::Id::new(("gta6_rewire", doc_id, child));
    let (mut primitive, mut vertices) =
        (ui.data(|d| d.get_temp::<(usize, [u16; 3])>(id))).unwrap_or_else(|| (0, own(0)));
    primitive = primitive.min(last);
    let top = u16::try_from(g.vertices.len().saturating_sub(1)).unwrap_or(u16::MAX);
    let mut rewire = false;
    ui.horizontal(|ui| {
        ui.label(tr!("Primitive"));
        if drag_value(ui, &mut primitive, |d| d.range(0..=last)).inner {
            vertices = own(primitive);
        }
        ui.label(tr!("Vertices"));
        for v in &mut vertices {
            drag_value(ui, v, |d| d.range(0..=top));
        }
        rewire = ui.button(tr!("Rewire")).clicked();
    });
    ui.data_mut(|d| d.insert_temp(id, (primitive, vertices)));
    if rewire {
        edits.push(CollisionEdit::Rewire {
            child,
            primitive,
            vertices,
        });
    }
}

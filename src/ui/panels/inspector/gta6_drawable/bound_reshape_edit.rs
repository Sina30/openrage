//! A scanned GTA VI bound's collision added to and taken from: a geometry's primitives and
//! vertices, a lone bound's children, a primitive child's type; each change a step of undo.

use std::sync::Arc;

use or_io::gta6::headerless::{Bound, BoundGeometry, ChildBound, Resource};

use super::bound_place_edit::{embedded_child, triple};
use super::*;
use crate::state::gta6_bound::selected_child;
use crate::state::gta6_physics_edit::bound_reshape::ReshapeEdit;

/// The types a primitive child is given, by type byte.
const TYPES: [(u8, &str); 6] = [
    (0, tr_key!("Sphere")),
    (1, tr_key!("Capsule")),
    (2, tr_key!("Tapered capsule")),
    (3, tr_key!("Box")),
    (8, tr_key!("Disc")),
    (9, tr_key!("Cylinder")),
];

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
    let (b, lone, child) = match &*resource {
        Resource::Bound(b) => match selected_child(b, selection) {
            Some((child, _)) => (b, true, child),
            None => return,
        },
        Resource::Drawable(d) => match &d.bound {
            Some(b) => (b, false, embedded_child(ui, doc_id)),
            None => return,
        },
        Resource::DrawableWithBound(r) => match &r.bound {
            Some(b) => (b, false, embedded_child(ui, doc_id)),
            None => return,
        },
        _ => return,
    };
    let mut edits = Vec::new();
    egui::CollapsingHeader::new(tr!("Add and delete collision"))
        .id_salt(("gta6_collision_grow", doc_id))
        .show(ui, |ui| {
            note(
                ui,
                tr!("A primitive or a vertex is added last and deleted where it stands, a new triangle linking to the triangles it shares an edge with; a lone bound's child is copied or deleted, a primitive child given another type. Save As writes the body as a loose resource."),
            );
            child_rows(ui, doc_id, b, child, lone, &mut edits);
        });
    // ⚠ An edit needs the body alone: this panel's hold on it goes first.
    drop(resource);
    for edit in edits {
        if let Err(why) = state.reshape_gta6_bound(doc_id, edit) {
            state.show_warning_toast(why);
        }
    }
}

/// Child `child`'s copy and delete where the bound is a lone bound's composite, then its geometry's
/// primitives and vertices or its type.
fn child_rows(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    b: &Bound,
    child: usize,
    lone: bool,
    edits: &mut Vec<ReshapeEdit>,
) {
    let Some(c) = b.children.get(child) else {
        return;
    };
    if lone {
        ui.horizontal(|ui| {
            if ui.button(tr!("Copy the child")).clicked() {
                edits.push(ReshapeEdit::CopyChild { child });
            }
            if ui.button(tr!("Delete the child")).clicked() {
                edits.push(ReshapeEdit::DeleteChild { child });
            }
        });
    }
    match &c.bound {
        Some(Ok(ChildBound::Geometry(g))) => geometry_rows(ui, doc_id, child, g, edits),
        Some(Ok(shape)) => type_row(ui, doc_id, child, shape, edits),
        _ => {}
    }
}

/// A primitive shape's type byte; `None` for a geometry.
fn kind_of(shape: &ChildBound) -> Option<u8> {
    Some(match shape {
        ChildBound::Geometry(_) => return None,
        ChildBound::Sphere { .. } => 0,
        ChildBound::Capsule { .. } => 1,
        ChildBound::TaperedCapsule { .. } => 2,
        ChildBound::Box { .. } => 3,
        ChildBound::Disc { .. } => 8,
        ChildBound::Cylinder { .. } => 9,
        ChildBound::Hull { .. } => 13,
        ChildBound::Frustum { .. } => 14,
    })
}

fn type_row(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    child: usize,
    shape: &ChildBound,
    edits: &mut Vec<ReshapeEdit>,
) {
    let now = kind_of(shape);
    let shown = (TYPES.iter()).find(|(k, _)| Some(*k) == now).map_or_else(
        || now.unwrap_or_default().to_string(),
        |&(_, name)| crate::i18n::tr(name).to_string(),
    );
    ui.horizontal(|ui| {
        ui.label(tr!("Type"));
        egui::ComboBox::from_id_salt(("gta6_child_type", doc_id, child))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (kind, name) in TYPES {
                    let chosen = ui
                        .selectable_label(Some(kind) == now, crate::i18n::tr(name))
                        .clicked();
                    if chosen && Some(kind) != now {
                        edits.push(ReshapeEdit::ChildType { child, kind });
                    }
                }
            });
    });
}

/// A geometry's primitive deleted, a triangle over three of its vertices added, a vertex deleted
/// and one added at a position.
fn geometry_rows(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    child: usize,
    g: &BoundGeometry,
    edits: &mut Vec<ReshapeEdit>,
) {
    if g.bvh.is_none() {
        note(
            ui,
            tr!("A geometry with no BVH is not reshaped: it carries a shrunk copy of each vertex, and how far a vertex is shrunk is not known."),
        );
        return;
    }
    let id = egui::Id::new(("gta6_collision_grow", doc_id, child));
    let (mut primitive, mut vertices, mut vertex, mut at) =
        (ui.data(|d| d.get_temp::<(usize, [u16; 3], usize, [f32; 3])>(id))).unwrap_or_default();
    let last_primitive = g.primitives.len().saturating_sub(1);
    let last_vertex = g.vertices.len().saturating_sub(1);
    (primitive, vertex) = (primitive.min(last_primitive), vertex.min(last_vertex));
    let top = u16::try_from(last_vertex).unwrap_or(u16::MAX);
    ui.horizontal(|ui| {
        ui.label(tr!("Primitive"));
        drag_value(ui, &mut primitive, |d| d.range(0..=last_primitive));
        if ui.button(tr!("Delete the primitive")).clicked() {
            edits.push(ReshapeEdit::DeletePrimitive { child, primitive });
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("Vertices"));
        for v in &mut vertices {
            drag_value(ui, v, |d| d.range(0..=top));
        }
        if ui.button(tr!("Add a triangle")).clicked() {
            let material = g.primitive_materials.get(primitive).copied().unwrap_or(0);
            edits.push(ReshapeEdit::AddTriangle {
                child,
                vertices,
                material,
            });
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("Vertex"));
        drag_value(ui, &mut vertex, |d| d.range(0..=last_vertex));
        if ui.button(tr!("Delete the vertex")).clicked() {
            edits.push(ReshapeEdit::DeleteVertex { child, vertex });
        }
    });
    triple(ui, tr!("Position"), &mut at, 0.01);
    if ui.button(tr!("Add a vertex")).clicked() {
        edits.push(ReshapeEdit::AddVertex {
            child,
            position: at,
        });
    }
    ui.data_mut(|d| d.insert_temp(id, (primitive, vertices, vertex, at)));
}

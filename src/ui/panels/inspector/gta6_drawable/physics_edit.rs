//! A scanned GTA VI bound's, fragment's and navigation cell's values edited in place as GTA V's
//! editors edit theirs: a child's or a primitive's material key, a primitive child's dimensions,
//! the masses, a polygon's flags; each change a step of undo.

use std::sync::Arc;

use or_io::gta6::headerless::{
    material_key, Bound, BoundChild, BoundGeometry, ChildBound, Fragment, Resource,
};

use super::*;
use crate::state::gta6_bound::selected_child;
use crate::state::gta6_physics_edit::{dimensions, BoundEdit, FragmentEdit};

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

/// A length or a mass, dragged at `speed`; whether it was edited.
fn amount(ui: &mut egui::Ui, value: &mut f32, speed: f64) -> bool {
    drag_value(ui, value, |d| {
        d.speed(speed).max_decimals(4).range(0.0..=f32::MAX)
    })
    .inner
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
    forget_stale_rows(ui, state, doc_id);
    let (mut bound_edits, mut fragment_edits) = (Vec::new(), Vec::new());
    match &*resource {
        Resource::Bound(b) => {
            bound(ui, doc_id, b, selection, &mut bound_edits);
            let child = selected_child(b, selection).map(|(i, _)| i);
            super::bound_place_edit::placement(ui, doc_id, b, child, &mut bound_edits);
        }
        Resource::Drawable(d) => {
            if let Some(b) = &d.bound {
                super::bound_place_edit::embedded(ui, doc_id, b, &mut bound_edits);
            }
        }
        Resource::DrawableWithBound(r) => {
            fragment(ui, doc_id, &r.fragment, &mut fragment_edits);
            if let Some(b) = &r.bound {
                super::bound_place_edit::embedded(ui, doc_id, b, &mut bound_edits);
            }
        }
        _ => {}
    }
    // ⚠ An edit needs the body alone: this panel's hold on it goes first.
    drop(resource);
    for edit in bound_edits {
        if let Err(why) = state.edit_gta6_bound(doc_id, edit) {
            state.show_warning_toast(why);
        }
    }
    for edit in fragment_edits {
        if let Err(why) = state.edit_gta6_fragment(doc_id, edit) {
            state.show_warning_toast(why);
        }
    }
}

/// The bound grid's rows, kept by the body they were built from, dropped once the document's
/// history moves, which an edit in place or an undo does without making another body.
fn forget_stale_rows(ui: &egui::Ui, state: &AppState, doc_id: DocumentId) {
    type Grid = ((usize, Option<u64>), Arc<(String, Vec<(String, String)>)>);
    let at = state.document(doc_id).map(|d| d.history.at());
    let seen = egui::Id::new(("gta6_physics_history", doc_id));
    if ui.data(|d| d.get_temp::<Option<u64>>(seen)) == Some(at) {
        return;
    }
    ui.data_mut(|d| {
        d.insert_temp(seen, at);
        d.remove::<Grid>(egui::Id::new(("gta6_bound_grid", doc_id)));
    });
}

/// The selected child's material key and dimensions, or each of its primitives' material.
fn bound(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    b: &Bound,
    selection: Option<u64>,
    edits: &mut Vec<BoundEdit>,
) {
    if let Some((child, c)) = selected_child(b, selection) {
        child_editor(ui, doc_id, child, c, edits);
    }
}

/// Child `child`'s material key and dimensions, or each of its primitives' material.
pub(super) fn child_editor(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    child: usize,
    c: &BoundChild,
    edits: &mut Vec<BoundEdit>,
) {
    let Some(Ok(shape)) = &c.bound else {
        return;
    };
    egui::CollapsingHeader::new(tr!("Edit the child"))
        .id_salt(("gta6_bound_edit", doc_id))
        .show(ui, |ui| {
            note(
                ui,
                tr!("A key goes to a material the body holds and a child keeps its type, so nothing moves; Save As writes the body as a loose resource."),
            );
            if let ChildBound::Geometry(g) = shape {
                polygons(ui, doc_id, child, g, edits);
                return;
            }
            if let Some(id) = c.material_id {
                ui.horizontal(|ui| {
                    ui.label(tr!("Material key"));
                    let mut key = material_key(id);
                    let edited = drag_value(ui, &mut key, |d| {
                        d.hexadecimal(4, false, true).range(0..=0x7FFF)
                    });
                    if edited.inner {
                        edits.push(BoundEdit::ChildKey { child, key });
                    }
                });
            }
            let shown = dimensions(shape);
            let mut values: Vec<f32> = shown.iter().map(|(_, v)| *v).collect();
            let mut changed = false;
            for (value, (name, _)) in values.iter_mut().zip(&shown) {
                ui.horizontal(|ui| {
                    ui.label(*name);
                    changed |= amount(ui, value, 0.01);
                });
            }
            if changed {
                edits.push(BoundEdit::Dimensions { child, values });
            }
        });
}

/// Each primitive of geometry child `child`, the material it names chosen among the geometry's.
fn polygons(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    child: usize,
    g: &BoundGeometry,
    edits: &mut Vec<BoundEdit>,
) {
    if g.primitive_materials.is_empty() {
        note(
            ui,
            tr!("This geometry names no material for its primitives."),
        );
        return;
    }
    let mut keys: Vec<u16> = g.material_ids.iter().map(|id| material_key(*id)).collect();
    keys.sort_unstable();
    keys.dedup();
    let row_height = ui.spacing().interact_size.y;
    // ⚠ Only the rows in view: a geometry may hold thousands of primitives.
    egui::ScrollArea::vertical()
        .id_salt(("gta6_bound_polygons", doc_id, child))
        .max_height(row_height * ROWS_IN_VIEW)
        .auto_shrink([false, true])
        .show_rows(ui, row_height, g.primitive_materials.len(), |ui, range| {
            for primitive in range {
                let current = g.primitive_key(primitive);
                let mut chosen = current;
                ui.horizontal(|ui| {
                    ui.label(tr!("Primitive {index} material", index = primitive));
                    let text = current.map_or_else(String::new, |key| format!("{key:#06X}"));
                    egui::ComboBox::from_id_salt(("gta6_polygon_key", doc_id, child, primitive))
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            for key in &keys {
                                let text = format!("{key:#06X}");
                                ui.selectable_value(&mut chosen, Some(*key), text);
                            }
                        });
                });
                if let Some(key) = chosen.filter(|_| chosen != current) {
                    edits.push(BoundEdit::PolygonKey {
                        child,
                        primitive,
                        key,
                    });
                }
            }
        });
}

/// The archetype's mass and angular inertia, and each child's.
fn fragment(ui: &mut egui::Ui, doc_id: DocumentId, f: &Fragment, edits: &mut Vec<FragmentEdit>) {
    let Some(archetype) = f.archetype else {
        return;
    };
    egui::CollapsingHeader::new(tr!("Edit the physics"))
        .id_salt(("gta6_fragment_edit", doc_id))
        .show(ui, |ui| {
            note(
                ui,
                tr!("The children the composite lists weigh what the archetype weighs, so a change to one changes the other; nothing moves, and Save As writes the body as a loose resource."),
            );
            ui.horizontal(|ui| {
                ui.label(tr!("Archetype mass (kg)"));
                let mut mass = archetype.mass;
                if amount(ui, &mut mass, 0.1) {
                    edits.push(FragmentEdit::Mass(mass));
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr!("Archetype angular inertia"));
                let mut inertia = archetype.angular_inertia;
                // Every lane is drawn, so `|` and not `||`.
                if inertia.iter_mut().fold(false, |any, x| amount(ui, x, 0.1) | any) {
                    edits.push(FragmentEdit::Inertia(inertia));
                }
            });
            let row_height = ui.spacing().interact_size.y;
            egui::ScrollArea::vertical()
                .id_salt(("gta6_fragment_children", doc_id))
                .max_height(row_height * ROWS_IN_VIEW)
                .auto_shrink([false, true])
                .show_rows(ui, row_height, f.children.len(), |ui, range| {
                    let shown = f.children.iter().enumerate().skip(range.start);
                    for (child, c) in shown.take(range.len()) {
                        ui.horizontal(|ui| {
                            ui.label(tr!("Child {index} mass (kg)", index = child));
                            let mut mass = c.mass;
                            if amount(ui, &mut mass, 0.1) {
                                edits.push(FragmentEdit::ChildMass { child, mass });
                            }
                            ui.label(tr!("angular inertia"));
                            let mut inertia = c.angular_inertia;
                            if inertia.iter_mut().fold(false, |any, x| amount(ui, x, 0.1) | any) {
                                edits.push(FragmentEdit::ChildInertia { child, inertia });
                            }
                        });
                    }
                });
        });
}

/// The flags of polygon `polygon` of the scanned GTA VI cell `doc_id`, edited as a word.
pub(super) fn draw_cell(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    polygon: Option<usize>,
) {
    let Some(polygon) = polygon else {
        return;
    };
    let Some(DocPayload::Resource(or_io::Asset::NavMesh(mesh))) =
        state.document(doc_id).map(|d| &d.payload)
    else {
        return;
    };
    let held = (mesh.polys_array.as_deref()).and_then(|l| l.iter().nth(polygon));
    let Some(mut flags) = held.map(|p| p.as_integer1()) else {
        return;
    };
    let mut edited = false;
    ui.horizontal(|ui| {
        ui.label(tr!("Polygon flags"));
        edited = drag_value(ui, &mut flags, |d| d.hexadecimal(8, false, true)).inner;
    });
    note(
        ui,
        tr!("The bits that hold GTA V's corner count in this view keep the cell's own; Save As writes the cell as a loose resource."),
    );
    if edited {
        if let Err(why) = state.set_gta6_polygon_flags(doc_id, polygon, flags) {
            state.show_warning_toast(why);
        }
    }
}

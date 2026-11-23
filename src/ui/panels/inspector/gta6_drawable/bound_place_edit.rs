//! A GTA VI bound's placements edited as GTA V's grid edits them: a child's position and turn, the
//! bound's own box, radius and centroid; and the bound a drawable or a fragment embeds, its child
//! chosen by number.

use glam::{EulerRot, Mat3, Quat, Vec3};
use or_io::gta6::headerless::Bound;

use super::*;
use crate::state::gta6_physics_edit::BoundEdit;

/// Three numbers in a row after `label`, dragged at `speed`; whether one was edited.
fn triple(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64) -> bool {
    let mut edited = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for x in values.iter_mut() {
            edited |= drag_value(ui, x, |d| d.speed(speed).max_decimals(4)).inner;
        }
    });
    edited
}

/// A child's turn in degrees about x, y and z: its matrix's first three rows are its axes.
fn degrees(m: &[[f32; 4]; 4]) -> [f32; 3] {
    let axis = |r: [f32; 4]| Vec3::new(r[0], r[1], r[2]);
    let q = Quat::from_mat3(&Mat3::from_cols(axis(m[0]), axis(m[1]), axis(m[2])));
    let (x, y, z) = q.to_euler(EulerRot::XYZ);
    [x, y, z].map(f32::to_degrees)
}

/// `m` turned to `turn` degrees about x, y and z; each row's fourth lane kept.
fn turned(m: &[[f32; 4]; 4], [x, y, z]: [f32; 3]) -> [[f32; 4]; 4] {
    let q = Quat::from_euler(
        EulerRot::XYZ,
        x.to_radians(),
        y.to_radians(),
        z.to_radians(),
    );
    let axes = Mat3::from_quat(q.normalize());
    let mut out = *m;
    for (r, row) in out.iter_mut().take(3).enumerate() {
        let a = axes.col(r);
        row[..3].copy_from_slice(&[a.x, a.y, a.z]);
    }
    out
}

/// Child `child`'s position and turn, then the bound's own box, radius and centroid.
pub(super) fn placement(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    b: &Bound,
    child: Option<usize>,
    edits: &mut Vec<BoundEdit>,
) {
    egui::CollapsingHeader::new(tr!("Edit the placement"))
        .id_salt(("gta6_bound_placement", doc_id))
        .show(ui, |ui| {
            note(
                ui,
                tr!("Every child's box, placed by its matrix, stays inside the bound's box, and the centroid inside it; nothing else moves."),
            );
            if let Some((child, c)) = child.and_then(|i| Some((i, b.children.get(i)?))) {
                let m = c.matrix;
                let mut at = [m[3][0], m[3][1], m[3][2]];
                if triple(ui, tr!("Position"), &mut at, 0.01) {
                    let mut matrix = m;
                    matrix[3][..3].copy_from_slice(&at);
                    edits.push(BoundEdit::Matrix { child, matrix });
                }
                let mut turn = degrees(&m);
                if triple(ui, tr!("Rotation (degrees)"), &mut turn, 0.5) {
                    let matrix = turned(&m, turn);
                    edits.push(BoundEdit::Matrix { child, matrix });
                }
            }
            let (mut radius, mut box_min, mut box_max, mut centroid) =
                (b.radius, b.box_min, b.box_max, b.centroid);
            let mut edited = false;
            ui.horizontal(|ui| {
                ui.label(tr!("Radius"));
                edited |= drag_value(ui, &mut radius, |d| d.speed(0.01).max_decimals(4)).inner;
            });
            edited |= triple(ui, tr!("Box minimum"), &mut box_min, 0.01);
            edited |= triple(ui, tr!("Box maximum"), &mut box_max, 0.01);
            edited |= triple(ui, tr!("Centroid"), &mut centroid, 0.01);
            if edited {
                edits.push(BoundEdit::Root {
                    radius,
                    box_min,
                    box_max,
                    centroid,
                });
            }
        });
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(theme::TEXT_SECONDARY),
    );
}

fn embedded_child_id(doc_id: DocumentId) -> egui::Id {
    egui::Id::new(("gta6_embedded_child", doc_id))
}

/// The child of the bound `doc_id` embeds that the collision editor last chose.
pub(super) fn embedded_child(ui: &egui::Ui, doc_id: DocumentId) -> usize {
    ui.data(|d| d.get_temp::<usize>(embedded_child_id(doc_id)))
        .unwrap_or(0)
}

/// The bound a drawable or a fragment embeds: a child chosen by number, its values, then its
/// placement and the bound's box.
pub(super) fn embedded(
    ui: &mut egui::Ui,
    doc_id: DocumentId,
    b: &Bound,
    edits: &mut Vec<BoundEdit>,
) {
    let Some(last) = b.children.len().checked_sub(1) else {
        return;
    };
    egui::CollapsingHeader::new(tr!("Edit the collision"))
        .id_salt(("gta6_embedded_bound", doc_id))
        .show(ui, |ui| {
            let id = embedded_child_id(doc_id);
            let mut child = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(last);
            ui.horizontal(|ui| {
                ui.label(tr!("Child"));
                if drag_value(ui, &mut child, |d| d.range(0..=last)).inner {
                    ui.data_mut(|d| d.insert_temp(id, child));
                }
            });
            if let Some(c) = b.children.get(child) {
                super::physics_edit::child_editor(ui, doc_id, child, c, edits);
            }
            placement(ui, doc_id, b, Some(child), edits);
        });
}

//! The bone the Outline or the viewport selects in a scanned GTA VI drawable, fragment or
//! dictionary entry, its name, position, rotation and scale edited as GTA V's bone fields edit
//! them, each a step of undo.

use std::sync::Arc;

use or_io::gta5::Vec4V;

use super::*;
use crate::state::bone_numbers::{euler_degrees, BoneEdit};
use crate::ui::panels::selection_ids::{payload_index, section_of, SECT_SKEL};

/// A bone's name, position, rotation in degrees and scale, as the fields show them.
type Shown = Arc<(String, [f32; 3], [f32; 3], [f32; 3])>;

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    selection: Option<u64>,
) {
    use crate::state::gta6_entry_skeletons::{entry_bone_of, entry_of, shown as shown_skeleton};
    let own =
        (selection.filter(|s| section_of(*s) == SECT_SKEL)).map(|s| payload_index(s) as usize);
    let entry = selection.and_then(entry_bone_of).map(|(_, bone)| bone);
    let Some(bone) = own.or(entry) else {
        return;
    };
    let Some(doc) = state.document(doc_id) else {
        return;
    };
    // A dictionary entry's bone is edited in the skeleton its entry holds, found by its code.
    let code = entry_of(doc, selection);
    // Built again only when the payload or the bone selected changes.
    let id = egui::Id::new(("gta6_bone_edit", doc_id));
    let stamp = (doc.payload_epoch(), code, bone);
    type Cached = ((u64, Option<u32>, usize), Option<Shown>);
    let shown = match ui.data(|d| d.get_temp::<Cached>(id)) {
        Some((s, shown)) if s == stamp => shown,
        _ => {
            let skeleton = shown_skeleton(doc, selection);
            let shown = skeleton.and_then(|s| {
                let b = s.bones.get(bone)?;
                let [x, y, z, w] = b.rotation;
                let rotation = Vec4V {
                    x,
                    y,
                    z,
                    w,
                    ..Vec4V::default()
                };
                let turned = euler_degrees(&rotation);
                Some(Arc::new((b.name.clone(), b.translation, turned, b.scale)))
            });
            ui.data_mut(|d| d.insert_temp(id, (stamp, shown.clone())));
            shown
        }
    };
    let Some(shown) = shown else {
        return;
    };
    let (name, position, rotation, scale) = &*shown;
    let mut edit = None;
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.strong(tr!("Edit bone {name}", name = name));
    // Taken once the field lets go, as a name typed letter by letter passes through names refused.
    ui.horizontal(|ui| {
        let label = egui::RichText::new(tr!("Name"))
            .small()
            .color(theme::TEXT_SECONDARY);
        ui.label(label).on_hover_text(tr!(
            "A rename keeps the bone's hash, which a clip's tracks and a geometry's palette find it by."
        ));
        let field = egui::Id::new(("gta6_bone_name", doc_id, code, bone));
        let mut text = ui
            .data(|d| d.get_temp::<String>(field))
            .unwrap_or_else(|| name.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
        if response.changed() {
            ui.data_mut(|d| d.insert_temp(field, text.clone()));
        }
        if response.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(field));
            if text != *name {
                edit = Some(BoneEdit::Name(text));
            }
        }
    });
    grid(ui, "gta6_bone_numbers", |ui| {
        let numbers = |ui: &mut egui::Ui, label: &str, held: [f32; 3], speed: f64| {
            ui.label(
                egui::RichText::new(label)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            let mut values = held;
            let mut changed = false;
            ui.horizontal(|ui| {
                for (value, axis) in values.iter_mut().zip(["x ", "y ", "z "]) {
                    let edited =
                        drag_value(ui, value, |d| d.speed(speed).max_decimals(6).prefix(axis));
                    changed |= edited.inner;
                }
            });
            ui.end_row();
            changed.then_some(values)
        };
        if let Some(v) = numbers(ui, tr!("Position"), *position, 0.01) {
            edit = Some(BoneEdit::Position(v));
        }
        if let Some(v) = numbers(ui, tr!("Rotation (°, XYZ)"), *rotation, 0.5) {
            edit = Some(BoneEdit::Rotation(v));
        }
        if let Some(v) = numbers(ui, tr!("Scale"), *scale, 0.01) {
            edit = Some(BoneEdit::Scale(v));
        }
    });
    ui.label(
        egui::RichText::new(tr!("Metres and degrees from the parent bone, as GTA V's bone fields: the bones under it move with it, and a save writes it and their inverse binds in place."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    if let Some(edit) = edit {
        let edited = match code {
            None => state.edit_gta6_bone(doc_id, bone, edit),
            Some(code) => state.edit_gta6_entry_bone(doc_id, code, bone, edit),
        };
        if let Err(why) = edited {
            state.show_warning_toast(why);
        }
    }
}

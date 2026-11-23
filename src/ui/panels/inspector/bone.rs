//! A bone in the Inspector: what its skeleton states of it, and the fields that edit its name, position, rotation and scale.

use or_io::gta5::CrSkeletonData;

use super::*;
use crate::state::bone_numbers::{
    bone_selected, euler_degrees, skeleton_of, BoneAddress, BoneEdit,
};

/// What the skeleton holds of one bone, as the fields show it.
struct Shown {
    name: String,
    bone_id: u16,
    parent: String,
    next_sibling: i16,
    mirror: u16,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
}

impl Shown {
    fn of(skeleton: &CrSkeletonData, index: usize) -> Option<Self> {
        let bone = skeleton.bones.get(index)?;
        let held = usize::try_from(bone.parent_index).ok();
        let parent = match held.and_then(|parent| skeleton.bones.get(parent)) {
            Some(parent) => format!("{} ({})", parent.name, bone.parent_index),
            None if bone.parent_index == -1 => "none".to_string(),
            None => bone.parent_index.to_string(),
        };
        let xyz = |v: &or_io::gta5::Vec4V| [v.x, v.y, v.z];
        Some(Self {
            name: bone.name.clone(),
            bone_id: bone.bone_id,
            parent,
            next_sibling: bone.next_index,
            mirror: bone.mirror_index,
            position: xyz(&bone.translation),
            rotation: euler_degrees(&bone.rotation),
            scale: xyz(&bone.scale),
        })
    }
}

/// The bone `selection` names in `doc_id`, drawn as fields; `false` where it names none.
pub(super) fn draw_selected_bone(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    selection: Option<u64>,
) -> bool {
    let named = state
        .document(doc_id)
        .zip(selection)
        .and_then(|(doc, sel)| bone_selected(&doc.payload, sel));
    let Some(at) = named else {
        return false;
    };
    draw_bone(ui, state, doc_id, at);
    true
}

pub(super) fn draw_bone(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    at: BoneAddress,
) {
    let shown = (state.document(doc_id))
        .and_then(|doc| skeleton_of(&doc.payload, at.owner))
        .and_then(|skeleton| Shown::of(skeleton, at.bone));
    let Some(shown) = shown else {
        empty(ui, "Selected bone is no longer valid");
        return;
    };

    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Bone")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    let mut edit = None;
    grid(ui, "bone_numbers", |ui| {
        row(ui, "Index", &at.bone.to_string());
        let mut name = shown.name.clone();
        edit_row(
            ui,
            "Name",
            "A rename keeps the bone's id, which is what animations use",
            |ui| {
                if ui.text_edit_singleline(&mut name).changed() {
                    edit = Some(BoneEdit::Name(name.clone()));
                }
            },
        );
        row_with_note(
            ui,
            "Bone id",
            &shown.bone_id.to_string(),
            Some("What an animation addresses this bone by; no edit here changes it"),
        );
        row(ui, "Parent", &shown.parent);
        row(ui, "Next sibling", &shown.next_sibling.to_string());
        row(ui, "Mirror", &shown.mirror.to_string());

        let mut position = shown.position;
        edit_row(
            ui,
            "Position",
            "Metres from the parent bone; the bones under this one move with it",
            |ui| {
                if triplet(ui, &mut position, 0.01, "") {
                    edit = Some(BoneEdit::Position(position));
                }
            },
        );
        let mut rotation = shown.rotation;
        edit_row(
            ui,
            "Rotation (°, XYZ)",
            "Euler angles in degrees relative to the parent: a turn about X, then about Y, \
             then about Z, each about the parent's axes. Stored as a unit quaternion.",
            |ui| {
                if triplet(ui, &mut rotation, 0.5, "°") {
                    edit = Some(BoneEdit::Rotation(rotation));
                }
            },
        );
        let mut scale = shown.scale;
        edit_row(
            ui,
            "Scale",
            "Along each of the bone's own axes, after its rotation; 1 leaves it as it is, \
             and 0 is refused",
            |ui| {
                if triplet(ui, &mut scale, 0.01, "") {
                    edit = Some(BoneEdit::Scale(scale));
                }
            },
        );
    });

    if holds_physics(state, doc_id) {
        ui.label(
            egui::RichText::new(PHYSICS_STAYS)
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }

    if let Some(edit) = edit {
        if let Err(why) = state.edit_bone(doc_id, at, edit) {
            state.show_warning_toast(why);
        }
    }
}

/// One row of a grid whose value is `fields`, which the property filter can hide as it hides a read-only row.
fn edit_row(ui: &mut egui::Ui, label: &str, note: &str, fields: impl FnOnce(&mut egui::Ui)) {
    if !filter_matches(ui.ctx(), label) {
        return;
    }
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(theme::TEXT_SECONDARY),
    )
    .on_hover_text(note);
    ui.horizontal(fields);
    ui.end_row();
}

/// Three fields for x, y and z; whether the user changed one.
fn triplet(ui: &mut egui::Ui, values: &mut [f32; 3], speed: f64, suffix: &str) -> bool {
    let mut changed = false;
    for (value, axis) in values.iter_mut().zip(["x ", "y ", "z "]) {
        changed |= drag_value(ui, value, |d| {
            d.speed(speed).max_decimals(6).prefix(axis).suffix(suffix)
        })
        .inner;
    }
    changed
}

/// What the fields of a fragment's bone say of its physics: they are placed by matrices of their own.
pub(crate) const PHYSICS_STAYS: &str =
    "The physics bounds and children of this fragment keep their own matrices: they do not move with the bone.";

/// Whether the document is a fragment with physics, whose children and bounds a bone edit leaves where they are.
fn holds_physics(state: &AppState, doc_id: DocumentId) -> bool {
    use or_io::Asset;
    let Some(DocPayload::Resource(asset)) = state.document(doc_id).map(|doc| &doc.payload) else {
        return false;
    };
    match asset {
        Asset::Fragment(f) => f.physics_lod_group.is_some(),
        Asset::FragmentGen9(f) => f.physics_lod_group.is_some(),
        _ => false,
    }
}

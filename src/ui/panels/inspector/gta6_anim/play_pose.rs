//! Posing a bone of the picked model at the playhead's frame, as GTA V's pose layer edits a clip:
//! the model is drawn in the pose while a value is dragged or typed, and letting go keys the pose
//! into the animation as one step of undo.

use glam::{EulerRot, Quat, Vec3};
use or_anim::{BoneTransform, Pose};
use or_io::gta6::anim::{AnimEdit, AnimResource};

use super::super::timeline::Playing;
use super::{choice, keys, picker, Model, PlayState};
use crate::state::gta6_anim_keys::{frame_at, pose_edits};
use crate::state::{AppState, DocumentId};
use crate::ui::theme;

/// The bone picked to pose, what the clip gives it at the playhead, and the pose being edited.
#[derive(Default)]
pub(super) struct PoseLayer {
    bone: Option<u16>,
    clip: Option<BoneTransform>,
    held: Option<BoneTransform>,
}

/// Records what the clip gives the picked bone in `pose`, then poses it as it is being edited.
pub(super) fn apply(layer: &mut PoseLayer, pose: &mut Pose) {
    let Some(bone) = layer.bone else {
        return;
    };
    layer.clip = pose.local(bone).copied();
    if let Some(held) = layer.held {
        pose.set_local(bone, held);
    }
}

/// The key edits that pose `bone` from `from` to `to` in each animation playing at the playhead.
fn edits(
    resource: &AnimResource,
    play: &PlayState,
    bone: u16,
    from: &BoneTransform,
    to: &BoneTransform,
) -> Vec<AnimEdit> {
    let (Some(choice), Some(model)) = (choice(resource, play.choice), play.model.as_ref()) else {
        return Vec::new();
    };
    let playing = Playing::new(resource, choice, &[], play.transport.time);
    let mut out = Vec::new();
    for layer in &playing.layers {
        let (Some(time), Some(at)) = (layer.time, keys::place(resource, layer.animation)) else {
            continue;
        };
        let bound = model
            .bound
            .iter()
            .find(|(key, _)| *key == layer.animation.key);
        let dofs = bound.and_then(|(_, b)| b.bones.iter().find(|d| d.bone == bone));
        let (Some(dofs), Some(frame)) = (dofs, frame_at(layer.animation, time)) else {
            continue;
        };
        let components = layer.animation.key_components();
        out.extend(pose_edits(
            layer.animation,
            at,
            frame,
            dofs,
            from,
            to,
            &components,
        ));
    }
    out
}

/// Three values side by side after `label`, each ending in `suffix`; whether one changed, and
/// `busy` set while one is dragged or typed into.
fn row(
    ui: &mut egui::Ui,
    label: &str,
    values: &mut [f32; 3],
    speed: f64,
    suffix: &str,
    busy: &mut bool,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for v in values.iter_mut() {
            let r = ui.add(egui::DragValue::new(v).speed(speed).suffix(suffix));
            *busy |= r.dragged() || r.has_focus();
            changed |= r.changed();
        }
    });
    changed
}

/// The bone picker and the picked bone's position, rotation and scale at the playhead.
pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    resource: &AnimResource,
    play: &mut PlayState,
) {
    if play.shown.why_not_posed.is_some() || play.shown.error.is_some() {
        return;
    }
    let Some(Model {
        read: Ok((skeleton, _)),
        ..
    }) = &play.model
    else {
        return;
    };
    let bones = &play.shown.bones;
    // Only the picked bone's name and those of the picker's rows in view are made.
    let name = |b: u16| (skeleton.bones.get(usize::from(b))).map_or("", |b| b.name.as_str());
    let current = (play.pose_layer.bone).and_then(|b| bones.iter().position(|x| *x == b));
    ui.separator();
    let mut picked = None;
    ui.horizontal(|ui| {
        let shown = current.map_or_else(|| tr!("none").to_string(), |i| name(bones[i]).into());
        picker(ui, tr!("Pose a bone"), shown, |ui| {
            let row_height = ui.spacing().interact_size.y;
            egui::ScrollArea::vertical().show_rows(ui, row_height, bones.len(), |ui, range| {
                for i in range {
                    if ui
                        .selectable_label(current == Some(i), name(bones[i]))
                        .clicked()
                    {
                        picked = Some(bones[i]);
                    }
                }
            });
        });
    });
    let current = current.map(|i| bones[i]);
    if let Some(bone) = picked {
        play.pose_layer = PoseLayer {
            bone: Some(bone),
            ..PoseLayer::default()
        };
        ui.ctx().request_repaint();
        return;
    }
    let (Some(bone), Some(clip)) = (current, play.pose_layer.clip) else {
        return;
    };
    if play.transport.playing {
        ui.label(
            egui::RichText::new(tr!("Pause the playhead to pose the bone at a frame."))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        return;
    }
    let mut to = play.pose_layer.held.unwrap_or(clip);
    let (x, y, z) = to.rotation.to_euler(EulerRot::XYZ);
    let mut euler = [x, y, z].map(f32::to_degrees);
    let mut translation = to.translation.to_array();
    let mut scale = to.scale.to_array();
    let mut busy = false;
    let moved = row(ui, tr!("Position"), &mut translation, 0.001, "", &mut busy);
    let turned = row(ui, tr!("Rotation"), &mut euler, 0.1, "°", &mut busy);
    let scaled = row(ui, tr!("Scale"), &mut scale, 0.001, "", &mut busy);
    ui.label(
        egui::RichText::new(tr!("Drag or type a value: the model shows the pose at once, and letting go keys it into the animation at the playhead's frame as one step of undo."))
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    if moved {
        to.translation = Vec3::from_array(translation);
    }
    if turned {
        let [x, y, z] = euler.map(f32::to_radians);
        to.rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
    }
    if scaled {
        to.scale = Vec3::from_array(scale);
    }
    if moved || turned || scaled {
        play.pose_layer.held = Some(to);
        ui.ctx().request_repaint();
    }
    if busy {
        return;
    }
    let Some(held) = play.pose_layer.held.take() else {
        return;
    };
    if held != clip {
        let edits = edits(resource, play, bone, &clip, &held);
        state.pose_gta6_bone(doc_id, &edits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pose_being_edited_is_drawn_and_what_the_clip_gives_the_bone_is_kept() {
        let skeleton = or_anim::Skeleton::build(&or_io::gta5::CrSkeletonData {
            parent_indices: vec![-1],
            bones: vec![or_io::gta5::CrBoneData::default()],
            ..Default::default()
        });
        let mut pose = Pose::bind(&skeleton);
        let clip = pose.local(0).copied().expect("one bone");
        let held = BoneTransform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..clip
        };
        let mut layer = PoseLayer {
            bone: Some(0),
            clip: None,
            held: Some(held),
        };
        apply(&mut layer, &mut pose);
        assert_eq!(layer.clip, Some(clip));
        assert_eq!(pose.local(0).copied(), Some(held));

        // No bone picked: the pose is the clip's.
        let mut pose = Pose::bind(&skeleton);
        let mut layer = PoseLayer::default();
        apply(&mut layer, &mut pose);
        assert_eq!((layer.clip, pose.local(0).copied()), (None, Some(clip)));
    }
}

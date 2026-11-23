//! Keys inserted and cleared at the playhead on the tracks of a GTA VI animation the timeline
//! selects, and a bone of the model posed there, as GTA V's timeline and pose layer key a clip:
//! each one step of the document's history, baked into the clip at once by the key writer.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Quat;
use or_anim::gta6::BoneDofs;
use or_anim::BoneTransform;
use or_io::codec::anim::AnimationSource;
use or_io::gta6::anim::{AnimAt, AnimEdit, AnimResource, Animation};

use super::{AppState, Change, DocPayload, DocumentId};

/// What a key click makes of a track at the playhead's frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// The frame takes the track's value at the playhead.
    Insert,
    /// The frame takes the value between the frames on either side.
    Clear,
}

/// A track keyed: its animation, its DOF, and the seconds into the animation the playhead is at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyTarget {
    pub animation: AnimAt,
    pub dof: usize,
    pub time: f32,
}

/// The frame of `animation` nearest `time` seconds into it; none for an animation of no frame.
pub fn frame_at(animation: &Animation, time: f32) -> Option<u16> {
    let last = animation.frames.checked_sub(1)?;
    let frame = animation.time_to_internal_frame(time).round().max(0.0);
    Some((frame as u16).min(last))
}

/// The key edits `action` makes of DOF `dof` of `animation`, which is at `at`, at the frame nearest
/// `time`: one for each of `components`, the `(DOF, component)` pairs a key is written to, whose
/// value changes. A value is the channel's as stored, before a rotation is normalised or rebuilt.
fn key_edits(
    animation: &Animation,
    at: AnimAt,
    dof: usize,
    time: f32,
    action: KeyAction,
    components: &[(usize, u8)],
) -> Vec<AnimEdit> {
    let Some(frame) = frame_at(animation, time) else {
        return Vec::new();
    };
    let last = animation.frames.saturating_sub(1);
    let held = held(components, dof);
    let around: Vec<u16> = [frame.checked_sub(1), (frame < last).then(|| frame + 1)]
        .into_iter()
        .flatten()
        .collect();
    let is_rotation = animation.dofs.get(dof).is_some_and(|d| d.kind == 0);
    let rotation = (action == KeyAction::Clear && is_rotation)
        .then(|| cleared_rotation(animation, dof, frame, &around, &held))
        .flatten();
    let mut edits = Vec::new();
    for &component in &held {
        let value_at = |f: u16| animation.key_value(dof, component, f);
        let Some(old) = value_at(frame) else {
            continue;
        };
        let value = match (action, rotation) {
            (KeyAction::Insert, _) => {
                let internal = (animation.time_to_internal_frame(time)).clamp(0.0, last.into());
                let first = (internal.floor() as u16).min(last);
                let next = first.saturating_add(1).min(last);
                let (Some(a), Some(b)) = (value_at(first), value_at(next)) else {
                    continue;
                };
                a + (b - a) * (internal - f32::from(first))
            }
            (KeyAction::Clear, Some(slots)) => slots[usize::from(component).min(3)],
            (KeyAction::Clear, None) => {
                let values: Vec<f32> = around.iter().copied().filter_map(value_at).collect();
                if values.is_empty() {
                    continue;
                }
                values.iter().sum::<f32>() / values.len() as f32
            }
        };
        if value.is_finite() && value != old {
            edits.push(AnimEdit::Key {
                animation: at,
                dof,
                component,
                frame,
                value,
            });
        }
    }
    edits
}

/// The components of DOF `dof` among `components`, the `(DOF, component)` pairs a key is written to.
fn held(components: &[(usize, u8)], dof: usize) -> Vec<u8> {
    (components.iter())
        .filter(|(d, _)| *d == dof)
        .map(|(_, c)| *c)
        .collect()
}

/// How a rotation DOF holding `held` stores itself at `frame`: three slots with the place its
/// block rebuilds as positive, or `None` for four slots; no form for any other set of slots.
fn rotation_form(
    animation: &Animation,
    dof: usize,
    frame: u16,
    held: &[u8],
) -> Option<Option<usize>> {
    match held {
        [0, 1, 2] => Some(Some(
            (animation.rebuilt_component(dof, frame)).map_or(3, |m| usize::from(m).min(3)),
        )),
        [0, 1, 2, 3] => Some(None),
        _ => None,
    }
}

/// `q` as the slots of `form`: with three, `q` turned so the rebuilt place is positive and that
/// place left out; with four, `q` turned so its w is positive.
fn rotation_slots(q: Quat, form: Option<usize>) -> [f32; 4] {
    let q = q.normalize().to_array();
    let side = q[form.unwrap_or(3)];
    let q = if side < 0.0 { q.map(|v| -v) } else { q };
    let Some(m) = form else {
        return q;
    };
    let mut slots = [0.0; 4];
    for (slot, v) in slots
        .iter_mut()
        .zip((0..4).filter(|k| *k != m).map(|k| q[k]))
    {
        *slot = v;
    }
    slots
}

/// The rotation that `slots` of `form` hold, its rebuilt place made again as the reader makes it.
fn slots_rotation(slots: [f32; 4], form: Option<usize>) -> Option<Quat> {
    let q = match form {
        None => slots,
        Some(m) => {
            let [a, b, c, _] = slots;
            let mut q = vec![a, b, c];
            q.insert(m.min(3), (1.0 - (a * a + b * b + c * c)).max(0.0).sqrt());
            [q[0], q[1], q[2], q[3]]
        }
    };
    let q = Quat::from_array(q);
    (q.is_finite() && q.length_squared() > 1e-12).then(|| q.normalize())
}

/// The mean of `rotations`, each turned to the first's side, since `q` and `-q` are one rotation.
fn mean_rotation(rotations: impl IntoIterator<Item = Quat>) -> Option<Quat> {
    let mut rotations = rotations.into_iter();
    let first = rotations.next()?;
    let mut sum = first.to_array();
    for q in rotations {
        let side = if first.dot(q) < 0.0 { -1.0 } else { 1.0 };
        for (s, v) in sum.iter_mut().zip(q.to_array()) {
            *s += side * v;
        }
    }
    let mean = Quat::from_array(sum);
    (mean.is_finite() && mean.length_squared() > 1e-12).then(|| mean.normalize())
}

/// The slots rotation DOF `dof` takes at `frame` when cleared: the mean of the rotations of the
/// frames `around`, as `frame` stores it.
fn cleared_rotation(
    animation: &Animation,
    dof: usize,
    frame: u16,
    around: &[u16],
    held: &[u8],
) -> Option<[f32; 4]> {
    let form = rotation_form(animation, dof, frame, held)?;
    let mut rotations = Vec::with_capacity(around.len());
    for &f in around {
        let mut slots = [0.0; 4];
        for &c in held {
            *slots.get_mut(usize::from(c))? = animation.key_value(dof, c, f)?;
        }
        rotations.push(slots_rotation(
            slots,
            rotation_form(animation, dof, f, held)?,
        )?);
    }
    Some(rotation_slots(mean_rotation(rotations)?, form))
}

/// The keys at `frame` of each track of the bone `dofs` names that changed from `from` to `to`.
// A rotation held in a form no key is written to is left as it is.
pub fn pose_edits(
    animation: &Animation,
    at: AnimAt,
    frame: u16,
    dofs: &BoneDofs,
    from: &BoneTransform,
    to: &BoneTransform,
    components: &[(usize, u8)],
) -> Vec<AnimEdit> {
    let mut tracks: Vec<(usize, [f32; 4])> = Vec::new();
    if let Some(dof) = dofs
        .translation
        .filter(|_| from.translation != to.translation)
    {
        tracks.push((dof, to.translation.extend(0.0).to_array()));
    }
    if let Some(dof) = dofs.rotation.filter(|_| from.rotation != to.rotation) {
        if let Some(form) = rotation_form(animation, dof, frame, &held(components, dof)) {
            tracks.push((dof, rotation_slots(to.rotation, form)));
        }
    }
    if let Some(dof) = dofs.scale.filter(|_| from.scale != to.scale) {
        tracks.push((dof, to.scale.extend(0.0).to_array()));
    }
    let mut edits = Vec::new();
    for (dof, slots) in tracks {
        for component in held(components, dof) {
            let old = animation.key_value(dof, component, frame);
            let (Some(old), Some(&value)) = (old, slots.get(usize::from(component))) else {
                continue;
            };
            if value.is_finite() && value != old {
                edits.push(AnimEdit::Key {
                    animation: at,
                    dof,
                    component,
                    frame,
                    value,
                });
            }
        }
    }
    edits
}

fn anim_of(payload: &mut DocPayload) -> Option<&mut Arc<AnimResource>> {
    match payload {
        DocPayload::Gta6Anim(resource) => Some(resource),
        _ => None,
    }
}

impl AppState {
    /// `action` made on each of `targets` as one step of `doc`'s history; a refusal is shown.
    pub fn key_gta6_tracks(
        &mut self,
        doc: DocumentId,
        targets: &[KeyTarget],
        action: KeyAction,
    ) -> bool {
        let Some(DocPayload::Gta6Anim(resource)) = self.document(doc).map(|d| &d.payload) else {
            return false;
        };
        let resource = Arc::clone(resource);
        // ⚠ Listed once for each animation: listing walks every block's channels.
        let mut components: HashMap<(usize, usize), Vec<(usize, u8)>> = HashMap::new();
        let mut edits = Vec::new();
        for target in targets {
            let at = target.animation;
            let Some(animation) =
                (resource.sets.get(at.set)).and_then(|s| s.animations.get(at.animation))
            else {
                continue;
            };
            let pairs = (components.entry((at.set, at.animation)))
                .or_insert_with(|| animation.key_components());
            edits.extend(key_edits(
                animation,
                at,
                target.dof,
                target.time,
                action,
                pairs,
            ));
        }
        if edits.is_empty() {
            self.show_info_toast(tr!("Nothing changed: each selected track already holds that value at this frame, or holds no channel a key is written to."));
            return false;
        }
        let label = match action {
            KeyAction::Insert => tr!("Insert key"),
            KeyAction::Clear => tr!("Clear key"),
        };
        self.bake_gta6_keys(doc, &edits, label)
    }

    /// `edits`, a bone posed at the playhead, as one step of `doc`'s history; a refusal is shown.
    pub fn pose_gta6_bone(&mut self, doc: DocumentId, edits: &[AnimEdit]) -> bool {
        if edits.is_empty() {
            self.show_info_toast(tr!("Nothing changed: no track the pose changed holds a channel a key is written to at this frame."));
            return false;
        }
        self.bake_gta6_keys(doc, edits, tr!("Pose bone"))
    }

    /// `edits` baked into `doc`'s animation as one step of its history, labelled `label`.
    fn bake_gta6_keys(&mut self, doc: DocumentId, edits: &[AnimEdit], label: &str) -> bool {
        // ponytail: each key copies the section on the UI thread, as one key edit does; a job if
        // many tracks are keyed at once.
        let made = self.replace_gta6_anim(doc, |held| {
            let mut edited: Option<AnimResource> = None;
            for edit in edits {
                let next = or_io::gta6::anim::edit(edited.as_ref().unwrap_or(held), edit)?;
                edited = Some(next);
            }
            Ok(edited.unwrap_or_else(|| held.clone()))
        });
        match made {
            Ok(before) => {
                // No key: two clicks in a row are two steps, never folded into one.
                let mut change = Change::part(String::new(), before, anim_of);
                if let Change::Part { key, .. } = &mut change {
                    *key = None;
                }
                self.record(doc, label, change);
                true
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot edit: {reason}", reason = e));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use or_io::container::rpf::{write_rpf9_resource, Rpf9Form, Rpf9Sections};

    use super::super::gta6_anim_edit::tests::dictionary;
    use super::super::opening_an_rpf9_archive::{said, settle};
    use super::super::ToastType;
    use super::*;

    const AT: AnimAt = AnimAt {
        set: 0,
        animation: 0,
    };

    fn walk() -> Animation {
        let read = or_io::gta6::anim::read(&dictionary()).expect("the dictionary reads");
        read.sets[0].animations[0].clone()
    }

    fn value(animation: &Animation, frame: u16) -> f32 {
        animation.key_value(0, 0, frame).expect("a key of frame")
    }

    #[test]
    fn insert_holds_the_value_at_the_playhead_and_clear_the_value_between_the_frames_beside() {
        let walk = walk();
        let pairs = walk.key_components();
        assert!(pairs.contains(&(0, 0)), "{pairs:?}");
        let (first, second) = (value(&walk, 0), value(&walk, 1));
        assert_ne!(first, second);

        // A quarter of the way to frame 1 the playhead is nearest frame 0, which takes the blend.
        let time = walk.duration * 0.25;
        let internal = walk.time_to_internal_frame(time);
        let edits = key_edits(&walk, AT, 0, time, KeyAction::Insert, &pairs);
        let expected = first + (second - first) * internal;
        assert_eq!(
            edits,
            [AnimEdit::Key {
                animation: AT,
                dof: 0,
                component: 0,
                frame: 0,
                value: expected,
            }]
        );
        // On a frame, the value is the frame's own: nothing to write.
        assert!(key_edits(&walk, AT, 0, 0.0, KeyAction::Insert, &pairs).is_empty());

        // Clearing an end frame takes the one frame beside it.
        let cleared = key_edits(&walk, AT, 0, walk.duration, KeyAction::Clear, &pairs);
        assert!(
            matches!(cleared[..], [AnimEdit::Key { frame: 1, value, .. }] if value == first),
            "{cleared:?}"
        );
        // A DOF the animation has no key of, and a time past its end, clamp or write nothing.
        assert!(key_edits(&walk, AT, 5, 0.0, KeyAction::Clear, &pairs).is_empty());
        assert_eq!(frame_at(&walk, 1e6), Some(walk.frames - 1));
        assert_eq!(frame_at(&walk, -1.0), Some(0));
    }

    #[test]
    fn a_key_cleared_from_the_timeline_is_one_step_that_undoes_and_one_that_changes_nothing_is_none(
    ) {
        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("dict_a.rsc");
        let sections = Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: dictionary(),
            physical: Vec::new(),
        };
        std::fs::write(&path, write_rpf9_resource(&sections).expect("writes")).expect("written");
        let mut state = AppState::new();
        state.load_file(path);
        settle(&mut state);
        let doc = state
            .active_document
            .unwrap_or_else(|| panic!("opened: {}", state.error_dialog_message));
        let now = |state: &AppState| {
            let Some(DocPayload::Gta6Anim(r)) = state.document(doc).map(|d| &d.payload) else {
                panic!("not a GTA VI animation document");
            };
            Arc::clone(r)
        };
        let (first, second) = (value(&walk(), 0), value(&walk(), 1));
        let target = KeyTarget {
            animation: AT,
            dof: 0,
            time: 0.0,
        };

        assert!(state.key_gta6_tracks(doc, &[target], KeyAction::Clear));
        let cleared = now(&state);
        assert!(cleared.edited);
        assert_eq!(value(&cleared.sets[0].animations[0], 0), second);
        assert!(state.undo().is_some());
        assert_eq!(value(&now(&state).sets[0].animations[0], 0), first);
        assert!(!now(&state).edited);
        assert!(state.redo().is_some());
        assert_eq!(value(&now(&state).sets[0].animations[0], 0), second);

        // Both frames now hold one value: a key inserted on a frame changes nothing and records nothing.
        state.toasts.clear();
        assert!(!state.key_gta6_tracks(doc, &[target], KeyAction::Insert));
        assert!(said(&state, ToastType::Info, &["Nothing changed"]));
        assert!(state.undo().is_some());
        assert_eq!(value(&now(&state).sets[0].animations[0], 0), first);

        // A bone posed at a frame is one step of its own, which undoes.
        let posed = AnimEdit::Key {
            animation: AT,
            dof: 0,
            component: 0,
            frame: 1,
            value: second + 1.0,
        };
        assert!(state.pose_gta6_bone(doc, &[posed]));
        assert_eq!(value(&now(&state).sets[0].animations[0], 1), second + 1.0);
        assert_eq!(state.undo().as_deref(), Some("Pose bone"));
        assert_eq!(value(&now(&state).sets[0].animations[0], 1), second);
        state.toasts.clear();
        assert!(!state.pose_gta6_bone(doc, &[]));
        assert!(said(&state, ToastType::Info, &["Nothing changed"]));
    }

    fn turned(q: Quat) -> Quat {
        Quat::from_array(q.to_array().map(|v| -v))
    }

    #[test]
    fn a_rotation_is_stored_with_its_rebuilt_place_positive_and_reads_back_as_itself() {
        let q = Quat::from_xyzw(0.1, -0.7, 0.2, -0.3).normalize();
        for form in [Some(0), Some(1), Some(2), Some(3), None] {
            let slots = rotation_slots(q, form);
            let back = slots_rotation(slots, form).expect("a rotation");
            assert!(
                back.dot(q).abs() > 1.0 - 1e-5,
                "{form:?}: {back:?} for {q:?}"
            );
            if form.is_some() {
                assert_eq!(slots[3], 0.0, "{form:?}: three slots are held");
            }
        }
        // The place rebuilt as positive is negative in `q`, so it is stored turned.
        assert!((rotation_slots(q, Some(1))[0] + q.x).abs() < 1e-6);
        assert!((rotation_slots(q, None)[3] + q.w).abs() < 1e-6);
        assert_eq!(rotation_form(&walk(), 0, 0, &[0, 1, 2]), Some(Some(3)));
        assert_eq!(rotation_form(&walk(), 0, 0, &[0]), None);
    }

    #[test]
    fn a_rotation_cleared_between_two_frames_is_their_mean_whichever_side_each_is_stored_on() {
        let (a, b) = (Quat::from_rotation_z(0.2), Quat::from_rotation_z(0.4));
        let expected = Quat::from_rotation_z(0.3);
        let mean = mean_rotation([a, turned(b)]).expect("a mean");
        assert!(mean.dot(expected).abs() > 1.0 - 1e-5, "{mean:?}");
        // The components' own mean of the two is almost no rotation, far from either.
        let raw: Vec<f32> = (a.to_array().iter().zip(turned(b).to_array()))
            .map(|(x, y)| (x + y) / 2.0)
            .collect();
        assert!(
            raw.iter().map(|v| v * v).sum::<f32>().sqrt() < 0.1,
            "{raw:?}"
        );
        assert_eq!(mean_rotation(Vec::new()), None);
    }

    #[test]
    fn a_posed_bone_keys_each_track_that_changed_and_no_other() {
        let walk = walk();
        let pairs = walk.key_components();
        assert_eq!(held(&pairs, 0), [0]);
        assert_eq!(walk.rebuilt_component(0, 0), None);
        let dofs = BoneDofs {
            bone: 0,
            translation: Some(0),
            rotation: None,
            scale: None,
        };
        let from = BoneTransform::IDENTITY;
        let x = value(&walk, 1) + 1.0;
        let moved = BoneTransform {
            translation: glam::Vec3::new(x, 0.0, 0.0),
            ..from
        };
        let edits = pose_edits(&walk, AT, 1, &dofs, &from, &moved, &pairs);
        assert_eq!(
            edits,
            [AnimEdit::Key {
                animation: AT,
                dof: 0,
                component: 0,
                frame: 1,
                value: x,
            }]
        );
        // Nothing changed, or a track the bone has no DOF of: no key.
        assert!(pose_edits(&walk, AT, 1, &dofs, &from, &from, &pairs).is_empty());
        let rotated = BoneTransform {
            rotation: Quat::from_rotation_z(1.0),
            ..from
        };
        assert!(pose_edits(&walk, AT, 1, &dofs, &from, &rotated, &pairs).is_empty());
    }
}

//! A bone's numbers, which the Inspector edits: its name, position, rotation and scale.

use glam::{EulerRot, Quat};
use or_io::gta5::{CrSkeletonData, DrawableTarget, FragType, PtxFxList, RmcDrawable, Vec4V};
use or_io::Asset;

use super::textures::FRAGMENT_ENTRY_HASH;
use super::{AppState, Change, DocPayload, DocumentId};
use crate::ui::panels::selection_ids::{
    dict_entry_unpack_part, is_dict_entry_selection, payload_index, section_of,
    ydd_selected_entry_index, SECT_SKEL, YPT_MODEL_KIND_BONE,
};

/// Where a bone is: the skeleton of drawable `owner` of a document, and its index there.
///
/// `owner` is the entry of a drawable dictionary, and 0 for a drawable and for a fragment's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoneAddress {
    pub owner: u32,
    pub bone: usize,
}

/// One change to a bone's numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum BoneEdit {
    Name(String),
    Position([f32; 3]),
    /// Euler angles in degrees, XYZ: a turn about the parent's X axis, then its Y, then its Z.
    Rotation([f32; 3]),
    Scale([f32; 3]),
}

/// The skeleton of drawable `owner` of a document, at either target.
pub fn skeleton_of(payload: &DocPayload, owner: u32) -> Option<&CrSkeletonData> {
    let DocPayload::Resource(asset) = payload else {
        return None;
    };
    let at = owner as usize;
    match asset {
        Asset::Drawable(d) if at == 0 => d.base.skeleton.as_deref(),
        Asset::DrawableGen9(d) if at == 0 => d.base.skeleton.as_deref(),
        Asset::DrawableDict(dict) => dict.entries.get(at)?.as_deref()?.base.skeleton.as_deref(),
        Asset::DrawableDictGen9(dict) => dict.entries.get(at)?.as_deref()?.base.skeleton.as_deref(),
        Asset::Particles(list) => model_of(list, at)?.skeleton.as_deref(),
        Asset::ParticlesGen9(list) => model_of(list, at)?.skeleton.as_deref(),
        Asset::Fragment(f) if at == 0 => f.common_drawable.as_deref()?.base.skeleton.as_deref(),
        Asset::FragmentGen9(f) if at == 0 => f.common_drawable.as_deref()?.base.skeleton.as_deref(),
        _ => None,
    }
}

/// Model `at` of a particle list, in the order its dictionary holds them.
fn model_of<T: DrawableTarget>(list: &PtxFxList<T>, at: usize) -> Option<&RmcDrawable<T>> {
    list.model_dict.as_deref()?.entries.get(at)?.as_deref()
}

/// The mutable half of [`model_of`].
fn model_mut<T: DrawableTarget>(list: &mut PtxFxList<T>, at: usize) -> Option<&mut RmcDrawable<T>> {
    list.model_dict
        .as_deref_mut()?
        .entries
        .get_mut(at)?
        .as_deref_mut()
}

/// The place in a particle list's model dictionary of the model a selection is a part of.
fn particle_model_named<T: DrawableTarget>(list: &PtxFxList<T>, selection: u64) -> Option<usize> {
    if !is_dict_entry_selection(selection) {
        return None;
    }
    let (hash, ..) = dict_entry_unpack_part(selection);
    list.model_dict
        .as_deref()?
        .codes
        .iter()
        .position(|code| *code == hash)
}

/// The mutable half of [`skeleton_of`].
pub fn skeleton_mut(payload: &mut DocPayload, owner: u32) -> Option<&mut CrSkeletonData> {
    let DocPayload::Resource(asset) = payload else {
        return None;
    };
    let at = owner as usize;
    match asset {
        Asset::Drawable(d) if at == 0 => d.base.skeleton.as_deref_mut(),
        Asset::DrawableGen9(d) if at == 0 => d.base.skeleton.as_deref_mut(),
        Asset::DrawableDict(dict) => dict
            .entries
            .get_mut(at)?
            .as_deref_mut()?
            .base
            .skeleton
            .as_deref_mut(),
        Asset::Particles(list) => model_mut(list, at)?.skeleton.as_deref_mut(),
        Asset::ParticlesGen9(list) => model_mut(list, at)?.skeleton.as_deref_mut(),
        Asset::DrawableDictGen9(dict) => dict
            .entries
            .get_mut(at)?
            .as_deref_mut()?
            .base
            .skeleton
            .as_deref_mut(),
        Asset::Fragment(f) if at == 0 => f
            .common_drawable
            .as_deref_mut()?
            .base
            .skeleton
            .as_deref_mut(),
        Asset::FragmentGen9(f) if at == 0 => f
            .common_drawable
            .as_deref_mut()?
            .base
            .skeleton
            .as_deref_mut(),
        _ => None,
    }
}

/// The skeleton of every drawable a fragment saves, in the order of [`fragment_skeleton_mut`]: each drawable's, then each environment cloth's own.
fn fragment_skeletons<T: DrawableTarget>(f: &FragType<T>) -> Vec<Option<&CrSkeletonData>> {
    let drawables = f.drawables().into_iter();
    let cloth = (f.env_cloth.iter()).filter_map(|c| c.referenced_drawable.as_deref());
    (drawables.chain(cloth))
        .map(|d| d.base.skeleton.as_deref())
        .collect()
}

/// Skeleton number `slot` of [`fragment_skeletons`].
fn fragment_skeleton_mut<T: DrawableTarget>(
    f: &mut FragType<T>,
    slot: usize,
) -> Option<&mut CrSkeletonData> {
    let drawables = f.drawables().len();
    match slot.checked_sub(drawables) {
        None => f
            .drawables_mut()
            .into_iter()
            .nth(slot)?
            .skeleton
            .as_deref_mut(),
        Some(cloth) => (f.env_cloth.iter_mut())
            .filter_map(|c| c.referenced_drawable.as_deref_mut())
            .nth(cloth)?
            .base
            .skeleton
            .as_deref_mut(),
    }
}

/// Skeleton number `slot` of the fragment a payload holds.
fn slot_mut(payload: &mut DocPayload, slot: usize) -> Option<&mut CrSkeletonData> {
    let DocPayload::Resource(asset) = payload else {
        return None;
    };
    match asset {
        Asset::Fragment(f) => fragment_skeleton_mut(&mut f.base, slot),
        Asset::FragmentGen9(f) => fragment_skeleton_mut(&mut f.base, slot),
        _ => None,
    }
}

/// Each skeleton of a fragment other than its common drawable's (slot 0) that is equal to `edited`, with its slot.
fn copies_of(payload: &DocPayload, edited: &CrSkeletonData) -> Vec<(usize, CrSkeletonData)> {
    let DocPayload::Resource(asset) = payload else {
        return Vec::new();
    };
    let held = match asset {
        Asset::Fragment(f) => fragment_skeletons(&f.base),
        Asset::FragmentGen9(f) => fragment_skeletons(&f.base),
        _ => return Vec::new(),
    };
    (held.into_iter().enumerate().skip(1))
        .filter_map(|(slot, held)| Some((slot, held.filter(|s| *s == edited)?.clone())))
        .collect()
}

/// The bone a selection names, whichever row of the Outline or pick of the viewport made it.
pub fn bone_selected(payload: &DocPayload, selection: u64) -> Option<BoneAddress> {
    let DocPayload::Resource(asset) = payload else {
        return None;
    };
    let named = |owner: usize| {
        let (_, kind, bone) = dict_entry_unpack_part(selection);
        (kind == YPT_MODEL_KIND_BONE).then_some(BoneAddress {
            owner: owner as u32,
            bone: bone as usize,
        })
    };
    match asset {
        Asset::Drawable(_) | Asset::DrawableGen9(_) => (section_of(selection) == SECT_SKEL)
            .then_some(BoneAddress {
                owner: 0,
                bone: payload_index(selection) as usize,
            }),
        Asset::DrawableDict(_) | Asset::DrawableDictGen9(_) => {
            named(ydd_selected_entry_index(payload, selection)?)
        }
        Asset::Particles(list) => particle_model_named(list, selection).and_then(named),
        Asset::ParticlesGen9(list) => particle_model_named(list, selection).and_then(named),
        Asset::Fragment(_) | Asset::FragmentGen9(_) => {
            let (hash, ..) = dict_entry_unpack_part(selection);
            if is_dict_entry_selection(selection) && hash == FRAGMENT_ENTRY_HASH {
                named(0)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// A bone's rotation as Euler angles in degrees, XYZ.
pub fn euler_degrees(rotation: &Vec4V) -> [f32; 3] {
    let q = Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w);
    let q = if q.is_finite() && q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    };
    let (z, y, x) = q.to_euler(EulerRot::ZYX);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// The unit quaternion that turns about X by `degrees[0]`, then Y by `degrees[1]`, then Z by `degrees[2]`.
fn quaternion_of(degrees: [f32; 3]) -> Quat {
    let [x, y, z] = degrees.map(f32::to_radians);
    Quat::from_euler(EulerRot::ZYX, z, y, x).normalize()
}

/// `edit` made to bone `bone` of `skeleton`, and what it derives from the pose made again.
fn apply(skeleton: &mut CrSkeletonData, bone: usize, edit: &BoneEdit) -> Result<(), String> {
    let count = skeleton.bones.len();
    let held = (skeleton.bones.get_mut(bone))
        .ok_or_else(|| format!("bone {bone} is not in a skeleton of {count}"))?;
    let numbers = match edit {
        BoneEdit::Name(name) => {
            if name.contains('\0') {
                return Err("a bone's name cannot hold a NUL".to_string());
            }
            held.name.clone_from(name);
            return Ok(());
        }
        BoneEdit::Position(v) | BoneEdit::Scale(v) | BoneEdit::Rotation(v) => v,
    };
    if !numbers.iter().all(|n| n.is_finite()) {
        return Err("a bone's numbers must be finite".to_string());
    }
    let set = |lanes: &mut Vec4V, [x, y, z]: [f32; 3]| (lanes.x, lanes.y, lanes.z) = (x, y, z);
    match edit {
        BoneEdit::Position(v) => set(&mut held.translation, *v),
        BoneEdit::Scale(v) => set(&mut held.scale, *v),
        _ => {
            let q = quaternion_of(*numbers);
            (held.rotation.x, held.rotation.y) = (q.x, q.y);
            (held.rotation.z, held.rotation.w) = (q.z, q.w);
        }
    }
    skeleton.rebuild_rest_pose(bone).map_err(|e| e.to_string())
}

impl AppState {
    /// Changes one of a bone's numbers and derives its rest pose, and every bone under it, again, as one step of the document's history.
    ///
    /// The bone's id, its place in the skeleton and the bone-id table stay as they were. In a
    /// fragment, each other skeleton equal to the one edited takes the same edit, in the same step.
    /// On an error the document is as it was.
    pub fn edit_bone(
        &mut self,
        doc: DocumentId,
        at: BoneAddress,
        edit: BoneEdit,
    ) -> Result<(), String> {
        // ⚠ An image edit not yet read back would be lost when this step swaps in a skeleton built from the payload as it was.
        self.catch_up(doc);
        let document = self.document_mut(doc).ok_or("the document is closed")?;
        let held = skeleton_of(&document.payload, at.owner)
            .ok_or("this document holds no skeleton here")?;
        let mut after = held.clone();
        apply(&mut after, at.bone, &edit)?;
        // ⚠ A save writes a fragment's other skeleton as a copy of the common drawable's only while the two are equal (OD-ALIAS-BY-VALUE), and otherwise as an allocation of its own that holds the old numbers: each skeleton equal to the one edited takes the same edit.
        let copies = (copies_of(&document.payload, held).into_iter())
            .map(|(slot, mut copy)| apply(&mut copy, at.bone, &edit).map(|()| (slot, copy)))
            .collect::<Result<Vec<_>, _>>()?;
        let skeleton = skeleton_mut(&mut document.payload, at.owner)
            .ok_or("this document holds no skeleton here")?;
        let name = after.bones[at.bone].name.clone();
        let (what, field) = match &edit {
            BoneEdit::Name(_) => ("Rename bone".to_string(), "name"),
            BoneEdit::Position(_) => (format!("Move bone {name}"), "position"),
            BoneEdit::Rotation(_) => (format!("Turn bone {name}"), "rotation"),
            BoneEdit::Scale(_) => (format!("Scale bone {name}"), "scale"),
        };
        let before = std::mem::replace(skeleton, after);
        let owner = at.owner;
        let key = format!("bone {owner} {} {field}", at.bone);
        let mut step = Change::part(key, before, move |payload| skeleton_mut(payload, owner));
        for (slot, copy) in copies {
            let payload = &mut document.payload;
            if let Some(held) = slot_mut(payload, slot).map(|s| std::mem::replace(s, copy)) {
                step = step.and_part(held, move |payload| slot_mut(payload, slot));
            }
        }
        self.record(doc, what, step);
        Ok(())
    }
}

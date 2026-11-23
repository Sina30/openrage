use crate::binding::ClipBinding;
use crate::pose::BoneTransform;
use crate::skeleton::Skeleton;
use glam::{Quat, Vec3};
use or_io::codec::anim::{
    format_of_track, format_type, track, AnimationSource, DofDecoder, DofSampleTable, EncodePolicy,
};
use or_io::gta5::{CrAnimation, Dof};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditTrack {
    Translation,
    Rotation,
    Scale,
}

impl EditTrack {
    pub fn track_id(self) -> u8 {
        match self {
            Self::Translation => track::BONE_TRANSLATION,
            Self::Rotation => track::BONE_ROTATION,
            Self::Scale => track::BONE_SCALE,
        }
    }

    pub fn from_track_id(track_id: u8) -> Option<Self> {
        match track_id {
            track::BONE_TRANSLATION => Some(Self::Translation),
            track::BONE_ROTATION => Some(Self::Rotation),
            track::BONE_SCALE => Some(Self::Scale),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Rotation => "rotation",
            Self::Scale => "scale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditValue {
    Vector(Vec3),
    Rotation(Quat),
}

impl EditValue {
    pub fn components(&self) -> [f32; 4] {
        match self {
            Self::Vector(v) => [v.x, v.y, v.z, 0.0],
            Self::Rotation(q) => [q.x, q.y, q.z, q.w],
        }
    }

    pub fn fits(&self, track: EditTrack) -> bool {
        matches!(
            (self, track),
            (Self::Vector(_), EditTrack::Translation)
                | (Self::Vector(_), EditTrack::Scale)
                | (Self::Rotation(_), EditTrack::Rotation)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditKey {
    pub bone: u16,
    pub track: EditTrack,
    pub frame: u16,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoseEdits {
    edits: BTreeMap<EditKey, EditValue>,
}

impl PoseEdits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn len(&self) -> usize {
        self.edits.len()
    }

    pub fn set(&mut self, key: EditKey, value: EditValue) -> bool {
        if !value.fits(key.track) {
            return false;
        }
        self.edits.insert(key, value);
        true
    }

    pub fn set_transform(&mut self, bone: u16, frame: u16, transform: &BoneTransform) {
        self.set(
            EditKey {
                bone,
                track: EditTrack::Translation,
                frame,
            },
            EditValue::Vector(transform.translation),
        );
        self.set(
            EditKey {
                bone,
                track: EditTrack::Rotation,
                frame,
            },
            EditValue::Rotation(transform.rotation),
        );
        self.set(
            EditKey {
                bone,
                track: EditTrack::Scale,
                frame,
            },
            EditValue::Vector(transform.scale),
        );
    }

    pub fn get(&self, key: &EditKey) -> Option<EditValue> {
        self.edits.get(key).copied()
    }

    pub fn remove(&mut self, key: &EditKey) -> bool {
        self.edits.remove(key).is_some()
    }

    pub fn clear_bone(&mut self, bone: u16) -> usize {
        let before = self.edits.len();
        self.edits.retain(|key, _| key.bone != bone);
        before - self.edits.len()
    }

    pub fn clear_track(&mut self, bone: u16, track: EditTrack) -> usize {
        let before = self.edits.len();
        self.edits
            .retain(|key, _| key.bone != bone || key.track != track);
        before - self.edits.len()
    }

    pub fn clear(&mut self) {
        self.edits.clear();
    }

    pub fn edited_bones(&self) -> BTreeSet<u16> {
        self.edits.keys().map(|key| key.bone).collect()
    }

    pub fn edited_frames(&self, bone: u16, track: EditTrack) -> Vec<u16> {
        self.edits
            .keys()
            .filter(|key| key.bone == bone && key.track == track)
            .map(|key| key.frame)
            .collect()
    }

    pub fn touches_bone(&self, bone: u16) -> bool {
        self.edits
            .range(
                EditKey {
                    bone,
                    track: EditTrack::Translation,
                    frame: 0,
                }..,
            )
            .next()
            .is_some_and(|(key, _)| key.bone == bone)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&EditKey, &EditValue)> {
        self.edits.iter()
    }

    pub fn apply_to_pose(&self, frame: u16, pose: &mut crate::pose::Pose) {
        for (key, value) in &self.edits {
            if key.frame != frame {
                continue;
            }
            let Some(local) = pose.local_mut(key.bone) else {
                continue;
            };
            match (key.track, value) {
                (EditTrack::Translation, EditValue::Vector(v)) => local.translation = *v,
                (EditTrack::Scale, EditValue::Vector(v)) => local.scale = *v,
                (EditTrack::Rotation, EditValue::Rotation(q)) => local.rotation = *q,
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BakedAnimation {
    pub tables: Vec<DofSampleTable>,
    pub dofs: Vec<Dof>,
    pub edited_dofs: usize,
    pub undecodable_dofs: usize,
}

impl BakedAnimation {
    pub fn is_lossy(&self) -> bool {
        self.undecodable_dofs > 0
    }
}

pub fn bake(
    animation: &CrAnimation,
    skeleton: &Skeleton,
    binding: &ClipBinding,
    edits: &PoseEdits,
) -> BakedAnimation {
    let num_frames = usize::from(animation.num_frames.max(1));
    let mut dofs = animation.dofs.clone();
    let mut tables: Vec<Vec<[f32; 4]>> = Vec::with_capacity(dofs.len());
    let mut undecodable_dofs = 0usize;
    let mut decoder = DofDecoder::new(animation);

    for (dof_index, dof) in dofs.iter().enumerate() {
        let mut frames = Vec::with_capacity(num_frames);
        let mut ok = true;
        for frame in 0..num_frames {
            let time = animation.internal_frame_to_time(frame as f32);
            match decoder.decode(dof_index, time) {
                Ok(Some(samples)) => frames.push(samples.components),
                Ok(None) => frames.push([0.0; 4]),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            undecodable_dofs += 1;
            frames = bind_pose_frames(skeleton, binding, dof, num_frames);
        }
        tables.push(frames);
    }

    let mut dof_for: BTreeMap<(u16, u8), usize> = BTreeMap::new();
    for (index, dof) in dofs.iter().enumerate() {
        dof_for.entry((dof.id, dof.track)).or_insert(index);
    }

    let mut edited: BTreeSet<usize> = BTreeSet::new();
    for (key, value) in edits.iter() {
        let Some(bone) = skeleton.bone(key.bone) else {
            continue;
        };
        let track_id = key.track.track_id();
        let dof_index = match dof_for.get(&(bone.tag, track_id)) {
            Some(index) => *index,
            None => {
                let dof = Dof {
                    id: bone.tag,
                    r#type: format_of_track(track_id),
                    track: track_id,
                    ..Dof::default()
                };
                let frames = bind_pose_frames(skeleton, binding, &dof, num_frames);
                dofs.push(dof);
                tables.push(frames);
                let index = dofs.len() - 1;
                dof_for.insert((bone.tag, track_id), index);
                index
            }
        };
        let frame = usize::from(key.frame);
        if frame >= num_frames {
            continue;
        }
        tables[dof_index][frame] = value.components();
        edited.insert(dof_index);
    }

    let tables = dofs
        .iter()
        .zip(tables.iter())
        .map(|(dof, frames)| DofSampleTable::from_frames(dof.r#type, frames))
        .collect();

    BakedAnimation {
        tables,
        dofs,
        edited_dofs: edited.len(),
        undecodable_dofs,
    }
}

fn bind_pose_frames(
    skeleton: &Skeleton,
    _binding: &ClipBinding,
    dof: &Dof,
    num_frames: usize,
) -> Vec<[f32; 4]> {
    let bone = skeleton.bone_for_tag(dof.id);
    let value = match (EditTrack::from_track_id(dof.track), bone) {
        (Some(EditTrack::Translation), Some(bone)) => {
            let t = bone.bind_translation;
            [t.x, t.y, t.z, 0.0]
        }
        (Some(EditTrack::Rotation), Some(bone)) => {
            let r = bone.bind_rotation;
            [r.x, r.y, r.z, r.w]
        }
        (Some(EditTrack::Scale), Some(bone)) => {
            let s = bone.bind_scale;
            [s.x, s.y, s.z, 0.0]
        }
        (Some(EditTrack::Rotation), None) => [0.0, 0.0, 0.0, 1.0],
        (Some(EditTrack::Scale), None) => [1.0, 1.0, 1.0, 0.0],
        _ => [0.0, 0.0, 0.0, 0.0],
    };
    vec![value; num_frames]
}

pub fn bake_into(
    animation: &mut CrAnimation,
    skeleton: &Skeleton,
    binding: &ClipBinding,
    edits: &PoseEdits,
    policy: EncodePolicy,
) -> or_io::error::Result<BakedAnimation> {
    let baked = bake(animation, skeleton, binding, edits);
    animation.dofs = baked.dofs.clone();

    let tables = baked.tables.clone();
    or_io::codec::anim::reencode_animation(animation, policy, |dof, frame| {
        tables
            .get(dof)
            .and_then(|table| {
                let components = format_type::component_count(table.format).min(4);
                let mut out = [0.0f32; 4];
                for (component, slot) in out.iter_mut().enumerate().take(components) {
                    *slot = *table.values.get(component)?.get(frame)?;
                }
                Some(out)
            })
            .unwrap_or([0.0; 4])
    })?;
    Ok(baked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dof, v4};
    use or_io::codec::anim::decode_dof;
    use or_io::codec::anim::ChannelFormat;
    use or_io::gta5::{CrBlockStream, CrBoneData, CrSkeletonData};
    const FORMAT_COUNT: usize = 9;
    fn bone(index: u16, tag: u16, parent: i16, translation: [f32; 3]) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(translation[0], translation[1], translation[2], 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("SKEL_{index}"),
            dofs: 0,
            index,
            bone_id: tag,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    fn skeleton() -> Skeleton {
        Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1, 0],
            bones: vec![
                bone(0, 100, -1, [0.0, 0.0, 0.0]),
                bone(1, 200, 0, [1.0, 2.0, 3.0]),
            ],
            ..CrSkeletonData::default()
        })
    }

    fn constant_vec_block(value: [f32; 3]) -> CrBlockStream {
        let mut data = Vec::new();
        for v in value {
            data.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        let constant_size = data.len() as u32;
        let mut counts = [0u16; FORMAT_COUNT];
        counts[ChannelFormat::ConstantVec as usize] = 1;
        for count in counts {
            data.extend_from_slice(&count.to_le_bytes());
        }
        for entry in [0u16; 4] {
            data.extend_from_slice(&entry.to_le_bytes());
        }
        for entry in [0u16; 4] {
            data.extend_from_slice(&entry.to_le_bytes());
        }
        CrBlockStream {
            constant_size,
            channel_offset: constant_size,
            num_frames: 4,
            frame_size: 0,
            segment_size: 64,
            data,
            ..CrBlockStream::default()
        }
    }

    fn animation() -> CrAnimation {
        CrAnimation {
            num_frames: 4,
            frames_per_chunk: 127,
            duration: 1.0,
            dofs: vec![dof(100, format_type::VECTOR3, track::BONE_TRANSLATION)],
            blocks: vec![Some(Box::new(constant_vec_block([5.0, 6.0, 7.0])))],
            ..CrAnimation::default()
        }
    }

    fn binding_for(animation: &CrAnimation, skeleton: &Skeleton) -> ClipBinding {
        ClipBinding::build(skeleton, animation)
    }

    #[test]
    fn an_empty_layer_bakes_the_source_unchanged() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);
        let baked = bake(&animation, &skeleton, &binding, &PoseEdits::new());

        assert_eq!(baked.edited_dofs, 0);
        assert!(!baked.is_lossy());
        assert_eq!(baked.tables.len(), 1);
        assert!((baked.tables[0].values[0][0] - 5.0).abs() < 1e-6);
        assert!((baked.tables[0].values[2][3] - 7.0).abs() < 1e-6);
    }

    #[test]
    fn an_edit_overwrites_only_its_own_frame() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);

        let mut edits = PoseEdits::new();
        assert!(edits.set(
            EditKey {
                bone: 0,
                track: EditTrack::Translation,
                frame: 2,
            },
            EditValue::Vector(Vec3::new(99.0, 98.0, 97.0)),
        ));

        let baked = bake(&animation, &skeleton, &binding, &edits);
        assert_eq!(baked.edited_dofs, 1);
        assert!((baked.tables[0].values[0][2] - 99.0).abs() < 1e-6);
        assert!((baked.tables[0].values[0][1] - 5.0).abs() < 1e-6);
        assert!((baked.tables[0].values[0][3] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn editing_an_undriven_track_appends_a_dof() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);

        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 1,
                track: EditTrack::Rotation,
                frame: 1,
            },
            EditValue::Rotation(Quat::from_rotation_z(0.5)),
        );

        let baked = bake(&animation, &skeleton, &binding, &edits);
        assert_eq!(baked.dofs.len(), 2, "a DOF should have been appended");
        let added = &baked.dofs[1];
        assert_eq!(added.id, 200);
        assert_eq!(added.track, track::BONE_ROTATION);
        assert_eq!(added.r#type, format_type::QUATERNION);

        let table = &baked.tables[1];
        assert!(
            (table.values[3][0] - 1.0).abs() < 1e-6,
            "frame 0 w should be 1"
        );
        let expected = Quat::from_rotation_z(0.5);
        assert!((table.values[2][1] - expected.z).abs() < 1e-6);
    }

    #[test]
    fn an_appended_translation_dof_is_seeded_from_the_bind_pose() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);

        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 1,
                track: EditTrack::Translation,
                frame: 0,
            },
            EditValue::Vector(Vec3::new(9.0, 9.0, 9.0)),
        );
        let baked = bake(&animation, &skeleton, &binding, &edits);
        let table = &baked.tables[1];
        assert!((table.values[0][2] - 1.0).abs() < 1e-6);
        assert!((table.values[1][2] - 2.0).abs() < 1e-6);
        assert!((table.values[0][0] - 9.0).abs() < 1e-6);
    }

    #[test]
    fn a_value_that_does_not_fit_its_track_is_rejected() {
        let mut edits = PoseEdits::new();
        assert!(!edits.set(
            EditKey {
                bone: 0,
                track: EditTrack::Translation,
                frame: 0,
            },
            EditValue::Rotation(Quat::IDENTITY),
        ));
        assert!(edits.is_empty());

        assert!(!edits.set(
            EditKey {
                bone: 0,
                track: EditTrack::Rotation,
                frame: 0,
            },
            EditValue::Vector(Vec3::ZERO),
        ));
        assert!(edits.is_empty());
    }

    #[test]
    fn setting_a_transform_records_all_three_tracks() {
        let mut edits = PoseEdits::new();
        edits.set_transform(
            3,
            5,
            &BoneTransform {
                translation: Vec3::splat(1.0),
                rotation: Quat::from_rotation_x(0.2),
                scale: Vec3::splat(2.0),
            },
        );
        assert_eq!(edits.len(), 3);
        assert!(edits.touches_bone(3));
        assert!(!edits.touches_bone(4));
        assert_eq!(
            edits.edited_bones().into_iter().collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(edits.edited_frames(3, EditTrack::Rotation), vec![5]);
    }

    #[test]
    fn edits_can_be_removed_individually_by_track_and_by_bone() {
        let mut edits = PoseEdits::new();
        edits.set_transform(1, 0, &BoneTransform::IDENTITY);
        edits.set_transform(2, 0, &BoneTransform::IDENTITY);
        assert_eq!(edits.len(), 6);

        assert!(edits.remove(&EditKey {
            bone: 1,
            track: EditTrack::Scale,
            frame: 0,
        }));
        assert_eq!(edits.len(), 5);
        assert!(!edits.remove(&EditKey {
            bone: 1,
            track: EditTrack::Scale,
            frame: 0,
        }));

        assert_eq!(edits.clear_track(1, EditTrack::Rotation), 1);
        assert_eq!(edits.clear_bone(2), 3);
        assert_eq!(edits.len(), 1);
        edits.clear();
        assert!(edits.is_empty());
    }

    #[test]
    fn applying_a_layer_to_a_pose_overrides_only_edited_bones() {
        let skeleton = skeleton();
        let mut pose = crate::pose::Pose::bind(&skeleton);
        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 1,
                track: EditTrack::Translation,
                frame: 7,
            },
            EditValue::Vector(Vec3::new(4.0, 5.0, 6.0)),
        );

        edits.apply_to_pose(0, &mut pose);
        assert_eq!(
            pose.local(1).unwrap().translation,
            skeleton.bones[1].bind_translation
        );

        edits.apply_to_pose(7, &mut pose);
        assert_eq!(pose.local(1).unwrap().translation, Vec3::new(4.0, 5.0, 6.0));
        assert_eq!(
            pose.local(0).unwrap().translation,
            skeleton.bones[0].bind_translation
        );
    }

    #[test]
    fn an_edit_past_the_end_of_the_animation_is_ignored() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);
        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 0,
                track: EditTrack::Translation,
                frame: 99,
            },
            EditValue::Vector(Vec3::splat(1.0)),
        );
        let baked = bake(&animation, &skeleton, &binding, &edits);
        assert_eq!(baked.edited_dofs, 0);
    }

    #[test]
    fn an_edit_on_an_unknown_bone_is_ignored() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);
        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 250,
                track: EditTrack::Translation,
                frame: 0,
            },
            EditValue::Vector(Vec3::splat(1.0)),
        );
        let baked = bake(&animation, &skeleton, &binding, &edits);
        assert_eq!(baked.dofs.len(), 1);
        assert_eq!(baked.edited_dofs, 0);
    }

    #[test]
    fn an_undecodable_dof_is_reported_and_filled_from_bind() {
        let mut animation = animation();
        animation.blocks[0].as_mut().expect("a block").compact_size = 16;
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);

        let baked = bake(&animation, &skeleton, &binding, &PoseEdits::new());
        assert_eq!(baked.undecodable_dofs, 1);
        assert!(baked.is_lossy());
        assert!((baked.tables[0].values[0][0]).abs() < 1e-6);
    }

    #[test]
    fn baking_into_an_animation_re_encodes_the_edit() {
        let mut animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);

        let mut edits = PoseEdits::new();
        edits.set(
            EditKey {
                bone: 0,
                track: EditTrack::Translation,
                frame: 2,
            },
            EditValue::Vector(Vec3::new(42.0, 0.0, 0.0)),
        );

        let baked = bake_into(
            &mut animation,
            &skeleton,
            &binding,
            &edits,
            EncodePolicy::Lossless,
        )
        .expect("bake");
        assert_eq!(baked.edited_dofs, 1);

        let time = animation.internal_frame_to_time(2.0);
        let samples = decode_dof(&animation, 0, time)
            .expect("decode")
            .expect("samples");
        assert!(
            (samples.components[0] - 42.0).abs() < 1e-4,
            "got {}",
            samples.components[0]
        );

        let time = animation.internal_frame_to_time(0.0);
        let samples = decode_dof(&animation, 0, time)
            .expect("decode")
            .expect("samples");
        assert!((samples.components[0] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn baking_is_deterministic() {
        let animation = animation();
        let skeleton = skeleton();
        let binding = binding_for(&animation, &skeleton);
        let mut edits = PoseEdits::new();
        edits.set_transform(1, 1, &BoneTransform::IDENTITY);
        edits.set_transform(0, 2, &BoneTransform::IDENTITY);

        let first = bake(&animation, &skeleton, &binding, &edits);
        let second = bake(&animation, &skeleton, &binding, &edits);
        assert_eq!(first.dofs, second.dofs);
        for (a, b) in first.tables.iter().zip(second.tables.iter()) {
            assert_eq!(a.values, b.values);
        }
    }

    #[test]
    fn track_ids_round_trip() {
        for track in [
            EditTrack::Translation,
            EditTrack::Rotation,
            EditTrack::Scale,
        ] {
            assert_eq!(EditTrack::from_track_id(track.track_id()), Some(track));
            assert!(!track.name().is_empty());
        }
        assert_eq!(EditTrack::from_track_id(track::VISIBILITY), None);
    }

    #[test]
    fn edit_values_expose_four_components() {
        assert_eq!(
            EditValue::Vector(Vec3::new(1.0, 2.0, 3.0)).components(),
            [1.0, 2.0, 3.0, 0.0]
        );
        assert_eq!(
            EditValue::Rotation(Quat::IDENTITY).components(),
            [0.0, 0.0, 0.0, 1.0]
        );
    }
}

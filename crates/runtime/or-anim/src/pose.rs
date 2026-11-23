// Skin matrices are `world * inverse_bind` (reverse order explodes the mesh).
use crate::skeleton::{Skeleton, NO_PARENT};
use glam::{Mat4, Quat, Vec3};
use or_io::codec::anim::BonePose;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for BoneTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl BoneTransform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    // Hemisphere-safe blend: glam's quat lerp already does PrepareSlerp's sign flip.
    pub fn blend(&self, other: &Self, t: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.lerp(other.rotation, t).normalize(),
            scale: self.scale.lerp(other.scale, t),
        }
    }

    // Additive delta: T/S combine component-wise, R as quaternion product.
    pub fn apply_additive(&self, delta: &Self) -> Self {
        Self {
            translation: self.translation + delta.translation,
            rotation: (delta.rotation * self.rotation).normalize(),
            scale: self.scale * delta.scale,
        }
    }
}

impl From<BonePose> for BoneTransform {
    fn from(p: BonePose) -> Self {
        Self {
            translation: Vec3::new(p.translation.x, p.translation.y, p.translation.z),
            rotation: sanitized_quat(p.rotation.x, p.rotation.y, p.rotation.z, p.rotation.w),
            scale: Vec3::new(p.scale.x, p.scale.y, p.scale.z),
        }
    }
}

impl From<BoneTransform> for BonePose {
    fn from(t: BoneTransform) -> Self {
        use or_io::decl::{Vec3 as V3, Vec4 as V4};
        let (tr, r, s) = (t.translation, t.rotation, t.scale);
        Self {
            translation: V3 {
                x: tr.x,
                y: tr.y,
                z: tr.z,
            },
            rotation: V4 {
                x: r.x,
                y: r.y,
                z: r.z,
                w: r.w,
            },
            scale: V3 {
                x: s.x,
                y: s.y,
                z: s.z,
            },
        }
    }
}

fn sanitized_quat(x: f32, y: f32, z: f32, w: f32) -> Quat {
    if !(x.is_finite() && y.is_finite() && z.is_finite() && w.is_finite()) {
        return Quat::IDENTITY;
    }
    let q = Quat::from_xyzw(x, y, z, w);
    if q.length_squared() < 1e-12 {
        Quat::IDENTITY
    } else {
        q.normalize()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pose {
    locals: Vec<BoneTransform>,
    pub root_motion: BoneTransform,
}

impl Pose {
    pub fn bind(skeleton: &Skeleton) -> Self {
        Self {
            locals: skeleton
                .bones
                .iter()
                .map(|b| BoneTransform {
                    translation: b.bind_translation,
                    rotation: b.bind_rotation,
                    scale: b.bind_scale,
                })
                .collect(),
            root_motion: BoneTransform::IDENTITY,
        }
    }

    pub fn identity(bone_count: usize) -> Self {
        Self {
            locals: vec![BoneTransform::IDENTITY; bone_count],
            root_motion: BoneTransform::IDENTITY,
        }
    }

    pub fn bone_count(&self) -> usize {
        self.locals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locals.is_empty()
    }

    pub fn local(&self, bone: u16) -> Option<&BoneTransform> {
        self.locals.get(bone as usize)
    }

    pub fn local_mut(&mut self, bone: u16) -> Option<&mut BoneTransform> {
        self.locals.get_mut(bone as usize)
    }

    pub fn set_local(&mut self, bone: u16, transform: BoneTransform) {
        if let Some(slot) = self.locals.get_mut(bone as usize) {
            *slot = transform;
        }
    }

    pub fn locals(&self) -> &[BoneTransform] {
        &self.locals
    }

    pub fn resize(&mut self, bone_count: usize) {
        self.locals.resize(bone_count, BoneTransform::IDENTITY);
    }

    pub fn reset_to_bind(&mut self, skeleton: &Skeleton) {
        self.locals
            .resize(skeleton.bone_count(), BoneTransform::IDENTITY);
        for (slot, bone) in self.locals.iter_mut().zip(skeleton.bones.iter()) {
            slot.translation = bone.bind_translation;
            slot.rotation = bone.bind_rotation;
            slot.scale = bone.bind_scale;
        }
        self.root_motion = BoneTransform::IDENTITY;
    }

    pub fn blend(&self, other: &Self, t: f32) -> Self {
        let mut out = self.clone();
        for (slot, rhs) in out.locals.iter_mut().zip(other.locals.iter()) {
            *slot = slot.blend(rhs, t);
        }
        out.root_motion = self.root_motion.blend(&other.root_motion, t);
        out
    }

    pub fn world_matrices(&self, skeleton: &Skeleton) -> Vec<Mat4> {
        let mut out = Vec::new();
        self.world_matrices_into(skeleton, &mut out);
        out
    }

    pub fn world_matrices_into(&self, skeleton: &Skeleton, out: &mut Vec<Mat4>) {
        let count = skeleton.bone_count();
        out.clear();
        out.resize(count, Mat4::IDENTITY);
        for &bone_index in &skeleton.eval_order {
            let i = bone_index as usize;
            let local = self
                .locals
                .get(i)
                .copied()
                .unwrap_or(BoneTransform::IDENTITY)
                .to_matrix();
            let parent = skeleton.parents[i];
            out[i] = if parent == NO_PARENT {
                local
            } else {
                out[parent as usize] * local
            };
        }
    }

    pub fn skin_matrices(&self, skeleton: &Skeleton) -> Vec<Mat4> {
        let world = self.world_matrices(skeleton);
        let mut out = Vec::new();
        skin_matrices_from_world(skeleton, &world, &mut out);
        out
    }

    pub fn pose_into(&self, skeleton: &Skeleton, world: &mut Vec<Mat4>, skin: &mut Vec<Mat4>) {
        self.world_matrices_into(skeleton, world);
        skin_matrices_from_world(skeleton, world, skin);
    }
}

// Prefer skeleton-computed inverse_bind over any file-shipped copy.
pub fn skin_matrices_from_world(skeleton: &Skeleton, world: &[Mat4], out: &mut Vec<Mat4>) {
    let count = skeleton.bone_count();
    out.clear();
    out.resize(count, Mat4::IDENTITY);
    for (bone, slot) in out.iter_mut().enumerate() {
        let w = world.get(bone).copied().unwrap_or(Mat4::IDENTITY);
        let inverse_bind = skeleton
            .inverse_bind
            .get(bone)
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        *slot = w * inverse_bind;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::v4;
    use or_io::gta5::{CrBoneData, CrSkeletonData};
    fn bone(index: u16, tag: u16, parent: i16, translation: [f32; 3]) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(translation[0], translation[1], translation[2], 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("BONE_{index}"),
            dofs: 0,
            index,
            bone_id: tag,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    fn chain() -> Skeleton {
        let data = CrSkeletonData {
            parent_indices: vec![-1, 0, 1],
            bones: vec![
                bone(0, 100, -1, [0.0, 0.0, 0.0]),
                bone(1, 200, 0, [1.0, 0.0, 0.0]),
                bone(2, 300, 1, [1.0, 0.0, 0.0]),
            ],
            ..CrSkeletonData::default()
        };
        Skeleton::build(&data)
    }

    #[test]
    fn bind_pose_world_matches_the_skeleton_bind() {
        let skeleton = chain();
        let pose = Pose::bind(&skeleton);
        let world = pose.world_matrices(&skeleton);
        for (i, m) in world.iter().enumerate() {
            let delta = (*m - skeleton.bind_world[i]).to_cols_array();
            assert!(delta.iter().all(|v| v.abs() < 1e-6), "bone {i} differs");
        }
    }

    #[test]
    fn bind_pose_skin_matrices_are_identity() {
        let skeleton = chain();
        let skin = Pose::bind(&skeleton).skin_matrices(&skeleton);
        for (i, m) in skin.iter().enumerate() {
            let delta = (*m - Mat4::IDENTITY).to_cols_array();
            assert!(
                delta.iter().all(|v| v.abs() < 1e-5),
                "bone {i} skin matrix was not identity: {m:?}"
            );
        }
    }

    #[test]
    fn rotating_a_parent_carries_the_child_subtree() {
        let skeleton = chain();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(0).unwrap().rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let world = pose.world_matrices(&skeleton);

        let child = world[1].w_axis.truncate();
        assert!(
            (child - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5,
            "child at {child}"
        );
        let grandchild = world[2].w_axis.truncate();
        assert!(
            (grandchild - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-5,
            "grandchild at {grandchild}"
        );
    }

    #[test]
    fn skin_matrix_moves_a_bind_space_vertex_with_its_bone() {
        let skeleton = chain();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(0).unwrap().translation = Vec3::new(0.0, 5.0, 0.0);
        let skin = pose.skin_matrices(&skeleton);

        let authored = Vec3::new(2.0, 0.0, 0.0);
        let posed = skin[2].transform_point3(authored);
        assert!(
            (posed - Vec3::new(2.0, 5.0, 0.0)).length() < 1e-5,
            "vertex landed at {posed}"
        );
    }

    #[test]
    fn reversed_multiplication_order_would_be_wrong() {
        // Guards the documented trap: world * inverse_bind, not the reverse.
        let skeleton = chain();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(0).unwrap().rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let world = pose.world_matrices(&skeleton);

        let correct = world[2] * skeleton.inverse_bind[2];
        let reversed = skeleton.inverse_bind[2] * world[2];
        let authored = Vec3::new(2.0, 0.0, 0.0);
        assert!((correct.transform_point3(authored) - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-5);
        assert!((reversed.transform_point3(authored) - Vec3::new(0.0, 2.0, 0.0)).length() > 1e-3);
    }

    #[test]
    fn pose_into_fills_both_buffers_and_reuses_them() {
        let skeleton = chain();
        let pose = Pose::bind(&skeleton);
        let mut world = Vec::new();
        let mut skin = Vec::new();
        pose.pose_into(&skeleton, &mut world, &mut skin);
        assert_eq!(world.len(), 3);
        assert_eq!(skin.len(), 3);

        pose.pose_into(&skeleton, &mut world, &mut skin);
        assert_eq!(world.len(), 3);
        assert_eq!(skin.len(), 3);
    }

    #[test]
    fn a_pose_shorter_than_the_skeleton_falls_back_to_identity_locals() {
        let skeleton = chain();
        let short = Pose::identity(1);
        let world = short.world_matrices(&skeleton);
        assert_eq!(world.len(), 3);
    }

    #[test]
    fn bone_transform_round_trips_through_bonepose() {
        let original = BoneTransform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_y(0.5),
            scale: Vec3::new(2.0, 2.0, 2.0),
        };
        let via_io: BonePose = original.into();
        let back: BoneTransform = via_io.into();
        assert!((back.translation - original.translation).length() < 1e-6);
        assert!((back.scale - original.scale).length() < 1e-6);
        assert!(back.rotation.dot(original.rotation).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn non_unit_decoded_quaternions_are_normalised() {
        use or_io::decl::{Vec3 as V3, Vec4 as V4};
        let pose = BonePose {
            translation: V3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: V4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.5,
            },
            scale: V3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        };
        let t: BoneTransform = pose.into();
        assert!((t.rotation.length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zero_and_nonfinite_quaternions_become_identity() {
        use or_io::decl::{Vec3 as V3, Vec4 as V4};
        for bad in [
            V4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            V4 {
                x: f32::NAN,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
        ] {
            let t: BoneTransform = BonePose {
                translation: V3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: bad,
                scale: V3 {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                },
            }
            .into();
            assert_eq!(t.rotation, Quat::IDENTITY);
        }
    }

    #[test]
    fn blending_poses_interpolates_every_bone() {
        let skeleton = chain();
        let a = Pose::bind(&skeleton);
        let mut b = Pose::bind(&skeleton);
        b.local_mut(1).unwrap().translation = Vec3::new(3.0, 0.0, 0.0);
        let mid = a.blend(&b, 0.5);
        assert!((mid.local(1).unwrap().translation.x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn additive_delta_composes_rotation_and_offsets_translation() {
        let base = BoneTransform {
            translation: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        };
        let delta = BoneTransform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(2.0),
        };
        let out = base.apply_additive(&delta);
        assert_eq!(out.translation, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(out.scale, Vec3::splat(2.0));
        assert!(out.rotation.dot(delta.rotation).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn reset_to_bind_restores_without_reallocating() {
        let skeleton = chain();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(2).unwrap().translation = Vec3::splat(99.0);
        pose.root_motion.translation = Vec3::splat(7.0);
        pose.reset_to_bind(&skeleton);
        assert_eq!(
            pose.local(2).unwrap().translation,
            skeleton.bones[2].bind_translation
        );
        assert_eq!(pose.root_motion, BoneTransform::IDENTITY);
    }

    #[test]
    fn empty_skeleton_poses_to_nothing() {
        let skeleton = Skeleton::build(&CrSkeletonData::default());
        let pose = Pose::bind(&skeleton);
        assert!(pose.is_empty());
        assert!(pose.world_matrices(&skeleton).is_empty());
        assert!(pose.skin_matrices(&skeleton).is_empty());
    }
}

use glam::{Mat4, Quat, Vec3, Vec4};
use or_io::gta5::{CrBoneData, CrSkeletonData, Mat34V};
use std::collections::HashMap;
pub const NO_PARENT: u16 = u16::MAX;
#[derive(Debug, Clone)]
pub struct Bone {
    pub index: u16,
    pub tag: u16,
    pub name: String,
    pub parent: u16,
    pub bind_translation: Vec3,
    pub bind_rotation: Quat,
    pub bind_scale: Vec3,
    pub dofs: u16,
}

impl Bone {
    pub fn bind_local(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            self.bind_scale,
            self.bind_rotation,
            self.bind_translation,
        )
    }

    pub fn is_root(&self) -> bool {
        self.parent == NO_PARENT
    }
}

#[derive(Debug, Clone, Default)]
pub struct Skeleton {
    // File-order bones: clip/geometry `matrix_palette` indices refer here.
    pub bones: Vec<Bone>,
    pub parents: Vec<u16>,
    pub eval_order: Vec<u16>,
    pub bind_world: Vec<Mat4>,
    pub inverse_bind: Vec<Mat4>,
    /// Per-bone **local** bind matrices as the file stored them
    /// (`crSkeletonData::m_DefaultTransforms`), when present.
    ///
    /// Despite the name this array is *not* the world bind pose — it is the
    /// engine's precomposed form of each bone's own
    /// `m_DefaultRotation`/`Translation`/`Scale`, so `file_local_bind[i]` equals
    /// `bones[i].bind_local()` and the root is the only bone where it also
    /// happens to equal the world matrix. Measured across 62 retail fragment and
    /// drawable skeletons; see [`Self::local_bind_deviation`].
    pub file_local_bind: Vec<Mat4>,
    // File-shipped inverse binds — golden check for FK via `inverse_bind_deviation`.
    pub file_inverse_bind: Vec<Mat4>,
    tag_to_index: HashMap<u16, u16>,
    file_order_was_topological: bool,
    pub signature: u32,
}

impl Skeleton {
    pub fn build(data: &CrSkeletonData) -> Self {
        let count = data.bones.len();
        let mut bones = Vec::with_capacity(count);
        let mut parents = Vec::with_capacity(count);
        let mut tag_to_index = HashMap::with_capacity(count);

        for (index, bone) in data.bones.iter().enumerate() {
            let index = index as u16;
            let parent = resolve_parent(data, bone, index, count);
            parents.push(parent);
            tag_to_index.entry(bone.bone_id).or_insert(index);
            bones.push(Bone {
                index,
                tag: bone.bone_id,
                name: bone.name.clone(),
                parent,
                bind_translation: vec3_of(
                    bone.translation.x,
                    bone.translation.y,
                    bone.translation.z,
                ),
                bind_rotation: quat_of(
                    bone.rotation.x,
                    bone.rotation.y,
                    bone.rotation.z,
                    bone.rotation.w,
                ),
                bind_scale: bind_scale_of(bone),
                dofs: bone.dofs,
            });
        }

        let (eval_order, file_order_was_topological) = topological_order(&parents);

        let mut skeleton = Self {
            bones,
            parents,
            eval_order,
            bind_world: Vec::new(),
            inverse_bind: Vec::new(),
            file_local_bind: data.default_transforms.iter().map(mat4_of).collect(),
            file_inverse_bind: data.inverse_transforms.iter().map(mat4_of).collect(),
            tag_to_index,
            file_order_was_topological,
            signature: data.signature,
        };
        skeleton.rebuild_bind_matrices();
        skeleton
    }

    // Rebuild bind_world / inverse_bind from local binds in eval_order (topological FK).
    fn rebuild_bind_matrices(&mut self) {
        let count = self.bones.len();
        let mut world = vec![Mat4::IDENTITY; count];
        for &bone_index in &self.eval_order {
            let i = bone_index as usize;
            let local = self.bones[i].bind_local();
            let parent = self.parents[i];
            world[i] = if parent == NO_PARENT {
                local
            } else {
                world[parent as usize] * local
            };
        }
        self.inverse_bind = world.iter().map(|m| m.inverse()).collect();
        self.bind_world = world;
    }

    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bones.is_empty()
    }

    pub fn index_for_tag(&self, tag: u16) -> Option<u16> {
        self.tag_to_index.get(&tag).copied()
    }

    pub fn bone_for_tag(&self, tag: u16) -> Option<&Bone> {
        self.index_for_tag(tag).map(|i| &self.bones[i as usize])
    }

    pub fn bone(&self, index: u16) -> Option<&Bone> {
        self.bones.get(index as usize)
    }

    pub fn index_for_name(&self, name: &str) -> Option<u16> {
        self.bones
            .iter()
            .position(|b| b.name == name)
            .map(|i| i as u16)
    }

    pub fn tags(&self) -> Vec<u16> {
        self.bones.iter().map(|b| b.tag).collect()
    }

    pub fn children_of(&self, index: u16) -> Vec<u16> {
        self.parents
            .iter()
            .enumerate()
            .filter(|(_, &p)| p == index)
            .map(|(i, _)| i as u16)
            .collect()
    }

    pub fn roots(&self) -> Vec<u16> {
        self.parents
            .iter()
            .enumerate()
            .filter(|(_, &p)| p == NO_PARENT)
            .map(|(i, _)| i as u16)
            .collect()
    }

    pub fn depth_of(&self, index: u16) -> u16 {
        let mut depth = 0;
        let mut cursor = index;
        for _ in 0..self.bones.len() {
            match self.parents.get(cursor as usize).copied() {
                Some(NO_PARENT) | None => break,
                Some(parent) => {
                    depth += 1;
                    cursor = parent;
                }
            }
        }
        depth
    }

    pub fn file_order_was_topological(&self) -> bool {
        self.file_order_was_topological
    }

    // Max |computed inverse_bind − file inverse_bind|; FK golden check.
    pub fn inverse_bind_deviation(&self) -> Option<f32> {
        matrix_deviation(&self.inverse_bind, &self.file_inverse_bind)
    }

    pub fn local_bind_deviation(&self) -> Option<f32> {
        if self.file_local_bind.is_empty() {
            return None;
        }
        let computed: Vec<Mat4> = self.bones.iter().map(|b| b.bind_local()).collect();
        matrix_deviation(&computed, &self.file_local_bind)
    }
}

fn matrix_deviation(computed: &[Mat4], from_file: &[Mat4]) -> Option<f32> {
    if from_file.is_empty() || from_file.len() != computed.len() {
        return None;
    }
    let mut worst = 0.0f32;
    for (a, b) in computed.iter().zip(from_file.iter()) {
        for column in 0..4 {
            let ca = a.col(column);
            let cb = b.col(column);
            for row in 0..4 {
                worst = worst.max((ca[row] - cb[row]).abs());
            }
        }
    }
    Some(worst)
}

fn resolve_parent(data: &CrSkeletonData, bone: &CrBoneData, index: u16, count: usize) -> u16 {
    let raw = data
        .parent_indices
        .get(index as usize)
        .copied()
        .unwrap_or(bone.parent_index);
    if raw < 0 {
        return NO_PARENT;
    }
    let parent = raw as usize;
    if parent >= count || parent == index as usize {
        NO_PARENT
    } else {
        parent as u16
    }
}

fn bind_scale_of(bone: &CrBoneData) -> Vec3 {
    let s = vec3_of(bone.scale.x, bone.scale.y, bone.scale.z);
    if s.x.abs() < f32::EPSILON && s.y.abs() < f32::EPSILON && s.z.abs() < f32::EPSILON {
        Vec3::ONE
    } else {
        s
    }
}

fn vec3_of(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(finite(x), finite(y), finite(z))
}

fn quat_of(x: f32, y: f32, z: f32, w: f32) -> Quat {
    let q = glam::Quat::from_xyzw(finite(x), finite(y), finite(z), finite(w));
    if q.length_squared() < 1e-12 {
        Quat::IDENTITY
    } else {
        q.normalize()
    }
}

fn finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

pub fn mat4_of(m: &Mat34V) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(m.col0.x, m.col0.y, m.col0.z, 0.0),
        Vec4::new(m.col1.x, m.col1.y, m.col1.z, 0.0),
        Vec4::new(m.col2.x, m.col2.y, m.col2.z, 0.0),
        Vec4::new(m.col3.x, m.col3.y, m.col3.z, 1.0),
    )
}

fn topological_order(parents: &[u16]) -> (Vec<u16>, bool) {
    let count = parents.len();
    let mut children: Vec<Vec<u16>> = vec![Vec::new(); count];
    let mut order = Vec::with_capacity(count);
    let mut stack: Vec<u16> = Vec::new();

    for (index, &parent) in parents.iter().enumerate() {
        if parent == NO_PARENT {
            stack.push(index as u16);
        } else {
            children[parent as usize].push(index as u16);
        }
    }
    stack.reverse();

    let mut visited = vec![false; count];
    while let Some(bone) = stack.pop() {
        let i = bone as usize;
        if visited[i] {
            continue;
        }
        visited[i] = true;
        order.push(bone);
        for &child in children[i].iter().rev() {
            if !visited[child as usize] {
                stack.push(child);
            }
        }
    }

    for (index, seen) in visited.iter().enumerate() {
        if !seen {
            order.push(index as u16);
        }
    }

    let already_sorted = order.iter().enumerate().all(|(i, &b)| i as u16 == b);
    (order, already_sorted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{m34, v4};
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

    fn chain() -> CrSkeletonData {
        let bones = vec![
            bone(0, 100, -1, [0.0, 0.0, 0.0]),
            bone(1, 200, 0, [1.0, 0.0, 0.0]),
            bone(2, 300, 1, [1.0, 0.0, 0.0]),
        ];
        CrSkeletonData {
            parent_indices: vec![-1, 0, 1],
            bones,
            signature: 0xABCD,
            ..CrSkeletonData::default()
        }
    }

    #[test]
    fn builds_bones_parents_and_tags() {
        let skeleton = Skeleton::build(&chain());
        assert_eq!(skeleton.bone_count(), 3);
        assert_eq!(skeleton.parents, vec![NO_PARENT, 0, 1]);
        assert_eq!(skeleton.index_for_tag(200), Some(1));
        assert_eq!(skeleton.index_for_tag(999), None);
        assert_eq!(skeleton.signature, 0xABCD);
        assert_eq!(skeleton.tags(), vec![100, 200, 300]);
    }

    #[test]
    fn forward_kinematics_accumulates_down_the_chain() {
        let skeleton = Skeleton::build(&chain());
        assert_eq!(skeleton.bind_world[0].w_axis.truncate(), Vec3::ZERO);
        assert_eq!(
            skeleton.bind_world[1].w_axis.truncate(),
            Vec3::new(1.0, 0.0, 0.0)
        );
        assert_eq!(
            skeleton.bind_world[2].w_axis.truncate(),
            Vec3::new(2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn inverse_bind_undoes_bind_world() {
        let skeleton = Skeleton::build(&chain());
        for i in 0..skeleton.bone_count() {
            let product = skeleton.inverse_bind[i] * skeleton.bind_world[i];
            let delta = (product - Mat4::IDENTITY).to_cols_array();
            assert!(
                delta.iter().all(|v| v.abs() < 1e-5),
                "bone {i} inverse bind did not cancel"
            );
        }
    }

    #[test]
    fn file_order_is_reported_as_topological_when_it_is() {
        let skeleton = Skeleton::build(&chain());
        assert!(skeleton.file_order_was_topological());
        assert_eq!(skeleton.eval_order, vec![0, 1, 2]);
    }

    #[test]
    fn out_of_order_bones_are_reordered() {
        let bones = vec![
            bone(0, 100, 1, [1.0, 0.0, 0.0]),
            bone(1, 200, -1, [5.0, 0.0, 0.0]),
        ];
        let data = CrSkeletonData {
            parent_indices: vec![1, -1],
            bones,
            ..CrSkeletonData::default()
        };
        let skeleton = Skeleton::build(&data);
        assert!(!skeleton.file_order_was_topological());
        assert_eq!(skeleton.eval_order, vec![1, 0]);
        assert_eq!(
            skeleton.bind_world[0].w_axis.truncate(),
            Vec3::new(6.0, 0.0, 0.0)
        );
    }

    #[test]
    fn a_parent_cycle_does_not_hang_and_covers_every_bone() {
        let bones = vec![bone(0, 1, 1, [0.0; 3]), bone(1, 2, 0, [0.0; 3])];
        let data = CrSkeletonData {
            parent_indices: vec![1, 0],
            bones,
            ..CrSkeletonData::default()
        };
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.eval_order.len(), 2);
        assert_eq!(skeleton.depth_of(0), 2);
    }

    #[test]
    fn out_of_range_and_self_parents_become_roots() {
        let bones = vec![bone(0, 1, 7, [0.0; 3]), bone(1, 2, 1, [0.0; 3])];
        let data = CrSkeletonData {
            parent_indices: vec![7, 1],
            bones,
            ..CrSkeletonData::default()
        };
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.parents, vec![NO_PARENT, NO_PARENT]);
        assert_eq!(skeleton.roots(), vec![0, 1]);
    }

    #[test]
    fn zero_bind_scale_is_replaced_with_unit() {
        let mut data = chain();
        data.bones[1].scale = v4(0.0, 0.0, 0.0, 0.0);
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.bones[1].bind_scale, Vec3::ONE);
        assert!(skeleton.inverse_bind[1].is_finite());
    }

    #[test]
    fn zero_bind_rotation_is_replaced_with_identity() {
        let mut data = chain();
        data.bones[1].rotation = v4(0.0, 0.0, 0.0, 0.0);
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.bones[1].bind_rotation, Quat::IDENTITY);
    }

    #[test]
    fn non_finite_bind_values_are_sanitised() {
        let mut data = chain();
        data.bones[1].translation = v4(f32::NAN, 1.0, f32::INFINITY, 0.0);
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.bones[1].bind_translation, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn children_and_depth_walk_the_tree() {
        let skeleton = Skeleton::build(&chain());
        assert_eq!(skeleton.children_of(0), vec![1]);
        assert_eq!(skeleton.children_of(2), Vec::<u16>::new());
        assert_eq!(skeleton.roots(), vec![0]);
        assert_eq!(skeleton.depth_of(0), 0);
        assert_eq!(skeleton.depth_of(2), 2);
    }

    #[test]
    fn deviation_is_none_without_file_matrices_and_zero_when_they_agree() {
        let skeleton = Skeleton::build(&chain());
        assert!(skeleton.inverse_bind_deviation().is_none());
        assert!(skeleton.local_bind_deviation().is_none());

        let mut with_file = skeleton.clone();
        with_file.file_inverse_bind = with_file.inverse_bind.clone();
        with_file.file_local_bind = with_file.bones.iter().map(|b| b.bind_local()).collect();
        assert_eq!(with_file.inverse_bind_deviation(), Some(0.0));
        assert_eq!(with_file.local_bind_deviation(), Some(0.0));
    }

    #[test]
    fn file_local_bind_is_local_not_world() {
        let mut data = chain();
        data.default_transforms = data
            .bones
            .iter()
            .map(|b| {
                m34(
                    v4(1.0, 0.0, 0.0, 0.0),
                    v4(0.0, 1.0, 0.0, 0.0),
                    v4(0.0, 0.0, 1.0, 0.0),
                    v4(b.translation.x, b.translation.y, b.translation.z, 0.0),
                )
            })
            .collect();

        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.local_bind_deviation(), Some(0.0));
        assert_eq!(
            skeleton.bind_world[2].w_axis.truncate(),
            Vec3::new(2.0, 0.0, 0.0)
        );
        assert_eq!(
            skeleton.file_local_bind[2].w_axis.truncate(),
            Vec3::new(1.0, 0.0, 0.0)
        );
    }

    #[test]
    fn mat34_conversion_forces_the_translation_w_to_one() {
        let m = m34(
            v4(1.0, 0.0, 0.0, 0.0),
            v4(0.0, 1.0, 0.0, 0.0),
            v4(0.0, 0.0, 1.0, 0.0),
            v4(3.0, 4.0, 5.0, 0.0),
        );
        let converted = mat4_of(&m);
        assert_eq!(converted.w_axis, Vec4::new(3.0, 4.0, 5.0, 1.0));
        assert_eq!(
            converted.transform_point3(Vec3::ZERO),
            Vec3::new(3.0, 4.0, 5.0)
        );
    }

    #[test]
    fn duplicate_tags_resolve_to_the_lowest_index() {
        let bones = vec![bone(0, 42, -1, [0.0; 3]), bone(1, 42, 0, [0.0; 3])];
        let data = CrSkeletonData {
            parent_indices: vec![-1, 0],
            bones,
            ..CrSkeletonData::default()
        };
        let skeleton = Skeleton::build(&data);
        assert_eq!(skeleton.index_for_tag(42), Some(0));
    }

    #[test]
    fn empty_skeleton_is_harmless() {
        let skeleton = Skeleton::build(&CrSkeletonData::default());
        assert!(skeleton.is_empty());
        assert_eq!(skeleton.bone_count(), 0);
        assert!(skeleton.roots().is_empty());
        assert!(skeleton.inverse_bind_deviation().is_none());
        assert!(skeleton.local_bind_deviation().is_none());
    }

    #[test]
    fn names_resolve_to_indices() {
        let skeleton = Skeleton::build(&chain());
        assert_eq!(skeleton.index_for_name("BONE_1"), Some(1));
        assert_eq!(skeleton.index_for_name("nope"), None);
    }
}

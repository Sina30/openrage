//! A rig: bones in a hierarchy, the matrices to skin with, and optional
//! per-bone limits. Shared by every drawable-bearing format and both targets.

use super::drawable_codecs::BoneTags;
use super::shared::{Mat34V, PgBase, Vec4V};
use crate::decl::*;

/// `crBoneData`: one bone, in a skeleton's bone run.
#[record(size = 0x50)]
pub struct CrBoneData {
    /// Rest-pose rotation, as a quaternion.
    pub rotation: Vec4V,
    pub translation: Vec4V,
    pub scale: Vec4V,
    /// The next sibling's index, or -1.
    pub next_index: i16,
    /// The parent's index, or -1 for a root.
    #[pad_after(4)]
    pub parent_index: i16,
    pub name: String,
    pub dofs: u16,
    pub index: u16,
    /// ⚠ What an animation addresses the bone by, and not its index: adding a
    /// bone renumbers indices and keeps ids.
    pub bone_id: u16,
    #[pad_after(8)]
    pub mirror_index: u16,
}

/// `crSkeletonData`: bones and four runs parallel to them, and the table that finds a bone by its id.
///
/// ⚠ **The bone-id table is derived state**: a bucket array of chained
/// entries written from the bone run by [`BoneTags`], and an open refuses a
/// table that is not that derivation.
#[record(size = 0x70)]
pub struct CrSkeletonData {
    pub base: PgBase,
    #[codec(Ptr<Opaque>)]
    bone_tags: BoneTags,
    /// Buckets in the bone-id table; zero where the file stores none.
    pub bone_tag_slot_count: u16,
    #[note(
        "the bone count in 6 of 13 skeletons and one less in the other 7: nothing measured says \
         which, so carried"
    )]
    bone_tags_count: u16,
    pub bone_tags_flags: u32,
    #[len(field = "num_bones")]
    pub bones: Vec<CrBoneData>,
    #[len(field = "num_bones")]
    pub inverse_transforms: Vec<Mat34V>,
    #[len(field = "num_bones")]
    pub default_transforms: Vec<Mat34V>,
    #[len(field = "num_bones")]
    pub parent_indices: Vec<i16>,
    /// Pairs of child and parent indices.
    #[len(field = "num_child_parents")]
    pub child_parent_indices: Vec<u16>,
    #[note("null in every skeleton shipped")]
    properties: Ptr<Opaque>,
    /// Checked against an animation's before it plays.
    pub signature: u32,
    pub signature_non_chiral: u32,
    pub signature_comprehensive: u32,
    #[init(1)]
    ref_count: u16,
    num_bones: u16,
    #[pad_after(0xe)]
    num_child_parents: u16,
}

impl CrSkeletonData {
    /// Appends `bone` under its `parent_index` with its two transforms, and returns its index.
    ///
    /// # Errors
    ///
    /// Refuses a `bone_id` a bone already holds, because both land in one
    /// bucket of the bone-id table and every lookup finds the first, and a
    /// parent the skeleton does not hold.
    pub fn add_bone(
        &mut self,
        mut bone: CrBoneData,
        inverse_transform: Mat34V,
        default_transform: Mat34V,
    ) -> Result<usize, crate::Error> {
        let refuse = |context: String| crate::Error::InvalidData { context };
        if let Some(existing) = self.bones.iter().position(|b| b.bone_id == bone.bone_id) {
            return Err(refuse(format!(
                "bone {existing} (`{}`) already holds id {:#x}, so `{}` would be unreachable \
                 through the bone-id table",
                self.bones[existing].name, bone.bone_id, bone.name
            )));
        }
        let index = self.bones.len();
        let parent = bone.parent_index;
        if parent < -1 || (parent >= 0 && parent as usize >= index) {
            return Err(refuse(format!(
                "`{}` names parent {parent} in a skeleton of {index} bone(s)",
                bone.name
            )));
        }
        let Ok(next) = i16::try_from(index) else {
            return Err(refuse(format!(
                "a skeleton holds at most {} bones",
                i16::MAX
            )));
        };
        if let Some(last) = self
            .bones
            .iter_mut()
            .rev()
            .find(|b| b.parent_index == parent && b.next_index == -1)
        {
            last.next_index = next;
        }
        bone.index = next as u16;
        bone.next_index = -1;
        self.bones.push(bone);
        self.inverse_transforms.push(inverse_transform);
        self.default_transforms.push(default_transform);
        self.parent_indices.push(parent);
        Ok(index)
    }

    /// Derives again, from the bone data, the default and inverse transform of bone `index` and of every bone under it, and the comprehensive signature.
    ///
    /// The signature and the non-chiral one hash what an edit of a bone's numbers keeps (each
    /// bone's id and degrees of freedom, in order), and so does the bone-id table, so they stay.
    ///
    /// # Errors
    ///
    /// Changes nothing and refuses a bone the skeleton does not hold, transform runs that are not
    /// one entry to a bone, a parent index that is not a bone or whose chain does not end, and a
    /// bone whose chain scales to nothing along an axis, which has no inverse.
    pub fn rebuild_rest_pose(&mut self, index: usize) -> Result<(), crate::Error> {
        let refuse = |context: String| crate::Error::InvalidData { context };
        let count = self.bones.len();
        if index >= count {
            return Err(refuse(format!("bone {index} in a skeleton of {count}")));
        }
        if self.default_transforms.len() != count || self.inverse_transforms.len() != count {
            return Err(refuse(format!(
                "{count} bone(s), {} default and {} inverse transform(s)",
                self.default_transforms.len(),
                self.inverse_transforms.len()
            )));
        }
        let mut rebuilt = Vec::new();
        for bone in 0..count {
            let chain = self.chain_of(bone)?;
            if chain.contains(&index) {
                let (axes, at) = self.rest_in_model_space(&chain);
                let held = self.inverse_transforms[bone].clone();
                let Some(inverse) = inverse_transform(axes, at, held) else {
                    return Err(refuse(format!(
                        "bone {bone} (`{}`) and the bones above it scale to nothing along an axis, \
                         which has no inverse",
                        self.bones[bone].name
                    )));
                };
                rebuilt.push((
                    bone,
                    default_transform(&self.bones[bone], self.default_transforms[bone].clone()),
                    inverse,
                ));
            }
        }
        for (bone, default, inverse) in rebuilt {
            self.default_transforms[bone] = default;
            self.inverse_transforms[bone] = inverse;
        }
        self.signature_comprehensive = comprehensive_signature(&self.bones);
        Ok(())
    }

    /// Bone `bone` then each parent above it, up to a root.
    fn chain_of(&self, bone: usize) -> Result<Vec<usize>, crate::Error> {
        let refuse = |context: String| crate::Error::InvalidData { context };
        let mut chain = vec![bone];
        loop {
            let last = chain[chain.len() - 1];
            let parent = self.bones[last].parent_index;
            if parent == -1 {
                return Ok(chain);
            }
            let Some(parent) = usize::try_from(parent)
                .ok()
                .filter(|&p| p < self.bones.len())
            else {
                return Err(refuse(format!("bone {last} names parent {parent}")));
            };
            if chain.len() == self.bones.len() {
                return Err(refuse(format!("the parents of bone {bone} loop")));
            }
            chain.push(parent);
        }
    }

    /// Where the first bone of `chain` rests in the skeleton's own space: its three axes, scale included, and its translation.
    fn rest_in_model_space(&self, chain: &[usize]) -> ([[f64; 3]; 3], [f64; 3]) {
        let mut axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut at = [0.0; 3];
        for &bone in chain {
            let bone = &self.bones[bone];
            let local = linear_of(bone);
            axes = axes.map(|axis| turn(&local, axis));
            let moved = turn(&local, at);
            at = [
                moved[0] + f64::from(bone.translation.x),
                moved[1] + f64::from(bone.translation.y),
                moved[2] + f64::from(bone.translation.z),
            ];
        }
        (axes, at)
    }
}

/// The three axes a quaternion turns the axes of space into.
fn axes_of(q: &Vec4V) -> [[f64; 3]; 3] {
    let [x, y, z, w] = [q.x, q.y, q.z, q.w].map(f64::from);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// `v` carried through the 3x3 whose columns are `axes`.
fn turn(axes: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| axes[0][i] * v[0] + axes[1][i] * v[1] + axes[2][i] * v[2])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A bone's own 3x3: its rotation's axes, column `k` stretched by scale `k`.
///
/// ⚠ Rotation times scale, so the bone stretches along its own axes, as `or_anim::Bone::bind_local` composes
/// them; scale times rotation would stretch along its parent's and differ for a bone both scaled and turned.
/// No staged bone has a scale other than 1, so no shipped file measures the order: it is the animation
/// skeleton's, which the preview draws and the tests check against.
fn linear_of(bone: &CrBoneData) -> [[f64; 3]; 3] {
    let scale = [bone.scale.x, bone.scale.y, bone.scale.z].map(f64::from);
    let mut axes = axes_of(&bone.rotation);
    for (axis, stretch) in axes.iter_mut().zip(scale) {
        *axis = axis.map(|lane| lane * stretch);
    }
    axes
}

/// A bone's own transform: [`linear_of`] and its translation.
///
/// ⚠ Only the first three lanes of each column are written; the fourth stays what `held` holds.
fn default_transform(bone: &CrBoneData, mut held: Mat34V) -> Mat34V {
    let columns = [&mut held.col0, &mut held.col1, &mut held.col2];
    for (column, axis) in columns.into_iter().zip(linear_of(bone)) {
        (column.x, column.y, column.z) = (axis[0] as f32, axis[1] as f32, axis[2] as f32);
    }
    let at = &bone.translation;
    (held.col3.x, held.col3.y, held.col3.z) = (at.x, at.y, at.z);
    held
}

/// The inverse of the transform with columns `axes` and translation `at`, scale included, so the world matrix of a bone times it is the identity; `None` where there is none.
fn inverse_transform(axes: [[f64; 3]; 3], at: [f64; 3], mut held: Mat34V) -> Option<Mat34V> {
    let [a, b, c] = axes;
    let rows = [cross(b, c), cross(c, a), cross(a, b)];
    let det: f64 = (0..3).map(|i| a[i] * rows[0][i]).sum();
    let inverse: [[f64; 3]; 3] =
        std::array::from_fn(|column| std::array::from_fn(|row| rows[row][column] / det));
    let back = turn(&inverse, at);
    let columns = [inverse[0], inverse[1], inverse[2], back.map(|v| -v)];
    if !columns.iter().flatten().all(|&v| (v as f32).is_finite()) {
        return None;
    }
    let lanes = [
        &mut held.col0,
        &mut held.col1,
        &mut held.col2,
        &mut held.col3,
    ];
    for (lane, column) in lanes.into_iter().zip(columns) {
        (lane.x, lane.y, lane.z) = (column[0] as f32, column[1] as f32, column[2] as f32);
    }
    Some(held)
}

/// CRC-32 over each bone's degrees of freedom and id, then its translation, rotation and scale as four lanes each.
fn comprehensive_signature(bones: &[CrBoneData]) -> u32 {
    let mut crc = flate2::Crc::new();
    for bone in bones {
        crc.update(&u32::from(bone.dofs).to_le_bytes());
        crc.update(&u32::from(bone.bone_id).to_le_bytes());
        for lanes in [&bone.translation, &bone.rotation, &bone.scale] {
            for lane in [lanes.x, lanes.y, lanes.z, lanes.w] {
                crc.update(&lane.to_le_bytes());
            }
        }
    }
    crc.sum()
}

/// Bytes one bone-tag table entry takes: a tag, an index and the next entry.
pub const BONE_TAG_ENTRY_SIZE: usize = 16;

/// The bone-tag table a bone run implies: tag modulo the slot count, each
/// bucket in reverse order of insertion.
pub fn bucket_bone_tags(slots: usize, tags: impl IntoIterator<Item = u16>) -> Vec<Vec<(u16, u32)>> {
    let mut buckets = vec![Vec::new(); slots];
    if slots == 0 {
        return buckets;
    }
    for (index, tag) in tags.into_iter().enumerate() {
        buckets[tag as usize % slots].push((tag, index as u32));
    }
    for bucket in &mut buckets {
        bucket.reverse();
    }
    buckets
}

/// `crJointControlPoint`: one sample around the rim of a swing cone.
#[record(size = 0x0c)]
pub struct CrJointControlPoint {
    pub max_swing: f32,
    pub min_twist: f32,
    pub max_twist: f32,
}

/// `crJointRotationLimit`: how far one bone may rotate from its rest orientation.
#[record(size = 0xc0)]
pub struct CrJointRotationLimit {
    #[note("a runtime address, carried")]
    vft: u64,
    pub bone_id: i32,
    pub num_control_points: i32,
    #[pad_after(0xc)]
    pub joint_dofs: i32,
    pub zero_rotation: Vec4V,
    pub zero_rotation_eulers: Vec4V,
    pub twist_axis: Vec4V,
    pub twist_limit_min: f32,
    pub twist_limit_max: f32,
    pub soft_limit_scale: f32,
    pub control_points: [CrJointControlPoint; 8],
    pub use_twist_limits: Bool8,
    pub use_euler_angles: Bool8,
    #[pad_after(1)]
    pub use_per_control_twist_limits: Bool8,
}

/// `crJointTranslationLimit`: how far one bone may slide from its rest position, as two corners.
#[record(size = 0x40)]
pub struct CrJointTranslationLimit {
    #[note("a runtime address, carried")]
    vft: u64,
    #[pad_after(0x14)]
    pub bone_id: i32,
    pub limit_min: Vec4V,
    pub limit_max: Vec4V,
}

/// `crJointScaleLimit`: how far one bone may scale from its rest pose, as two corners.
#[record(size = 0x40)]
pub struct CrJointScaleLimit {
    #[note("a runtime address, carried")]
    vft: u64,
    #[pad_after(0x14)]
    pub bone_id: i32,
    pub limit_min: Vec4V,
    pub limit_max: Vec4V,
}

/// `crJointData`: per-bone limits, each naming the bone it constrains by id.
#[record(size = 0x38)]
pub struct CrJointData {
    pub base: PgBase,
    #[len(field = "num_rotation_limits")]
    pub rotation_limits: Vec<CrJointRotationLimit>,
    #[len(field = "num_translation_limits")]
    pub translation_limits: Vec<CrJointTranslationLimit>,
    #[len(field = "num_scale_limits")]
    pub scale_limits: Vec<CrJointScaleLimit>,
    pub name: String,
    num_rotation_limits: u16,
    num_translation_limits: u16,
    num_scale_limits: u16,
    #[init(1)]
    ref_count: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bone_tag_table_buckets_by_tag_and_reverses_each_chain() {
        let buckets = bucket_bone_tags(4, [8, 1, 4, 5]);
        assert_eq!(buckets[0], [(4, 2), (8, 0)]);
        assert_eq!(buckets[1], [(5, 3), (1, 1)]);
        assert!(buckets[2].is_empty() && buckets[3].is_empty());
        assert!(bucket_bone_tags(0, [1]).is_empty());
    }

    #[test]
    fn a_bone_is_added_by_id_and_an_id_is_not_shared() {
        let bone = |name: &str, bone_id, parent_index| CrBoneData {
            name: name.into(),
            bone_id,
            parent_index,
            ..Default::default()
        };
        let mut skeleton = CrSkeletonData::default();
        let add = |s: &mut CrSkeletonData, b| s.add_bone(b, Mat34V::default(), Mat34V::default());
        assert_eq!(add(&mut skeleton, bone("SKEL_ROOT", 0, -1)).unwrap(), 0);
        assert_eq!(
            add(&mut skeleton, bone("SKEL_Pelvis", 0x2e28, 0)).unwrap(),
            1
        );
        assert_eq!(
            add(&mut skeleton, bone("SKEL_Spine_Root", 0xe0fd, 0)).unwrap(),
            2
        );
        assert_eq!(skeleton.bones[1].next_index, 2, "siblings chain in order");
        assert_eq!(skeleton.bones[2].index, 2);
        assert_eq!(skeleton.parent_indices, [-1, 0, 0]);
        assert_eq!(skeleton.inverse_transforms.len(), 3);
        assert_eq!(skeleton.default_transforms.len(), 3);

        let why = add(&mut skeleton, bone("SKEL_Spine0", 0x2e28, 1))
            .expect_err("a second bone with one id would be unreachable")
            .to_string();
        assert!(why.contains("already holds id 0x2e28"), "{why}");
        assert!(add(&mut skeleton, bone("SKEL_Spine0", 0x5c01, 7)).is_err());
        assert_eq!(
            skeleton.bones.len(),
            3,
            "a refused bone leaves the skeleton as it was"
        );
        assert_eq!(skeleton.parent_indices.len(), 3);
    }

    fn lanes(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
        Vec4V {
            x,
            y,
            z,
            w,
            ..Vec4V::default()
        }
    }

    /// A transform no derivation produces, with a fourth lane of its own in each column.
    fn held() -> Mat34V {
        Mat34V {
            col0: lanes(99.0, 0.0, 0.0, 0.25),
            col1: lanes(0.0, 99.0, 0.0, 0.5),
            col2: lanes(0.0, 0.0, 99.0, 0.75),
            col3: lanes(9.0, 9.0, 9.0, 1.0),
            ..Mat34V::default()
        }
    }

    /// Bone 0 turned a quarter about z and a metre along x; bones 1 (two metres along y) and 3 (five along x)
    /// under it; bone 2 under bone 1, three metres along z and twice as wide. Every transform is [`held`].
    fn four_bones() -> CrSkeletonData {
        let quarter_turn = std::f32::consts::FRAC_1_SQRT_2;
        let bone = |index, parent_index, rotation, at: [f32; 3], scale: [f32; 3]| CrBoneData {
            name: format!("BONE_{index}"),
            bone_id: 100 + index,
            index,
            parent_index,
            rotation,
            translation: lanes(at[0], at[1], at[2], 0.0),
            scale: lanes(scale[0], scale[1], scale[2], 0.0),
            ..Default::default()
        };
        let straight = lanes(0.0, 0.0, 0.0, 1.0);
        CrSkeletonData {
            bones: vec![
                bone(
                    0,
                    -1,
                    lanes(0.0, 0.0, quarter_turn, quarter_turn),
                    [1.0, 0.0, 0.0],
                    [1.0; 3],
                ),
                bone(1, 0, straight.clone(), [0.0, 2.0, 0.0], [1.0; 3]),
                bone(2, 1, straight.clone(), [0.0, 0.0, 3.0], [2.0, 1.0, 1.0]),
                bone(3, 0, straight, [5.0, 0.0, 0.0], [1.0; 3]),
            ],
            parent_indices: vec![-1, 0, 1, 0],
            default_transforms: vec![held(); 4],
            inverse_transforms: vec![held(); 4],
            ..Default::default()
        }
    }

    /// Whether `m`'s four columns hold `columns` to a millionth, whatever its fourth lanes hold.
    fn holds(m: &Mat34V, columns: [[f32; 3]; 4]) -> bool {
        let found = [&m.col0, &m.col1, &m.col2, &m.col3];
        found.iter().zip(columns).all(|(have, want)| {
            [have.x, have.y, have.z]
                .iter()
                .zip(want)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        })
    }

    #[test]
    fn a_bones_rest_pose_is_its_own_numbers_under_every_parent_above_it() {
        let mut skeleton = four_bones();
        skeleton.rebuild_rest_pose(0).unwrap();
        let (default, inverse) = (&skeleton.default_transforms, &skeleton.inverse_transforms);

        let turned = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let back = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let with = |axes: [[f32; 3]; 3], at| [axes[0], axes[1], axes[2], at];
        assert!(holds(&default[0], with(turned, [1.0, 0.0, 0.0])));
        assert!(holds(&inverse[0], with(back, [0.0, 1.0, 0.0])));
        assert!(holds(&inverse[1], with(back, [0.0, -1.0, 0.0])));
        assert!(holds(&inverse[3], with(back, [-5.0, 1.0, 0.0])));

        let wide = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let narrowed = [[0.0, -1.0, 0.0], [0.5, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(holds(&default[2], with(wide, [0.0, 0.0, 3.0])));
        assert!(holds(&inverse[2], with(narrowed, [0.0, -1.0, -3.0])));

        for (bone, (d, i)) in default.iter().zip(inverse).enumerate() {
            let fourth = |m: &Mat34V| [m.col0.w, m.col1.w, m.col2.w, m.col3.w];
            let kept = [0.25, 0.5, 0.75, 1.0];
            assert_eq!(
                fourth(d),
                kept,
                "bone {bone}'s default keeps its fourth lanes"
            );
            assert_eq!(
                fourth(i),
                kept,
                "bone {bone}'s inverse keeps its fourth lanes"
            );
        }
    }

    /// `a` then `b` laid on it: what `a` does to what `b` does, as four columns.
    fn compose(a: &Mat34V, b: &Mat34V) -> [[f32; 3]; 4] {
        let axes = [&a.col0, &a.col1, &a.col2].map(|c| [c.x, c.y, c.z].map(f64::from));
        let at = |c: &Vec4V| turn(&axes, [c.x, c.y, c.z].map(f64::from)).map(|v| v as f32);
        let moved = at(&b.col3);
        let c3 = [
            moved[0] + a.col3.x,
            moved[1] + a.col3.y,
            moved[2] + a.col3.z,
        ];
        [at(&b.col0), at(&b.col1), at(&b.col2), c3]
    }

    #[test]
    fn a_bones_inverse_undoes_the_default_transforms_laid_down_its_chain() {
        let mut skeleton = four_bones();
        // Every level of the chain scaled, bone 1 turned as well as bone 0.
        skeleton.bones[0].scale = lanes(1.5, 1.0, 0.5, 0.0);
        skeleton.bones[1].scale = lanes(1.0, 2.0, 1.0, 0.0);
        skeleton.bones[1].rotation = lanes(0.5, 0.5, -0.5, 0.5);
        skeleton.rebuild_rest_pose(0).unwrap();
        let d = &skeleton.default_transforms;
        let world = |chain: &[usize]| {
            let mut at = d[chain[0]].clone();
            for &bone in &chain[1..] {
                let [c0, c1, c2, c3] = compose(&at, &d[bone]);
                at = Mat34V {
                    col0: lanes(c0[0], c0[1], c0[2], 0.0),
                    col1: lanes(c1[0], c1[1], c1[2], 0.0),
                    col2: lanes(c2[0], c2[1], c2[2], 0.0),
                    col3: lanes(c3[0], c3[1], c3[2], 0.0),
                    ..Mat34V::default()
                };
            }
            at
        };
        for (bone, chain) in [
            (0, vec![0]),
            (1, vec![0, 1]),
            (2, vec![0, 1, 2]),
            (3, vec![0, 3]),
        ] {
            let product = compose(&world(&chain), &skeleton.inverse_transforms[bone]);
            let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]];
            for (col, (got, want)) in product.iter().zip(identity).enumerate() {
                for (a, b) in got.iter().zip(want) {
                    assert!((a - b).abs() < 1e-5, "bone {bone} column {col}: {got:?}");
                }
            }
        }
    }

    #[test]
    fn a_scaled_bone_that_is_turned_stretches_its_own_axes() {
        // A quarter about z, scaled twice along x: x lands on y and is stretched there (rotation times
        // scale). Stretching the parent's x instead would give (0, 1, 0) and (-2, 0, 0).
        let mut skeleton = four_bones();
        skeleton.bones[0].scale = lanes(2.0, 1.0, 1.0, 0.0);
        skeleton.rebuild_rest_pose(0).unwrap();
        let stretched = [[0.0, 2.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let narrowed = [[0.0, -1.0, 0.0], [0.5, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let with = |axes: [[f32; 3]; 3], at| [axes[0], axes[1], axes[2], at];
        assert!(holds(
            &skeleton.default_transforms[0],
            with(stretched, [1.0, 0.0, 0.0])
        ));
        assert!(holds(
            &skeleton.inverse_transforms[0],
            with(narrowed, [0.0, 1.0, 0.0])
        ));
    }

    #[test]
    fn a_rebuild_derives_the_bone_and_each_bone_under_it_and_no_other() {
        let mut skeleton = four_bones();
        skeleton.rebuild_rest_pose(1).unwrap();
        for bone in [0, 3] {
            assert_eq!(skeleton.default_transforms[bone], held(), "bone {bone}");
            assert_eq!(skeleton.inverse_transforms[bone], held(), "bone {bone}");
        }
        for bone in [1, 2] {
            assert_ne!(skeleton.default_transforms[bone], held(), "bone {bone}");
            assert_ne!(skeleton.inverse_transforms[bone], held(), "bone {bone}");
        }
    }

    #[test]
    fn only_the_comprehensive_signature_follows_an_edit_of_a_bones_numbers() {
        let mut skeleton = four_bones();
        (skeleton.signature, skeleton.signature_non_chiral) = (11, 22);
        skeleton.rebuild_rest_pose(0).unwrap();
        let first = skeleton.signature_comprehensive;
        assert_ne!(first, 0);

        skeleton.bones[1].translation.y = 4.0;
        skeleton.rebuild_rest_pose(1).unwrap();
        let moved = skeleton.signature_comprehensive;
        assert_ne!(moved, first, "a bone's position is in it");
        assert_eq!(
            (skeleton.signature, skeleton.signature_non_chiral),
            (11, 22)
        );

        skeleton.bones[1].name = "SKEL_Renamed".into();
        skeleton.rebuild_rest_pose(1).unwrap();
        assert_eq!(skeleton.signature_comprehensive, moved, "a name is not");

        skeleton.bones[1].translation.y = 2.0;
        skeleton.rebuild_rest_pose(1).unwrap();
        assert_eq!(
            skeleton.signature_comprehensive, first,
            "the pose is all it hashes"
        );
    }

    #[test]
    fn a_rebuild_that_cannot_finish_changes_nothing() {
        let before = four_bones();
        let refused = |edit: fn(&mut CrSkeletonData), bone: usize, said: &str| {
            let mut skeleton = before.clone();
            edit(&mut skeleton);
            let kept = skeleton.clone();
            let why = skeleton.rebuild_rest_pose(bone).unwrap_err().to_string();
            assert!(why.contains(said), "{why}");
            assert_eq!(skeleton, kept, "{said}");
        };
        refused(|_| {}, 4, "bone 4 in a skeleton of 4");
        refused(
            |s| drop(s.inverse_transforms.pop()),
            0,
            "3 inverse transform(s)",
        );
        refused(|s| s.bones[3].parent_index = 9, 0, "bone 3 names parent 9");
        refused(
            |s| s.bones[0].parent_index = 2,
            0,
            "the parents of bone 0 loop",
        );
        refused(|s| s.bones[1].scale.y = 0.0, 0, "has no inverse");
    }
}

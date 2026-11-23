use glam::{Mat4, Vec3};
use or_anim::{Bone, Skeleton, NO_PARENT};
use or_renderer::LineVertex;

pub(crate) const BONE: [f32; 4] = [0.45, 0.85, 1.0, 1.0];
/// The bone selected in the Outline, which no other line of a skeleton is drawn in.
pub(crate) const SELECTED: [f32; 4] = [1.0, 0.25, 0.85, 1.0];
const JOINT: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
const GHOST: [f32; 4] = [0.55, 0.55, 0.6, 1.0];
const ROOT_X: [f32; 4] = [1.0, 0.25, 0.25, 1.0];
const ROOT_Y: [f32; 4] = [0.25, 1.0, 0.25, 1.0];
const ROOT_Z: [f32; 4] = [0.35, 0.45, 1.0, 1.0];

const JOINT_SIZE: f32 = 0.01;

const ROOT_AXIS: f32 = 0.1;

/// The skeleton's bones where `world` puts them, `selected` by index in [`SELECTED`], over the bind-pose ghost where asked.
pub fn build_pose_comparison(
    skeleton: &Skeleton,
    world: &[Mat4],
    show_ghost: bool,
    selected: Option<u16>,
) -> Vec<LineVertex> {
    let mut out = Vec::with_capacity(skeleton.bone_count() * 16);
    if show_ghost {
        push_skeleton(
            &mut out,
            skeleton,
            &skeleton.bind_world,
            [GHOST, GHOST],
            false,
            None,
        );
    }
    push_skeleton(&mut out, skeleton, world, [BONE, JOINT], true, selected);
    out
}

/// The bones of the dressed ped's skeleton where its clip puts them, over the rest pose while one plays and raised as the ped is; none unless Bones is on and the install gave a skeleton.
pub fn dressed_bones(
    dressed: &crate::state::workbench::Dressed,
    ped: or_wardrobe::Ped,
) -> Vec<LineVertex> {
    let Some(skeleton) = dressed.skeleton(ped).filter(|_| dressed.bones) else {
        return Vec::new();
    };
    let raised = Mat4::from_translation(Vec3::Z * dressed.lift);
    let world: Vec<Mat4> = match &dressed.motion {
        Some(motion) => motion.world().iter(),
        None => skeleton.bind_world.iter(),
    }
    .map(|bone| raised * *bone)
    .collect();
    build_pose_comparison(skeleton, &world, dressed.motion.is_some(), None)
}

fn push_skeleton(
    out: &mut Vec<LineVertex>,
    skeleton: &Skeleton,
    world: &[Mat4],
    colours: [[f32; 4]; 2],
    decorate: bool,
    selected: Option<u16>,
) {
    for bone in &skeleton.bones {
        let colours = if selected == Some(bone.index) {
            [SELECTED; 2]
        } else {
            colours
        };
        push_bone(out, skeleton, world, bone, colours, decorate);
    }
}

/// One bone's lines: the segment from its parent, its joint, and a root's axes.
fn push_bone(
    out: &mut Vec<LineVertex>,
    skeleton: &Skeleton,
    world: &[Mat4],
    bone: &Bone,
    [bone_colour, joint_colour]: [[f32; 4]; 2],
    decorate: bool,
) {
    let Some(matrix) = world.get(bone.index as usize) else {
        return;
    };
    let origin = matrix.w_axis.truncate();
    if !origin.is_finite() {
        return;
    }

    let parent = skeleton.parents[bone.index as usize];
    if parent == NO_PARENT {
        if decorate {
            push_axes(out, matrix, ROOT_AXIS);
        }
    } else if let Some(parent_matrix) = world.get(parent as usize) {
        let parent_origin = parent_matrix.w_axis.truncate();
        if parent_origin.is_finite() {
            push_segment(out, parent_origin, origin, bone_colour);
        }
    }

    if decorate {
        push_joint(out, origin, joint_colour);
    }
}

/// The lines [`build_pose_comparison`] draws for each bone of the posed skeleton, by bone index.
pub fn bone_lines(skeleton: &Skeleton, world: &[Mat4]) -> Vec<(u16, Vec<LineVertex>)> {
    lines_by_bone(skeleton, world, [BONE, JOINT], true)
}

/// The lines [`build_pose_comparison`] draws for each bone of the bind-pose ghost, by bone index.
pub fn ghost_lines(skeleton: &Skeleton) -> Vec<(u16, Vec<LineVertex>)> {
    lines_by_bone(skeleton, &skeleton.bind_world, [GHOST, GHOST], false)
}

fn lines_by_bone(
    skeleton: &Skeleton,
    world: &[Mat4],
    colours: [[f32; 4]; 2],
    decorate: bool,
) -> Vec<(u16, Vec<LineVertex>)> {
    (skeleton.bones.iter())
        .map(|bone| {
            let mut out = Vec::new();
            push_bone(&mut out, skeleton, world, bone, colours, decorate);
            (bone.index, out)
        })
        .filter(|(_, lines)| !lines.is_empty())
        .collect()
}

fn push_joint(out: &mut Vec<LineVertex>, origin: Vec3, colour: [f32; 4]) {
    let s = JOINT_SIZE;
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        push_segment(out, origin - axis * s, origin + axis * s, colour);
    }
}

fn push_axes(out: &mut Vec<LineVertex>, matrix: &Mat4, length: f32) {
    let origin = matrix.w_axis.truncate();
    for (axis, colour) in [
        (matrix.x_axis.truncate(), ROOT_X),
        (matrix.y_axis.truncate(), ROOT_Y),
        (matrix.z_axis.truncate(), ROOT_Z),
    ] {
        if axis.length_squared() < 1e-12 {
            continue;
        }
        push_segment(out, origin, origin + axis.normalize() * length, colour);
    }
}

fn push_segment(out: &mut Vec<LineVertex>, a: Vec3, b: Vec3, colour: [f32; 4]) {
    out.push(LineVertex::new([a.x, a.y, a.z], colour));
    out.push(LineVertex::new([b.x, b.y, b.z], colour));
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_anim::Pose;
    use or_io::gta5::{CrBoneData, CrSkeletonData, Vec4V};

    fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
        Vec4V {
            x,
            y,
            z,
            w,
            ..Vec4V::default()
        }
    }

    fn bone(index: u16, parent: i16, translation: [f32; 3]) -> CrBoneData {
        CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(translation[0], translation[1], translation[2], 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: parent,
            name: format!("BONE_{index}"),
            dofs: 0,
            index,
            bone_id: 100 + index,
            mirror_index: 0,
            ..CrBoneData::default()
        }
    }

    fn two_bone_skeleton() -> Skeleton {
        Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1, 0],
            bones: vec![bone(0, -1, [0.0; 3]), bone(1, 0, [1.0, 0.0, 0.0])],
            ..CrSkeletonData::default()
        })
    }

    #[test]
    fn a_bone_is_a_segment_from_its_parent_to_itself() {
        let skeleton = two_bone_skeleton();
        let world = Pose::bind(&skeleton).world_matrices(&skeleton);
        let lines = build_pose_comparison(&skeleton, &world, false, None);

        let found = lines.chunks(2).any(|pair| {
            pair.len() == 2
                && Vec3::from(pair[0].position).abs_diff_eq(Vec3::ZERO, 1e-5)
                && Vec3::from(pair[1].position).abs_diff_eq(Vec3::new(1.0, 0.0, 0.0), 1e-5)
        });
        assert!(found, "no parent-to-child segment found");
    }

    #[test]
    fn output_is_always_whole_segments() {
        let skeleton = two_bone_skeleton();
        let world = Pose::bind(&skeleton).world_matrices(&skeleton);
        assert_eq!(
            build_pose_comparison(&skeleton, &world, false, None).len() % 2,
            0
        );
        assert_eq!(
            build_pose_comparison(&skeleton, &world, true, None).len() % 2,
            0
        );
    }

    #[test]
    fn a_root_only_skeleton_still_draws_something() {
        let skeleton = Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1],
            bones: vec![bone(0, -1, [0.0; 3])],
            ..CrSkeletonData::default()
        });
        let world = Pose::bind(&skeleton).world_matrices(&skeleton);
        let lines = build_pose_comparison(&skeleton, &world, false, None);
        assert!(!lines.is_empty(), "a root-only skeleton drew nothing");
    }

    #[test]
    fn the_ghost_adds_geometry_and_can_be_turned_off() {
        let skeleton = two_bone_skeleton();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(1).unwrap().translation = Vec3::new(3.0, 0.0, 0.0);
        let world = pose.world_matrices(&skeleton);

        let without = build_pose_comparison(&skeleton, &world, false, None);
        let with = build_pose_comparison(&skeleton, &world, true, None);
        assert!(with.len() > without.len());
    }

    #[test]
    fn the_ghost_shows_the_bind_pose_not_the_animated_one() {
        let skeleton = two_bone_skeleton();
        let mut pose = Pose::bind(&skeleton);
        pose.local_mut(1).unwrap().translation = Vec3::new(5.0, 0.0, 0.0);
        let world = pose.world_matrices(&skeleton);
        let lines = build_pose_comparison(&skeleton, &world, true, None);

        let has_animated = lines.iter().any(|v| (v.position[0] - 5.0).abs() < 1e-5);
        let has_bind = lines.iter().any(|v| (v.position[0] - 1.0).abs() < 1e-5);
        assert!(has_animated, "animated pose missing");
        assert!(has_bind, "bind pose ghost missing");
    }

    #[test]
    fn a_short_matrix_buffer_is_tolerated() {
        let skeleton = two_bone_skeleton();
        let lines = build_pose_comparison(&skeleton, &[Mat4::IDENTITY], false, None);
        assert!(!lines.is_empty());
        assert_eq!(lines.len() % 2, 0);

        assert!(build_pose_comparison(&skeleton, &[], false, None).is_empty());
    }

    #[test]
    fn non_finite_matrices_are_skipped() {
        let skeleton = two_bone_skeleton();
        let world = vec![Mat4::from_translation(Vec3::new(f32::NAN, 0.0, 0.0)); 2];
        let lines = build_pose_comparison(&skeleton, &world, false, None);
        assert!(
            lines
                .iter()
                .all(|v| v.position.iter().all(|c| c.is_finite())),
            "emitted a non-finite vertex"
        );
    }

    #[test]
    fn a_degenerate_root_matrix_emits_no_axis_ticks() {
        let skeleton = Skeleton::build(&CrSkeletonData {
            parent_indices: vec![-1],
            bones: vec![bone(0, -1, [0.0; 3])],
            ..CrSkeletonData::default()
        });
        let lines = build_pose_comparison(&skeleton, &[Mat4::ZERO], false, None);
        assert!(lines
            .iter()
            .all(|v| v.position.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn an_empty_skeleton_draws_nothing() {
        let skeleton = Skeleton::build(&Default::default());
        assert!(build_pose_comparison(&skeleton, &[], false, None).is_empty());
        assert!(build_pose_comparison(&skeleton, &[], true, None).is_empty());
    }
}

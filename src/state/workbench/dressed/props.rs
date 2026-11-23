//! A prop worn on the dressed ped: carried to the bone of its anchor, and bound to that bone so that a clip moves it with the ped.
//!
//! ⚠ A prop is stored in the axes of the bone it is worn at, where a component is stored in the skeleton's own space and
//! drawn as it is. So a prop is carried by the bone's bind transform at rest, and a rigid skin to that one bone lets the
//! pose the clip gives the bone move it from there.

use std::sync::Arc;

use glam::Mat4;
use or_anim::skin::VertexInfluences;
use or_anim::SkinnedMesh;
use or_wardrobe::Ped;

use crate::render::dressed::Geometry;

/// `geometry` carried by `at`: positions as points and normals as directions.
pub(super) fn placed(geometry: &[Geometry], at: Mat4) -> Vec<Geometry> {
    (geometry.iter())
        .map(|g| Geometry {
            vertices: (g.vertices.iter())
                .map(|v| or_renderer::MeshVertex {
                    position: at.transform_point3(v.position.into()).into(),
                    normal: at
                        .transform_vector3(v.normal.into())
                        .normalize_or_zero()
                        .into(),
                    ..*v
                })
                .collect(),
            indices: g.indices.clone(),
        })
        .collect()
}

/// What a prop draws as when worn at bone `bone` of the skeleton, whose bind transform is `at`: its meshes carried there, and for each a skin that holds every vertex to that bone.
pub(super) fn carried(
    geometry: &[Geometry],
    (bone, at): (usize, Mat4),
) -> (Arc<Vec<Geometry>>, Vec<Option<Arc<SkinnedMesh>>>) {
    let geometry = placed(geometry, at);
    let skins = geometry
        .iter()
        .map(|g| {
            Some(Arc::new(SkinnedMesh {
                influences: vec![VertexInfluences::rigid(bone as u16); g.vertices.len()],
                bind_positions: g.vertices.iter().map(|v| v.position).collect(),
                bind_normals: g.vertices.iter().map(|v| v.normal).collect(),
                bones_used: vec![bone as u16],
                ..SkinnedMesh::default()
            }))
        })
        .collect();
    (Arc::new(geometry), skins)
}

impl super::Dressed {
    /// Where the bone called `bone` of `ped`'s skeleton stands at rest, with its place in the skeleton; none while the skeleton is not read.
    pub fn bone_at_rest(&self, ped: Ped, bone: &str) -> Option<(usize, Mat4)> {
        let skeleton = self.skeleton(ped)?;
        let at = (skeleton.bones.iter()).position(|b| b.name.eq_ignore_ascii_case(bone))?;
        Some((at, *skeleton.bind_world.get(at)?))
    }
}

/// A skeleton of a root and a head bone a metre and six tenths above it, which a hat is worn at.
#[cfg(test)]
pub(crate) fn head_skeleton() -> or_anim::Skeleton {
    use or_io::gta5::{CrBoneData, CrSkeletonData, Vec4V};
    let v4 = |x: f32, y: f32, z: f32, w: f32| Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    };
    let bone = |index: u16, name: &str, parent: i16, z: f32| CrBoneData {
        rotation: v4(0.0, 0.0, 0.0, 1.0),
        translation: v4(0.0, 0.0, z, 0.0),
        scale: v4(1.0, 1.0, 1.0, 0.0),
        next_index: -1,
        parent_index: parent,
        name: name.to_string(),
        index,
        bone_id: 100 + index,
        ..CrBoneData::default()
    };
    or_anim::Skeleton::build(&CrSkeletonData {
        parent_indices: vec![-1, 0],
        bones: vec![bone(0, "SKEL_ROOT", -1, 0.0), bone(1, "SKEL_Head", 0, 1.6)],
        ..CrSkeletonData::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_renderer::MeshVertex;

    fn triangle() -> Vec<Geometry> {
        vec![Geometry {
            vertices: vec![
                MeshVertex::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0]),
                MeshVertex::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0]),
                MeshVertex::new([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0]),
            ],
            indices: vec![0, 1, 2],
        }]
    }

    #[test]
    fn a_prop_is_carried_by_its_bone_and_a_rigid_skin_lets_the_pose_of_that_bone_move_it() {
        let at = Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 1.7))
            * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let (geometry, skins) = carried(&triangle(), (3, at));

        let moved = &geometry[0].vertices;
        assert!(
            (glam::Vec3::from(moved[0].position) - glam::Vec3::new(0.0, 0.0, 1.7)).length() < 1e-5
        );
        // x turned to y by the bone's rotation, and a normal along z stays a normal along z.
        assert!(
            (glam::Vec3::from(moved[1].position) - glam::Vec3::new(0.0, 1.0, 1.7)).length() < 1e-5
        );
        assert!((glam::Vec3::from(moved[1].normal) - glam::Vec3::Z).length() < 1e-5);

        let skin = skins[0].as_ref().expect("a prop is skinned to its bone");
        assert_eq!(skin.bones_used, [3]);
        assert!(skin
            .influences
            .iter()
            .all(|i| i.bones[0] == 3 && i.weights[0] == 1.0));
        // Under the bone's rest pose (its skin matrix is the identity) the prop stays where it was carried; under a pose that lifts the bone a metre, it rises with it.
        let rest = [Mat4::IDENTITY; 4];
        let (mut at_rest, mut normals) = (Vec::new(), Vec::new());
        skin.skin_into(&rest, &mut at_rest, &mut normals);
        assert_eq!(at_rest[1], moved[1].position);
        let mut lifted = [Mat4::IDENTITY; 4];
        lifted[3] = Mat4::from_translation(glam::Vec3::Z);
        skin.skin_into(&lifted, &mut at_rest, &mut normals);
        assert!((glam::Vec3::from(at_rest[1]) - glam::Vec3::new(0.0, 1.0, 2.7)).length() < 1e-5);
    }

    #[test]
    fn the_bone_of_an_anchor_is_found_by_name_whatever_its_case_and_a_ped_with_no_skeleton_has_none(
    ) {
        let mut dressed = super::super::Dressed::default();
        assert!(dressed.bone_at_rest(Ped::Male, "SKEL_Head").is_none());
        dressed.show_skeleton(Ped::Male, head_skeleton());
        let (at, matrix) = dressed
            .bone_at_rest(Ped::Male, "skel_head")
            .expect("found by any case");
        assert_eq!(at, 1);
        assert!(
            (matrix.transform_point3(glam::Vec3::ZERO) - glam::Vec3::new(0.0, 0.0, 1.6)).length()
                < 1e-5
        );
        assert!(dressed.bone_at_rest(Ped::Male, "PH_L_Hand").is_none());
        assert!(
            dressed.bone_at_rest(Ped::Female, "SKEL_Head").is_none(),
            "another ped's skeleton"
        );
    }
}

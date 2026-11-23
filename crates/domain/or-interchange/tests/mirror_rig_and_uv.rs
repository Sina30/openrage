//! A mirror is its own inverse, a rig with a group no skeleton knows is mended, a weight is cleaned, and the texture a mesh uses is found and moved.

mod common;

use common::sphere;
use or_interchange::doctor::mirror::{mirror_scene, BoneMirror};
use or_interchange::doctor::rig::{self, Defect};
use or_interchange::doctor::uv::{self, UvBox};
use or_interchange::{Bone, Mesh, Object, Primitive, Scene};

fn bone(name: &str, parent: Option<usize>) -> Bone {
    Bone {
        name: name.to_string(),
        parent,
        scale: [1.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        ..Bone::default()
    }
}

/// A sphere skinned to the bones `names` by latitude (the sphere's own four bands use bones 0 to 3), as the one mesh of one object.
fn scene_of(names: &[(&str, Option<usize>)]) -> Scene {
    let primitive = sphere(12, 24);
    Scene {
        name: "s".into(),
        objects: vec![Object {
            name: "s".into(),
            meshes: vec![Mesh {
                name: "lod0_model0".into(),
                lod: primitive.lod,
                primitives: vec![primitive],
            }],
            bones: names.iter().map(|(n, p)| bone(n, *p)).collect(),
        }],
        ..Scene::default()
    }
}

fn same(a: &Primitive, b: &Primitive) -> bool {
    a.positions == b.positions
        && a.normals == b.normals
        && a.tangents == b.tangents
        && a.indices == b.indices
        && a.joints == b.joints
        && a.weights == b.weights
        && a.uvs == b.uvs
}

/// Six times the volume the triangles enclose; positive for a closed mesh wound outward.
fn volume(p: &Primitive) -> f32 {
    p.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| p.positions[t[k] as usize]);
            a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum()
}

const SIDED: [(&str, Option<usize>); 4] = [
    ("SKEL_ROOT", None),
    ("SKEL_L_Foot", Some(0)),
    ("SKEL_R_Foot", Some(0)),
    ("SKEL_Spine0", Some(0)),
];

#[test]
fn mirroring_twice_is_the_identity() {
    let mut scene = scene_of(&SIDED);
    let first = scene.objects[0].meshes[0].primitives[0].clone();
    mirror_scene(&mut scene, None).expect("a skin with names mirrors");
    mirror_scene(&mut scene, None).expect("and mirrors back");
    assert!(same(&first, &scene.objects[0].meshes[0].primitives[0]));
}

#[test]
fn a_mirror_turns_the_winding_over_with_the_coordinates() {
    let mut scene = scene_of(&SIDED);
    let was = volume(&scene.objects[0].meshes[0].primitives[0]);
    assert!(was > 0.0, "the sphere is wound outward");
    mirror_scene(&mut scene, None).unwrap();
    let primitive = &scene.objects[0].meshes[0].primitives[0];
    assert!(
        volume(primitive) > 0.0,
        "the mirrored sphere is wound outward too"
    );
    assert!((volume(primitive) - was).abs() < 1e-3 * was);
    // Every coordinate is turned about x = 0, and a tangent's handedness with it.
    let source = sphere(12, 24);
    for (v, p) in primitive.positions.iter().enumerate() {
        assert_eq!(
            *p,
            [
                -source.positions[v][0],
                source.positions[v][1],
                source.positions[v][2]
            ]
        );
        assert_eq!(primitive.tangents[v][3], -source.tangents[v][3]);
    }
}

#[test]
fn a_mirror_gives_the_skin_to_the_bones_on_the_other_side() {
    let mut scene = scene_of(&SIDED);
    let before = scene.objects[0].meshes[0].primitives[0].clone();
    let report = mirror_scene(&mut scene, None).unwrap();
    assert_eq!(report.pairs, 1);
    let after = &scene.objects[0].meshes[0].primitives[0];
    let mut moved = 0;
    for v in 0..before.joints.len() {
        for lane in 0..4 {
            if before.weights[v][lane] <= 0.0 {
                continue;
            }
            let (was, is) = (before.joints[v][lane], after.joints[v][lane]);
            let expected = match was {
                1 => 2,
                2 => 1,
                other => other,
            };
            assert_eq!(is, expected);
            moved += usize::from(was != is);
        }
    }
    assert!(moved > 0, "no vertex was weighted to a foot");
    assert_eq!(report.rebound, {
        (0..before.joints.len())
            .filter(|v| {
                (0..4).any(|l| before.weights[*v][l] > 0.0 && matches!(before.joints[*v][l], 1 | 2))
            })
            .count()
    });
}

#[test]
fn a_weight_on_a_bone_with_a_side_and_no_partner_is_refused_and_nothing_moves() {
    let names = [
        ("SKEL_ROOT", None),
        ("SKEL_L_Foot", Some(0)),
        ("SKEL_Spine0", Some(0)),
        ("SKEL_Spine1", Some(2)),
    ];
    let mut scene = scene_of(&names);
    let before = scene.objects[0].meshes[0].primitives[0].clone();
    let refused = mirror_scene(&mut scene, None).expect_err("SKEL_L_Foot has no right foot");
    assert!(refused.to_string().contains("SKEL_L_Foot"), "{refused}");
    assert!(same(&before, &scene.objects[0].meshes[0].primitives[0]));
}

#[test]
fn a_skin_with_no_names_to_pair_by_is_refused() {
    let mut scene = scene_of(&SIDED);
    scene.objects[0].bones.clear();
    let refused = mirror_scene(&mut scene, None).expect_err("nothing names the bones");
    assert!(
        refused.to_string().contains("nobody has named"),
        "{refused}"
    );
    // Given the skeleton's names, it mirrors.
    let names: Vec<String> = SIDED.iter().map(|(n, _)| n.to_string()).collect();
    mirror_scene(&mut scene, Some(&BoneMirror::of_names(&names))).expect("with names it mirrors");
}

#[test]
fn bones_are_paired_by_the_side_in_their_name() {
    let names: Vec<String> = [
        "SKEL_L_Hand",
        "SKEL_R_Hand",
        "SKEL_Spine0",
        "MH_L_Elbow",
        "MH_R_Elbow",
        "PH_L_Foot",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let pairs = BoneMirror::of_names(&names);
    assert_eq!(pairs.counterpart(0), Some(1));
    assert_eq!(pairs.counterpart(1), Some(0));
    assert_eq!(
        pairs.counterpart(2),
        Some(2),
        "a middle bone is its own counterpart"
    );
    assert_eq!(pairs.counterpart(3), Some(4));
    assert_eq!(pairs.counterpart(5), None, "PH_L_Foot has no right bone");
    assert_eq!(pairs.pairs(), 2);
}

const WITH_A_GROUP: [(&str, Option<usize>); 4] = [
    ("SKEL_ROOT", None),
    ("SKEL_Spine0", Some(0)),
    ("Group.001", Some(1)),
    ("SKEL_Head", Some(2)),
];

fn skeleton() -> Vec<String> {
    ["SKEL_ROOT", "SKEL_Spine0", "SKEL_Spine1", "SKEL_Head"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn a_group_with_no_name_of_a_bone_is_found_and_its_weight_goes_to_the_bone_above_it() {
    let mut scene = scene_of(&WITH_A_GROUP);
    let before = scene.objects[0].meshes[0].primitives[0].clone();
    let found = rig::check(&scene, &skeleton());
    let unnamed = found
        .iter()
        .find(|f| f.defect == Defect::UnnamedGroup)
        .expect("the group is found");
    assert_eq!(unnamed.bones, vec!["Group.001".to_string()]);
    assert!(unnamed.count > 0);

    let said = rig::repair(&mut scene, &skeleton());
    assert!(said.iter().any(|l| l.contains("Group.001")), "{said:?}");
    assert!(
        rig::check(&scene, &skeleton())
            .iter()
            .all(|f| f.defect != Defect::UnnamedGroup),
        "the group is still found"
    );
    // Every weight that was on the group is on its parent, SKEL_Spine0, and the rest did not move.
    let after = &scene.objects[0].meshes[0].primitives[0];
    for v in 0..before.joints.len() {
        let weight_of = |p: &Primitive, joint: u16| -> f32 {
            (0..4)
                .filter(|l| p.joints[v][*l] == joint)
                .map(|l| p.weights[v][l])
                .sum()
        };
        assert_eq!(weight_of(after, 2), 0.0, "vertex {v} is still on the group");
        assert!(
            (weight_of(after, 1) - (weight_of(&before, 1) + weight_of(&before, 2))).abs() < 1e-5
        );
        assert!((weight_of(after, 3) - weight_of(&before, 3)).abs() < 1e-5);
        assert!((after.weights[v].iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }
}

#[test]
fn a_bone_spelt_otherwise_takes_the_skeletons_spelling() {
    let mut scene = scene_of(&[
        ("SKEL_ROOT", None),
        ("skel_spine0.001", Some(0)),
        ("SKEL_Spine1", Some(1)),
        ("SKEL_Head", Some(2)),
    ]);
    let found = rig::check(&scene, &skeleton());
    assert!(found.iter().any(|f| f.defect == Defect::Misspelt));
    rig::repair(&mut scene, &skeleton());
    assert_eq!(scene.objects[0].bones[1].name, "SKEL_Spine0");
    assert!(
        rig::check(&scene, &skeleton()).is_empty(),
        "{:?}",
        rig::check(&scene, &skeleton())
    );
}

#[test]
fn a_rig_that_is_the_skeletons_own_is_left_exactly_as_it_is() {
    let mut scene = scene_of(&[
        ("SKEL_ROOT", None),
        ("SKEL_Spine0", Some(0)),
        ("SKEL_Spine1", Some(1)),
        ("SKEL_Head", Some(2)),
    ]);
    let before = scene.objects[0].meshes[0].primitives[0].clone();
    assert!(rig::check(&scene, &skeleton()).is_empty());
    assert!(rig::repair(&mut scene, &skeleton()).is_empty());
    assert!(same(&before, &scene.objects[0].meshes[0].primitives[0]));
}

#[test]
fn a_group_with_no_bone_above_it_is_dropped_and_the_vertex_takes_its_neighbours_skin() {
    // The group is a root of its own, so there is nothing above it that the skeleton has.
    let mut scene = scene_of(&[
        ("SKEL_ROOT", None),
        ("SKEL_Spine0", Some(0)),
        ("Cube", None),
        ("SKEL_Head", Some(1)),
    ]);
    rig::repair(&mut scene, &skeleton());
    let after = &scene.objects[0].meshes[0].primitives[0];
    assert!(after
        .weights
        .iter()
        .zip(&after.joints)
        .all(|(w, j)| (w.iter().sum::<f32>() - 1.0).abs() < 1e-5
            && (0..4).all(|l| w[l] == 0.0 || j[l] != 2)));
}

#[test]
fn weights_are_cleaned_of_noise_and_made_to_add_up() {
    let mut p = sphere(4, 8);
    p.joints = vec![[1, 2, 3, 0]; p.positions.len()];
    p.weights = vec![[0.6, 0.35, 0.004, 0.0]; p.positions.len()];
    p.weights[3] = [0.0; 4];
    let cleaned = rig::clean_weights(&mut p, rig::FAINT);
    assert_eq!(cleaned.faint, p.positions.len() - 1);
    assert_eq!(cleaned.unweighted, 1);
    for weights in &p.weights {
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(weights.iter().all(|w| *w == 0.0 || *w >= rig::FAINT));
    }
    assert_ne!(p.weights[3], [0.0; 4], "the bare vertex took a skin");
    // A second pass finds nothing to do.
    assert!(rig::clean_weights(&mut p, rig::FAINT).is_empty());
}

#[test]
fn the_texture_a_mesh_uses_is_found_and_moved_into_a_smaller_one_and_back() {
    let mut scene = scene_of(&SIDED);
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    // Squeeze the first coordinates into a corner so that the mesh uses a part of the texture.
    for uv in &mut primitive.uvs[0] {
        *uv = [0.25 + uv[0] * 0.25, 0.5 + uv[1] * 0.125];
    }
    let used = uv::used(&scene).expect("the mesh has texture coordinates");
    assert_eq!(used.min, [0.25, 0.5]);
    assert_eq!(used.max, [0.5, 0.625]);
    assert!(used.inside_unit());
    assert!((used.covers() - 0.25 * 0.125).abs() < 1e-7);

    let before = scene.objects[0].meshes[0].primitives[0].uvs.clone();
    uv::remap(&mut scene, used, UvBox::UNIT);
    let filled = uv::used(&scene).unwrap();
    assert_eq!(filled, UvBox::UNIT);
    // The second set is another map and is not moved.
    assert_eq!(scene.objects[0].meshes[0].primitives[0].uvs[1], before[1]);
    uv::remap(&mut scene, UvBox::UNIT, used);
    for (a, b) in scene.objects[0].meshes[0].primitives[0].uvs[0]
        .iter()
        .zip(&before[0])
    {
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    }
}

/// Two shoes, one a side, as a grid of vertices each; `heel_bone` is the bone the rear and bottom of each is bound to, and the front is bound to the toe.
fn shoes(heel_bone_left: u16, heel_bone_right: u16) -> Scene {
    let mut p = Primitive::default();
    for side in [1.0f32, -1.0] {
        for iy in 0..8 {
            for iz in 0..6 {
                let (y, z) = (-0.1 + iy as f32 * 0.035, iz as f32 * 0.02);
                p.positions.push([side * 0.1, y, z]);
                let heel = y < 0.05;
                let bone = match (heel, side > 0.0) {
                    (true, true) => heel_bone_left,
                    (true, false) => heel_bone_right,
                    (false, true) => 2,
                    (false, false) => 4,
                };
                p.joints.push([bone, 0, 0, 0]);
                p.weights.push([1.0, 0.0, 0.0, 0.0]);
            }
        }
    }
    Scene {
        objects: vec![Object {
            name: "shoes".into(),
            meshes: vec![Mesh {
                name: "lod0_model0".into(),
                lod: p.lod,
                primitives: vec![p],
            }],
            bones: [
                "SKEL_L_Foot",
                "SKEL_R_Foot",
                "SKEL_L_Toe0",
                "SKEL_L_Calf",
                "SKEL_R_Toe0",
            ]
            .iter()
            .map(|n| bone(n, None))
            .collect(),
        }],
        ..Scene::default()
    }
}

#[test]
fn a_heel_the_foot_bone_holds_passes_and_one_the_toe_drags_is_found() {
    let sound = rig::heel_bones(&shoes(0, 1), &[]);
    assert!(sound.vertices > 0);
    assert_eq!(sound.on_foot, sound.vertices);
    assert!(sound.holds());

    // The left heel follows the toe bone, so the heel lifts whenever the toe bends.
    let dragged = rig::heel_bones(&shoes(2, 1), &[]);
    assert!(!dragged.holds(), "{}", dragged.sentence());
    assert_eq!(
        dragged.elsewhere.first().map(|e| e.0.as_str()),
        Some("SKEL_L_Toe0")
    );
    assert!(
        dragged.share() < 0.9 && dragged.share() > 0.3,
        "{}",
        dragged.share()
    );
    assert!(dragged.sentence().contains("SKEL_L_Toe0"));
}

#[test]
fn the_window_that_holds_the_texture_a_mesh_uses_is_the_smallest_aligned_power_of_two_part() {
    use or_interchange::doctor::uv::Window;
    let held = |min: [f32; 2], max: [f32; 2]| Window::holding(&UvBox { min, max });
    // A quarter of the width and half the height, where the used part lies inside it.
    let w = held([0.30, 0.05], [0.49, 0.45]).expect("a part of the texture");
    assert_eq!((w.x.index, w.x.power, w.y.index, w.y.power), (1, 2, 0, 1));
    assert_eq!(w.share(), 0.125);
    assert_eq!(
        w.bounds(),
        UvBox {
            min: [0.25, 0.0],
            max: [0.5, 0.5]
        }
    );
    // The midline is a part's edge and holds what stops at it, but not what crosses it.
    assert!(held([0.0, 0.0], [0.5, 1.0]).is_some());
    assert!(
        held([0.4, 0.0], [0.6, 1.0]).is_none(),
        "a box that straddles the middle needs the whole width, and the height is whole too"
    );
    // The whole texture, a texture that repeats and a speck are none.
    assert!(held([0.0, 0.0], [1.0, 1.0]).is_none());
    assert!(held([0.0, 0.0], [1.5, 0.2]).is_none());
    assert!(held([0.5, 0.5], [0.5001, 0.5001]).is_some());
    // Applying the window moves the used part to the whole texture, held inside it.
    let mut scene = scene_of(&SIDED);
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    for uv in &mut primitive.uvs[0] {
        *uv = [0.25 + uv[0] * 0.25, uv[1] * 0.5];
    }
    let used = uv::used(&scene).unwrap();
    let w = Window::holding(&used).unwrap();
    w.apply(&mut scene);
    let after = uv::used(&scene).unwrap();
    assert_eq!(after, UvBox::UNIT, "the used part now fills the texture");
}

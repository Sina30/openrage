use std::mem::offset_of;

use glam::{Mat4, Quat, Vec3};
use or_io::decl::Raw;
use or_io::gta5::CrBlockStream;
use or_io::gta6::anim::{read, Animation, ClipEntry, ClipGroup};
use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{Bone, Model, Palette, Skeleton as Gta6Skeleton, Vertices};

use super::*;

const VIRTUAL: u64 = 0x5000_0000;
/// Three different lanes, so a lane put in the wrong place moves the pose.
const ROOT_TRANSLATION: [f32; 3] = [0.5, -1.0, 2.0];
/// x, y and z of a turn whose lanes all differ; the codec rebuilds w.
const SPINE_ROTATION: [f32; 3] = [0.1, 0.2, 0.7];

fn hash(name: &str) -> u32 {
    name_hash32(name.as_bytes())
}

#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn alloc(&mut self, len: usize) -> usize {
        let at = self.0.len().next_multiple_of(16);
        self.0.resize(at + len, 0);
        at
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.put(at, &(VIRTUAL + to as u64).to_le_bytes());
    }
}

/// A standalone animation set of one two-frame animation of four DOFs, each a constant: SKEL_Spine0's
/// rotation, SKEL_ROOT's translation, the rotation of a bone no skeleton here holds, and a
/// genericControl float.
fn animation() -> Animation {
    let dofs = [
        (hash("SKEL_Spine0"), "boneRotation", 0u32),
        (hash("SKEL_ROOT"), "boneTranslation", 2),
        (hash("SKEL_Missing"), "boneRotation", 0),
        (0, "genericControl", 4),
    ];
    // Constants in section order: two quaternions of section 0, a vector of section 1, a float of 2.
    let constants: Vec<f32> = [SPINE_ROTATION, [0.0, 0.0, 0.0], ROOT_TRANSLATION]
        .concat()
        .into_iter()
        .chain([0.5])
        .collect();
    built(&dofs, [&[0, 2 << 2], &[1 << 2], &[3 << 2]], &constants)
}

/// A two-frame animation of `dofs`, each a constant: `runs` are the entries of directory sections
/// 0 to 2 and `constants` their values in that order.
fn built(dofs: &[(u32, &str, u32)], runs: [&[u16]; 3], constants: &[f32]) -> Animation {
    let mut b = Body::default();
    let root = b.alloc(0x70);
    let set_name = "set_a-0";
    b.put(root + 0x38, set_name.as_bytes());
    b.0[root + 0x38 + 15] = 0x80 | set_name.len() as u8;
    b.u32(root + 0x68, hash(set_name));

    let animation = b.alloc(0x70);
    b.ptr(animation + 0x18, root);
    b.u16(animation + 0x20, 0x140);
    b.u16(animation + 0x24, 2);
    b.u16(animation + 0x26, 255);
    b.put(animation + 0x28, &1.0f32.to_le_bytes());
    let name = b.alloc(16);
    b.put(name, b"pack:/walk");
    b.ptr(animation + 0x30, name);
    let table = b.alloc(12 * dofs.len());
    for (i, (id, track, kind)) in dofs.iter().enumerate() {
        b.u32(table + 12 * i, *id);
        b.u32(table + 12 * i + 4, hash(track));
        b.u32(table + 12 * i + 8, *kind);
    }
    b.ptr(animation + 0x60, table);
    b.u16(animation + 0x68, dofs.len() as u16);
    b.u16(animation + 0x6A, dofs.len() as u16);

    let mut data: Vec<u8> = constants.iter().flat_map(|c| c.to_le_bytes()).collect();
    let constant_size = data.len();
    let pad = (4 * dofs.len()) as u16;
    let mut runs = runs.to_vec();
    runs.resize(11, &[]);
    let mut words: Vec<u16> = runs.iter().map(|r| r.len() as u16).collect();
    for run in runs {
        words.extend(run);
        words.resize(words.len() + (4 - run.len() % 4) % 4, pad);
    }
    data.extend(words.iter().flat_map(|w| w.to_le_bytes()));
    let block = b.alloc(0x20 + data.len());
    b.u32(
        block + offset_of!(Raw<CrBlockStream>, data_size),
        data.len() as u32,
    );
    b.u32(
        block + offset_of!(Raw<CrBlockStream>, constant_size),
        constant_size as u32,
    );
    b.u32(
        block + offset_of!(Raw<CrBlockStream>, channel_offset),
        data.len() as u32 + 0x20,
    );
    b.u16(block + offset_of!(Raw<CrBlockStream>, slop_size), 2);
    b.u16(block + offset_of!(Raw<CrBlockStream>, num_frames), 2);
    b.0[block + offset_of!(Raw<CrBlockStream>, segment_size)] = 11;
    b.put(block + 0x20, &data);
    let list = b.alloc(8);
    b.ptr(list, block);
    b.ptr(animation + 0x50, list);
    b.u16(animation + 0x58, 1);
    b.u16(animation + 0x5A, 1);
    b.u32(animation + 0x48, data.len() as u32 + 0x20);

    let entry = b.alloc(0x20);
    b.u32(entry, hash("walk"));
    b.ptr(entry + 8, animation);
    let buckets = b.alloc(8);
    b.ptr(buckets, entry);
    b.ptr(root + 0x48, buckets);
    b.u32(root + 0x48 + 8, 1);
    b.u32(root + 0x48 + 0x10, 1);
    b.0[root + 0x48 + 0x17] = 1;

    let mut resource = read(&b.0).expect("the built set reads");
    resource.sets.remove(0).animations.remove(0)
}

fn bone(name: &str, parent: Option<u16>, translation: [f32; 3]) -> Bone {
    turned(name, parent, Quat::IDENTITY, translation)
}

fn turned(name: &str, parent: Option<u16>, rotation: Quat, translation: [f32; 3]) -> Bone {
    Bone {
        name: name.to_string(),
        hash: hash(name),
        parent,
        rotation: rotation.to_array(),
        translation,
        scale: [1.0; 3],
        inverse_bind: [[0.0; 3]; 4],
    }
}

/// The local bind matrix of a built bone.
fn local_of(b: &Bone) -> Mat4 {
    Mat4::from_rotation_translation(Quat::from_array(b.rotation), Vec3::from(b.translation))
}

/// `bones` with each one's inverse bind the inverse of its parent's world times its local.
fn with_inverse_binds(mut bones: Vec<Bone>) -> Gta6Skeleton {
    let mut world: Vec<Mat4> = Vec::new();
    for b in &mut bones {
        let w = b.parent.map_or(Mat4::IDENTITY, |p| world[usize::from(p)]) * local_of(b);
        let inverse = w.inverse();
        b.inverse_bind = std::array::from_fn(|c| inverse.col(c).truncate().to_array());
        world.push(w);
    }
    Gta6Skeleton { bones }
}

/// SKEL_ROOT, SKEL_Spine0 under it and SKEL_Head under that, each a step along x.
fn gta6_skeleton() -> Gta6Skeleton {
    with_inverse_binds(vec![
        bone("SKEL_ROOT", None, [0.0, 0.0, 1.0]),
        bone("SKEL_Spine0", Some(0), [0.5, 0.0, 0.0]),
        bone("SKEL_Head", Some(1), [0.25, 0.0, 0.0]),
    ])
}

fn close(a: Mat4, b: Mat4) -> bool {
    a.abs_diff_eq(b, 1e-5)
}

#[test]
fn a_gta6_skeleton_becomes_one_whose_bind_world_is_each_parents_world_times_the_local() {
    let source = gta6_skeleton();
    let skeleton = skeleton(&source);
    let local = |b: &Bone| Mat4::from_translation(Vec3::from(b.translation));
    let root = local(&source.bones[0]);
    let spine = root * local(&source.bones[1]);
    let head = spine * local(&source.bones[2]);
    assert_eq!(skeleton.bone_count(), 3);
    assert_eq!(skeleton.bones[2].name, "SKEL_Head");
    assert!(close(skeleton.bind_world[0], root));
    assert!(close(skeleton.bind_world[1], spine));
    assert!(close(skeleton.bind_world[2], head));
}

#[test]
fn the_pose_at_a_frame_puts_each_tracks_value_in_its_bones_local_and_chains_the_worlds() {
    let animation = animation();
    let source = gta6_skeleton();
    let skeleton = skeleton(&source);
    let binding = Binding::build(&source, &animation);
    let pose = binding.pose(&skeleton, &animation, 0.0).unwrap();
    let world = pose.world_matrices(&skeleton);

    let [x, y, z] = SPINE_ROTATION;
    let spin = Quat::from_xyzw(x, y, z, (1.0 - x * x - y * y - z * z).sqrt());
    let root = Mat4::from_translation(Vec3::from(ROOT_TRANSLATION));
    let spine =
        root * Mat4::from_rotation_translation(spin, Vec3::from(source.bones[1].translation));
    let head = spine * Mat4::from_translation(Vec3::from(source.bones[2].translation));
    assert!(close(world[0], root), "{:?}", world[0]);
    assert!(close(world[1], spine), "{:?}", world[1]);
    assert!(close(world[2], head), "{:?}", world[2]);
}

#[test]
fn a_bone_track_whose_bone_is_missing_and_a_track_no_bone_has_are_skipped_by_name() {
    let animation = animation();
    let binding = Binding::build(&gta6_skeleton(), &animation);
    assert_eq!(
        binding.skipped,
        vec![
            Skipped::NoSuchBone {
                dof: 2,
                id: hash("SKEL_Missing")
            },
            Skipped::NotABoneTrack {
                dof: 3,
                track: hash("genericControl")
            },
        ]
    );
    assert_eq!(
        binding.bones,
        vec![
            BoneDofs {
                bone: 0,
                translation: Some(1),
                rotation: None,
                scale: None
            },
            BoneDofs {
                bone: 1,
                translation: None,
                rotation: Some(0),
                scale: None
            },
        ]
    );
}

#[test]
fn a_bone_no_track_moves_keeps_its_bind_local() {
    let animation = animation();
    let source = gta6_skeleton();
    let skeleton = skeleton(&source);
    let pose = Binding::build(&source, &animation)
        .pose(&skeleton, &animation, 0.0)
        .unwrap();
    let head = pose.local(2).unwrap();
    assert_eq!(head.translation, Vec3::from(source.bones[2].translation));
    assert_eq!(head.rotation, Quat::IDENTITY);
}

#[test]
fn a_mover_that_holds_still_moves_the_root_nowhere_whatever_its_value_and_moves_no_bone() {
    let at = [1.5, -2.25, 0.125];
    let [x, y, z] = [0.05, -0.1, 0.3];
    let dofs = [
        (0, "moverAbsoluteTranslation", 2),
        (0, "moverAbsoluteRotation", 0),
    ];
    let animation = built(
        &dofs,
        [&[1 << 2], &[0], &[]],
        &[x, y, z, at[0], at[1], at[2]],
    );
    let source = gta6_skeleton();
    let skeleton = skeleton(&source);
    let binding = Binding::build(&source, &animation);
    assert_eq!(
        (binding.mover_translation, binding.mover_rotation),
        (Some(0), Some(1))
    );
    assert!(binding.skipped.is_empty() && binding.bones.is_empty());

    let pose = binding.pose(&skeleton, &animation, 0.5).unwrap();
    assert!(pose.root_motion.translation.abs_diff_eq(Vec3::ZERO, 1e-6));
    assert!(pose.root_motion.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5));
    assert_eq!(pose.locals(), Pose::bind(&skeleton).locals());
}

#[test]
fn a_window_s_root_motion_is_the_mover_s_change_since_its_start_in_the_start_s_frame() {
    let start = (Vec3::new(1.5, -2.25, 0.125), Quat::from_rotation_z(0.9));
    let step = (Vec3::new(0.3, 2.0, -0.05), Quat::from_rotation_z(-0.4));
    let now = Mat4::from_rotation_translation(start.1, start.0)
        * Mat4::from_rotation_translation(step.1, step.0);
    let (_, rotation, translation) = now.to_scale_rotation_translation();
    let moved = from_window_start(start, (translation, rotation));
    assert!(moved.translation.abs_diff_eq(step.0, 1e-5), "{moved:?}");
    assert!(moved.rotation.abs_diff_eq(step.1, 1e-5), "{moved:?}");
}

fn entry(start: f32, end: f32, rate: f32) -> ClipEntry {
    ClipEntry {
        start,
        end,
        rate,
        ..ClipEntry::default()
    }
}

/// An animation of `frames` frames at `fps`, with no DOF.
fn timed(frames: u16, fps: f32) -> Animation {
    Animation {
        frames,
        frames_per_chunk: 255,
        duration: f32::from(frames - 1) / fps,
        ..Animation::default()
    }
}

#[test]
fn a_group_s_two_layers_land_their_frames_at_their_own_rate_after_the_groups_before() {
    // Group 1 holds a 30 fps layer at rate 1 and a 15 fps layer at rate 0.5, each spanning 1.2 s.
    let groups = [
        ClipGroup {
            duration: 0.8,
            entries: vec![entry(0.0, 0.8, 1.0)],
        },
        ClipGroup {
            duration: 1.2,
            entries: vec![entry(0.31, 1.51, 1.0), entry(0.1, 0.7, 0.5)],
        },
    ];
    assert_eq!(group_starts(&groups), [0.0, 0.8]);
    let (fast, slow) = (timed(46, 30.0), timed(13, 15.0));
    let fast_times = entry_frame_times(0.8, &groups[1].entries[0], &fast);
    let slow_times = entry_frame_times(0.8, &groups[1].entries[1], &slow);
    // Frames 10 to 45 of the fast layer lie in its window, and 2 to 10 of the slow one.
    assert_eq!((fast_times.len(), slow_times.len()), (36, 9));
    let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
    assert!(close(fast_times[0], 0.8 + (10.0 / 30.0 - 0.31)));
    assert!(close(slow_times[0], 0.8 + (2.0 / 15.0 - 0.1) / 0.5));
    assert!(close(slow_times[8], 0.8 + (10.0 / 15.0 - 0.1) / 0.5));

    // Played back at the time a slow frame lands, each layer reads its own animation time.
    let layers = layers_at(&groups, slow_times[1]);
    assert_eq!(layers.len(), 2);
    assert_eq!((layers[1].group, layers[1].entry), (1, 1));
    assert!(close(layers[1].time, 3.0 / 15.0));
    assert!(close(layers[0].time, 0.31 + (slow_times[1] - 0.8)));
    let first = layers_at(&groups, 0.5);
    assert_eq!((first.len(), first[0].group), (1, 0));
    assert!(close(first[0].time, 0.5));
    let end = layers_at(&groups, 2.0);
    assert!(close(end[0].time, 1.51) && close(end[1].time, 0.7));
}

/// Three bones in a chain, each turned about another axis and stepped along all three.
fn chain() -> Gta6Skeleton {
    with_inverse_binds(vec![
        turned(
            "SKEL_ROOT",
            None,
            Quat::from_rotation_z(0.3),
            [0.2, -0.4, 1.1],
        ),
        turned(
            "SKEL_Spine0",
            Some(0),
            Quat::from_rotation_x(0.5),
            [0.5, 0.1, -0.2],
        ),
        turned(
            "SKEL_Head",
            Some(1),
            Quat::from_rotation_y(-0.7),
            [0.25, 0.6, 0.05],
        ),
    ])
}

/// A model whose palette is `palette` and whose matrix index is `matrix_index`.
fn model(palette: Palette, matrix_index: u8) -> Model {
    Model {
        geometries: Vec::new(),
        palette: Ok(palette),
        matrix_index,
    }
}

/// The sorted hash palette of `names`, and where each name sits in it.
fn hash_palette(names: &[&str]) -> (Palette, Vec<u16>) {
    let mut hashes: Vec<u32> = names.iter().map(|n| hash(n)).collect();
    hashes.sort_unstable();
    let at = (names.iter())
        .map(|n| hashes.iter().position(|h| *h == hash(n)).unwrap() as u16)
        .collect();
    (Palette::Hashes(hashes), at)
}

/// One vertex's weights and indices: lanes 0 to 3 in channels 16 and 20, 4 to 7 in 17 and 21.
fn lanes(weights: &[u8], indices: &[u16]) -> Vertices {
    let four = |k: usize, of: &[u8]| -> [f32; 4] {
        std::array::from_fn(|l| of.get(4 * k + l).map_or(0.0, |w| f32::from(*w) / 255.0))
    };
    let joints = |k: usize| -> [u16; 4] {
        std::array::from_fn(|l| indices.get(4 * k + l).copied().unwrap_or(0))
    };
    let sets = weights.len().div_ceil(4);
    Vertices {
        weights: (0..sets)
            .map(|k| (16 + k as u8, vec![four(k, weights)]))
            .collect(),
        joints: (0..sets).map(|k| (20 + k as u8, vec![joints(k)])).collect(),
        ..Vertices::default()
    }
}

/// The skin matrices of `source` with `moved` locals set over its bind pose.
fn skin_of(source: &Gta6Skeleton, moved: &[(u16, Quat, Vec3)]) -> Vec<Mat4> {
    let skeleton = skeleton(source);
    let mut pose = Pose::bind(&skeleton);
    for (bone, rotation, translation) in moved {
        pose.set_local(
            *bone,
            crate::pose::BoneTransform {
                translation: *translation,
                rotation: *rotation,
                scale: Vec3::ONE,
            },
        );
    }
    let (mut world, mut skin) = (Vec::new(), Vec::new());
    pose.pose_into(&skeleton, &mut world, &mut skin);
    skin
}

fn skinned(mesh: &SkinnedMesh, skin: &[Mat4]) -> Vec3 {
    let (mut positions, mut normals) = (Vec::new(), Vec::new());
    mesh.skin_into(skin, &mut positions, &mut normals);
    Vec3::from(positions[0])
}

const VERTEX: [f32; 3] = [0.7, -0.3, 1.9];

#[test]
fn the_root_motion_moves_a_skinned_vertex_over_its_bones_skin() {
    let dofs = [(hash("SKEL_Spine0"), "boneRotation", 0)];
    let animation = built(&dofs, [&[0], &[], &[]], &[0.4, -0.2, 0.1]);
    let source = chain();
    let skeleton = skeleton(&source);
    let mut pose = Binding::build(&source, &animation)
        .pose(&skeleton, &animation, 0.5)
        .unwrap();
    pose.root_motion = from_window_start(
        (Vec3::ZERO, Quat::IDENTITY),
        (
            Vec3::new(1.5, -2.25, 0.125),
            Quat::from_xyzw(0.05, -0.1, 0.3, 0.9).normalize(),
        ),
    );
    let root = pose.root_motion.to_matrix();
    assert!(!close(root, Mat4::IDENTITY));
    let unmoved = pose.world_matrices(&skeleton);
    assert!(
        !close(unmoved[2], skeleton.bind_world[2]),
        "the spine turns"
    );
    let (palette, index) = hash_palette(&["SKEL_ROOT", "SKEL_Spine0", "SKEL_Head"]);
    let normal = [[0.0, 0.0, 1.0]];
    let vertices = lanes(&[255], &[index[2]]);
    let mesh = skin(
        &vertices,
        &[VERTEX],
        &normal,
        &model(palette, 0),
        None,
        &source,
    )
    .unwrap();

    let (world, skin_matrices) = posed_matrices(&pose, &skeleton);
    assert!(close(world[2], root * unmoved[2]));
    let in_place = unmoved[2] * skeleton.inverse_bind[2];
    let expected = (root * in_place).transform_point3(Vec3::from(VERTEX));
    let got = skinned(&mesh, &skin_matrices);
    assert!(got.distance(expected) < 1e-4, "{got} against {expected}");
    let without = in_place.transform_point3(Vec3::from(VERTEX));
    assert!(got.distance(without) > 0.1, "the root motion moves it");
}

#[test]
fn the_skeleton_the_pose_code_takes_keeps_the_inverse_binds_the_file_states() {
    let mut source = chain();
    source.bones[1].inverse_bind = [
        [1.0, 2.0, 3.0],
        [4.0, 5.0, 6.0],
        [7.0, 8.0, 9.0],
        [10.0, 11.0, 12.0],
    ];
    let read = skeleton(&source);
    let stated = Mat4::from_cols_array(&[
        1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0, 0.0, 7.0, 8.0, 9.0, 0.0, 10.0, 11.0, 12.0, 1.0,
    ]);
    assert_eq!(read.inverse_bind[1], stated);
    assert!(close(read.inverse_bind[2], read.bind_world[2].inverse()));
}

#[test]
fn a_vertex_bound_to_one_bone_moves_by_that_bones_world_times_its_inverse_bind() {
    let source = chain();
    let (palette, at) = hash_palette(&["SKEL_ROOT", "SKEL_Spine0", "SKEL_Head"]);
    let vertices = lanes(&[255], &[at[1]]);
    let mesh = skin(
        &vertices,
        &[VERTEX],
        &[[0.0, 0.0, 1.0]],
        &model(palette, 0),
        None,
        &source,
    )
    .expect("every index joins");
    assert_eq!(mesh.bones_used, [1]);

    let (spin, step) = (Quat::from_rotation_y(1.1), Vec3::new(-0.3, 0.8, 0.4));
    let moved = skin_of(&source, &[(1, spin, step)]);
    let world = local_of(&source.bones[0]) * Mat4::from_rotation_translation(spin, step);
    let bind = local_of(&source.bones[0]) * local_of(&source.bones[1]);
    let expected = (world * bind.inverse()).transform_point3(Vec3::from(VERTEX));
    let got = skinned(&mesh, &moved);
    assert!(got.distance(expected) < 1e-4, "{got} against {expected}");
    let reversed = (bind.inverse() * world).transform_point3(Vec3::from(VERTEX));
    assert!(
        got.distance(reversed) > 0.1,
        "the reverse order lands elsewhere"
    );

    // At bind the vertex stays where the file put it.
    let rest = skinned(&mesh, &skin_of(&source, &[]));
    assert!(rest.distance(Vec3::from(VERTEX)) < 1e-4, "{rest}");
}

#[test]
fn eight_influences_blend_each_bones_skin_by_its_weight() {
    let names = ["B0", "B1", "B2", "B3", "B4", "B5", "B6", "B7"];
    let source = with_inverse_binds(
        (names.iter().enumerate())
            .map(|(i, n)| {
                let i = i as f32;
                let at = [0.1 * i, -0.2 * i + 0.3, 0.05 * i * i];
                turned(n, None, Quat::from_rotation_z(0.2 * i), at)
            })
            .collect(),
    );
    let moved: Vec<(u16, Quat, Vec3)> = (0..8u16)
        .map(|b| {
            let f = f32::from(b);
            let turn = Quat::from_rotation_x(0.3 * f - 0.5);
            (b, turn, Vec3::new(f, 2.0 - f, 0.5 * f))
        })
        .collect();
    let skin_matrices = skin_of(&source, &moved);
    let (palette, at) = hash_palette(&names);
    // Lanes out of weight order, summing to 255; lane k names bone 7 - k.
    let weights = [10u8, 20, 30, 40, 50, 60, 25, 20];
    let indices: Vec<u16> = (0..8).map(|k| at[7 - k]).collect();
    let vertices = lanes(&weights, &indices);
    let mesh = skin(
        &vertices,
        &[VERTEX],
        &[[0.0, 0.0, 1.0]],
        &model(palette, 0),
        None,
        &source,
    )
    .expect("every index joins");
    assert_eq!(mesh.influences[0].count, 8);

    let skin_k = |b: usize| {
        let (_, turn, step) = moved[b];
        Mat4::from_rotation_translation(turn, step) * local_of(&source.bones[b]).inverse()
    };
    let p = Vec3::from(VERTEX);
    let expected: Vec3 = (0..8)
        .map(|k| skin_k(7 - k).transform_point3(p) * f32::from(weights[k]) / 255.0)
        .sum();
    let got = skinned(&mesh, &skin_matrices);
    assert!(got.distance(expected) < 1e-4, "{got} against {expected}");
    let first_four: Vec3 = (0..4)
        .map(|k| skin_k(7 - k).transform_point3(p) * f32::from(weights[k]) / 100.0)
        .sum();
    assert!(got.distance(first_four) > 0.1, "the last four lanes count");
}

#[test]
fn a_palette_hash_no_skeleton_has_leaves_its_weight_to_the_other_lanes_and_alone_is_named() {
    let source = chain();
    let (palette, at) = hash_palette(&["SKEL_ROOT", "SKEL_Missing", "SKEL_Head"]);
    let model = model(palette, 0);
    let normal = [[0.0, 0.0, 1.0]];
    let uses_it = lanes(&[200, 55], &[at[0], at[1]]);
    let mesh = skin(&uses_it, &[VERTEX], &normal, &model, None, &source).unwrap();
    assert_eq!(
        mesh.influences[0],
        VertexInfluences::rigid(0),
        "renormalised"
    );
    let only_it = lanes(&[255], &[at[1]]);
    assert_eq!(
        skin(&only_it, &[VERTEX], &normal, &model, None, &source).unwrap_err(),
        Unjoined::NoSuchHash {
            hash: hash("SKEL_Missing")
        }
    );
    // A lane of no weight names nothing.
    let leaves_it = lanes(&[200, 0, 55], &[at[0], at[1], at[2]]);
    let mesh = skin(&leaves_it, &[VERTEX], &normal, &model, None, &source).unwrap();
    assert_eq!(mesh.bones_used, [0, 2]);
    let past = lanes(&[255], &[3]);
    assert_eq!(
        skin(&past, &[VERTEX], &normal, &model, None, &source).unwrap_err(),
        Unjoined::PastPalette { index: 3, count: 3 }
    );
}

#[test]
fn an_own_skeleton_index_joins_by_its_bones_name_and_a_geometry_with_no_indices_hangs_off_the_matrix_index(
) {
    let picked = chain();
    // The drawable's own skeleton lists the same bones in another order.
    let own = with_inverse_binds(vec![
        bone("SKEL_Head", None, [0.0; 3]),
        bone("SKEL_ROOT", None, [0.0; 3]),
        bone("SKEL_Spine0", Some(1), [0.0; 3]),
    ]);
    let normal = [[0.0, 0.0, 1.0]];
    let indexed = lanes(&[255], &[0]);
    let own_model = model(Palette::Own, 2);
    let mesh = skin(
        &indexed,
        &[VERTEX],
        &normal,
        &own_model,
        Some(&own),
        &picked,
    )
    .unwrap();
    assert_eq!(
        mesh.influences[0].bones[0], 2,
        "own bone 0 is the head, bone 2"
    );
    let same = skin(
        &indexed,
        &[VERTEX],
        &normal,
        &own_model,
        Some(&picked),
        &picked,
    )
    .unwrap();
    assert_eq!(same.influences[0].bones[0], 0);

    // One index a vertex and no weights: all of its weight.
    let one = Vertices {
        joints: vec![(20, vec![[1, 9, 9, 9]])],
        ..Vertices::default()
    };
    let mesh = skin(&one, &[VERTEX], &normal, &own_model, Some(&own), &picked).unwrap();
    assert_eq!(mesh.influences[0], VertexInfluences::rigid(0));

    let rigid = skin(
        &Vertices::default(),
        &[VERTEX],
        &normal,
        &own_model,
        Some(&own),
        &picked,
    );
    assert_eq!(rigid.unwrap().influences[0], VertexInfluences::rigid(1));
    assert_eq!(
        skin(
            &Vertices::default(),
            &[VERTEX],
            &normal,
            &own_model,
            None,
            &picked
        )
        .unwrap_err(),
        Unjoined::NoOwnSkeleton
    );
}

#[test]
fn a_hash_the_body_lacks_moves_with_its_own_skeleton_s_ancestor_and_a_face_bone_with_the_head() {
    let body = chain();
    let normal = [[0.0, 0.0, 1.0]];
    // The part's own skeleton: the head, a bone under it the body lacks, and a stray root.
    let own = with_inverse_binds(vec![
        bone("SKEL_Head", None, [0.0; 3]),
        bone("RB_Jaw", Some(0), [0.1, 0.0, 0.0]),
        bone("Stray_Root", None, [0.0; 3]),
    ]);
    let face = with_inverse_binds(vec![
        bone("FACIAL_C_FacialRoot", None, [0.0; 3]),
        bone("FB_Brow", Some(0), [0.0, 0.1, 0.0]),
    ]);
    let joined = |names: &[&str], own: &Gta6Skeleton| {
        let (palette, at) = hash_palette(names);
        let vertices = lanes(&[255], &[at[0]]);
        skin(
            &vertices,
            &[VERTEX],
            &normal,
            &model(palette, 0),
            Some(own),
            &body,
        )
        .map(|mesh| mesh.influences[0])
    };
    let head = VertexInfluences::rigid(2);
    assert_eq!(joined(&["RB_Jaw", "SKEL_ROOT"], &own), Ok(head));
    assert_eq!(
        joined(&["FB_Brow"], &face),
        Ok(head),
        "the face moves with the head"
    );
    assert_eq!(
        joined(&["Stray_Root"], &own),
        Err(Unjoined::NoSuchHash {
            hash: hash("Stray_Root")
        }),
        "a root of no face skeleton that the body lacks joins nothing"
    );
    assert_eq!(
        joined(&["FB_Brow"], &own),
        Err(Unjoined::NoSuchHash {
            hash: hash("FB_Brow")
        })
    );
}

#[test]
fn a_palette_entry_of_zero_joins_no_bone_even_where_a_bone_has_hash_zero() {
    let mut body = chain();
    body.bones[0].hash = 0;
    let spine = hash("SKEL_Spine0");
    let model = model(Palette::Hashes(vec![0, spine]), 0);
    let normal = [[0.0, 0.0, 1.0]];
    let vertices = lanes(&[200, 55], &[0, 1]);
    let mesh = skin(&vertices, &[VERTEX], &normal, &model, None, &body).unwrap();
    assert_eq!(
        mesh.influences[0],
        VertexInfluences::rigid(1),
        "renormalised"
    );
    let only_zero = lanes(&[255], &[0]);
    assert_eq!(
        skin(&only_zero, &[VERTEX], &normal, &model, None, &body).unwrap_err(),
        Unjoined::NoSuchHash { hash: 0 }
    );
}

#[test]
fn a_hash_only_an_extra_skeleton_holds_stays_at_bind_rather_than_going_missing() {
    let body = chain();
    let strap = with_inverse_binds(vec![
        bone("GM_Strap_Root", None, [0.1, 0.0, 0.0]),
        bone("GM_Strap_End", Some(0), [0.0, 0.2, 0.0]),
    ]);
    let (palette, at) = hash_palette(&["SKEL_ROOT", "GM_Strap_End", "SKEL_Missing"]);
    let model = model(palette, 0);
    let normal = [[0.0, 0.0, 1.0]];
    let extra = [strap];
    let influences = |weights: &[u8], indices: &[u16]| {
        let vertices = lanes(weights, indices);
        skin_with_extra(&vertices, &[VERTEX], &normal, &model, None, &extra, &body)
            .map(|mesh| mesh.influences[0])
    };
    // Every weighted lane names the strap, or names it and a bone no skeleton has: at bind.
    let at_bind = Ok(VertexInfluences::UNBOUND);
    assert_eq!(influences(&[255], &[at[1]]), at_bind);
    assert_eq!(influences(&[200, 55], &[at[1], at[2]]), at_bind);
    // Beside a body lane, the strap's lane leaves its weight to it.
    let body_lane = influences(&[200, 55], &[at[0], at[1]]);
    assert_eq!(body_lane, Ok(VertexInfluences::rigid(0)));
    // With no extra skeleton the strap's hash is missing.
    let vertices = lanes(&[255], &[at[1]]);
    assert_eq!(
        skin(&vertices, &[VERTEX], &normal, &model, None, &body).unwrap_err(),
        Unjoined::NoSuchHash {
            hash: hash("GM_Strap_End")
        }
    );
}

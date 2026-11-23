use glam::{Quat, Vec3};
use or_anim::{
    bake_into, clip, ClipBinding, ClipPlayer, EditKey, EditTrack, EditValue, PoseEdits, Skeleton,
};
use or_io::codec::anim::{
    encode_block, format_type, track, AnimationSource, DofSampleTable, EncodePolicy,
};
use or_io::decl::{Keyed, Open};
use or_io::gta5::{
    AtMap, AtMapEntry, AtString, CrAnimDictionary, CrAnimation, CrBoneData, CrClip,
    CrClipAnimation, CrClipAny, CrClipDictionary, CrSkeletonData, Dof, EClipType, File as _, Vec4V,
};

const FRAMES: u16 = 12;
const ANIMATION: u32 = 0x1000;
const CLIP: u32 = 0x2000;

fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
    Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    }
}

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
            bone(1, 200, 0, [1.0, 0.0, 0.0]),
        ],
        ..CrSkeletonData::default()
    })
}

fn dof(id: u16, r#type: u8, track: u8) -> Dof {
    Dof {
        id,
        r#type,
        track,
        ..Dof::default()
    }
}

fn translation_dof(tag: u16) -> Dof {
    dof(tag, format_type::VECTOR3, track::BONE_TRANSLATION)
}

fn rotation_dof(tag: u16) -> Dof {
    dof(tag, format_type::QUATERNION, track::BONE_ROTATION)
}

fn animation() -> CrAnimation {
    let frames: Vec<[f32; 4]> = (0..FRAMES)
        .map(|i| [i as f32 * 0.1, 0.0, 0.0, 0.0])
        .collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
    let block = encode_block(&[table], FRAMES, EncodePolicy::Lossless).expect("encode");
    CrAnimation {
        num_frames: FRAMES,
        frames_per_chunk: 127,
        duration: 1.1,
        name: "walk".into(),
        dofs: vec![translation_dof(100)],
        max_block_size: block.data.len() as u32,
        blocks: vec![Some(Box::new(block.into()))],
        ..CrAnimation::default()
    }
}

fn one_entry<T: or_io::decl::Owned>(key: u32, data: T) -> AtMap<T>
where
    or_io::decl::Raw<T>: or_io::decl::Record,
{
    AtMap {
        hash: vec![Some(Box::new(AtMapEntry {
            key,
            data: Some(Box::new(data)),
            ..AtMapEntry::default()
        }))],
        ..AtMap::default()
    }
}

fn dictionary_with(animation: CrAnimation) -> CrClipDictionary {
    let clip = CrClipAnimation {
        base: CrClip {
            r#type: Open::new(EClipType::Animation),
            name: AtString::new("walk"),
            ..CrClip::default()
        },
        animation: Keyed::to(ANIMATION),
        start_time: 0.0,
        end_time: animation.duration,
        rate: 1.0,
        ..CrClipAnimation::default()
    };
    CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: one_entry(ANIMATION, animation),
            ..CrAnimDictionary::default()
        })),
        clips: one_entry(CLIP, CrClipAny::CrClipAnimation(clip)),
        ..CrClipDictionary::default()
    }
}

/// The dictionary written as a file and opened again.
fn saved_and_reopened(animation: CrAnimation) -> CrClipDictionary {
    let bytes = dictionary_with(animation).to_bytes().expect("save");
    CrClipDictionary::from_bytes(&bytes).expect("load")
}

fn play(dictionary: &CrClipDictionary, skeleton: &Skeleton) -> ClipPlayer {
    let clip = clip::clip(dictionary, CLIP).expect("clip");
    ClipPlayer::bind(skeleton, dictionary, clip)
}

#[test]
fn an_edit_survives_bake_save_and_reload() {
    let skeleton = skeleton();
    let mut animation = animation();
    let binding = ClipBinding::build(&skeleton, &animation);

    let mut edits = PoseEdits::new();
    edits.set(
        EditKey {
            bone: 0,
            track: EditTrack::Translation,
            frame: 5,
        },
        EditValue::Vector(Vec3::new(42.0, -7.0, 3.5)),
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
    assert!(!baked.is_lossy());

    let reloaded = saved_and_reopened(animation);

    let mut player = play(&reloaded, &skeleton);
    let frame_rate = player.frame_rate().expect("frame rate");
    let pose = player.evaluate(&skeleton, 5.0 / frame_rate);
    let posed = pose.local(0).expect("bone 0").translation;
    assert!(
        (posed - Vec3::new(42.0, -7.0, 3.5)).length() < 1e-3,
        "edit did not survive: bone 0 is at {posed}"
    );
}

#[test]
fn baking_leaves_untouched_frames_alone() {
    let skeleton = skeleton();
    let mut animation = animation();
    let binding = ClipBinding::build(&skeleton, &animation);

    let mut edits = PoseEdits::new();
    edits.set(
        EditKey {
            bone: 0,
            track: EditTrack::Translation,
            frame: 5,
        },
        EditValue::Vector(Vec3::new(42.0, 0.0, 0.0)),
    );
    bake_into(
        &mut animation,
        &skeleton,
        &binding,
        &edits,
        EncodePolicy::Lossless,
    )
    .expect("bake");

    let reloaded = saved_and_reopened(animation);
    let mut player = play(&reloaded, &skeleton);
    let frame_rate = player.frame_rate().expect("frame rate");

    for frame in [0u16, 2, 8, 11] {
        let pose = player.evaluate(&skeleton, f32::from(frame) / frame_rate);
        let x = pose.local(0).expect("bone 0").translation.x;
        let expected = f32::from(frame) * 0.1;
        assert!(
            (x - expected).abs() < 1e-3,
            "frame {frame} should still be {expected}, got {x}"
        );
    }
}

#[test]
fn editing_an_unanimated_bone_adds_a_channel_that_survives_the_file() {
    let skeleton = skeleton();
    let mut animation = animation();
    let binding = ClipBinding::build(&skeleton, &animation);
    assert_eq!(animation.dofs.len(), 1, "the source drives one DOF");

    let mut edits = PoseEdits::new();
    let rotation = Quat::from_rotation_z(0.4);
    edits.set(
        EditKey {
            bone: 1,
            track: EditTrack::Rotation,
            frame: 3,
        },
        EditValue::Rotation(rotation),
    );

    bake_into(
        &mut animation,
        &skeleton,
        &binding,
        &edits,
        EncodePolicy::Lossless,
    )
    .expect("bake");
    assert_eq!(animation.dofs.len(), 2, "a rotation DOF should be appended");

    let reloaded = saved_and_reopened(animation);
    let reloaded_animation = clip::animation(&reloaded, ANIMATION).expect("animation");
    assert_eq!(reloaded_animation.dofs.len(), 2);
    assert!(reloaded_animation
        .dofs
        .iter()
        .any(|dof| dof.id == 200 && dof.track == track::BONE_ROTATION));

    let mut player = play(&reloaded, &skeleton);
    let frame_rate = player.frame_rate().expect("frame rate");
    let pose = player.evaluate(&skeleton, 3.0 / frame_rate);
    let posed = pose.local(1).expect("bone 1").rotation;
    assert!(
        posed.dot(rotation).abs() > 1.0 - 1e-3,
        "rotation edit did not survive: {posed:?} vs {rotation:?}"
    );

    let pose = player.evaluate(&skeleton, 0.0);
    let at_zero = pose.local(1).expect("bone 1").rotation;
    assert!(
        at_zero.dot(Quat::IDENTITY).abs() > 1.0 - 1e-3,
        "frame 0 should hold the bind rotation, got {at_zero:?}"
    );
}

#[test]
fn multiple_edits_across_bones_and_tracks_all_survive() {
    let skeleton = skeleton();
    let mut animation = CrAnimation {
        dofs: vec![translation_dof(100), rotation_dof(100)],
        ..animation()
    };
    or_io::codec::anim::reencode_animation(&mut animation, EncodePolicy::Lossless, |dof, frame| {
        if dof == 0 {
            [frame as f32 * 0.1, 0.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    })
    .expect("seed");
    let binding = ClipBinding::build(&skeleton, &animation);

    let mut edits = PoseEdits::new();
    edits.set(
        EditKey {
            bone: 0,
            track: EditTrack::Translation,
            frame: 1,
        },
        EditValue::Vector(Vec3::new(5.0, 0.0, 0.0)),
    );
    edits.set(
        EditKey {
            bone: 0,
            track: EditTrack::Rotation,
            frame: 1,
        },
        EditValue::Rotation(Quat::from_rotation_y(0.3)),
    );
    edits.set(
        EditKey {
            bone: 1,
            track: EditTrack::Scale,
            frame: 7,
        },
        EditValue::Vector(Vec3::splat(2.0)),
    );

    let baked = bake_into(
        &mut animation,
        &skeleton,
        &binding,
        &edits,
        EncodePolicy::Lossless,
    )
    .expect("bake");
    assert_eq!(baked.edited_dofs, 3);

    let reloaded = saved_and_reopened(animation);
    let mut player = play(&reloaded, &skeleton);
    let frame_rate = player.frame_rate().expect("frame rate");

    let pose = player.evaluate(&skeleton, 1.0 / frame_rate);
    assert!((pose.local(0).unwrap().translation.x - 5.0).abs() < 1e-3);
    assert!(
        pose.local(0)
            .unwrap()
            .rotation
            .dot(Quat::from_rotation_y(0.3))
            .abs()
            > 1.0 - 1e-3
    );

    let pose = player.evaluate(&skeleton, 7.0 / frame_rate);
    let scale = pose.local(1).unwrap().scale;
    assert!(
        (scale - Vec3::splat(2.0)).length() < 1e-3,
        "scale edit did not survive: {scale}"
    );
}

#[test]
fn re_baking_preserves_the_samples_even_though_the_bytes_move() {
    let skeleton = skeleton();
    let mut animation = animation();
    let binding = ClipBinding::build(&skeleton, &animation);

    let mut edits = PoseEdits::new();
    edits.set(
        EditKey {
            bone: 0,
            track: EditTrack::Translation,
            frame: 4,
        },
        EditValue::Vector(Vec3::new(9.0, 0.0, 0.0)),
    );
    bake_into(
        &mut animation,
        &skeleton,
        &binding,
        &edits,
        EncodePolicy::Lossless,
    )
    .expect("first bake");

    let sample_all = |animation: &CrAnimation| -> Vec<[f32; 4]> {
        (0..animation.num_frames)
            .map(|frame| {
                let time = animation.internal_frame_to_time(frame as f32);
                or_io::codec::anim::decode_dof(animation, 0, time)
                    .expect("decode")
                    .expect("samples")
                    .components
            })
            .collect()
    };
    let after_first = sample_all(&animation);

    let binding = ClipBinding::build(&skeleton, &animation);
    bake_into(
        &mut animation,
        &skeleton,
        &binding,
        &PoseEdits::new(),
        EncodePolicy::Lossless,
    )
    .expect("second bake");
    let after_second = sample_all(&animation);

    assert_eq!(after_first.len(), after_second.len());
    for (frame, (a, b)) in after_first.iter().zip(after_second.iter()).enumerate() {
        for component in 0..3 {
            assert!(
                (a[component] - b[component]).abs() < 1e-4,
                "frame {frame} component {component} drifted on a re-bake: {} -> {}",
                a[component],
                b[component]
            );
        }
    }

    assert!((after_second[4][0] - 9.0).abs() < 1e-3);

    let once = dictionary_with(animation.clone()).to_bytes().expect("save");
    let twice = dictionary_with(animation).to_bytes().expect("save");
    assert_eq!(once, twice);
}

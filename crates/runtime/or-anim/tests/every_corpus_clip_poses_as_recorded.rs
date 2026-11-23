//! Every clip of every Gen8 corpus clip dictionary poses as it posed when the
//! baseline was recorded: the player's pose at each frame of the clip's whole
//! duration, and the decoded value of each DOF the player does not bind at
//! each of its animation's frames.

#[path = "../../../domain/or-interchange/tests/baseline/mod.rs"]
mod baseline;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use baseline::{Key, Now};
use or_anim::{clip, ClipBinding, ClipPlayer, Skeleton};
use or_io::codec::anim::{track, DofDecoder};
use or_io::gta5::{
    CrAnimation, CrBoneData, CrClipAny, CrClipDictionary, CrSkeletonData, File as _, Vec4V,
};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const KIND: &str = "pose";

/// Frames per second a clip is sampled at when its first animation states no
/// rate.
const FALLBACK_FRAME_RATE: f32 = 30.0;

fn v4(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
    Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    }
}

/// One unparented bone at the identity for every bone tag the clip animates,
/// so every bone track binds and the pose is the clip's alone.
fn skeleton_for(dict: &CrClipDictionary, clip: &CrClipAny) -> Skeleton {
    let mut tags: Vec<u16> = clip::played(dict, clip)
        .iter()
        .flat_map(|p| p.animation.dofs.iter())
        .filter(|d| {
            matches!(
                d.track,
                track::BONE_TRANSLATION | track::BONE_ROTATION | track::BONE_SCALE
            )
        })
        .map(|d| d.id)
        .collect();
    tags.sort_unstable();
    tags.dedup();
    let bones = tags
        .iter()
        .enumerate()
        .map(|(index, &tag)| CrBoneData {
            rotation: v4(0.0, 0.0, 0.0, 1.0),
            translation: v4(0.0, 0.0, 0.0, 0.0),
            scale: v4(1.0, 1.0, 1.0, 0.0),
            next_index: -1,
            parent_index: -1,
            name: format!("tag_{tag}"),
            index: index as u16,
            bone_id: tag,
            ..CrBoneData::default()
        })
        .collect();
    Skeleton::build(&CrSkeletonData {
        parent_indices: vec![-1; tags.len()],
        bones,
        ..CrSkeletonData::default()
    })
}

fn push(stream: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        stream.extend_from_slice(&v.to_bits().to_le_bytes());
    }
}

/// Clip times one frame apart from zero to the clip's duration, which runs
/// past the first animation for a sequential or rate-scaled clip.
fn clip_times(player: &ClipPlayer) -> Vec<f32> {
    let rate = player.frame_rate().unwrap_or(FALLBACK_FRAME_RATE);
    let duration = player.duration();
    let last = if duration.is_finite() && duration > 0.0 {
        (duration * rate).round() as u32
    } else {
        0
    };
    (0..=last).map(|frame| frame as f32 / rate).collect()
}

/// The DOF indices the player decodes for `animation`.
fn bound_dofs(skeleton: &Skeleton, animation: &CrAnimation) -> BTreeSet<usize> {
    let binding = ClipBinding::build(skeleton, animation);
    binding
        .bones
        .iter()
        .flat_map(|b| [b.translation, b.rotation, b.scale])
        .chain([binding.mover_translation, binding.mover_rotation])
        .flatten()
        .map(|dof| dof as usize)
        .collect()
}

fn frame_time(animation: &CrAnimation, frame: u16) -> f32 {
    if animation.num_frames < 2 {
        return 0.0;
    }
    f32::from(frame) * animation.duration / f32::from(animation.num_frames - 1)
}

/// Every value a clip's poses take frame by frame, and each DOF the player
/// does not bind, as the bytes the digest is taken over: floats as their
/// bits, so one ULP moves it.
fn pose_stream(dict: &CrClipDictionary, clip: &CrClipAny) -> Vec<u8> {
    let mut stream = Vec::new();
    let skeleton = skeleton_for(dict, clip);
    let mut player = ClipPlayer::bind(&skeleton, dict, clip);
    for time in clip_times(&player) {
        let pose = player.evaluate(&skeleton, time);
        for bone in pose.locals().iter().chain([&pose.root_motion]) {
            push(&mut stream, &bone.translation.to_array());
            push(&mut stream, &bone.rotation.to_array());
            push(&mut stream, &bone.scale.to_array());
        }
    }
    // ⚠ A DOF the player binds reaches the stream only through the pose, and
    // only over the part of its animation the clip plays.
    for played in clip::played(dict, clip) {
        let animation = played.animation;
        let bound = bound_dofs(&skeleton, animation);
        let mut decoder = DofDecoder::new(animation);
        for dof in 0..animation.dofs.len() {
            if bound.contains(&dof) {
                continue;
            }
            for frame in 0..animation.num_frames.max(1) {
                match decoder.decode(dof, frame_time(animation, frame)) {
                    Ok(Some(samples)) => {
                        stream.push(1);
                        push(&mut stream, samples.as_slice());
                    }
                    Ok(None) => stream.push(0),
                    Err(_) => stream.push(2),
                }
            }
        }
    }
    stream
}

fn corpus(what: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "ycd")
        .target(TARGET_GEN8)
        .find_or_skip(what)?;
    Some((root, files))
}

fn dictionary(path: &Path) -> Result<CrClipDictionary, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    CrClipDictionary::from_bytes(&bytes).map_err(|e| e.to_string())
}

/// Each clip's digest keyed by its dictionary key, the clips shared out over
/// every core: a corpus holds tens of thousands of clips.
fn digests(dict: &CrClipDictionary) -> Vec<(String, String)> {
    let clips: Vec<(u32, &CrClipAny)> = clip::clips(dict).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut out: Vec<(String, String)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some((key, clip)) = clips.get(at) else {
                            return mine;
                        };
                        let digest = baseline::digest(&pose_stream(dict, clip));
                        mine.push((format!("{key:#010x}"), digest));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a pose worker finished"))
            .collect()
    });
    out.sort();
    out
}

#[test]
fn every_corpus_clip_poses_as_recorded() {
    let what = "every_corpus_clip_poses_as_recorded";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let mut now: BTreeMap<Key, Now> = BTreeMap::new();
    for path in &files {
        match dictionary(path) {
            Ok(dict) => {
                for (clip, digest) in digests(&dict) {
                    now.insert(
                        baseline::key(&root, path, Some(&clip), TARGET_GEN8),
                        Now::Digest(digest),
                    );
                }
            }
            Err(why) => {
                now.insert(
                    baseline::key(&root, path, None, TARGET_GEN8),
                    Now::Refused(why),
                );
            }
        }
    }
    baseline::settle(env!("CARGO_MANIFEST_DIR"), KIND, &now);
}

#[test]
fn a_perturbed_pose_names_its_file() {
    let what = "a_perturbed_pose_names_its_file";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let path = &files[0];
    let dict = dictionary(path).expect("the first corpus clip dictionary reads");
    let (key, stream) = clip::clips(&dict)
        .map(|(key, clip)| (key, pose_stream(&dict, clip)))
        .find(|(_, s)| s.len() >= 4)
        .expect("it holds a clip with a pose");
    // Byte 0 of a float is its lowest mantissa byte: one ULP.
    baseline::a_perturbed_stream_is_named(
        baseline::key(&root, path, Some(&format!("{key:#010x}")), TARGET_GEN8),
        &stream,
        0,
    );
}

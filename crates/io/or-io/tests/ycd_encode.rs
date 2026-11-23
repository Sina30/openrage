//! The channel encoder on owned clip dictionaries: encoded blocks survive a
//! save and an open, and corpus animations re-encode to the samples they had.

use std::path::PathBuf;

use or_io::codec::anim::{
    decode_dof, encode_block, format_type, reencode_animation, track, AnimationSource,
    DofSampleTable, EncodePolicy, EncodedBlock, COMPACT,
};
use or_io::decl::{File, Keyed, Open, Owned, Raw, Record};
use or_io::gta5::*;

fn fixtures_dir() -> PathBuf {
    or_test_support::fixtures_root(env!("CARGO_MANIFEST_DIR"))
}

fn ycd_fixtures() -> Vec<PathBuf> {
    or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ycd").find()
}

fn dof(id: u16, format: u8, track: u8) -> Dof {
    Dof {
        id,
        r#type: format,
        track,
        ..Dof::default()
    }
}

fn vector_dof(tag: u16) -> Dof {
    dof(tag, format_type::VECTOR3, track::BONE_TRANSLATION)
}

fn quat_dof(tag: u16) -> Dof {
    dof(tag, format_type::QUATERNION, track::BONE_ROTATION)
}

fn one_entry<T: Owned>(key: u32, data: T) -> AtMap<T>
where
    Raw<T>: Record,
{
    AtMap {
        hash: vec![Some(Box::new(AtMapEntry {
            key,
            data: Some(Box::new(data)),
            ..Default::default()
        }))],
        ..Default::default()
    }
}

fn dictionary_with(animation: CrAnimation, name: &str) -> CrClipDictionary {
    let clip = CrClipAnimation {
        base: CrClip {
            r#type: Open::new(EClipType::Animation),
            name: AtString::new(name),
            ..Default::default()
        },
        animation: Keyed::to(0x1000),
        end_time: animation.duration,
        rate: 1.0,
        ..Default::default()
    };
    CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: one_entry(0x1000, animation),
            ..Default::default()
        })),
        clips: one_entry(0x2000, CrClipAny::CrClipAnimation(clip)),
        ..Default::default()
    }
}

/// Each clip's key and the first animation it plays.
fn primary_animations(dict: &CrClipDictionary) -> Vec<(u32, &CrAnimation)> {
    let Some(anims) = dict.anim_dictionary.as_deref() else {
        return Vec::new();
    };
    dict.clips
        .entries()
        .filter_map(|entry| {
            let key = match entry.data.as_deref()? {
                CrClipAny::CrClipAnimation(c) => c.animation.key?,
                CrClipAny::CrClipAnimations(c) => c.animations.first()?.animation.key?,
                CrClipAny::Base(_) => return None,
            };
            let animation = anims
                .animations
                .entries()
                .find(|e| e.key == key)?
                .data
                .as_deref()?;
            Some((entry.key, animation))
        })
        .collect()
}

fn reopened(dict: &CrClipDictionary) -> CrClipDictionary {
    CrClipDictionary::from_bytes(&dict.to_bytes().expect("save")).expect("open")
}

fn with_block(block: EncodedBlock, dofs: Vec<Dof>, frames: u16, duration: f32) -> CrAnimation {
    CrAnimation {
        num_frames: frames,
        frames_per_chunk: 127,
        duration,
        dofs,
        max_block_size: block.data.len() as u32,
        blocks: vec![Some(Box::new(block.into()))],
        ..CrAnimation::default()
    }
}

fn is_compact(animation: &CrAnimation) -> bool {
    animation.flags & COMPACT != 0
        || animation
            .blocks
            .iter()
            .flatten()
            .any(|b| b.compact_size != 0)
}

fn decode_all_frames(animation: &CrAnimation) -> Vec<Vec<[f32; 4]>> {
    (0..animation.dofs.len())
        .map(|dof| {
            (0..animation.num_frames)
                .map(|frame| {
                    let time = animation.internal_frame_to_time(frame as f32);
                    decode_dof(animation, dof, time)
                        .expect("decode")
                        .map(|s| s.components)
                        .unwrap_or([0.0; 4])
                })
                .collect()
        })
        .collect()
}

fn try_decode_all_frames(animation: &CrAnimation) -> Vec<Option<Vec<[f32; 4]>>> {
    (0..animation.dofs.len())
        .map(|dof| {
            let mut frames = Vec::with_capacity(usize::from(animation.num_frames));
            for frame in 0..animation.num_frames {
                let time = animation.internal_frame_to_time(frame as f32);
                match decode_dof(animation, dof, time) {
                    Ok(Some(samples)) => frames.push(samples.components),
                    Ok(None) => frames.push([0.0; 4]),
                    Err(_) => return None,
                }
            }
            Some(frames)
        })
        .collect()
}

#[allow(clippy::type_complexity)]
fn try_decode_frames_strided(
    animation: &CrAnimation,
    stride: usize,
) -> Vec<Option<Vec<(usize, [f32; 4])>>> {
    let stride = stride.max(1);
    let last = usize::from(animation.num_frames).saturating_sub(1);
    (0..animation.dofs.len())
        .map(|dof| {
            let mut frames = Vec::new();
            let mut frame = 0usize;
            loop {
                let time = animation.internal_frame_to_time(frame as f32);
                match decode_dof(animation, dof, time) {
                    Ok(Some(samples)) => frames.push((frame, samples.components)),
                    Ok(None) => frames.push((frame, [0.0; 4])),
                    Err(_) => return None,
                }
                if frame >= last {
                    break;
                }
                frame = (frame + stride).min(last);
            }
            Some(frames)
        })
        .collect()
}

const KNOWN_DECODE_GAPS: &str =
    "kLinear channels whose Rice stream this decoder cannot walk to the last frame, \
     and compact (crAnimCache) blocks";

/// Whether to run the corpus tests exhaustively.
///
/// Decoding every frame of every DOF walks the channel directory once per sample,
/// and a retail clip can be 3,000 frames across 40 DOFs. Unbounded, the harness
/// took seven minutes, so the default is a bounded sweep that still touches many
/// fixtures, and `OPENRAGE_YCD_CORPUS=1` opts into the full run.
fn exhaustive() -> bool {
    std::env::var("OPENRAGE_YCD_CORPUS").is_ok_and(|v| v != "0")
}

fn clips_per_fixture() -> usize {
    if exhaustive() {
        usize::MAX
    } else {
        2
    }
}

fn fixture_limit() -> usize {
    if exhaustive() {
        usize::MAX
    } else {
        8
    }
}

fn comparison_stride(num_frames: u16) -> usize {
    if exhaustive() || num_frames <= 256 {
        1
    } else {
        7
    }
}

fn samples_agree(a: f32, b: f32) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return a.is_finite() == b.is_finite();
    }
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() <= 2e-3 * scale
}

#[test]
fn encoded_channels_survive_a_file_round_trip() {
    let frames: Vec<[f32; 4]> = (0..24)
        .map(|i| [i as f32 * 0.25, -(i as f32) * 0.5, 3.0, 0.0])
        .collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
    let block = encode_block(&[table], 24, EncodePolicy::Lossless).expect("encode");

    let animation = with_block(block, vec![vector_dof(0x1234)], 24, 1.0);
    let reloaded = reopened(&dictionary_with(animation, "encoded"));

    let (key, animation) = primary_animations(&reloaded)[0];
    assert_eq!(key, 0x2000, "clip survived");
    assert_eq!(animation.num_frames, 24);

    let decoded = decode_all_frames(animation);
    for (index, (got, want)) in decoded[0].iter().zip(frames.iter()).enumerate() {
        for component in 0..3 {
            assert!(
                (got[component] - want[component]).abs() < 1e-5,
                "frame {index} component {component}: {} vs {}",
                got[component],
                want[component]
            );
        }
    }
}

#[test]
fn a_quaternion_track_survives_a_file_round_trip() {
    let frames: Vec<[f32; 4]> = (0..16)
        .map(|i| {
            let (s, c) = ((i as f32 * 0.05) * 0.5).sin_cos();
            [s, 0.0, 0.0, c]
        })
        .collect();
    let table = DofSampleTable::from_frames(format_type::QUATERNION, &frames);
    let block = encode_block(&[table], 16, EncodePolicy::Lossless).expect("encode");

    let animation = with_block(block, vec![quat_dof(0x5678)], 16, 1.0);
    let reloaded = reopened(&dictionary_with(animation, "rotate"));
    let (_, animation) = primary_animations(&reloaded)[0];

    let decoded = decode_all_frames(animation);
    for (index, (got, want)) in decoded[0].iter().zip(frames.iter()).enumerate() {
        assert!(
            (got[0] - want[0]).abs() < 1e-4,
            "frame {index}: x {} vs {}",
            got[0],
            want[0]
        );
        assert!(
            (got[3] - want[3]).abs() < 1e-3,
            "frame {index}: w {} vs {}",
            got[3],
            want[3]
        );
    }
}

#[test]
fn quantized_channels_stay_within_half_a_step() {
    const BITS: u32 = 12;
    let frames: Vec<[f32; 4]> = (0..64)
        .map(|i| [(i as f32 / 63.0) * 20.0 - 10.0, 0.0, 0.0, 0.0])
        .collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
    let block = encode_block(&[table], 64, EncodePolicy::Quantize { bits: BITS }).expect("encode");

    let animation = with_block(block, vec![vector_dof(0x1111)], 64, 2.0);
    let decoded = decode_all_frames(&animation);

    let step = 20.0 / ((1u32 << BITS) - 1) as f32;
    for (index, (got, want)) in decoded[0].iter().zip(frames.iter()).enumerate() {
        assert!(
            (got[0] - want[0]).abs() <= step,
            "frame {index}: {} vs {} exceeds step {step}",
            got[0],
            want[0]
        );
    }
}

#[test]
fn quantizing_shrinks_the_payload() {
    let frames: Vec<[f32; 4]> = (0..128)
        .map(|i| [i as f32, i as f32 * 2.0, i as f32 * 3.0, 0.0])
        .collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);

    let raw =
        encode_block(std::slice::from_ref(&table), 128, EncodePolicy::Lossless).expect("encode");
    let quantized =
        encode_block(&[table], 128, EncodePolicy::Quantize { bits: 12 }).expect("encode");

    assert!(
        quantized.data.len() < raw.data.len(),
        "quantized {} bytes vs raw {} bytes",
        quantized.data.len(),
        raw.data.len()
    );
    assert!(quantized.frame_size < raw.frame_size);
}

#[test]
fn a_multi_block_animation_reencodes_and_decodes_throughout() {
    let mut animation = CrAnimation {
        num_frames: 260,
        frames_per_chunk: 64,
        duration: 4.0,
        dofs: vec![vector_dof(0x100), quat_dof(0x200)],
        ..CrAnimation::default()
    };
    let block_count = animation.calc_num_blocks();
    assert!(
        block_count >= 4,
        "expected several blocks, got {block_count}"
    );

    reencode_animation(&mut animation, EncodePolicy::Lossless, |dof, frame| {
        if dof == 0 {
            [frame as f32 * 0.01, 1.0, 2.0, 0.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    })
    .expect("reencode");

    assert_eq!(animation.blocks.len(), block_count);

    let reloaded = reopened(&dictionary_with(animation, "long"));
    let (_, animation) = primary_animations(&reloaded)[0];

    for frame in 0..animation.num_frames {
        let time = animation.internal_frame_to_time(frame as f32);
        let samples = decode_dof(animation, 0, time)
            .expect("decode")
            .expect("samples");
        let expected = frame as f32 * 0.01;
        assert!(
            (samples.components[0] - expected).abs() < 0.02,
            "frame {frame}: {} vs {expected}",
            samples.components[0]
        );
    }
}

#[test]
fn saving_without_re_encoding_is_byte_identical() {
    let frames: Vec<[f32; 4]> = (0..32).map(|i| [i as f32, 1.0, 2.0, 0.0]).collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);
    let block = encode_block(&[table], 32, EncodePolicy::Lossless).expect("encode");

    let animation = with_block(block, vec![vector_dof(0x3333)], 32, 1.0);
    let first = dictionary_with(animation, "stable")
        .to_bytes()
        .expect("save");

    let reloaded = CrClipDictionary::from_bytes(&first).expect("open");
    let second = reloaded.to_bytes().expect("resave");

    assert_eq!(
        first, second,
        "an untouched file did not round-trip byte-identically"
    );

    let third = CrClipDictionary::from_bytes(&second)
        .expect("open")
        .to_bytes()
        .expect("save");
    assert_eq!(second, third);
}

#[test]
fn re_encoding_changes_the_bytes() {
    let mut animation = CrAnimation {
        num_frames: 16,
        frames_per_chunk: 127,
        duration: 1.0,
        dofs: vec![vector_dof(0x4444)],
        ..CrAnimation::default()
    };
    reencode_animation(&mut animation, EncodePolicy::Lossless, |_, frame| {
        [frame as f32, 0.0, 0.0, 0.0]
    })
    .expect("reencode");
    let raw_bytes = dictionary_with(animation.clone(), "changed")
        .to_bytes()
        .expect("save");

    reencode_animation(
        &mut animation,
        EncodePolicy::Quantize { bits: 10 },
        |_, frame| [frame as f32, 0.0, 0.0, 0.0],
    )
    .expect("reencode");
    let quantized_bytes = dictionary_with(animation, "changed")
        .to_bytes()
        .expect("save");

    assert_ne!(raw_bytes, quantized_bytes);
}

#[test]
fn fixture_animations_reencode_to_the_same_samples() {
    let fixtures = ycd_fixtures();
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "fixture_animations_reencode_to_the_same_samples",
            format_args!("no .ycd fixtures in {:?}", fixtures_dir()),
        );
        eprintln!("         Place .ycd files there to verify the encoder on real data.");
        return;
    }

    let mut checked = 0usize;
    let mut compared_dofs = 0usize;
    let mut skipped_compact = 0usize;
    let mut skipped_undecodable = 0usize;
    let mut worst: f32 = 0.0;

    for path in fixtures.iter().take(fixture_limit()) {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let Ok(dict) = CrClipDictionary::from_bytes(&data) else {
            continue;
        };

        for (key, animation) in primary_animations(&dict)
            .into_iter()
            .take(clips_per_fixture())
        {
            if is_compact(animation) {
                skipped_compact += 1;
                continue;
            }
            if animation.num_frames < 2 || animation.dofs.is_empty() {
                continue;
            }

            let before = try_decode_all_frames(animation);
            skipped_undecodable += before.iter().filter(|d| d.is_none()).count();

            let mut rebuilt = animation.clone();
            let samples = before.clone();
            reencode_animation(&mut rebuilt, EncodePolicy::Lossless, |dof, frame| {
                samples
                    .get(dof)
                    .and_then(|frames| frames.as_ref())
                    .and_then(|frames| frames.get(frame))
                    .copied()
                    .unwrap_or([0.0; 4])
            })
            .expect("reencode");

            let stride = comparison_stride(animation.num_frames);
            let after = try_decode_frames_strided(&rebuilt, stride);
            assert_eq!(
                before.len(),
                after.len(),
                "{}: clip {key:#010X} lost a DOF",
                path.display()
            );

            for (dof, (before_frames, after_frames)) in before.iter().zip(after.iter()).enumerate()
            {
                let Some(before_frames) = before_frames else {
                    continue;
                };
                let after_frames = after_frames.as_ref().unwrap_or_else(|| {
                    panic!(
                        "{}: clip {key:#010X} DOF {dof} was re-encoded into a block that \
                         does not decode",
                        path.display()
                    )
                });

                let components = format_type::component_count(animation.dofs[dof].r#type).min(4);
                for (frame, b) in after_frames {
                    let a = &before_frames[*frame];
                    for component in 0..components {
                        worst = worst.max((a[component] - b[component]).abs());
                        assert!(
                            samples_agree(a[component], b[component]),
                            "{}: clip {key:#010X} DOF {dof} frame {frame} component {component} \
                             drifted ({} -> {})",
                            path.display(),
                            a[component],
                            b[component]
                        );
                    }
                }
                compared_dofs += 1;
            }
            checked += 1;
        }
    }

    eprintln!(
        "re-encoded {checked} fixture animations, compared {compared_dofs} DOFs; \
         skipped {skipped_compact} compact animations and {skipped_undecodable} \
         undecodable DOFs ({KNOWN_DECODE_GAPS}); worst drift {worst}{}",
        if exhaustive() {
            " [exhaustive]"
        } else {
            " [bounded — set OPENRAGE_YCD_CORPUS=1 for the full sweep]"
        }
    );
    assert!(
        compared_dofs > 0,
        "no DOF could be compared — the corpus produced nothing to verify against"
    );
}

#[test]
fn reencoded_fixtures_still_write_and_read() {
    let fixtures = ycd_fixtures();
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "reencoded_fixtures_still_write_and_read",
            format_args!("no .ycd fixtures in {:?}", fixtures_dir()),
        );
        return;
    }

    let mut verified = 0usize;
    for path in fixtures.iter().take(fixture_limit()) {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let Ok(dict) = CrClipDictionary::from_bytes(&data) else {
            continue;
        };

        for (key, animation) in primary_animations(&dict)
            .into_iter()
            .take(clips_per_fixture())
        {
            if is_compact(animation) || animation.num_frames < 2 || animation.dofs.is_empty() {
                continue;
            }

            let samples = try_decode_all_frames(animation);
            let mut rebuilt = animation.clone();
            reencode_animation(&mut rebuilt, EncodePolicy::Lossless, |dof, frame| {
                samples
                    .get(dof)
                    .and_then(|frames| frames.as_ref())
                    .and_then(|frames| frames.get(frame))
                    .copied()
                    .unwrap_or([0.0; 4])
            })
            .expect("reencode");

            let name = format!("clip_{key:08X}");
            let reloaded = reopened(&dictionary_with(rebuilt, &name));
            let clips = primary_animations(&reloaded);
            assert_eq!(clips.len(), 1, "{}: clip lost on reload", path.display());
            let (_, reloaded_animation) = clips[0];

            let stride = comparison_stride(animation.num_frames);
            let after = try_decode_frames_strided(reloaded_animation, stride);
            for (dof, (before_frames, after_frames)) in samples.iter().zip(after.iter()).enumerate()
            {
                let (Some(before_frames), Some(after_frames)) = (before_frames, after_frames)
                else {
                    continue;
                };
                let components = format_type::component_count(animation.dofs[dof].r#type).min(4);
                for (frame, b) in after_frames {
                    let a = &before_frames[*frame];
                    for component in 0..components {
                        assert!(
                            samples_agree(a[component], b[component]),
                            "{}: {name} DOF {dof} frame {frame} component {component} drifted \
                             through the file ({} -> {})",
                            path.display(),
                            a[component],
                            b[component]
                        );
                    }
                }
            }
            verified += 1;
            break;
        }
    }
    eprintln!("re-encoded and re-read {verified} fixture animations through a real file");
}

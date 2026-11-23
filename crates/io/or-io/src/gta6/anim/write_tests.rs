use std::sync::Arc;

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

/// Steps spanning `0 ..= 2^Q - 1` exactly, as every channel of the game does.
fn spanning(frames: usize) -> Vec<i64> {
    let mut q = quantised(frames, 3);
    let lo = *q.iter().min().expect("a frame");
    q.iter_mut().for_each(|x| *x -= lo);
    let bits = 64 - (*q.iter().max().expect("a frame") as u64).leading_zeros();
    q[0] = 0;
    q[frames - 1] = (1 << bits) - 1;
    q
}

/// Steps rising evenly from 0 to 65,535, whose second differences are at most one.
fn ramp(frames: usize) -> Vec<i64> {
    (0..frames as i64)
        .map(|f| f * 65_535 / (frames as i64 - 1))
        .collect()
}

fn spec_of(q: &[i64]) -> Spec {
    let mut constants = floats(&[1.0, 2.0, 3.0, 0.5]);
    constants.extend(delta_channel(q));
    Spec {
        frames: q.len() as u16,
        constants,
        ..Spec::default()
    }
}

fn opened(bytes: Vec<u8>, form: Rpf9Form) -> AnimResource {
    let mut resource = read(&bytes).expect("reads");
    resource.sections = Some(Arc::new(Rpf9Sections {
        form,
        virtual_section: bytes,
        physical: Vec::new(),
    }));
    resource
}

fn key(frame: u16, value: f32) -> AnimEdit {
    AnimEdit::Key {
        animation: AnimAt {
            set: 0,
            animation: 0,
        },
        dof: 2,
        component: 0,
        frame,
        value,
    }
}

fn section(resource: &AnimResource) -> &[u8] {
    &resource
        .sections
        .as_ref()
        .expect("sections")
        .virtual_section
}

fn step_value(q: i64) -> f32 {
    section10::value(SCALE, OFFSET, q)
}

#[test]
fn the_encoder_gives_back_the_words_of_every_channel_from_its_steps() {
    for frames in [1, 2, 9, 63, 64, 65, 70, 128, 129, 200] {
        for phase in [0, 11] {
            let q = quantised(frames, phase);
            let words = delta_channel(&q);
            assert_eq!(section10::quantised(&words, frames).expect("decodes"), q);
            assert_eq!(
                section10::encode(&q, SCALE.to_bits(), OFFSET.to_bits()).expect("encodes"),
                words,
                "{frames} frames, phase {phase}"
            );
        }
    }
    assert!(section10::encode(&[-1, 0], 0, 0).is_err());
}

#[test]
fn an_unchanged_resource_written_through_the_encoder_is_the_section_it_was_read_from() {
    for bytes in [
        build(&Spec::default()).bytes,
        build(&spec_of(&spanning(130))).bytes,
    ] {
        assert!(reencode(&bytes, false).expect("writes") == bytes);
    }
    // A zero word before the lanes still decodes, and is no tightest encoding.
    let q = spanning(70);
    let mut loose = delta_channel(&q);
    let lanes = loose.len() - 8 * (loose[1] >> 16) as usize;
    loose.insert(lanes, 0);
    loose[0] += 1;
    let mut spec = spec_of(&q);
    spec.constants.truncate(4);
    spec.constants.extend(loose);
    let bytes = build(&spec).bytes;
    let tight = reencode(&bytes, false).expect("writes");
    assert!(tight != bytes);
    assert_eq!(
        read(&tight).expect("reads").sets[0].animations[0].delta_value(2, 0, 40),
        Some(step_value(q[40]))
    );
    assert!(reencode(&tight, false).expect("writes") == tight);
}

#[test]
fn a_key_on_its_channel_s_range_keeps_its_scale_and_every_other_step() {
    let q = spanning(70);
    let resource = opened(build(&spec_of(&q)).bytes, Rpf9Form::BlockMapped);
    let edited = edit(&resource, &key(30, step_value(q[30] + 2))).expect("edits");
    assert!(edited.edited && !resource.edited);
    let animation = &edited.sets[0].animations[0];
    for (frame, step) in q.iter().enumerate() {
        let step = if frame == 30 { step + 2 } else { *step };
        assert_eq!(
            animation.delta_value(2, 0, frame as u16),
            Some(step_value(step)),
            "frame {frame}"
        );
    }
    let read_again = read(section(&edited)).expect("reads");
    assert_eq!(
        read_again.sets[0].animations[0].delta_value(2, 0, 30),
        Some(step_value(q[30] + 2))
    );
    assert!(reencode(section(&edited), false).expect("writes") == section(&edited));
}

#[test]
fn a_key_past_its_range_quantises_its_channel_afresh_and_a_block_that_grows_moves_past_the_end() {
    let q = spanning(70);
    let built = build(&spec_of(&q));
    let resource = opened(built.bytes.clone(), Rpf9Form::BlockMapped);
    let top = *q.iter().max().expect("a frame");
    let high = step_value(top) + 40.0;
    let edited = edit(&resource, &key(12, high)).expect("edits");
    let bytes = section(&edited);
    let animation = &read(bytes).expect("reads").sets[0].animations[0];
    let bits = 64 - (top as u64).leading_zeros();
    let step = (high - step_value(0)) / ((1i64 << bits) - 1) as f32;
    for (frame, was) in q.iter().enumerate() {
        let wanted = if frame == 12 { high } else { step_value(*was) };
        let got = animation.delta_value(2, 0, frame as u16).expect("a value");
        assert!(
            (got - wanted).abs() <= step / 2.0 + 1e-4,
            "frame {frame}: {got} against {wanted}"
        );
    }
    assert!(reencode(bytes, false).expect("writes") == bytes);

    // A jump to the range's foot widens the groups it falls in, so the block grows.
    let built = build(&spec_of(&ramp(70)));
    let resource = opened(built.bytes.clone(), Rpf9Form::BlockMapped);
    let jump = key(31, step_value(0));
    let grown = edit(&resource, &jump).expect("edits");
    let bytes = section(&grown);
    let block = pointer_at(bytes, pointer_at(bytes, built.animation + 0x50));
    assert!(bytes.len() > built.bytes.len() && block >= built.bytes.len());
    let data_size = u32::from_le_bytes(bytes[block + 4..block + 8].try_into().expect("four"));
    let animation = &grown.sets[0].animations[0];
    assert_eq!(animation.max_block_size, data_size + 0x20);
    assert_eq!(animation.delta_value(2, 0, 31), Some(step_value(0)));
    assert!(reencode(bytes, false).expect("writes") == bytes);

    // A stored section that ends with no last region takes the block past its end too.
    let stored = opened(built.bytes, Rpf9Form::StoredRsc8 { config: 0 });
    let grown = edit(&stored, &jump).expect("edits");
    assert_eq!(
        grown.sets[0].animations[0].delta_value(2, 0, 31),
        Some(step_value(0))
    );
}

#[test]
fn a_mover_entry_of_section_10_follows_its_channel_when_one_before_it_grows() {
    let (first, second) = (ramp(70), quantised(70, 9));
    let mut spec = spec_of(&first);
    spec.dofs.push((4, "moverAbsoluteTranslation"));
    spec.sections[2].1.push(3 << 2);
    let mover_word = spec.constants.len() as u16;
    spec.constants.extend(delta_channel(&second));
    let mut built = build(&spec);
    let data_size = u32::from_le_bytes(
        built.bytes[built.block + 4..][..4]
            .try_into()
            .expect("four"),
    );
    let mut block = built.bytes[built.block..built.block + 0x20 + data_size as usize].to_vec();
    block.extend([10, 0]);
    block.extend(mover_word.to_le_bytes());
    block.extend([0, 0]);
    block[4..8].copy_from_slice(&(data_size + 6).to_le_bytes());
    block[0x1F] = 1 << 4;
    let at = built.bytes.len().next_multiple_of(16);
    built.bytes.resize(at, 0);
    built.bytes.extend(&block);
    let slot = pointer_at(&built.bytes, built.animation + 0x50);
    built.bytes[slot..slot + 8].copy_from_slice(&(VIRTUAL + at as u64).to_le_bytes());
    built.bytes[built.animation + 0x48..][..4]
        .copy_from_slice(&(data_size + 6 + 0x20).to_le_bytes());
    assert!(reencode(&built.bytes, false).expect("writes") == built.bytes);

    let resource = opened(built.bytes, Rpf9Form::BlockMapped);
    let edited = edit(&resource, &key(31, step_value(0))).expect("edits");
    let bytes = section(&edited);
    let block = pointer_at(bytes, pointer_at(bytes, built.animation + 0x50));
    let constant_size = u32::from_le_bytes(bytes[block + 0xC..][..4].try_into().expect("four"));
    let stride = |at: usize| {
        u32::from_le_bytes(
            bytes[block + 0x20 + 4 * at..][..4]
                .try_into()
                .expect("four"),
        )
    };
    let moved = 4 + stride(4) as usize;
    assert_ne!(moved, usize::from(mover_word));
    assert_eq!(moved + stride(moved) as usize, constant_size as usize / 4);
    let data_size = u32::from_le_bytes(bytes[block + 4..][..4].try_into().expect("four")) as usize;
    let entry = &bytes[block + 0x20 + data_size - 6..block + 0x20 + data_size];
    assert_eq!(entry, [10, 0, moved as u8, (moved >> 8) as u8, 0, 0]);
    let animation = &edited.sets[0].animations[0];
    for (frame, step) in second.iter().enumerate() {
        assert_eq!(
            animation.delta_value(3, 0, frame as u16),
            Some(step_value(*step))
        );
    }
    assert!(reencode(bytes, false).expect("writes") == bytes);
}

#[test]
fn a_clip_of_one_animation_takes_a_window_and_a_clip_of_groups_refuses_one() {
    let resource = opened(build(&Spec::default()).bytes, Rpf9Form::BlockMapped);
    let window = AnimEdit::ClipWindow {
        clip: 0,
        start: 0.125,
        end: 0.75,
        rate: 1.5,
    };
    let edited = edit(&resource, &window).expect("edits");
    for clips in [&edited.clips, &read(section(&edited)).expect("reads").clips] {
        assert!(matches!(
            clips[0].play,
            ClipPlay::One {
                start: 0.125,
                end: 0.75,
                rate: 1.5,
                ..
            }
        ));
    }
    let backwards = AnimEdit::ClipWindow {
        clip: 0,
        start: 0.5,
        end: 0.25,
        rate: 1.0,
    };
    assert!(edit(&resource, &backwards).is_err());
    let groups = Spec {
        clip: 2,
        ..Spec::default()
    };
    let groups = opened(build(&groups).bytes, Rpf9Form::BlockMapped);
    assert!(edit(&groups, &window).is_err_and(|e| matches!(e, AnimError::Unwritten(_))));
}

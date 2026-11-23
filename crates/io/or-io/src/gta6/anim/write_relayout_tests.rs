use std::mem::offset_of;
use std::sync::Arc;

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

const AT: AnimAt = AnimAt {
    set: 0,
    animation: 0,
};

/// `bytes` with a block map of one virtual block at their end, which root 0x08 points at, as
/// every block-mapped body has.
fn mapped(mut bytes: Vec<u8>) -> Vec<u8> {
    let map = bytes.len().next_multiple_of(16);
    bytes.resize(map + 24, 0);
    bytes[map + 8] = 1;
    bytes[0x08..0x10].copy_from_slice(&(VIRTUAL + map as u64).to_le_bytes());
    bytes
}

fn opened(bytes: Vec<u8>, form: Rpf9Form) -> AnimResource {
    let mut resource = match form {
        Rpf9Form::StoredRsc8 { .. } => read_stored(&bytes),
        Rpf9Form::BlockMapped => read(&bytes),
    }
    .expect("reads");
    resource.sections = Some(Arc::new(Rpf9Sections {
        form,
        virtual_section: bytes,
        physical: Vec::new(),
    }));
    resource
}

fn section(resource: &AnimResource) -> &[u8] {
    &resource
        .sections
        .as_ref()
        .expect("sections")
        .virtual_section
}

fn again(resource: &AnimResource) -> AnimResource {
    let form = resource.sections.as_ref().expect("sections").form;
    opened(section(resource).to_vec(), form)
}

fn values(resource: &AnimResource, dof: usize, component: u8) -> Vec<Option<f32>> {
    let animation = the_animation(resource);
    (0..animation.frames)
        .map(|f| animation.key_value(dof, component, f))
        .collect()
}

fn pose(dof: usize, value: [f32; 4]) -> AnimEdit {
    AnimEdit::Pose {
        animation: AT,
        dof,
        value,
    }
}

fn add(dof: Dof, value: [f32; 4]) -> AnimEdit {
    AnimEdit::AddDof {
        animation: AT,
        dof,
        value,
    }
}

fn every(value: f32) -> Vec<Option<f32>> {
    vec![Some(value); usize::from(FRAMES)]
}

#[test]
fn a_dof_posed_at_the_value_it_holds_lays_its_block_out_byte_for_byte() {
    let bytes = mapped(build(&Spec::default()).bytes);
    let resource = opened(bytes.clone(), Rpf9Form::BlockMapped);
    for edited in [pose(0, [0.5, 0.0, 0.0, 0.0]), pose(1, [1.0, 2.0, 3.0, 0.0])] {
        let same = edit(&resource, &edited).expect("edits");
        assert_eq!(section(&same), &bytes[..], "{edited:?}");
        assert!(same.edited);
    }
}

#[test]
fn a_varying_dof_posed_over_the_clip_takes_one_constant_and_its_block_shrinks_in_place() {
    let bytes = mapped(build(&Spec::default()).bytes);
    let resource = opened(bytes.clone(), Rpf9Form::BlockMapped);
    assert!(the_animation(&resource).in_delta_section(2));
    let posed = edit(&resource, &pose(2, [0.75, 0.0, 0.0, 0.0])).expect("edits");
    assert_eq!(section(&posed).len(), bytes.len());
    for r in [&posed, &again(&posed)] {
        let animation = the_animation(r);
        assert!(!animation.in_delta_section(2));
        assert_eq!(animation.blocks[0].sections[2], 2);
        assert_eq!(animation.blocks[0].sections[10], 0);
        assert_eq!(values(r, 2, 0), every(0.75));
        assert_eq!(values(r, 0, 0), every(0.5));
        assert_eq!(values(r, 1, 2), every(3.0));
        assert_eq!(animation.signature, the_animation(&resource).signature);
    }
}

/// A resource whose DOF 0 is raw in section 3, DOF 1 quantised in 5 bits in section 4 and DOF 2
/// in section 10, each frame's raw value `f / 2` and step `f % 32`, with a block map.
fn framed() -> (Vec<u8>, usize, f32, f32) {
    let (scale, offset) = (0.5f32, -1.0f32);
    let mut constants = vec![5, scale.to_bits(), offset.to_bits()];
    constants.extend(delta_channel(&quantised(usize::from(FRAMES), 0)));
    let spec = Spec {
        dofs: vec![
            (4, "genericControl"),
            (4, "genericControl"),
            (4, "facialTranslation"),
        ],
        sections: vec![(3, vec![0]), (4, vec![1 << 2]), (10, vec![2 << 2])],
        constants,
        ..Spec::default()
    };
    let mut built = build(&spec);
    let (data, constant_size) = payload(&spec);
    let mut block = data[..constant_size].to_vec();
    for f in 0..u32::from(FRAMES) {
        block.extend((f as f32 / 2.0).to_bits().to_le_bytes());
        block.extend((f % 32).to_le_bytes());
    }
    block.extend(&data[constant_size..]);
    let mut head = built.bytes[built.block..built.block + 0x20].to_vec();
    for (field, bytes) in [
        (
            offset_of!(Raw<CrBlockStream>, data_size),
            (block.len() as u32).to_le_bytes().to_vec(),
        ),
        (
            offset_of!(Raw<CrBlockStream>, channel_offset),
            (block.len() as u32 + 0x20).to_le_bytes().to_vec(),
        ),
        (
            offset_of!(Raw<CrBlockStream>, frame_size),
            8u16.to_le_bytes().to_vec(),
        ),
        (
            offset_of!(Raw<CrBlockStream>, quantize_size),
            5u16.to_le_bytes().to_vec(),
        ),
    ] {
        head[field..field + bytes.len()].copy_from_slice(&bytes);
    }
    let at = built.bytes.len().next_multiple_of(16);
    built.bytes.resize(at, 0);
    built.bytes.extend(head);
    built.bytes.extend(&block);
    let slot = pointer_at(&built.bytes, built.animation + 0x50);
    built.bytes[slot..slot + 8].copy_from_slice(&(VIRTUAL + at as u64).to_le_bytes());
    built.bytes[built.animation + 0x48..][..4]
        .copy_from_slice(&(block.len() as u32 + 0x20).to_le_bytes());
    (mapped(built.bytes), built.animation, scale, offset)
}

#[test]
fn a_raw_dof_posed_over_the_clip_moves_the_quantised_bits_after_it_to_the_frames_start() {
    let (bytes, animation, scale, offset) = framed();
    let resource = opened(bytes, Rpf9Form::BlockMapped);
    let steps: Vec<Option<f32>> = (0..FRAMES)
        .map(|f| Some(scale * f32::from(f % 32) + offset))
        .collect();
    assert_eq!(values(&resource, 1, 0), steps);
    let deltas = values(&resource, 2, 0);

    let posed = edit(&resource, &pose(0, [2.5, 0.0, 0.0, 0.0])).expect("edits");
    for r in [&posed, &again(&posed)] {
        assert_eq!(values(r, 0, 0), every(2.5));
        assert_eq!(values(r, 1, 0), steps);
        assert_eq!(values(r, 2, 0), deltas);
    }
    let out = section(&posed);
    let block = pointer_at(out, pointer_at(out, animation + 0x50));
    let half = |at: usize| u16::from_le_bytes([out[at], out[at + 1]]);
    assert_eq!(half(block + offset_of!(Raw<CrBlockStream>, frame_size)), 4);
    assert_eq!(
        half(block + offset_of!(Raw<CrBlockStream>, quantize_size)),
        5
    );
    assert_eq!(the_animation(&posed).blocks[0].sections[3], 0);
    assert_eq!(the_animation(&posed).blocks[0].sections[2], 1);
    assert!(reencode(out, false).expect("writes") == out);
}

#[test]
fn a_removed_dof_takes_its_channels_with_it_and_the_list_is_signed_again() {
    let built = build(&Spec::default());
    let bytes = mapped(built.bytes);
    let resource = opened(bytes.clone(), Rpf9Form::BlockMapped);
    let read_dofs = the_animation(&resource).dofs.clone();
    let deltas = values(&resource, 2, 0);
    let remove = AnimEdit::RemoveDof {
        animation: AT,
        dof: 0,
    };
    let removed = edit(&resource, &remove).expect("edits");
    for r in [&removed, &again(&removed)] {
        let animation = the_animation(r);
        assert_eq!(animation.dofs, read_dofs[1..]);
        assert_eq!(animation.signature, dof_signature(&read_dofs[1..]));
        assert!(animation.signature_holds());
        assert_eq!(values(r, 0, 2), every(3.0));
        assert_eq!(values(r, 1, 0), deltas);
        assert_eq!(r.block_map, Some([1, 0]));
    }
    let out = section(&removed);
    assert!(pointer_at(out, built.animation + 0x60) >= bytes.len());
    let refused = edit(
        &resource,
        &AnimEdit::RemoveDof {
            animation: AT,
            dof: 3,
        },
    );
    assert!(matches!(refused, Err(AnimError::NoDof { dof: 3, .. })));
}

#[test]
fn an_added_dof_takes_its_sorted_place_and_a_constant_channel_in_every_block() {
    let built = build(&Spec::default());
    let resource = opened(mapped(built.bytes), Rpf9Form::BlockMapped);
    let read_dofs = the_animation(&resource).dofs.clone();
    let deltas = values(&resource, 2, 0);
    let new = Dof {
        id: hash("SKEL_Head"),
        track: hash("boneRotation"),
        kind: 0,
    };
    let added = edit(&resource, &add(new, [0.0, 0.0, -0.6, -0.8])).expect("edits");
    for r in [&added, &again(&added)] {
        let animation = the_animation(r);
        let at = (animation.dofs.iter().position(|d| *d == new)).expect("the DOF");
        let kept: Vec<Dof> = (animation.dofs.iter().copied())
            .filter(|d| *d != new)
            .collect();
        assert_eq!(kept, read_dofs);
        assert!(animation.signature_holds());
        assert_eq!(animation.flags & 0x10, 0);
        // A rotation is stored with its w made positive.
        let q = animation.sample(at, 0.5).expect("samples").components;
        for (got, want) in q.iter().zip([0.0, 0.0, 0.6, 0.8]) {
            assert!((got - want).abs() < 1e-5, "{q:?}");
        }
        let place =
            |i: usize| (animation.dofs.iter().position(|d| *d == read_dofs[i])).expect("kept");
        assert_eq!(values(r, place(0), 0), every(0.5));
        assert_eq!(values(r, place(1), 1), every(2.0));
        assert_eq!(values(r, place(2), 0), deltas);
    }

    // A mover track sets the animation's flag and the block names its channel in a mover entry.
    let mover = Dof {
        id: 0,
        track: hash("moverAbsoluteTranslation"),
        kind: 2,
    };
    let moved = edit(&resource, &add(mover, [1.0, 2.0, 3.0, 0.0])).expect("edits");
    let animation = the_animation(&moved);
    assert_eq!(animation.flags & 0x10, 0x10);
    assert!(animation.signature_holds());
    let at = (animation.dofs.iter().position(|d| *d == mover)).expect("the mover");
    let vector = (animation.dofs.iter().position(|d| *d == read_dofs[1])).expect("kept");
    let out = section(&moved);
    let block = pointer_at(out, pointer_at(out, built.animation + 0x50));
    assert_eq!(out[block + 0x1F], 1 << 4);
    let size = u32::from_le_bytes(out[block + 4..block + 8].try_into().expect("four")) as usize;
    let entry = &out[block + 0x20 + size - 6..block + 0x20 + size];
    let word: u16 = if vector < at { 3 } else { 0 };
    let want: Vec<u8> = [1u16, word, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(entry, &want[..]);
}

#[test]
fn a_dof_edit_the_rules_do_not_cover_is_refused_by_name() {
    let resource = opened(mapped(build(&Spec::default()).bytes), Rpf9Form::BlockMapped);
    let held = the_animation(&resource).dofs[1];
    let other = Dof { kind: 1, ..held };
    let other = Dof {
        id: held.id ^ 1,
        ..other
    };
    for (edited, want) in [
        (add(held, [0.0; 4]), "already has a DOF"),
        (add(other, [0.0; 4]), "type 1"),
        (pose(0, [f32::NAN, 0.0, 0.0, 0.0]), "not finite"),
    ] {
        let refused = edit(&resource, &edited).expect_err("refused").to_string();
        assert!(refused.contains(want), "{want}: {refused}");
    }
    // A block-mapped section with no block map at 0x08 has nothing the list's growth can count.
    let unmapped = opened(build(&Spec::default()).bytes, Rpf9Form::BlockMapped);
    let new = Dof {
        id: 7,
        track: hash("boneScale"),
        kind: 2,
    };
    let refused = edit(&unmapped, &add(new, [1.0; 4]))
        .expect_err("refused")
        .to_string();
    assert!(refused.contains("not placed"), "{refused}");
}

#[test]
fn a_stored_resource_takes_its_new_list_before_its_last_region_which_reads_again() {
    let built = build(&Spec::default());
    let mut bytes = built.bytes;
    let start = bytes.len().next_multiple_of(16);
    bytes.resize(start + 0x50, 0);
    bytes[start + 8] = 1;
    bytes[start + 0x18..start + 0x1E].copy_from_slice(DICTIONARY.as_bytes());
    bytes[start + 0x20..start + 0x24].copy_from_slice(&0x77u32.to_le_bytes());
    let list = pointer_at(&bytes, 0x70);
    let entry: Vec<u8> = bytes[list..list + 16].to_vec();
    bytes[start + 0x30..start + 0x40].copy_from_slice(&entry);
    bytes[0x70..0x78].copy_from_slice(&(VIRTUAL + start as u64 + 0x30).to_le_bytes());
    bytes[start + 0x40..start + 0x48].copy_from_slice(&(VIRTUAL + built.set as u64).to_le_bytes());
    bytes[0x08..0x10].copy_from_slice(&(VIRTUAL + start as u64).to_le_bytes());
    let resource = opened(bytes, Rpf9Form::StoredRsc8 { config: 0 });
    let region = resource.end_region.clone().expect("a last region");
    let first = the_animation(&resource).dofs[0];
    let new = Dof {
        id: 7,
        track: hash("boneScale"),
        kind: 2,
    };
    let added = edit(&resource, &add(new, [1.0, 1.0, 1.0, 0.0])).expect("edits");
    let out = section(&added);
    let moved_to = pointer_at(out, 0x08);
    assert!(moved_to > start);
    assert!(pointer_at(out, built.animation + 0x60) < moved_to);
    for r in [&added, &again(&added)] {
        assert_eq!(r.end_region, Some(region.clone()));
        let animation = the_animation(r);
        assert!(animation.dofs.contains(&new));
        assert!(animation.signature_holds());
        let at = (animation.dofs.iter().position(|d| *d == first)).expect("kept");
        assert_eq!(values(r, at, 0), every(0.5));
    }
}

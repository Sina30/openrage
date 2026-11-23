use std::mem::offset_of;
use std::sync::Arc;

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

const AT: AnimAt = AnimAt {
    set: 0,
    animation: 0,
};

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

/// `bytes` with a block map of one virtual block at their end, which root 0x08 points at.
fn mapped(mut bytes: Vec<u8>) -> Vec<u8> {
    let map = bytes.len().next_multiple_of(16);
    bytes.resize(map + 24, 0);
    bytes[map + 8] = 1;
    bytes[0x08..0x10].copy_from_slice(&(VIRTUAL + map as u64).to_le_bytes());
    bytes
}

fn key(dof: usize, frame: u16, value: f32) -> AnimEdit {
    AnimEdit::Key {
        animation: AT,
        dof,
        component: 0,
        frame,
        value,
    }
}

fn values(resource: &AnimResource, dof: usize, component: u8) -> Vec<Option<f32>> {
    let animation = the_animation(resource);
    (0..animation.frames)
        .map(|f| animation.key_value(dof, component, f))
        .collect()
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

/// The step frame `f` of the section-5 channel takes: three steps, first used in this order.
fn step(f: u16) -> i64 {
    [0, 3, 1][usize::from(f % 3)]
}

/// The packed table of `entries`, each `bits` wide.
fn table(entries: &[u32], bits: usize) -> u32 {
    (entries.iter().enumerate()).fold(0, |t, (i, e)| t | e << (i * bits))
}

/// A resource whose DOF 0 is a section-5 channel taking [`step`] at each frame through a table,
/// beside the default's section-1 vector and section-10 channel; and where its animation is.
fn indirect() -> (Vec<u8>, usize) {
    let mut constants = floats(&[1.0, 2.0, 3.0]);
    constants.extend([
        2,
        2,
        1,
        SCALE.to_bits(),
        OFFSET.to_bits(),
        table(&[0, 3, 1], 2),
    ]);
    constants.extend(delta_channel(&quantised(usize::from(FRAMES), 0)));
    let mut spec = Spec {
        constants,
        ..Spec::default()
    };
    spec.sections[1] = (5, vec![0]);
    let mut built = build(&spec);
    let (data, constant_size) = payload(&spec);
    let mut block = data[..constant_size].to_vec();
    // Frame f indexes entry f % 3, each frame one word.
    for f in 0..FRAMES {
        block.extend(u32::from(f % 3).to_le_bytes());
    }
    block.extend(&data[constant_size..]);
    let mut head = built.bytes[built.block..built.block + 0x20].to_vec();
    let fields = [
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
            4u16.to_le_bytes().to_vec(),
        ),
    ];
    for (field, bytes) in fields {
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
    (built.bytes, built.animation)
}

/// The five words and the table of the section-5 channel of the block of the animation at
/// `animation`, after the section-1 vector's three words.
fn indirect_words(resource: &AnimResource, animation: usize) -> Vec<u32> {
    let bytes = section(resource);
    let block = pointer_at(bytes, pointer_at(bytes, animation + 0x50));
    let words = |i: usize| u32_at(bytes, block + 0x20 + 4 * (3 + i));
    (0..5 + words(2) as usize).map(words).collect()
}

#[test]
fn a_key_of_a_section_5_channel_derives_its_table_again_from_its_frames() {
    let (bytes, animation) = indirect();
    let resource = opened(mapped(bytes), Rpf9Form::BlockMapped);
    let value = |q: i64| Some(SCALE * q as f32 + OFFSET);
    let read: Vec<_> = (0..FRAMES).map(|f| value(step(f))).collect();
    assert_eq!(values(&resource, 0, 0), read);
    assert!(the_animation(&resource).key_components().contains(&(0, 0)));
    let deltas = values(&resource, 2, 0);

    // A fourth step: four entries take 3 index bits, the largest step 5 takes 3 value bits.
    let edited = edit(&resource, &key(0, 4, SCALE * 5.0 + OFFSET)).expect("edits");
    let mut want = read.clone();
    want[4] = value(5);
    let mut steps: Vec<i64> = (0..FRAMES).map(step).collect();
    steps[4] = 5;
    let mut entries: Vec<u32> = Vec::new();
    for s in &steps {
        if !entries.contains(&(*s as u32)) {
            entries.push(*s as u32);
        }
    }
    assert_eq!(entries, vec![0, 3, 1, 5]);
    let bits = |v: u32| (32 - v.leading_zeros()) as usize;
    let (ib, vb) = (bits(entries.len() as u32), bits(5));
    let words = vec![
        ib as u32,
        vb as u32,
        (entries.len() * vb).div_ceil(32) as u32,
        SCALE.to_bits(),
        OFFSET.to_bits(),
        table(&entries, vb),
    ];
    for r in [&edited, &again(&edited)] {
        assert_eq!(values(r, 0, 0), want);
        assert_eq!(values(r, 2, 0), deltas);
        assert_eq!(indirect_words(r, animation), words);
    }

    // A step below the smallest moves the offset down by whole steps, so the others stay.
    let low = edit(&resource, &key(0, 1, OFFSET - 2.0 * SCALE)).expect("edits");
    let mut want = read.clone();
    want[1] = Some(OFFSET - 2.0 * SCALE);
    for r in [&low, &again(&low)] {
        assert_eq!(values(r, 0, 0), want);
        assert_eq!(
            indirect_words(r, animation)[4],
            (OFFSET - 2.0 * SCALE).to_bits()
        );
    }

    let refused = edit(&resource, &key(0, 1, f32::NAN)).expect_err("refused");
    assert!(refused.to_string().contains("not a finite"), "{refused}");
}

#[test]
fn a_dof_of_type_3_is_held_as_two_constant_floats() {
    let resource = opened(mapped(build(&Spec::default()).bytes), Rpf9Form::BlockMapped);
    let dof = Dof {
        id: 7,
        track: hash("genericControl"),
        kind: 3,
    };
    let added = AnimEdit::AddDof {
        animation: AT,
        dof,
        value: [0.25, -1.5, 9.0, 9.0],
    };
    let added = edit(&resource, &added).expect("edits");
    for r in [&added, &again(&added)] {
        let animation = the_animation(r);
        let at = (animation.dofs.iter().position(|d| *d == dof)).expect("added");
        assert_eq!(values(r, at, 0), vec![Some(0.25); usize::from(FRAMES)]);
        assert_eq!(values(r, at, 1), vec![Some(-1.5); usize::from(FRAMES)]);
        assert!(animation.signature_holds());
    }
    let at = (the_animation(&added).dofs.iter().position(|d| *d == dof)).expect("added");
    let posed = AnimEdit::Pose {
        animation: AT,
        dof: at,
        value: [2.0, 4.0, 0.0, 0.0],
    };
    let posed = edit(&added, &posed).expect("edits");
    assert_eq!(
        values(&again(&posed), at, 1),
        vec![Some(4.0); usize::from(FRAMES)]
    );
}

/// `built` as a stored resource whose last region holds, as the game's do, a map-2 entry its
/// bucket's chain reaches and the animation's block list.
fn stored_with_region(built: &Built) -> (Vec<u8>, usize, u32) {
    let mut bytes = built.bytes.clone();
    let start = bytes.len().next_multiple_of(16);
    bytes.resize(start + 0x50, 0);
    bytes[start + 8] = 1;
    bytes[start + 0x18..start + 0x1E].copy_from_slice(DICTIONARY.as_bytes());
    let list = pointer_at(&bytes, 0x70);
    let entry: Vec<u8> = bytes[list..list + 16].to_vec();
    bytes[start + 0x30..start + 0x40].copy_from_slice(&entry);
    bytes[0x70..0x78].copy_from_slice(&(VIRTUAL + start as u64 + 0x30).to_le_bytes());
    // The block list, last.
    let pointer = |to: usize| (VIRTUAL + to as u64).to_le_bytes();
    bytes[start + 0x40..start + 0x48].copy_from_slice(&pointer(built.block));
    let block_list = built.animation + 0x50;
    bytes[block_list..block_list + 8].copy_from_slice(&pointer(start + 0x40));
    // A second map-2 key in the first's bucket, its entry the region's word.
    let first = hash(ANIMATION);
    let second = first % 11 + 11 * 7;
    bytes[start + 0x20..start + 0x24].copy_from_slice(&second.to_le_bytes());
    let buckets = pointer_at(&bytes, 0x80);
    let head = pointer_at(&bytes, buckets + 8 * (first % 11) as usize);
    bytes[head + 8..head + 16].copy_from_slice(&(VIRTUAL + start as u64 + 0x20).to_le_bytes());
    bytes[0x90..0x94].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x08..0x10].copy_from_slice(&(VIRTUAL + start as u64).to_le_bytes());
    (bytes, start, second)
}

#[test]
fn a_stored_block_grows_while_its_last_region_holds_map_entries_and_the_block_list() {
    let steps: Vec<i64> = (0..70).map(|f| f * 65_535 / 69).collect();
    let mut spec = Spec::default();
    spec.constants.truncate(4);
    spec.constants.extend(delta_channel(&steps));
    spec.frames = 70;
    let built = build(&spec);
    let (bytes, start, second) = stored_with_region(&built);
    let resource = opened(bytes.clone(), Rpf9Form::StoredRsc8 { config: 0 });
    let region = resource.end_region.clone().expect("a last region");
    assert_eq!(region.word, second);
    assert!(resource.animation_keys.contains(&second));
    let zero = section10::value(SCALE, OFFSET, 0);

    let grown = edit(&resource, &key(2, 31, zero)).expect("edits");
    let out = section(&grown);
    let by = out.len() - bytes.len();
    assert!(by > 0);
    assert_eq!(pointer_at(out, 0x08), start + by);
    let list = pointer_at(out, built.animation + 0x50);
    assert_eq!(
        list,
        start + by + 0x40,
        "the block list moved with the region"
    );
    assert_eq!(
        pointer_at(out, list),
        start,
        "the block lies where the region began"
    );
    let read_again = again(&grown);
    assert_eq!(read_again.end_region, Some(region.clone()));
    assert!(read_again.animation_keys.contains(&second));
    assert_eq!(the_animation(&read_again).delta_value(2, 0, 31), Some(zero));

    // A longer clip name goes before the region too.
    let name = "pack:/clip_a_whose_name_is_longer.clip";
    let renamed = rename_clip(&resource, 0, name).expect("renames");
    let out = section(&renamed);
    assert_eq!(pointer_at(out, built.clip + 0x28), start);
    for r in [&renamed, &again(&renamed)] {
        assert_eq!(r.clips[0].name, name);
        assert_eq!(r.end_region, Some(region.clone()));
    }
}

#[test]
fn a_longer_clip_name_in_a_block_mapped_body_is_placed_at_its_end() {
    let resource = opened(mapped(build(&Spec::default()).bytes), Rpf9Form::BlockMapped);
    let name = "pack:/clip_a_whose_name_is_longer.clip";
    let renamed = rename_clip(&resource, 0, name).expect("renames");
    assert!(section(&renamed).len() > section(&resource).len());
    for r in [&renamed, &again(&renamed)] {
        assert_eq!(r.clips[0].name, name);
        assert_eq!(r.clips[0].key, rename_key(name));
    }
    assert!(rename_clip(&resource, 0, "pack:/\u{1}.clip").is_err());
}

fn rename_key(name: &str) -> u64 {
    let leaf = name.trim_start_matches("pack:/").trim_end_matches(".clip");
    u64::from(hash(DICTIONARY)) << 32 | u64::from(hash(leaf))
}

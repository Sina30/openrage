use std::mem::offset_of;
use std::sync::Arc;

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

fn with_sections(mut resource: AnimResource, bytes: Vec<u8>, form: Rpf9Form) -> AnimResource {
    resource.sections = Some(Arc::new(Rpf9Sections {
        form,
        virtual_section: bytes,
        physical: Vec::new(),
    }));
    resource
}

fn opened(bytes: Vec<u8>) -> AnimResource {
    with_sections(read(&bytes).expect("reads"), bytes, Rpf9Form::BlockMapped)
}

fn section(resource: &AnimResource) -> &[u8] {
    &resource
        .sections
        .as_ref()
        .expect("sections")
        .virtual_section
}

fn key(dof: usize, component: u8, frame: u16, value: f32) -> AnimEdit {
    AnimEdit::Key {
        animation: AnimAt {
            set: 0,
            animation: 0,
        },
        dof,
        component,
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

#[test]
fn a_key_of_a_constant_channel_sets_it_at_every_frame_of_its_block_in_place() {
    let resource = opened(build(&Spec::default()).bytes);
    let deltas = values(&resource, 2, 0);
    let pairs = the_animation(&resource).key_components();
    assert_eq!(pairs, vec![(0, 0), (1, 0), (1, 1), (1, 2), (2, 0)]);
    assert_eq!(
        values(&resource, 0, 0),
        vec![Some(0.5); usize::from(FRAMES)]
    );

    let constant = edit(&resource, &key(0, 0, 3, 1.25)).expect("edits");
    let vector = edit(&constant, &key(1, 1, 60, 7.5)).expect("edits");
    assert_eq!(section(&vector).len(), section(&resource).len());
    let read_again = opened(section(&vector).to_vec());
    for r in [&vector, &read_again] {
        assert_eq!(values(r, 0, 0), vec![Some(1.25); usize::from(FRAMES)]);
        assert_eq!(values(r, 1, 0), vec![Some(1.0); usize::from(FRAMES)]);
        assert_eq!(values(r, 1, 1), vec![Some(7.5); usize::from(FRAMES)]);
        assert_eq!(values(r, 2, 0), deltas);
    }
    assert!(reencode(section(&vector), false).expect("writes") == section(&vector));

    // A component no channel holds is refused by name.
    let refused = edit(&resource, &key(1, 3, 3, 1.0)).expect_err("refused");
    assert!(
        refused.to_string().contains("no channel of component 3"),
        "{refused}"
    );
}

/// A resource whose DOF 0 is raw in section 3, DOF 1 quantised in 5 bits in section 4 and DOF 2
/// in section 10, each frame's raw value `f / 2` and step `f % 32`.
fn framed() -> (Vec<u8>, f32, f32) {
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
    let frame_size = 8;
    let mut block = data[..constant_size].to_vec();
    for f in 0..u32::from(FRAMES) {
        block.extend((f as f32 / 2.0).to_bits().to_le_bytes());
        block.extend((f % 32).to_le_bytes());
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
            (frame_size as u16).to_le_bytes().to_vec(),
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
    (built.bytes, scale, offset)
}

#[test]
fn a_raw_and_a_quantised_key_are_written_in_place_in_their_frames() {
    let (bytes, scale, offset) = framed();
    let resource = opened(bytes);
    let deltas = values(&resource, 2, 0);
    let raw: Vec<Option<f32>> = (0..FRAMES).map(|f| Some(f32::from(f) / 2.0)).collect();
    let step = |q: u16| Some(scale * f32::from(q % 32) + offset);
    assert_eq!(values(&resource, 0, 0), raw);
    assert_eq!(
        values(&resource, 1, 0),
        (0..FRAMES).map(step).collect::<Vec<_>>()
    );
    assert!(reencode(section(&resource), false).expect("writes") == section(&resource));

    // A raw value, then a value on the quantised channel's range: one frame each, nothing moves.
    let edited = edit(&resource, &key(0, 0, 5, 9.75)).expect("edits");
    let edited = edit(&edited, &key(1, 0, 7, scale * 3.0 + offset)).expect("edits");
    assert_eq!(section(&edited).len(), section(&resource).len());
    let read_again = opened(section(&edited).to_vec());
    for r in [&edited, &read_again] {
        let mut want = raw.clone();
        want[5] = Some(9.75);
        assert_eq!(values(r, 0, 0), want);
        let mut want: Vec<_> = (0..FRAMES).map(step).collect();
        want[7] = Some(scale * 3.0 + offset);
        assert_eq!(values(r, 1, 0), want);
        assert_eq!(values(r, 2, 0), deltas);
    }

    // A value off its range quantises the channel afresh in its own 5 bits.
    let high = edit(&resource, &key(1, 0, 9, 100.0)).expect("edits");
    assert_eq!(section(&high).len(), section(&resource).len());
    let lo = offset;
    let width = (100.0 - lo) / 31.0;
    for (frame, got) in values(&high, 1, 0).into_iter().enumerate() {
        let wanted = if frame == 9 {
            100.0
        } else {
            step(frame as u16).expect("a step")
        };
        let got = got.expect("a value");
        assert!(
            (got - wanted).abs() <= width / 2.0 + 1e-4,
            "frame {frame}: {got} against {wanted}"
        );
    }
    assert_eq!(values(&high, 0, 0), raw);
    assert!(reencode(section(&high), false).expect("writes") == section(&high));
}

/// `bytes` of a clip dictionary given the region a stored resource ends with: the `{0, 1, 0}`
/// record, the dictionary's name, a word, the set list moved there, and a pointer to its set.
fn with_last_region(mut bytes: Vec<u8>, set: usize) -> (Vec<u8>, usize) {
    let start = bytes.len().next_multiple_of(16);
    bytes.resize(start + 0x50, 0);
    bytes[start + 8] = 1;
    bytes[start + 0x18..start + 0x1E].copy_from_slice(DICTIONARY.as_bytes());
    bytes[start + 0x20..start + 0x24].copy_from_slice(&0x77u32.to_le_bytes());
    let list = pointer_at(&bytes, 0x70);
    let entry: Vec<u8> = bytes[list..list + 16].to_vec();
    bytes[start + 0x30..start + 0x40].copy_from_slice(&entry);
    bytes[0x70..0x78].copy_from_slice(&(VIRTUAL + start as u64 + 0x30).to_le_bytes());
    bytes[start + 0x40..start + 0x48].copy_from_slice(&(VIRTUAL + set as u64).to_le_bytes());
    bytes[0x08..0x10].copy_from_slice(&(VIRTUAL + start as u64).to_le_bytes());
    (bytes, start)
}

#[test]
fn a_block_that_grows_in_a_stored_resource_goes_before_its_last_region_which_moves() {
    let steps: Vec<i64> = (0..70).map(|f| f * 65_535 / 69).collect();
    let mut spec = Spec::default();
    spec.constants.truncate(4);
    spec.constants.extend(delta_channel(&steps));
    spec.frames = 70;
    let built = build(&spec);
    let (bytes, start) = with_last_region(built.bytes, built.set);
    let stored = Rpf9Form::StoredRsc8 { config: 0 };
    let read = read_stored(&bytes).expect("reads stored");
    let region = read.end_region.clone().expect("a last region");
    assert_eq!(
        (region.names.as_slice(), region.word),
        (&[DICTIONARY.to_string()][..], 0x77)
    );
    let resource = with_sections(read, bytes.clone(), stored);
    let zero = section10::value(SCALE, OFFSET, 0);

    let grown = edit(&resource, &key(2, 0, 31, zero)).expect("edits");
    let out = section(&grown);
    assert!(out.len() > bytes.len());
    let block = pointer_at(out, pointer_at(out, built.animation + 0x50));
    assert_eq!(block, start, "the block lies where the region began");
    let again = read_stored(out).expect("reads stored again");
    assert_eq!(again.end_region, Some(region));
    assert_eq!(pointer_at(out, 0x08), start + (out.len() - bytes.len()));
    assert_eq!(the_animation(&again).delta_value(2, 0, 31), Some(zero));
    assert!(reencode(out, true).expect("writes") == out);

    // A word no known pointer field holds that reads as an address in the region refuses it.
    let mut stray = bytes;
    stray[0x60..0x68].copy_from_slice(&(VIRTUAL + start as u64 + 0x18).to_le_bytes());
    let stray = with_sections(read_stored(&stray).expect("reads"), stray, stored);
    let refused = edit(&stray, &key(2, 0, 31, zero)).expect_err("refused");
    assert!(refused.to_string().contains("0x60"), "{refused}");
}

#[test]
fn a_group_entry_s_window_sets_its_rate_and_its_three_point_warp_list() {
    let groups = Spec {
        clip: 2,
        ..Spec::default()
    };
    let resource = opened(build(&groups).bytes);
    let window = |entry: usize, start: f32, end: f32| AnimEdit::EntryWindow {
        clip: 0,
        group: 0,
        entry,
        start,
        end,
    };
    let edited = edit(&resource, &window(1, 0.5, 1.25)).expect("edits");
    let edited = edit(&edited, &window(0, 0.1, 0.9)).expect("edits");
    let (d, fps) = (
        1.5f64,
        f64::from(the_animation(&resource).rate().expect("a rate")).round(),
    );
    let w = 0.75f64;
    let middle = [
        ((d * fps).floor() / fps) as f32,
        (d * (w - 1.0 / fps) / w) as f32,
    ];
    for r in [&edited, &read(section(&edited)).expect("reads")] {
        let ClipPlay::Groups { groups, .. } = &r.clips[0].play else {
            panic!("a clip of groups");
        };
        let [first, second] = [&groups[0].entries[0], &groups[0].entries[1]];
        assert_eq!(
            (first.start, first.end, first.rate),
            (0.1, 0.9, (0.9 - 0.1) / 1.5)
        );
        assert!(first.warp_points.is_empty());
        assert_eq!(
            (second.start, second.end, second.rate),
            (0.5, 1.25, 0.75 / 1.5)
        );
        assert_eq!(second.warp_points, vec![[0.0, 0.0], middle, [1.5, 1.5]]);
    }
    assert!(edit(&resource, &window(1, 0.5, 0.5)).is_err());
    assert!(edit(&resource, &window(2, 0.0, 1.0)).is_err());
    let one = opened(build(&Spec::default()).bytes);
    assert!(edit(&one, &window(0, 0.0, 1.0)).is_err_and(|e| matches!(e, AnimError::Unwritten(_))));
}

#[test]
fn a_renamed_clip_takes_its_new_key_and_bucket() {
    let resource = opened(build(&Spec::default()).bytes);
    let old = resource.clips[0].key;
    let leaf = |name: &str| u64::from(hash(DICTIONARY)) << 32 | u64::from(hash(name));
    // A leaf of the old name's length whose key falls in another of the map's 11 buckets.
    let moved = (["clip_z", "clip_y", "clip_x", "clip_w"].into_iter())
        .find(|l| leaf(l) % 11 != old % 11)
        .expect("a leaf in another bucket");
    let name = format!("pack:/{moved}.clip");
    let renamed = rename_clip(&resource, 0, &name).expect("renames");
    for r in [&renamed, &read(section(&renamed)).expect("reads")] {
        assert_eq!(r.clips[0].name, name);
        assert_eq!(r.clips[0].key, leaf(moved));
        assert_eq!(r.root_word, 2);
    }
    assert!(renamed.edited);

    // A folder gives the key its high half; the name may be shorter than the one it replaces.
    let folder = rename_clip(&renamed, 0, "pack:/f/c.clip").expect("renames");
    let read_again = read(section(&folder)).expect("reads");
    assert_eq!(read_again.clips[0].name, "pack:/f/c.clip");
    assert_eq!(
        read_again.clips[0].key,
        u64::from(hash("f")) << 32 | u64::from(hash("c"))
    );
    // A longer name takes room the section grows by, and a section with no block map has none.
    assert!(rename_clip(&resource, 0, "pack:/clip_longer.clip").is_err());
    assert!(rename_clip(&resource, 0, "").is_err());
    assert!(rename_clip(&resource, 1, "pack:/clip_y.clip").is_err());
}

use std::sync::Arc;

use super::*;
use crate::container::rpf::{
    grow_section, write_rpf9_resource, Rpf9Form, Rpf9GrowError, Rpf9Section, Rpf9Sections,
};

const AT: AnimAt = AnimAt {
    set: 0,
    animation: 0,
};
/// A built dictionary's root, which the block map is placed right after.
const ROOT_LEN: usize = 0xB0;
const MAP_LEN: usize = 0x20;
/// A section longer than this needs a second block-map slot.
const PIECE: usize = 0x1_0000;

/// A built dictionary's `bytes` with a one-slot block map right after the root, every word that
/// addresses what follows the root moved past the map, and zeros to one whole piece.
fn mapped_after_root(bytes: &[u8]) -> Vec<u8> {
    let mut out = [&bytes[..ROOT_LEN], &[0u8; MAP_LEN][..], &bytes[ROOT_LEN..]].concat();
    let after_root = VIRTUAL + ROOT_LEN as u64..VIRTUAL + out.len() as u64;
    for at in (0..out.len() - 7).step_by(8) {
        let word = u64::from_le_bytes(out[at..at + 8].try_into().expect("eight bytes"));
        if after_root.contains(&word) {
            out[at..at + 8].copy_from_slice(&(word + MAP_LEN as u64).to_le_bytes());
        }
    }
    out[ROOT_LEN + 8] = 1;
    out[0x08..0x10].copy_from_slice(&(VIRTUAL + ROOT_LEN as u64).to_le_bytes());
    out.resize(PIECE, 0);
    out
}

fn opened(bytes: Vec<u8>) -> AnimResource {
    let mut resource = read(&bytes).expect("reads");
    resource.sections = Some(Arc::new(Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: bytes,
        physical: Vec::new(),
    }));
    resource
}

fn sections(resource: &AnimResource) -> &Rpf9Sections {
    resource.sections.as_deref().expect("sections")
}

/// What a clip states, its offsets aside.
fn clip_values(resource: &AnimResource) -> Vec<String> {
    (resource.clips.iter())
        .map(|c| {
            format!(
                "{} {:#X} {:?} {:?} {:?}",
                c.name, c.key, c.play, c.properties, c.tags
            )
        })
        .collect()
}

#[test]
fn a_dof_list_that_takes_a_second_block_map_slot_moves_every_pointer_past_the_map() {
    for clip in [1, 2] {
        let spec = Spec {
            clip,
            ..Spec::default()
        };
        let bytes = mapped_after_root(&build(&spec).bytes);
        let resource = opened(bytes.clone());
        assert_eq!(resource.block_map, Some([1, 0]));
        // With the block map's word alone, the slot's growth meets a pointer it does not name.
        let mut bare = sections(&resource).clone();
        let refused = grow_section(&mut bare, &[0x08], Rpf9Section::Virtual, PIECE, 16);
        assert!(
            matches!(refused, Err(Rpf9GrowError::LooksLikePointer { .. })),
            "{refused:?}"
        );

        let new = Dof {
            id: 7,
            track: hash("boneScale"),
            kind: 2,
        };
        let add = AnimEdit::AddDof {
            animation: AT,
            dof: new,
            value: [1.0, 1.0, 1.0, 0.0],
        };
        let added = edit(&resource, &add).expect("edits");
        let out = &sections(&added).virtual_section;
        assert!(out.len() > PIECE);
        assert!(write_rpf9_resource(sections(&added)).is_ok());
        let first = the_animation(&resource).dofs[0];
        for r in [&added, &opened(out.clone())] {
            assert_eq!(r.block_map, Some([2, 0]), "clip type {clip}");
            assert_eq!(clip_values(r), clip_values(&resource), "clip type {clip}");
            let animation = the_animation(r);
            assert!(animation.dofs.contains(&new));
            assert!(animation.signature_holds());
            let at = (animation.dofs.iter().position(|d| *d == first)).expect("kept");
            let held: Vec<Option<f32>> = (0..animation.frames)
                .map(|f| animation.key_value(at, 0, f))
                .collect();
            assert_eq!(held, vec![Some(0.5); usize::from(FRAMES)]);
        }
    }
}

#[test]
fn a_metadata_word_that_reads_as_an_address_past_the_slots_is_kept_as_data_by_the_growth() {
    let mut bytes = mapped_after_root(&build(&Spec::default()).bytes);
    // The GUID property's 16 bytes, which the reader takes as they are.
    let guid = (bytes.windows(16))
        .position(|w| w == [0xA5u8; 16].as_slice())
        .expect("the GUID's bytes");
    let word = VIRTUAL + 0x100;
    bytes[guid..guid + 8].copy_from_slice(&word.to_le_bytes());
    let resource = opened(bytes);
    let add = AnimEdit::AddDof {
        animation: AT,
        dof: Dof {
            id: 7,
            track: hash("boneScale"),
            kind: 2,
        },
        value: [1.0, 1.0, 1.0, 0.0],
    };
    let added = edit(&resource, &add).expect("edits");
    let out = &sections(&added).virtual_section;
    assert!(out.len() > PIECE);
    for r in [&added, &opened(out.clone())] {
        assert_eq!(r.block_map, Some([2, 0]));
        assert_eq!(clip_values(r), clip_values(&resource));
    }
}

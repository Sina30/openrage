use super::skeleton::{skeleton_into, BuiltBone, BONES};
use super::*;

const STRAP: [BuiltBone; 2] = [
    BuiltBone {
        name: "GM_Strap_Root",
        parent: -1,
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation: [0.1, 0.2, 0.3],
    },
    BuiltBone {
        name: "GM_Strap_End",
        parent: 0,
        rotation: [0.0, 0.70710677, 0.0, 0.70710677],
        translation: [0.0, 0.0, 0.4],
    },
];

/// The built drawable whose root lists a skeleton of each of `rigs` at 0xA0, counted at 0x9C.
fn with_extra(rigs: &[&[BuiltBone]]) -> (Image, Vec<u8>, usize) {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let mut skeletons = Vec::new();
    for bones in rigs {
        skeletons.push(skeleton_into(&mut img, bones).0);
    }
    let list = img.pointers(&skeletons);
    img.ptr(parts.root + 0xA0, list);
    img.u32(parts.root + 0x9C, rigs.len() as u32);
    (img, phys, parts.root)
}

/// Whether the drawable is refused for the word at `at`.
fn refused_at(img: &Image, phys: &[u8], at: usize) -> bool {
    matches!(
        read_drawable(&img.bytes, Some(phys)),
        Err(HeaderlessError::Constant { at: found, .. }) if found == at as u64
    )
}

#[test]
fn a_drawable_reads_each_skeleton_its_root_lists_at_0xa0() {
    let (img, phys, _) = with_extra(&[&STRAP[..], &BONES[..1]]);
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let names: Vec<Vec<&str>> = (d.extra_skeletons.iter())
        .map(|s| s.bones.iter().map(|b| b.name.as_str()).collect())
        .collect();
    assert_eq!(
        names,
        [vec!["GM_Strap_Root", "GM_Strap_End"], vec!["SKEL_ROOT"]]
    );
    assert_eq!(d.extra_skeletons[0].bones[1].parent, Some(0));
    assert_eq!(d.extra_skeletons[0].bones[1].translation, [0.0, 0.0, 0.4]);

    let (img, phys, _) = drawable([1.0, 1.0, 0.0]);
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    assert!(d.extra_skeletons.is_empty());
}

#[test]
fn a_list_of_extra_skeletons_off_the_measured_form_is_refused() {
    // A word at 0x98, a count of zero and a count past the bound.
    for (offset, word) in [(0x98, 1u32), (0x9C, 0), (0x9C, 64)] {
        let (mut img, phys, root) = with_extra(&[&STRAP[..]]);
        img.u32(root + offset, word);
        assert!(
            refused_at(&img, &phys, root + offset),
            "{offset:#X} = {word}"
        );
    }
    // An entry that is not a skeleton: its two bone counts differ.
    let (mut img, phys, root) = with_extra(&[&STRAP[..]]);
    let (at, _) = skeleton_into(&mut img, &STRAP);
    img.u16(at + 0x6E, 1);
    let list = img.pointers(&[at]);
    img.ptr(root + 0xA0, list);
    assert!(matches!(
        read_drawable(&img.bytes, Some(&phys)),
        Err(HeaderlessError::Disagree {
            first: 2,
            second: 1,
            ..
        })
    ));
}

//! A pack whose BC1 body is stored through the BCPack coder, coded here from made-up blocks.

use super::*;
use crate::gta6::bcpack::tests::{bc1_lanes, encode};
use crate::gta6::bcpack::BcPackError;

/// [`textures`] with texture 0's stored top mip coded as a body of codec kind 1 instead.
fn coded() -> (Vec<Spec>, Vec<Vec<u8>>) {
    let (mut specs, expected) = textures();
    let Body::Stored(tiled) = &specs[0].bodies[1] else {
        panic!("texture 0's second body is its stored top mip");
    };
    let packed = Body::Packed {
        kind: 1,
        stored: encode(1, &bc1_lanes(tiled)),
        decoded: tiled.len() as u64,
    };
    specs[0].bodies[1] = packed;
    (specs, expected)
}

#[test]
fn a_bc1_body_through_the_coder_reads_as_the_blocks_it_codes() {
    let (specs, expected) = coded();
    let (_dir, pack) = pack_of(&specs);
    let row = pack.rows_of(0).expect("rows").start + 1;
    assert_eq!(pack.rows[row].kind, 1);
    pack.check(0)
        .expect("a BC1 coder body is one the pack reads");
    let texture = pack.texture(0).expect("reads");
    assert_eq!(texture.levels, 7);
    assert!(
        texture.pixels == expected[0],
        "every level, the top mip decoded"
    );
}

#[test]
fn a_bc1_body_that_does_not_decode_is_refused_by_its_row_and_the_coder_says_why() {
    let (mut specs, _) = coded();
    let Body::Packed { stored, .. } = &mut specs[0].bodies[1] else {
        panic!("coded");
    };
    stored.truncate(stored.len() / 2);
    let (_dir, pack) = pack_of(&specs);
    let row = pack.rows_of(0).expect("rows").start + 1;
    let refused = pack.texture(0).expect_err("its top mip is cut short");
    assert!(
        matches!(
            refused,
            TtpfError::BcPack { row: r, kind: 1, source: BcPackError::Truncated { .. } } if r == row
        ),
        "{refused}"
    );
    assert!(pack.texture(1).is_ok(), "the other textures read");
}

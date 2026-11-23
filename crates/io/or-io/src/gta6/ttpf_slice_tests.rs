//! One slice of a cube, an array or a one-level volume replaced alone, in packs built here.

use super::*;

/// Made-up bytes, none zero, `n` of them, from `seed`.
fn bytes(n: usize, seed: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (1 + (i * 11 + seed * 37) % 251) as u8)
        .collect()
}

fn saved(pack: &TexturePack) -> Vec<u8> {
    let mut out = Vec::new();
    pack.save_to(&mut out).expect("the pack saves");
    out
}

/// `pack`'s texture `index` with slice `slice` replaced by `levels`: every other slice reads as
/// before, that slice reads as `levels`, and a save reopens to the same texture and saves again
/// byte for byte.
fn replaces_alone(pack: &mut TexturePack, index: usize, slice: u32, levels: &[Vec<u8>]) {
    let before = pack.texture(index).expect("it reads");
    pack.replace_slice_levels(index, slice, levels)
        .expect("the slice is replaced");
    let after = pack.texture(index).expect("it reads after");
    for face in 0..before.faces {
        let (was, now) = (before.face(face), after.face(face));
        match face == slice {
            true => assert_eq!(now.expect("the slice").pixels, levels.concat()),
            false => assert_eq!(now, was, "slice {face} is kept"),
        }
    }
    let once = saved(pack);
    let (_dir, again) = open(&once);
    let again = again.expect("the saved pack opens");
    assert_eq!(again.texture(index).expect("it reads saved"), after);
    assert!(
        saved(&again) == once,
        "saving a saved pack is byte-identical"
    );
}

#[test]
fn an_array_s_slice_is_replaced_alone_in_its_64_kib_tiles() {
    // BC7 512×256 of one level, two slices of one 128 KiB chain each, piece by piece.
    let mut array = spec(512, 256, 0x310, 0, 1, vec![Body::Zlib(bytes(0x40000, 1))]);
    (array.dimension, array.slices) = (5, 2);
    let (_dir, mut pack) = pack_of(&[array]);
    replaces_alone(&mut pack, 0, 1, &[bytes(128 * 64 * 16, 2)]);
    assert!(matches!(
        pack.replace_slice_levels(0, 2, &[]),
        Err(TtpfError::NoSuchSlice {
            index: 0,
            slice: 2,
            slices: 2
        })
    ));
}

#[test]
fn a_cube_s_face_is_replaced_alone_in_its_last_row() {
    // BC1 16×16 of one level: each face a lone 4 KiB tile, six in one row.
    let mut cube = spec(16, 16, 0x238, 1, 1, vec![Body::Zlib(bytes(6 * 0x1000, 3))]);
    (cube.dimension, cube.slices) = (2, 6);
    let (_dir, mut pack) = pack_of(&[cube]);
    replaces_alone(&mut pack, 0, 4, &[bytes(4 * 4 * 8, 4)]);
}

#[test]
fn a_one_level_volume_s_z_plane_is_replaced_and_one_of_two_levels_is_refused() {
    // RGBA8 8×16×8: one 4 KiB tile, one z-slab.
    let mut volume = spec(8, 16, 0x0E0, 1, 1, vec![Body::Zlib(bytes(0x1000, 5))]);
    (volume.dimension, volume.slices) = (3, 8);
    let (_dir, mut pack) = pack_of(&[volume]);
    replaces_alone(&mut pack, 0, 3, &[bytes(8 * 16 * 4, 6)]);

    // Two levels: level 0's tile, then the tail tile holding level 1.
    let mut volume = spec(8, 16, 0x0E0, 1, 2, vec![Body::Zlib(bytes(0x2000, 7))]);
    (volume.dimension, volume.slices) = (3, 8);
    let (_dir, mut pack) = pack_of(&[volume]);
    assert!(pack.texture(0).is_ok(), "the volume reads");
    assert!(matches!(
        pack.replace_slice_levels(0, 0, &[]),
        Err(TtpfError::VolumeLevels { index: 0, mips: 2 })
    ));
}

#[test]
fn a_2d_texture_s_only_slice_is_the_texture_and_it_has_no_other() {
    let flat = spec(128, 64, 0x238, 1, 1, vec![Body::Zlib(bytes(0x1000, 8))]);
    let (_dir, mut pack) = pack_of(&[flat]);
    assert!(matches!(
        pack.replace_slice_levels(0, 1, &[]),
        Err(TtpfError::NoSuchSlice { slices: 1, .. })
    ));
}

#[cfg(feature = "authoring")]
#[test]
fn a_cube_s_face_is_replaced_from_an_image_of_its_size() {
    let mut cube = spec(16, 16, 0x238, 1, 1, vec![Body::Zlib(bytes(6 * 0x1000, 9))]);
    (cube.dimension, cube.slices) = (2, 6);
    let (_dir, mut pack) = pack_of(&[cube]);
    let before = pack.texture(0).expect("it reads");
    let red = crate::codec::texture::RgbaImage {
        width: 16,
        height: 16,
        data: [255, 0, 0, 255].repeat(256),
        is_srgb: false,
    };
    let options = crate::authoring::TextureBuildOptions::default();
    assert_eq!(
        pack.replace_slice(0, 2, &red, &options).expect("replaced"),
        1
    );
    let after = pack.texture(0).expect("it reads");
    assert_ne!(after.face(2), before.face(2));
    assert_eq!(after.face(3), before.face(3));
    let small = crate::codec::texture::RgbaImage {
        width: 8,
        height: 8,
        data: [0; 256].to_vec(),
        is_srgb: false,
    };
    assert!(matches!(
        pack.replace_slice(0, 2, &small, &options),
        Err(TtpfError::ImageSize { .. })
    ));
}

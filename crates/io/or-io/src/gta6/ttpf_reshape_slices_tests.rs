//! Pack cubes and arrays given a new size, format or mip count, and a 16-byte array tail of eight
//! levels, read back.

use super::*;

/// Where each level of a 16-byte 64 KiB tail tile starts.
const SLOTS_64K_16: [usize; 8] = [0x8000, 0x4000, 0x2000, 0x1000, 0x800, 0x600, 0x500, 0x400];

/// `slices` chains of `mips` levels each, slice after slice.
fn slices_of(width: u32, height: u32, format: Format, mips: u32, slices: usize) -> Vec<Vec<u8>> {
    (0..slices)
        .flat_map(|s| chain_elements(width, height, format, mips, 40 + s))
        .collect()
}

/// A 16x16 BC7 cube of one row, flags 0: face f's five levels in the slots of tail tile f.
fn small_cube() -> Spec {
    let faces: Vec<u8> = (0..6)
        .flat_map(|face| {
            let levels = chain_of(5, 4, 4, |w, h, level| {
                blocks(w, h, 16, (0x20 * face + level + 1) as u8)
            });
            tail(&levels, &SLOTS_4K_16, 16, TILES_4K_16)
        })
        .collect();
    let mut cube = spec(16, 16, 0x310, 1, 5, vec![Body::Zlib(faces)]);
    (cube.dimension, cube.slices) = (2, 6);
    cube
}

/// A 1024x1024 BC1 array of two slices in 64 KiB tiles, each holding nine levels.
fn small_array() -> Spec {
    let slices: Vec<_> = (0..2)
        .map(|s| {
            chain_of(11, 256, 256, |w, h, level| {
                blocks(w, h, 8, (0x40 * s + level + 1) as u8)
            })
        })
        .collect();
    let piece = |s: usize, j: usize| match j {
        0 => tail(&slices[s][2..], &SLOTS_64K_8, 8, TILES_64K_8),
        1 => tile(&slices[s][1].0, 128, 128, 8, TILES_64K_8),
        _ => tile(&slices[s][0].0, 256, 256, 8, TILES_64K_8),
    };
    let bodies = (0..3)
        .flat_map(|j| (0..2).map(move |s| (s, j)))
        .map(|(s, j)| Body::Zlib(piece(s, j)))
        .collect();
    let mut array = spec(1024, 1024, 0x239, 0, 11, bodies);
    (array.dimension, array.slices) = (5, 2);
    array
}

/// [`textures`] with `extra` put in as texture 1, and what each other texture reads as.
fn with(extra: Spec) -> (Vec<Spec>, Vec<(usize, Vec<u8>)>) {
    let (mut specs, expected) = textures();
    specs.insert(1, extra);
    let others = (expected.into_iter().enumerate())
        .map(|(k, pixels)| (if k == 0 { 0 } else { k + 1 }, pixels))
        .collect();
    (specs, others)
}

#[test]
fn a_16_byte_array_slice_in_64_kib_tiles_reads_its_eight_tail_levels() {
    // 512x512 BC7 x 2: per slice mip0 in four 64 KiB tiles, mip1 in one, mips 2 to 9 in the
    // tail's eight slots, the last two at 0x500 and 0x400.
    let slices: Vec<_> = (0..2)
        .map(|s| {
            chain_of(10, 128, 128, |w, h, level| {
                blocks(w, h, 16, (0x40 * s + level + 1) as u8)
            })
        })
        .collect();
    let piece = |s: usize, j: usize| match j {
        0 => tail(&slices[s][2..], &SLOTS_64K_16, 16, TILES_64K_16),
        1 => tile(&slices[s][1].0, 64, 64, 16, TILES_64K_16),
        _ => tile(&slices[s][0].0, 128, 128, 16, TILES_64K_16),
    };
    let bodies = (0..3)
        .flat_map(|j| (0..2).map(move |s| (s, j)))
        .map(|(s, j)| Body::Zlib(piece(s, j)))
        .collect();
    let mut array = spec(512, 512, 0x312, 0, 8, bodies);
    (array.dimension, array.slices) = (5, 2);
    let (_dir, pack) = pack_of(&[array]);
    let texture = pack.texture(0).expect("the array reads");
    assert_eq!((texture.faces, texture.levels), (2, 10));
    for (s, levels) in slices.iter().enumerate() {
        let slice = texture.face(s as u32).expect("a slice");
        assert!(slice.pixels == concat(levels), "slice {s}");
    }
}

#[test]
fn a_cube_given_another_size_and_format_holds_its_smaller_cube_in_its_first_row() {
    let (specs, others) = with(small_cube());
    let (_dir, mut pack) = pack_of(&specs);
    // 128x128 BC1 in 4 KiB tiles: mip 0 explicit and mips 1 to 7 in the tail, so flags 1, two
    // rows: the six faces at 64x64, then at 128x128.
    let bc1 = format(0x238);
    let levels = slices_of(128, 128, bc1, 8, 6);
    (pack.reshape_levels(1, shape(128, 128, bc1, 8), &levels)).expect("reshapes");
    let entry = pack.entries[1];
    assert_eq!(entry.format_code, 71 << 3 | 1);
    assert_eq!((entry.mip_count, entry.tile_mode), (7, 2));
    assert_eq!(row_pages(&pack, 1), [6, 18], "6 x 4 KiB, then 6 x 12 KiB");
    assert_eq!(entry.row_codes, [code_of(6), code_of(18), 0, 0]);
    let read = pack.texture(1).expect("the cube reads");
    assert_eq!((read.faces, read.levels), (6, 8));
    assert!(
        read.pixels == levels.concat(),
        "every face where it was written"
    );

    // The first row is the cube at 64x64: each face's tail tile holds mips 1 to 7.
    let mut file = File::open(pack.path()).expect("opens");
    let first = pack.body(&mut file, pack.rows_of(1).expect("rows").start);
    let first = first.expect("the first row inflates");
    for face in 0..6 {
        let held: Vec<(Vec<u8>, usize, usize)> = (1..8)
            .map(|m| {
                let side = (32usize >> m).max(1);
                (levels[face * 8 + m].clone(), side, side)
            })
            .collect();
        let want = tail(&held, &SLOTS_4K_8, 8, TILES_4K_8);
        assert!(
            first[face * 0x1000..(face + 1) * 0x1000] == want[..],
            "face {face} of the smaller cube"
        );
    }
    for (k, want) in &others {
        assert!(
            pack.texture(*k).expect("reads").pixels == *want,
            "texture {k}"
        );
    }
    assert_packed(&pack);
    saved_and_read_back(&pack);
}

#[test]
fn a_cube_of_four_rows_takes_tile_mode_0_and_layout_bit_26() {
    let (specs, _) = with(small_cube());
    let (_dir, mut pack) = pack_of(&specs);
    // 512x512 BC7: four explicit levels, so flags 3 and four rows, the smallest cube first.
    let bc7 = format(0x310);
    let levels = slices_of(512, 512, bc7, 10, 6);
    (pack.reshape_levels(1, shape(512, 512, bc7, 10), &levels)).expect("reshapes");
    let entry = pack.entries[1];
    assert_eq!((entry.format_code, entry.mip_count), (98 << 3 | 3, 7));
    assert_eq!(entry.tile_mode, 0);
    assert_eq!(row_pages(&pack, 1), [12, 48, 144, 528]);
    assert!(pack.texture(1).expect("reads").pixels == levels.concat());
    let saved = save(&pack);
    assert!(
        entry_word(&saved, 1, 1) & 1 << 26 != 0,
        "a cube in tile mode 0"
    );
    saved_and_read_back(&pack);
}

#[test]
fn an_array_given_another_size_and_format_lays_each_piece_of_every_slice_in_turn() {
    let (specs, others) = with(small_array());
    let bc7 = format(0x310);
    // 512x512 BC7 in 64 KiB tiles: mips 0 and 1 explicit, mips 2 to 9 in the tail's eight slots.
    let (_dir, mut pack) = pack_of(&specs);
    let levels = slices_of(512, 512, bc7, 10, 2);
    (pack.reshape_levels(1, shape(512, 512, bc7, 10), &levels)).expect("reshapes");
    let entry = pack.entries[1];
    assert_eq!(entry.format_code, 98 << 3 | 2, "flags the explicit levels");
    assert_eq!(
        (entry.mip_count, entry.tile_mode),
        (8, 0),
        "the tail levels held"
    );
    assert_eq!(entry.row_codes, [0; 4]);
    assert_eq!(row_pages(&pack, 1), [16, 16, 16, 16, 64, 64]);
    let read = pack.texture(1).expect("the array reads");
    assert_eq!((read.faces, read.levels), (2, 10));
    assert!(
        read.pixels == levels.concat(),
        "every slice where it was written"
    );
    for (k, want) in &others {
        assert!(
            pack.texture(*k).expect("reads").pixels == *want,
            "texture {k}"
        );
    }
    assert_packed(&pack);
    saved_and_read_back(&pack);

    // Two mips stop above the tail: no tail tile, a row for each of them in each slice.
    let (_dir, mut pack) = pack_of(&specs);
    let levels = slices_of(512, 512, bc7, 2, 2);
    (pack.reshape_levels(1, shape(512, 512, bc7, 2), &levels)).expect("reshapes");
    let entry = pack.entries[1];
    assert_eq!((entry.format_code, entry.mip_count), (98 << 3 | 2, 0));
    assert_eq!(row_pages(&pack, 1), [16, 16, 64, 64]);
    assert!(pack.texture(1).expect("reads").pixels == levels.concat());
    saved_and_read_back(&pack);
}

#[test]
fn a_cube_or_array_shape_the_planner_does_not_cover_is_refused_by_name() {
    let (specs, _) = with(small_cube());
    let (_dir, pack) = pack_of(&specs);
    let refused = |shape: Shape| {
        let mut pack = pack.clone();
        let refusal = pack.reshape_levels(1, shape, &[]).expect_err("refused");
        assert!(!pack.is_relaid(), "a refused reshape moves nothing");
        refusal
    };
    // A cube of 8-byte pixels, and a nibble of 1 that would read as a lone level.
    assert!(matches!(
        refused(shape(16, 16, format(0x050), 1)),
        TtpfError::CubeNotMeasured { index: 1, bytes: 8 }
    ));
    assert!(matches!(
        refused(shape(128, 128, format(0x238), 2)),
        TtpfError::NibbleCannotState { index: 1, mips: 2 }
    ));
    assert!(matches!(
        refused(shape(128, 128, format(0x238), 9)),
        TtpfError::MipsNotPlaced {
            index: 1,
            mips: 9,
            most: 8
        }
    ));
    // Six faces of 1024x1024 BC7 pass the largest row a code states.
    assert!(matches!(
        refused(shape(1024, 1024, format(0x310), 1)),
        TtpfError::RowNotCoded { index: 1 }
    ));
    // Levels for one face where six are planned.
    let mut pack = pack.clone();
    let one = chain_elements(16, 16, format(0x310), 1, 3);
    assert!(matches!(
        pack.reshape_levels(1, shape(16, 16, format(0x310), 1), &one),
        Err(TtpfError::Malformed(_))
    ));
}

#[cfg(feature = "authoring")]
#[test]
fn a_cube_takes_one_image_for_each_face_all_of_one_size() {
    use crate::authoring::TextureBuildOptions;
    use crate::codec::texture::RgbaImage;

    let (specs, _) = with(small_cube());
    let (_dir, mut pack) = pack_of(&specs);
    let image = |side: u32, red: u8| RgbaImage {
        width: side,
        height: side,
        data: [red, 0, 0, 0xFF].repeat((side * side) as usize),
        is_srgb: false,
    };
    let one = TextureBuildOptions {
        mip_levels: Some(1),
        ..TextureBuildOptions::default()
    };
    let rgba8 = format(0x0E0);
    assert!(matches!(
        pack.reshape(1, &image(32, 1), rgba8, &one),
        Err(TtpfError::SliceImages {
            index: 1,
            slices: 6
        })
    ));
    let uneven: Vec<RgbaImage> = (0..6).map(|f| image(32 + 32 * (f / 5), 1)).collect();
    assert!(matches!(
        pack.reshape_slices(1, &uneven, rgba8, &one),
        Err(TtpfError::SliceImages { index: 1, .. })
    ));
    let faces: Vec<RgbaImage> = (0..6).map(|f| image(32, 10 * f as u8 + 1)).collect();
    assert_eq!(
        (pack.reshape_slices(1, &faces, rgba8, &one)).expect("reshapes"),
        1
    );
    let read = pack.texture(1).expect("reads");
    assert_eq!((read.width, read.faces, read.levels), (32, 6, 1));
    for (f, face) in faces.iter().enumerate() {
        let got = read.face(f as u32).expect("a face");
        assert!(got.pixels == face.data, "RGBA8 face {f} is its image");
    }
    saved_and_read_back(&pack);
}

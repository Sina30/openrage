//! A pack volume of one level given a new size or format, and the images a cube's or a volume's
//! reshape from one image takes for its other slices.

use super::*;
use crate::codec::texture::{MipFilter, RgbaImage};

/// A 9x9x9 RGBA8 volume of one level in tile mode `tile_mode`: 2 by 1 tiles of 8x16x8 in each of two z-slabs.
fn small_volume(tile_mode: u8) -> Spec {
    let mut volume = spec(
        9,
        9,
        0x0E0,
        tile_mode,
        1,
        vec![Body::Zlib(vec![0x11; 0x4000])],
    );
    (volume.dimension, volume.slices) = (3, 9);
    volume
}

/// A 16x16 RGBA8 cube of one level, face f's tile all `0x20 + 0x10 * f`.
fn uniform_cube() -> Spec {
    let faces = (0..6u8).flat_map(|f| [0x20 + 0x10 * f; 0x1000]).collect();
    let mut cube = spec(16, 16, 0x0E0, 1, 1, vec![Body::Zlib(faces)]);
    (cube.dimension, cube.slices) = (2, 6);
    cube
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

fn red(width: u32, height: u32) -> RgbaImage {
    RgbaImage {
        width,
        height,
        data: [0xFF, 0, 0, 0xFF].repeat((width * height) as usize),
        is_srgb: false,
    }
}

#[test]
fn a_volume_given_another_size_and_format_holds_its_z_slabs_in_one_row() {
    let (specs, others) = with(small_volume(1));
    let (_dir, mut pack) = pack_of(&specs);
    // 20x12 R8 in 16x16x16 tiles: two tiles in the one z-slab nine slices take.
    let r8 = format(0x1E8);
    let levels: Vec<Vec<u8>> = (0..9)
        .flat_map(|s| chain_elements(20, 12, r8, 1, 50 + s))
        .collect();
    (pack.reshape_levels(1, shape(20, 12, r8, 1), &levels)).expect("reshapes");
    let entry = pack.entries[1];
    assert_eq!(
        (entry.format_code, entry.tile_mode, entry.mip_count),
        (61 << 3, 1, 1),
        "flags 0, tile mode 1, a nibble of 1"
    );
    assert_eq!((entry.dimension, entry.slices), (3, 9), "the depth is kept");
    assert_eq!(entry.row_codes, [0; 4]);
    let tiles = 20u64.div_ceil(16) * 12u64.div_ceil(16);
    assert_eq!(row_pages(&pack, 1), [tiles]);
    let read = pack.texture(1).expect("the volume reads");
    assert_eq!(
        (read.width, read.height, read.faces, read.levels),
        (20, 12, 9, 1)
    );
    assert!(
        read.pixels == levels.concat(),
        "every z-plane where it was written"
    );
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
fn a_volume_shape_no_pack_volume_shows_is_refused_by_name_and_moves_nothing() {
    let (specs, _) = with(small_volume(1));
    let (_dir, pack) = pack_of(&specs);
    let r8 = format(0x1E8);
    let refused = |shape: Shape, levels: &[Vec<u8>]| {
        let mut pack = pack.clone();
        let refusal = pack.reshape_levels(1, shape, levels).expect_err("refused");
        assert!(!pack.is_relaid(), "a refused reshape moves nothing");
        refusal
    };
    assert!(matches!(
        refused(shape(20, 12, r8, 2), &[]),
        TtpfError::MipsNotPlaced {
            index: 1,
            mips: 2,
            most: 1
        }
    ));
    // One z-slab of 64 by 64 tiles passes the 4 MiB no volume is seen to exceed.
    assert!(matches!(
        refused(shape(1024, 1024, r8, 1), &[]),
        TtpfError::RowNotCoded { index: 1 }
    ));
    // One z-plane where nine are planned.
    let one = chain_elements(20, 12, r8, 1, 3);
    assert!(matches!(
        refused(shape(20, 12, r8, 1), &one),
        TtpfError::Malformed(_)
    ));

    let (specs, _) = with(small_volume(2));
    let (_dir, mut moded) = pack_of(&specs);
    assert!(matches!(
        moded.reshape_levels(1, shape(20, 12, r8, 1), &[]),
        Err(TtpfError::Volume {
            index: 1,
            tile_mode: 2
        })
    ));
}

#[test]
fn each_other_slice_takes_its_own_top_mip_resampled_to_the_image_s_size() {
    let (specs, _) = with(uniform_cube());
    let (_dir, pack) = pack_of(&specs);
    let image = red(32, 32);
    let sources = (pack.slice_sources(1, Some(&image), 2, MipFilter::Box)).expect("six images");
    assert_eq!(sources.len(), 6);
    for (f, source) in sources.iter().enumerate() {
        assert_eq!((source.width, source.height), (32, 32), "face {f}");
        if f == 2 {
            assert!(source.data == image.data, "the shown face is the image");
            continue;
        }
        let own = 0x20 + 0x10 * f as u8;
        assert!(
            source.data.iter().all(|&b| b.abs_diff(own) <= 1),
            "face {f} is its own top mip at the image's size"
        );
    }
    let own = (pack.slice_sources(1, None, 0, MipFilter::Box)).expect("six images");
    assert!(own.iter().all(|s| (s.width, s.height) == (16, 16)));
    assert!(matches!(
        pack.slice_sources(1, Some(&image), 6, MipFilter::Box),
        Err(TtpfError::NoSuchSlice {
            index: 1,
            slice: 6,
            slices: 6
        })
    ));

    // A 2D texture's one image is the image, and it has no slice 1.
    let flat = (pack.slice_sources(0, Some(&image), 0, MipFilter::Box)).expect("one image");
    assert!(flat.len() == 1 && flat[0].data == image.data);
    assert!(matches!(
        pack.slice_sources(0, Some(&image), 1, MipFilter::Box),
        Err(TtpfError::NoSuchSlice {
            index: 0,
            slice: 1,
            slices: 1
        })
    ));
}

#[cfg(feature = "authoring")]
#[test]
fn a_cube_and_a_volume_take_another_size_from_one_image() {
    use crate::authoring::TextureBuildOptions;

    let one = TextureBuildOptions {
        mip_levels: Some(1),
        ..TextureBuildOptions::default()
    };
    let rgba8 = format(0x0E0);
    for (extra, image, shown, faces) in [
        (uniform_cube(), red(32, 32), 2, 6),
        (small_volume(1), red(20, 12), 4, 9),
    ] {
        let (specs, _) = with(extra);
        let (_dir, mut pack) = pack_of(&specs);
        let sources = (pack.slice_sources(1, Some(&image), shown, one.filter)).expect("sources");
        let mips = (pack.reshape_slices(1, &sources, rgba8, &one)).expect("reshapes");
        assert_eq!(mips, 1);
        let read = pack.texture(1).expect("reads");
        assert_eq!(
            (read.width, read.height, read.faces),
            (image.width, image.height, faces)
        );
        let got = read.face(shown).expect("the shown slice");
        assert!(got.pixels == image.data, "RGBA8 slice {shown} is its image");
        saved_and_read_back(&pack);
    }
}

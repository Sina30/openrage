//! Pack textures given a new size, format or mip count, the textures after them moved, then saved
//! and read back.

use super::*;

/// Linear elements for `mips` levels of a `width` by `height` texture of `format`, differing in every position, level and seed.
fn chain_elements(width: u32, height: u32, format: Format, mips: u32, seed: usize) -> Vec<Vec<u8>> {
    (0..mips)
        .map(|level| {
            let side = |pixels: u32| (pixels >> level).max(1).div_ceil(format.element_side());
            let count = (side(width) * side(height)) as usize * format.element_bytes();
            (0..count)
                .map(|i| (1 + (i * 13 + seed * 7 + level as usize * 31) % 251) as u8)
                .collect()
        })
        .collect()
}

fn shape(width: u32, height: u32, format: Format, mips: u32) -> Shape {
    Shape {
        width,
        height,
        format,
        mips: Some(mips),
    }
}

fn save(pack: &TexturePack) -> Vec<u8> {
    let mut bytes = Vec::new();
    pack.save_to(&mut bytes).expect("the pack saves");
    bytes
}

/// The 4 KiB pages each row of texture `index` decodes to.
fn row_pages(pack: &TexturePack, index: usize) -> Vec<u64> {
    (pack.rows_of(index).expect("rows"))
        .map(|row| pack.size(row) >> 12)
        .collect()
}

/// The 4 KiB tiles of 32x16 8-byte elements a level of `width` by `height` elements takes.
fn tiles_8(width: u32, height: u32) -> u64 {
    u64::from(width.div_ceil(32) * height.div_ceil(16))
}

/// The row-size code of a row of `pages` pages.
fn code_of(pages: u64) -> u8 {
    (1..32)
        .find(|&code| row_code_pages(code).map(u64::from) == Some(pages))
        .expect("a code states the row")
}

/// The directory's file offset, from row 0 of the table.
fn directory_of(bytes: &[u8]) -> usize {
    (u32::from_le_bytes(bytes[16..20].try_into().expect("a row")) & 0xFF_FFFF) as usize * 0x100
}

/// Word `word` of texture `k`'s directory entry.
fn entry_word(bytes: &[u8], k: usize, word: usize) -> u32 {
    let at = directory_of(bytes) + 16 + 16 * k + 4 * word;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("a word"))
}

/// Every entry starts on the page the one before it ends on, the first after the directory.
fn assert_packed(pack: &TexturePack) {
    for (k, entry) in pack.entries.iter().enumerate() {
        let rows = pack.rows_of(k).expect("rows");
        let start = pack.rows[rows.start].virtual_address;
        assert_eq!(u64::from(entry.page) << 12, start, "texture {k}'s page");
        let before = if k == 0 {
            pack.rows[1].virtual_address
        } else {
            pack.rows[pack.rows_of(k - 1).expect("rows").end].virtual_address
        };
        assert_eq!(start, before, "texture {k} follows the one before it");
    }
    let sentinel = pack.rows[pack.rows.len() - 1];
    assert_eq!(pack.header.virtual_size, sentinel.virtual_address);
    assert_eq!(pack.header.row_count as usize, pack.rows.len());
}

/// Saved and opened again: the same entries, table and textures, and a save of that byte-identical.
fn saved_and_read_back(pack: &TexturePack) -> (tempfile::TempDir, TexturePack) {
    let once = save(pack);
    let (dir, again) = open(&once);
    let again = again.expect("the saved pack opens");
    assert!(!again.is_relaid());
    assert_eq!(again.entries, pack.entries, "entries and keys");
    assert_eq!(
        again.rows, pack.rows,
        "the table the plan holds is the one written"
    );
    assert_eq!(again.header, pack.header);
    for index in 0..pack.entries.len() {
        match (pack.texture(index), again.texture(index)) {
            (Ok(was), Ok(now)) => assert_eq!(was, now, "texture {index}"),
            (Err(was), Err(now)) => assert_eq!(was.to_string(), now.to_string()),
            _ => panic!("texture {index} reads in one pack only"),
        }
    }
    assert_eq!(again.save_layout().expect("plans").rows, again.rows);
    assert!(
        save(&again) == once,
        "saving the saved pack is byte-identical"
    );
    (dir, again)
}

#[test]
fn a_texture_given_a_larger_size_reads_back_and_every_texture_after_it_moves_with_its_rows() {
    let (specs, expected) = textures();
    let (_dir, mut pack) = pack_of(&specs);
    let rows_before = pack.rows.len();
    let old_rows = pack.rows_of(0).expect("rows").len();
    let bc1 = format(0x238);
    let mips = u32::BITS - 1024u32.leading_zeros();
    let levels = chain_elements(1024, 512, bc1, mips, 1);
    (pack.reshape_levels(0, shape(1024, 512, bc1, mips), &levels)).expect("reshapes");
    assert!(pack.is_relaid());

    let read = pack.texture(0).expect("the new texture reads");
    assert_eq!(
        (read.width, read.height, read.format, read.levels),
        (1024, 512, bc1, mips)
    );
    assert!(
        read.pixels == levels.concat(),
        "every mip where it was written"
    );
    for (k, want) in expected.iter().enumerate().skip(1) {
        let texture = pack.texture(k).expect("a moved texture reads");
        assert!(texture.pixels == *want, "texture {k} moved with its rows");
    }
    assert_packed(&pack);

    // A top mip of 128 KiB or more takes flags 3: the tail tile and mip 3 in row 0, then one row
    // for each larger mip.
    let pages = [
        tiles_8(256, 128),
        tiles_8(128, 64),
        tiles_8(64, 32),
        tiles_8(32, 16),
    ];
    let want = [1 + pages[3], pages[2], pages[1], pages[0]];
    assert_eq!(row_pages(&pack, 0), want);
    assert_eq!(pack.rows.len(), rows_before - old_rows + want.len());
    let entry = pack.entries[0];
    assert_eq!(entry.format_code, 71 << 3 | 3);
    assert_eq!(u32::from(entry.mip_count), mips - (want.len() as u32 - 1));
    assert_eq!(entry.row_codes, want.map(code_of));
    assert_eq!(
        entry.tile_mode, 2,
        "four rows whose top row is over 32 pages"
    );
    assert_eq!(entry.key, key_of(0));
    saved_and_read_back(&pack);
}

#[test]
fn a_shape_s_rows_codes_and_tile_mode_are_the_ones_its_pieces_give() {
    let (specs, expected) = textures();
    // BC1 512x512 to the 4x4 mip: tail, mip 2, mip 1, mip 0 one row each, four rows of 32 pages
    // or fewer at the top.
    let bc1 = format(0x238);
    let (_dir, mut pack) = pack_of(&specs);
    let levels = chain_elements(512, 512, bc1, 8, 2);
    (pack.reshape_levels(1, shape(512, 512, bc1, 8), &levels)).expect("reshapes");
    let want = [1, tiles_8(32, 32), tiles_8(64, 64), tiles_8(128, 128)];
    assert_eq!(row_pages(&pack, 1), want);
    let entry = pack.entries[1];
    assert_eq!(
        entry.tile_mode, 3,
        "four rows whose top row is 32 pages or fewer"
    );
    assert_eq!(entry.mip_count, 8 - 3);
    assert_eq!(entry.row_codes, want.map(code_of));
    let small = want.iter().take_while(|&&p| p < 16).count();
    let align = (16 - want[..small].iter().sum::<u64>() % 16) % 16;
    assert_eq!(
        (u64::from(entry.small_rows), u64::from(entry.align_pages)),
        (small as u64, align)
    );
    assert!(pack.texture(1).expect("reads").pixels == levels.concat());
    for (k, want) in expected.iter().enumerate().skip(2) {
        assert!(
            pack.texture(k).expect("reads").pixels == *want,
            "texture {k}"
        );
    }
    assert_packed(&pack);
    saved_and_read_back(&pack);

    // BC4 1024x512 to mip 7: four mips in the tail tile, which row 0 shares with mip 3.
    let bc4 = format(0x280);
    let (_dir, mut pack) = pack_of(&specs);
    let levels = chain_elements(1024, 512, bc4, 8, 3);
    (pack.reshape_levels(3, shape(1024, 512, bc4, 8), &levels)).expect("reshapes");
    let want = [
        1 + tiles_8(32, 16),
        tiles_8(64, 32),
        tiles_8(128, 64),
        tiles_8(256, 128),
    ];
    assert_eq!(row_pages(&pack, 3), want);
    assert_eq!(pack.entries[3].tile_mode, 2);
    assert_eq!(pack.entries[3].mip_count, 8 - 3);
    assert!(pack.texture(3).expect("reads").pixels == levels.concat());
    saved_and_read_back(&pack);
}

#[test]
fn a_large_texture_takes_64_kib_tiles_and_a_top_mip_over_4_mib_whole_4_mib_rows() {
    let (specs, expected) = textures();
    let bc1 = format(0x238);
    // In 64 KiB tiles of 128x64 elements.
    let tiles_64k =
        |width: u32, height: u32| u64::from(width.div_ceil(128) * height.div_ceil(64)) << 16;
    for (side, mips) in [(2048, 1), (4096, 1), (2048, 3)] {
        let (_dir, mut pack) = pack_of(&specs);
        let levels = chain_elements(side, side, bc1, mips, side as usize);
        (pack.reshape_levels(2, shape(side, side, bc1, mips), &levels)).expect("reshapes");
        let entry = pack.entries[2];
        assert_eq!(entry.tile_mode, 0, "{side}");
        let flags = if mips == 1 { 1 } else { 3 };
        assert_eq!(entry.format_code, 71 << 3 | flags);
        assert_eq!((entry.mip_count, entry.row_codes), (0, [0; 4]), "{side}");
        let mut want = Vec::new();
        for level in (0..mips).rev() {
            let bytes = tiles_64k((side >> level) / 4, (side >> level) / 4);
            let rows = bytes.div_ceil(TOP_ROW_BYTES);
            want.extend(std::iter::repeat_n(bytes / rows, rows as usize));
        }
        let got: Vec<u64> = row_pages(&pack, 2)
            .iter()
            .map(|pages| pages << 12)
            .collect();
        assert_eq!(got, want, "{side}x{side}, {mips} mip(s)");
        assert!(pack.texture(2).expect("reads").pixels == levels.concat());
        assert!(pack.texture(3).expect("reads").pixels == expected[3]);
        assert_packed(&pack);
        saved_and_read_back(&pack);
    }
}

/// The bytes of one row a top mip over 4 MiB is cut into.
const TOP_ROW_BYTES: u64 = 4 << 20;

#[test]
fn a_smaller_texture_moves_the_rest_back_and_rows_of_a_codec_nothing_reads_are_carried() {
    let coded = || {
        spec(
            128,
            64,
            0x238,
            1,
            1,
            vec![Body::Coded {
                kind: 3,
                decoded: 0x1000,
            }],
        )
    };
    let (specs, _) = textures();
    let mut specs: Vec<Spec> = specs.into_iter().take(2).collect();
    specs.insert(0, coded());
    specs.push(coded());
    let (_dir, pack) = pack_of(&specs);
    let mut file = File::open(pack.path()).expect("opens");
    let last = pack.rows_of(3).expect("rows").start;
    let carried = pack.stored(&mut file, last).expect("stored");
    let page = pack.entries[3].page;

    let mut small = pack.clone();
    let rgba8 = format(0x0E0);
    let levels = chain_elements(64, 64, rgba8, 1, 4);
    (small.reshape_levels(2, shape(64, 64, rgba8, 1), &levels)).expect("reshapes");
    assert!(
        small.entries[3].page < page,
        "the texture after it moves back"
    );
    let entry = small.entries[2];
    assert_eq!(
        (entry.format_code, entry.tile_mode, entry.mip_count),
        (28 << 3, 1, 1),
        "one level in 4 KiB tiles: flags 0, tile mode 1, its nibble 1"
    );
    assert!(small.texture(2).expect("reads").pixels == levels.concat());
    for index in [0, 3] {
        assert!(matches!(
            small.texture(index),
            Err(TtpfError::UnidentifiedCodec { kind: 3, .. })
        ));
    }
    assert_packed(&small);
    let (_dir, again) = saved_and_read_back(&small);
    let mut file = File::open(again.path()).expect("opens");
    let row = again.rows_of(3).expect("rows").start;
    assert!(
        again.stored(&mut file, row).expect("stored") == carried,
        "carried byte for byte"
    );
    assert_eq!(again.rows[row].kind, 3);

    // A replacement in the new shape, and of the coded texture in row 1, save as the plan has them.
    let mut replaced = small.clone();
    let again_levels = chain_elements(64, 64, rgba8, 1, 5);
    replaced
        .replace_levels(2, &again_levels)
        .expect("replaces in its new shape");
    replaced
        .replace_levels(0, &chain_elements(128, 64, format(0x238), 1, 6))
        .expect("the coded texture is written readable");
    assert!(replaced.texture(2).expect("reads").pixels == again_levels.concat());
    let (_dir, again) = open(&save(&replaced));
    let again = again.expect("opens");
    assert!(again.texture(2).expect("reads").pixels == again_levels.concat());
    assert_eq!(again.entries, replaced.entries);
}

#[test]
fn a_relaid_save_restates_each_entry_and_keeps_the_bits_no_field_reads() {
    let (specs, expected) = textures();
    let mut bytes = build(&specs);
    let at = directory_of(&bytes) + 16 + 16 * 3;
    for (word, bit) in [(1, 28), (3, 12)] {
        let at = at + 4 * word;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("a word")) | 1 << bit;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let (_dir, pack) = open(&bytes);
    let mut pack = pack.expect("opens");
    let bc1 = format(0x238);
    let levels = chain_elements(1024, 512, bc1, 4, 7);
    (pack.reshape_levels(0, shape(1024, 512, bc1, 4), &levels)).expect("reshapes");
    let saved = save(&pack);
    assert!(
        entry_word(&saved, 3, 1) & 1 << 28 != 0,
        "layout bit 28 is copied"
    );
    assert!(
        entry_word(&saved, 3, 3) & 1 << 12 != 0,
        "an unread bit is kept"
    );
    assert_eq!(entry_word(&saved, 3, 2) & 0x1F_FFFF, pack.entries[3].page);
    let (_dir, again) = open(&saved);
    let again = again.expect("opens");
    assert_eq!(again.entries, pack.entries);
    assert!(again.texture(3).expect("reads").pixels == expected[3]);
    assert!(again.texture(0).expect("reads").pixels == levels.concat());
}

#[test]
fn a_texture_reshaped_twice_and_back_reads_as_written_each_time() {
    let (specs, expected) = textures();
    let (_dir, mut pack) = pack_of(&specs);
    let bc7 = format(0x310);
    let bc3 = format(0x268);
    let first = chain_elements(512, 512, bc3, 1, 8);
    (pack.reshape_levels(1, shape(512, 512, bc3, 1), &first)).expect("reshapes");
    let back = chain_elements(512, 256, bc7, 1, 9);
    (pack.reshape_levels(1, shape(512, 256, bc7, 1), &back)).expect("reshapes back");
    // A lone level of 128 KiB or more in 64 KiB tiles takes flags 1.
    assert_eq!(pack.entries[1].format_code, specs[1].code & !7 | 1);
    assert_eq!(pack.rows_of(1).map(|rows| rows.len()), Some(1));
    assert!(pack.texture(1).expect("reads").pixels == back.concat());
    for k in [0, 2, 3] {
        assert!(
            pack.texture(k).expect("reads").pixels == expected[k],
            "texture {k}"
        );
    }
    assert_packed(&pack);
    saved_and_read_back(&pack);
}

#[test]
fn each_shape_the_planner_does_not_cover_is_refused_by_name_and_changes_nothing() {
    let (specs, _) = textures();
    let (_dir, pack) = pack_of(&specs);
    let refused = |index: usize, shape: Shape| {
        let mut pack = pack.clone();
        let refusal = pack.reshape_levels(index, shape, &[]).expect_err("refused");
        assert!(!pack.is_relaid(), "a refused reshape moves nothing");
        refusal
    };
    let bc1 = format(0x238);

    // R16F's 4 KiB tail has no measured slot: mip 2 of 128x64 would lie in it.
    assert!(matches!(
        refused(0, shape(128, 64, format(0x1B0), 3)),
        TtpfError::MipsNotPlaced {
            index: 0,
            mips: 3,
            most: 2
        }
    ));
    assert!(matches!(
        refused(0, shape(128, 64, bc1, 0)),
        TtpfError::MipsNotPlaced { mips: 0, .. }
    ));
    assert!(matches!(
        refused(0, shape(16385, 4, bc1, 1)),
        TtpfError::ShapeTooLarge {
            index: 0,
            width: 16385,
            height: 4
        }
    ));
    // A 6 MiB top mip is no whole number of 4 MiB rows.
    assert!(matches!(
        refused(0, shape(4096, 3072, bc1, 1)),
        TtpfError::RowNotCoded { index: 0 }
    ));
    assert!(matches!(
        refused(9, shape(128, 64, bc1, 1)),
        TtpfError::NoSuchTexture { index: 9, count: 4 }
    ));

    let mut cube = spec(16, 16, 0x311, 1, 5, vec![Body::Zlib(vec![1; 6 * 0x1000])]);
    (cube.dimension, cube.slices) = (2, 6);
    let (_dir, mut cubes) = pack_of(&[cube]);
    assert!(matches!(
        cubes.reshape_levels(0, shape(16, 16, bc1, 1), &[]),
        Err(TtpfError::ReplaceShape { index: 0 })
    ));

    let whole = build(&specs);
    let (_dir, short) = open(&whole[..whole.len() - 0x100]);
    let mut short = short.expect("a short pack opens");
    assert!(matches!(
        short.reshape_levels(0, shape(128, 64, bc1, 1), &[]),
        Err(TtpfError::NotWhole)
    ));

    // Levels of another count than the plan places are refused after planning.
    assert!(matches!(
        refused(1, shape(512, 256, format(0x310), 1)),
        TtpfError::Malformed(_)
    ));

    assert_eq!(
        Format::of_grc(GrcTextureFormat::DXT1, true),
        Format::of_dxgi(72)
    );
    assert_eq!(
        Format::of_grc(GrcTextureFormat::BC7, false),
        Format::of_dxgi(98)
    );
    assert_eq!(Format::of_grc(GrcTextureFormat::A1R5G5B5, false), None);
}

#[test]
fn a_small_chain_of_several_mips_takes_the_flags_its_tiled_pieces_give() {
    let (specs, expected) = textures();
    let bc1 = format(0x238);
    // In 4 KiB pages: the largest piece under 4 gives flags 0, the second 8 or more 3, else 1.
    let rule = |pieces: &[u64]| {
        let mut sorted = pieces.to_vec();
        sorted.sort_unstable();
        let (largest, second) = (sorted[sorted.len() - 1], sorted[sorted.len() - 2]);
        match (largest < 4, second >= 8) {
            (true, _) => 0,
            (_, true) => 3,
            _ => 1,
        }
    };
    // Each shape's pieces: the tail tile, then its explicit levels in 32x16-element tiles.
    let cases = [
        (128, 128, 3, vec![1, tiles_8(32, 32)]),
        (128, 64, 4, vec![1, tiles_8(32, 16)]),
        (64, 256, 9, vec![1, tiles_8(8, 32), tiles_8(16, 64)]),
        (
            2048,
            4,
            12,
            (0..5)
                .map(|level| tiles_8(512 >> level, 1))
                .chain([1])
                .collect(),
        ),
    ];
    let flags: Vec<u16> = cases.iter().map(|(.., pieces)| rule(pieces)).collect();
    assert_eq!(flags, [0, 0, 1, 3], "the cases cover each flag");
    for ((width, height, mips, _), want) in cases.iter().zip(flags) {
        let (_dir, mut pack) = pack_of(&specs);
        let levels = chain_elements(*width, *height, bc1, *mips, *mips as usize);
        let shape = shape(*width, *height, bc1, *mips);
        pack.reshape_levels(2, shape, &levels).expect("reshapes");
        let entry = pack.entries[2];
        assert_eq!(entry.format_code, 71 << 3 | want, "{width}x{height}");
        assert!(pack.texture(2).expect("reads").pixels == levels.concat());
        assert!(pack.texture(3).expect("reads").pixels == expected[3]);
        assert_packed(&pack);
        saved_and_read_back(&pack);
    }
}

#[cfg(feature = "authoring")]
#[test]
fn a_texture_encoded_from_an_image_of_another_size_and_format_reads_back_as_the_image() {
    use crate::authoring::TextureBuildOptions;
    use crate::codec::texture::RgbaImage;

    let (specs, expected) = textures();
    let (_dir, mut pack) = pack_of(&specs);
    let red = RgbaImage {
        width: 64,
        height: 64,
        data: [0xFF, 0, 0, 0xFF].repeat(64 * 64),
        is_srgb: false,
    };
    let one = TextureBuildOptions {
        mip_levels: Some(1),
        ..TextureBuildOptions::default()
    };
    let rgba8 = format(0x0E0);
    assert_eq!(pack.reshape(1, &red, rgba8, &one).expect("reshapes"), 1);
    let read = pack.texture(1).expect("reads");
    assert_eq!((read.width, read.height, read.format), (64, 64, rgba8));
    assert!(read.pixels == red.data, "RGBA8 is the image itself");
    assert!(pack.texture(2).expect("reads").pixels == expected[2]);
    let image = decode_level(&pack.texture(1).expect("reads"), 0).expect("decodes");
    assert!(image
        .data
        .chunks(4)
        .all(|pixel| pixel == [0xFF, 0, 0, 0xFF]));
    assert!(matches!(
        pack.reshape(1, &red, format(0x300), &one),
        Err(TtpfError::ReshapeFormat { index: 1, .. })
    ));
    saved_and_read_back(&pack);
}

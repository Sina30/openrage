//! Packs built here, replaced and saved, then read back.

use super::super::write::{Rule, Start};
use super::*;

/// Elements of `element` bytes, none zero, that differ from one position and seed to the next.
fn elements(count: usize, element: usize, seed: usize) -> Vec<u8> {
    (0..count * element)
        .map(|i| (1 + (i * 7 + seed * 31) % 251) as u8)
        .collect()
}

/// The directory's file offset, from row 0 of the table.
fn directory_of(bytes: &[u8]) -> usize {
    (u32::from_le_bytes(bytes[16..20].try_into().expect("a row")) & 0xFF_FFFF) as usize * 0x100
}

/// Ors `bits` into word `word` of texture `k`'s directory entry.
fn set_entry_bits(bytes: &mut [u8], k: usize, word: usize, bits: u32) {
    let at = directory_of(bytes) + 16 + 16 * k + 4 * word;
    let value = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("a word")) | bits;
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// A one-tile BC1 texture of one stored body, which nothing here replaces.
fn filler() -> Spec {
    spec(128, 64, 0x238, 1, 1, vec![Body::Stored(vec![0x77; 0x1000])])
}

/// A texture of `pages` zlib rows of 4 KiB pages each, whose bytes the replacement overwrites.
struct Case {
    code: u16,
    tile_mode: u8,
    width: u32,
    height: u32,
    mips: u8,
    pages: &'static [usize],
    codes: [u8; 4],
    small_rows: u32,
    align_pages: u32,
    /// The levels its chain holds, as the layout tables give them.
    levels: usize,
}

impl Case {
    fn spec(&self) -> Spec {
        let bodies = (self.pages.iter())
            .map(|&pages| Body::Zlib(vec![0x5A; pages << 12]))
            .collect();
        let mut spec = spec(
            self.width,
            self.height,
            self.code,
            self.tile_mode,
            self.mips,
            bodies,
        );
        spec.codes = self.codes;
        spec
    }

    /// A pack of the filler, then this texture.
    fn pack(&self) -> Vec<u8> {
        let mut bytes = build(&[filler(), self.spec()]);
        set_entry_bits(
            &mut bytes,
            1,
            3,
            self.small_rows << 19 | self.align_pages << 21,
        );
        bytes
    }

    /// Distinct elements for every level of its chain, top first.
    fn levels(&self, seed: usize) -> Vec<Vec<u8>> {
        let format = format(self.code);
        let side = |pixels: u32, level: usize| {
            ((pixels >> level).max(1)).div_ceil(format.element_side()) as usize
        };
        (0..self.levels)
            .map(|level| {
                let count = side(self.width, level) * side(self.height, level);
                elements(count, format.element_bytes(), seed + level)
            })
            .collect()
    }
}

/// One texture per element size and tile size the writer takes.
fn cases() -> Vec<Case> {
    let case = |code, tile_mode, width, height, mips, pages, codes, levels| Case {
        code,
        tile_mode,
        width,
        height,
        mips,
        pages,
        codes,
        small_rows: 0,
        align_pages: 0,
        levels,
    };
    vec![
        // BC1, 4 KiB: the tail tile and mip 1, then mip 0; seven tail slots hold mips 2 to 8.
        case(0x239, 1, 256, 128, 8, &[2, 4], [2, 4, 0, 0], 9),
        // BC1, 64 KiB: mip 1, then mip 0, with no tail.
        case(0x239, 0, 1024, 512, 1, &[16, 64], [0; 4], 2),
        // BC7, 4 KiB, three rows: the tail with mips 2 to 7, mip 1, mip 0.
        Case {
            small_rows: 3,
            align_pages: 12,
            ..case(0x312, 2, 128, 64, 6, &[1, 1, 2], [1, 1, 2, 0], 8)
        },
        case(0x310, 0, 512, 256, 1, &[32], [0; 4], 1),
        case(0x0E1, 2, 64, 64, 1, &[1, 4], [1, 4, 0, 0], 2),
        case(0x0E1, 0, 256, 256, 1, &[64], [0; 4], 1),
        // R8, 4 KiB, three rows: the tail with mips 2 to 7, mip 1, mip 0.
        Case {
            small_rows: 3,
            align_pages: 10,
            ..case(0x1EA, 2, 128, 128, 6, &[1, 1, 4], [1, 1, 4, 0], 8)
        },
        case(0x1E9, 0, 512, 512, 1, &[64], [0; 4], 1),
        // A lone level in one tile, at its start.
        case(0x238, 1, 128, 64, 1, &[1], [0; 4], 1),
    ]
}

fn saved(pack: &TexturePack) -> Vec<u8> {
    let mut bytes = Vec::new();
    pack.save_to(&mut bytes).expect("the pack saves");
    bytes
}

/// Every row's stored bytes, read from the file.
fn stored_rows(pack: &TexturePack) -> Vec<Vec<u8>> {
    let mut file = File::open(pack.path()).expect("opens");
    (0..pack.rows.len() - 1)
        .map(|row| pack.stored(&mut file, row).expect("on disk"))
        .collect()
}

#[test]
fn an_unchanged_pack_plans_its_own_table_and_saves_to_a_fixed_point_with_the_same_textures() {
    let (mut specs, _) = textures();
    specs.push(spec(
        128,
        64,
        0x238,
        1,
        1,
        vec![Body::Coded {
            kind: 3,
            decoded: 0x1000,
        }],
    ));
    let (_dir, pack) = pack_of(&specs);
    let layout = pack.save_layout().expect("plans");
    assert_eq!(
        layout.rows, pack.rows,
        "an unchanged pack plans its own table"
    );
    assert!(layout.undecided.contains(&1) && layout.undecided.contains(&(pack.rows.len() - 1)));

    let once = saved(&pack);
    let (_dir, again) = open(&once);
    let again = again.expect("the saved pack opens");
    assert_eq!(again.rows, pack.rows);
    assert_eq!(again.entries, pack.entries, "keys and entries");
    assert_eq!(stored_rows(&again), stored_rows(&pack));
    for index in 0..pack.entries.len() {
        match (pack.texture(index), again.texture(index)) {
            (Ok(was), Ok(now)) => assert_eq!(was, now, "texture {index}"),
            (Err(was), Err(now)) => assert_eq!(was.to_string(), now.to_string()),
            _ => panic!("texture {index} reads in one pack only"),
        }
    }
    assert!(
        saved(&again) == once,
        "saving a saved pack is byte-identical"
    );
}

#[test]
fn a_replaced_texture_reads_back_to_the_elements_written_for_every_element_and_tile_size() {
    for (n, case) in cases().iter().enumerate() {
        let (_dir, pack) = open(&case.pack());
        let mut pack = pack.expect("the pack opens");
        let levels = case.levels(n);
        pack.replace_levels(1, &levels)
            .unwrap_or_else(|e| panic!("case {n} ({:#05x}) replaces: {e}", case.code));
        let want = levels.concat();
        let read = pack
            .texture(1)
            .expect("the replacement reads before a save");
        assert_eq!(read.levels as usize, case.levels, "case {n}'s mips");
        assert!(read.pixels == want, "case {n} reads back what was written");

        let once = saved(&pack);
        let (_dir, reopened) = open(&once);
        let reopened = reopened.expect("the saved pack opens");
        let read = reopened
            .texture(1)
            .expect("the replacement reads after a save");
        assert!(read.pixels == want, "case {n} reads back after a save");
        assert_eq!(
            reopened.texture(0).expect("reads"),
            pack.texture(0).expect("reads"),
            "case {n}'s other texture"
        );
        assert_eq!(
            reopened.entries, pack.entries,
            "case {n} keeps every entry and key"
        );
        assert!(
            (reopened.rows_of(1).expect("rows")).all(|row| reopened.rows[row].kind == KIND_ZLIB)
        );
        assert!(
            saved(&reopened) == once,
            "case {n} saves again byte-identical"
        );
    }
}

#[test]
fn replacing_one_texture_leaves_every_other_row_byte_for_byte_and_coded_rows_survive() {
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
    let (_dir, pack) = pack_of(&[filler(), coded(), filler(), coded()]);
    let before = stored_rows(&pack);
    let mut replaced = pack.clone();
    // A texture whose only row is of a codec nothing reads is written readable.
    replaced
        .replace_levels(1, &[elements(32 * 16, 8, 3)])
        .expect("replaces");
    assert!(replaced.texture(1).is_ok());

    let (_dir, saved) = open(&saved(&replaced));
    let saved = saved.expect("opens");
    let after = stored_rows(&saved);
    for row in (1..pack.rows.len() - 1).filter(|&row| row != 2) {
        assert!(
            after[row] == before[row],
            "row {row} is carried byte for byte"
        );
        assert_eq!(
            saved.rows[row].kind, pack.rows[row].kind,
            "row {row}'s kind"
        );
    }
    assert!(after[2] != before[2] && saved.rows[2].kind == KIND_ZLIB);
    assert!(matches!(
        saved.texture(3),
        Err(TtpfError::UnidentifiedCodec { kind: 3, .. })
    ));
    assert_eq!(
        saved.texture(0).expect("reads"),
        pack.texture(0).expect("reads")
    );
}

#[cfg(feature = "authoring")]
#[test]
fn a_texture_replaced_from_an_image_reads_back_as_the_image() {
    use crate::authoring::TextureBuildOptions;
    use crate::codec::texture::RgbaImage;

    let rgba = cases().remove(4);
    let (_dir, pack) = open(&rgba.pack());
    let mut pack = pack.expect("opens");
    let gradient: Vec<u8> = (0..64 * 64)
        .flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 0x40, 0xFF])
        .collect();
    let image = RgbaImage {
        width: 64,
        height: 64,
        data: gradient.clone(),
        is_srgb: false,
    };
    let options = TextureBuildOptions::default();
    assert_eq!(pack.replace(1, &image, &options).expect("replaces"), 2);
    let read = pack.texture(1).expect("reads");
    assert!(
        read.pixels[..gradient.len()] == gradient,
        "RGBA8 mip 0 is the image"
    );

    let (_dir, mut pack) = pack_of(&[filler(), filler()]);
    let red = RgbaImage {
        width: 128,
        height: 64,
        data: [0xFF, 0, 0, 0xFF].repeat(128 * 64),
        is_srgb: false,
    };
    assert_eq!(pack.replace(1, &red, &options).expect("replaces"), 1);
    let image = decode_level(&pack.texture(1).expect("reads"), 0).expect("decodes");
    assert!(image
        .data
        .chunks(4)
        .all(|pixel| pixel == [0xFF, 0, 0, 0xFF]));

    let small = RgbaImage {
        width: 64,
        height: 64,
        ..red.clone()
    };
    assert!(matches!(
        pack.replace(1, &small, &options),
        Err(TtpfError::ImageSize {
            index: 1,
            width: 128,
            height: 64,
            got: (64, 64)
        })
    ));
    let r16f = spec(128, 64, 0x1B0, 1, 1, vec![Body::Zlib(vec![1; 0x4000])]);
    let (_dir, mut half) = pack_of(&[filler(), r16f]);
    assert!(matches!(
        half.replace(1, &red, &options),
        Err(TtpfError::ReplaceFormat { index: 1, .. })
    ));
}

#[test]
fn each_texture_the_writer_does_not_cover_is_refused_by_name() {
    let refusal = |bytes: Vec<u8>, levels: Vec<Vec<u8>>| {
        let (_dir, pack) = open(&bytes);
        pack.expect("opens")
            .replace_levels(1, &levels)
            .expect_err("refused")
    };
    let one = |spec: Spec| build(&[filler(), spec]);

    let mut cube = spec(16, 16, 0x311, 1, 5, vec![Body::Zlib(vec![1; 6 * 0x1000])]);
    (cube.dimension, cube.slices) = (2, 6);
    assert!(matches!(
        refusal(one(cube), vec![]),
        TtpfError::ReplaceShape { index: 1 }
    ));
    let mut array = spec(512, 256, 0x310, 0, 1, vec![Body::Zlib(vec![1; 0x40000])]);
    (array.dimension, array.slices) = (5, 2);
    assert!(matches!(
        refusal(one(array), vec![]),
        TtpfError::ReplaceShape { index: 1 }
    ));

    // R16F in 4 KiB tiles: the nibble counts mip 2, which the tail tile holds and no 2-byte slot places.
    let tail = spec(128, 64, 0x1B0, 1, 3, vec![Body::Zlib(vec![1; 6 * 0x1000])]);
    assert!(matches!(
        refusal(one(tail), vec![]),
        TtpfError::TailSlotNotMeasured { index: 1 }
    ));

    let case = &cases()[0];
    let mut uncoded = case.spec();
    uncoded.codes = [0; 4];
    assert!(matches!(
        refusal(one(uncoded), case.levels(0)),
        TtpfError::CodesNotByRule { index: 1 }
    ));

    // One 64 KiB tile of BC1 mip 0 over five rows, none of them the 4 MiB a cut top mip takes.
    let rows = [4, 4, 4, 2, 2].map(|pages| Body::Zlib(vec![1; pages << 12]));
    let split = spec(512, 256, 0x238, 0, 1, rows.into());
    assert!(matches!(
        refusal(one(split), vec![]),
        TtpfError::SplitTopMip { index: 1, rows: 5 }
    ));

    let whole = case.pack();
    let (_dir, short) = open(&whole[..whole.len() - 0x100]);
    let mut short = short.expect("a short pack opens");
    assert!(matches!(
        short.replace_levels(1, &case.levels(0)),
        Err(TtpfError::NotWhole)
    ));
    assert!(matches!(
        short.save_to(&mut Vec::new()),
        Err(TtpfError::NotWhole)
    ));
}

#[test]
fn a_texture_setting_layout_bit_28_reads_and_replaces_as_one_without_it() {
    let case = &cases()[0];
    let mut flagged = case.pack();
    set_entry_bits(&mut flagged, 1, 1, 1 << 28);
    let (_dir, plain) = open(&case.pack());
    let (_dir, pack) = open(&flagged);
    let (mut plain, mut pack) = (plain.expect("opens"), pack.expect("opens"));
    assert_eq!(
        pack.texture(1).expect("reads"),
        plain.texture(1).expect("reads")
    );
    for pack in [&mut plain, &mut pack] {
        pack.replace_levels(1, &case.levels(0)).expect("replaces");
    }
    assert_eq!(
        pack.save_layout().expect("plans"),
        plain.save_layout().expect("plans")
    );
    assert_eq!(
        pack.texture(1).expect("reads"),
        plain.texture(1).expect("reads")
    );
}

#[test]
fn the_start_rule_moves_a_forcible_row_near_a_page_end_or_after_three_unmoved_ones() {
    let mut rule = Rule::default();
    assert_eq!(rule.page(Start::Forcible, 0x1D01), Some(true), "0x200 left");
    assert_eq!(
        rule.page(Start::Forcible, 0x1C00),
        Some(false),
        "0x400 left"
    );
    assert_eq!(
        rule.page(Start::Forcible, 0x2000),
        Some(true),
        "already on a page"
    );
    assert_eq!(rule.page(Start::Aligned, 0x1F00), Some(false));
    assert_eq!(rule.page(Start::Undecided, 0x1A00), None);
    for at in [0x1100, 0x2200, 0x3300] {
        rule.step(Start::Forcible, at, at);
    }
    assert_eq!(
        rule.page(Start::Forcible, 0x4400),
        Some(true),
        "after three unmoved"
    );
    rule.step(Start::Forcible, 0x4400, 0x5000);
    assert_eq!(
        rule.page(Start::Forcible, 0x5400),
        Some(false),
        "a moved row resets it"
    );
}

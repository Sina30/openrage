use std::io::Cursor;

use super::*;
use crate::container::rpf::{read_rpf9_body, write_rpf9_resource};
use crate::gta6::headerless::{read_dictionary, Entry};

/// A texture's levels, top first, as made-up `element`-byte elements `side` pixels across.
fn levels(width: u32, height: u32, side: u32, element: usize, mips: u32, seed: u8) -> Vec<Vec<u8>> {
    (0..mips)
        .map(|m| {
            let along = |pixels: u32| (pixels >> m).max(1).div_ceil(side) as usize;
            let n = along(width) * along(height) * element;
            let seed = seed.wrapping_add(m as u8);
            (0..n)
                .map(|i| (i as u8).wrapping_mul(31) ^ seed | 1)
                .collect()
        })
        .collect()
}

/// Three made-up textures: a 64x64 BC1 of one level, a 128x32 BC3 of three, a 16x16 RGBA of five.
fn three() -> (DictionaryModel, Vec<Vec<u8>>) {
    let made = [
        ("zq_alpha", 64, 64, 71, levels(64, 64, 4, 8, 1, 1)),
        ("zq_beta", 128, 32, 77, levels(128, 32, 4, 16, 3, 2)),
        ("zq_gamma", 16, 16, 28, levels(16, 16, 1, 4, 5, 3)),
    ];
    let textures = (made.iter())
        .map(|(name, w, h, format, levels)| ModelTexture::tiled(name, *w, *h, *format, levels))
        .collect::<Written<Vec<_>>>()
        .expect("each tiles");
    let pixels = made.iter().map(|m| m.4.concat()).collect();
    let model = DictionaryModel {
        textures,
        capacity: 3,
    };
    (model, pixels)
}

/// Where the record of `name` starts in V, and where its payload starts in P.
fn places(sections: &Rpf9Sections, name: &str) -> (usize, u64) {
    let v = &sections.virtual_section;
    let word = |at: usize| u64::from_le_bytes(v[at..at + 8].try_into().expect("8 bytes"));
    let entries = (word(0x30) - VIRTUAL) as usize;
    let count = u16::from_le_bytes([v[0x28], v[0x29]]) as usize;
    let codes = (word(0x20) - VIRTUAL) as usize;
    let key = name_hash32(name.as_bytes());
    let k = (0..count)
        .find(|k| v[codes + 4 * k..codes + 4 * k + 4] == key.to_le_bytes())
        .expect("its key");
    let record = (word(entries + 8 * k) - VIRTUAL) as usize;
    (record, word(record + 0x18) - PHYSICAL)
}

/// The block map's virtual and physical counts.
fn counts(sections: &Rpf9Sections) -> [u8; 2] {
    let v = &sections.virtual_section;
    let map = (u64::from_le_bytes(v[8..16].try_into().expect("8 bytes")) - VIRTUAL) as usize;
    [v[map + 8], v[map + 9]]
}

/// A texture read back: its name, size, format, levels and pixels.
type ReadBack = (String, [u16; 2], u8, u8, Vec<u8>);

/// Each texture `sections` read back to: its name, shape and pixels.
fn read_back(sections: &Rpf9Sections) -> Vec<ReadBack> {
    let dictionary = read_dictionary(&sections.virtual_section, Some(&sections.physical))
        .expect("the dictionary reads");
    (dictionary.entries.into_iter())
        .map(|(_, entry)| {
            let Ok(Entry::Texture(t)) = entry else {
                panic!("every entry is a texture");
            };
            let pixels = t.texture.expect("its pixels read").pixels;
            (t.name, [t.width, t.height], t.format, t.mip_levels, pixels)
        })
        .collect()
}

/// What a read at offset 0 of `sections` written as a loose resource returns.
fn loose(sections: &Rpf9Sections) -> Rpf9Sections {
    let file = write_rpf9_resource(sections).expect("written");
    let (find, bytes) = read_rpf9_body(Cursor::new(file), 0, u64::MAX).expect("it reads");
    Rpf9Sections::of(&find, &bytes).expect("a resource")
}

/// `sections` read back to their model, which plans again to the same bytes.
fn fixed_point(sections: &Rpf9Sections) -> DictionaryModel {
    let model = read_dictionary_model(&sections.virtual_section, &sections.physical)
        .expect("the model reads");
    assert!(
        write_dictionary(&model).expect("planned again") == *sections,
        "a dictionary read back plans to the bytes it was read from"
    );
    model
}

#[test]
fn the_block_rule_fills_the_head_first_and_grows_it_while_the_tail_passes_a_block() {
    // A head of one piece; what does not fit it goes to the tail, a later object to the head's gap.
    let objects = [(0x9000, PAGE), (0x8000, PAGE), (0x3000, PAGE)];
    let planned = (vec![0, 0x1_0000, 0x9000], 0x1_0000, 0x8000);
    assert_eq!(plan(&objects, 0), planned);
    // A tail of exactly one piece is never planned: the head grows and takes everything.
    let objects = [(0x1_8000, PAGE), (0x1_0000, PAGE), (0x1000, PAGE)];
    let planned = (vec![0, 0x1_8000, 0x2_8000], 0x3_0000, 0);
    assert_eq!(plan(&objects, 0), planned);
    let objects = [(0x1_4000, PAGE), (0x1_4000, PAGE)];
    assert_eq!(plan(&objects, 0), (vec![0, 0x1_4000], 0x3_0000, 0));
    // With no head the tail starts after the root.
    let planned = (vec![0x40, 0xB0], 0, 0xC1);
    assert_eq!(plan(&[(0x70, 16), (0x11, 8)], ROOT), planned);

    assert_eq!(blocks(0x2_0000, 0x8000).expect("two"), 2);
    assert_eq!(blocks(0, 0x1_0000).expect("one"), 1);
    assert!(matches!(
        blocks(0x2_0000, 0x1_0000),
        Err(DictionaryWriteError::WholePieceTail)
    ));
}

#[test]
fn a_planned_dictionary_lays_its_allocations_out_by_size_and_reads_back_to_its_model() {
    let (model, pixels) = three();
    let sections = write_dictionary(&model).expect("planned");
    // P: beta's 16 KiB first, then alpha's and gamma's 4 KiB in creation order, in one block.
    assert_eq!(sections.physical.len(), 0x6000);
    // V: the three records from the root's end, the 32-byte map, the 24-byte entries and the
    // 17-byte names of alpha and gamma, beta's 16-byte name and the 12-byte codes, padded to 16.
    assert_eq!(sections.virtual_section.len(), 0x220);
    let expected = [
        ("zq_alpha", 0x40, 0x4000),
        ("zq_beta", 0xB0, 0),
        ("zq_gamma", 0x120, 0x5000),
    ];
    for (name, record, payload) in expected {
        assert_eq!(places(&sections, name), (record, payload), "{name}");
    }
    assert_eq!(counts(&sections), [1, 1]);

    let read = read_back(&sections);
    let names: Vec<&str> = read.iter().map(|t| t.0.as_str()).collect();
    let mut ascending: Vec<&str> = model.textures.iter().map(|t| t.name.as_str()).collect();
    ascending.sort_by_key(|name| name_hash32(name.as_bytes()));
    assert_eq!(names, ascending, "the entries run in key order");
    for (texture, pixels) in model.textures.iter().zip(&pixels) {
        let held = read
            .iter()
            .find(|t| t.0 == texture.name)
            .expect("its entry");
        let shape = (
            [texture.width, texture.height],
            texture.format,
            texture.mip_levels,
        );
        assert_eq!((held.1, held.2, held.3), shape, "{}", texture.name);
        assert!(held.4 == *pixels, "{} reads back its levels", texture.name);
    }
    assert_eq!(fixed_point(&sections), model);
    assert!(
        loose(&sections) == sections,
        "the loose resource reads back to its sections"
    );
}

#[test]
fn a_texture_resized_reformatted_added_and_removed_is_planned_anew_and_reads_back() {
    let (mut model, pixels) = three();
    let wider = levels(256, 256, 4, 16, 9, 7);
    model.textures[1].flags = CPU_FLAGS;
    let reshaped = (model.textures[1].reshaped(256, 256, 98, &wider)).expect("BC7 tiles");
    model.insert(reshaped);
    assert!(model.remove("ZQ_ALPHA").is_some(), "a name of either case");
    let added = levels(32, 32, 1, 4, 1, 9);
    let delta = ModelTexture::tiled("zq_delta", 32, 32, 87, &added).expect("BGRA tiles");
    model.insert(delta);
    assert_eq!(model.capacity, 3);
    let names: Vec<&str> = model.textures.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["zq_beta", "zq_gamma", "zq_delta"],
        "a new texture is created last"
    );

    let sections = write_dictionary(&model).expect("planned");
    // 88 KiB, then two of 4 KiB: the 64 KiB head grows once its tail passes a block, and holds all.
    assert_eq!(sections.physical.len(), 0x2_0000);
    for (name, payload) in [
        ("zq_beta", 0),
        ("zq_gamma", 0x1_6000),
        ("zq_delta", 0x1_7000),
    ] {
        assert_eq!(places(&sections, name).1, payload, "{name}");
    }
    assert_eq!(counts(&sections), [1, 1]);
    let read = read_back(&sections);
    let beta = read.iter().find(|t| t.0 == "zq_beta").expect("beta");
    assert_eq!((beta.1, beta.2, beta.3), ([256, 256], 98, 9));
    assert!(beta.4 == wider.concat(), "beta reads back resized");
    let gamma = read.iter().find(|t| t.0 == "zq_gamma").expect("gamma");
    assert!(gamma.4 == pixels[2], "gamma's payload is carried");
    let delta = read.iter().find(|t| t.0 == "zq_delta").expect("delta");
    assert!(delta.4 == added.concat(), "delta reads back");

    let again = fixed_point(&sections);
    assert_eq!(again, model);
    assert_eq!(again.textures[0].flags, CPU_FLAGS, "the flags are carried");
    assert_eq!(again.textures[2].flags, FLAGS, "a new texture's flags");
    assert!(loose(&sections) == sections);
}

#[test]
fn a_virtual_section_past_one_piece_takes_a_second_block_and_a_map_that_counts_it() {
    let textures = (0..600)
        .map(|i| {
            let levels = levels(4, 4, 4, 8, 1, i as u8);
            ModelTexture::tiled(&format!("zq_{i:03}"), 4, 4, 71, &levels)
        })
        .collect::<Written<Vec<_>>>()
        .expect("each tiles");
    let model = DictionaryModel {
        textures,
        capacity: 600,
    };
    let sections = write_dictionary(&model).expect("planned");
    assert!(sections.virtual_section.len() > PIECE);
    assert_eq!(sections.physical.len(), 600 * PAGE);
    assert_eq!(counts(&sections), [2, 2]);
    let read = read_back(&sections);
    assert_eq!(read.len(), 600);
    for (i, texture) in model.textures.iter().enumerate() {
        let held = levels(4, 4, 4, 8, 1, i as u8).concat();
        let back = read
            .iter()
            .find(|t| t.0 == texture.name)
            .expect("its entry");
        assert!(back.4 == held, "{}", texture.name);
    }
    assert_eq!(fixed_point(&sections), model);
    assert!(loose(&sections) == sections);
}

#[test]
fn what_the_plan_does_not_cover_is_refused_by_name() {
    let refused = |edit: fn(&mut DictionaryModel)| {
        let mut model = three().0;
        edit(&mut model);
        write_dictionary(&model).map(|_| ())
    };
    assert!(matches!(
        refused(|m| m.textures.clear()),
        Err(DictionaryWriteError::Empty)
    ));
    assert!(matches!(
        refused(|m| m.capacity = 2),
        Err(DictionaryWriteError::Count {
            count: 3,
            capacity: 2
        })
    ));
    assert!(matches!(
        refused(|m| m.textures[1].name = "zq_alpha".into()),
        Err(DictionaryWriteError::SameKey { .. })
    ));
    for name in ["", "zq_Alpha", "zq_\u{e9}", "zq\0a"] {
        let mut model = three().0;
        model.textures[0].name = name.to_string();
        assert!(
            matches!(
                write_dictionary(&model),
                Err(DictionaryWriteError::Name { .. })
            ),
            "{name:?}"
        );
    }
    assert!(matches!(
        refused(|m| m.textures[0].format = 2),
        Err(DictionaryWriteError::Format { format: 2, .. })
    ));
    assert!(matches!(
        refused(|m| m.textures[0].flags = 0x984),
        Err(DictionaryWriteError::Flags { flags: 0x984, .. })
    ));
    assert!(matches!(
        refused(|m| m.textures[1].payload.truncate(0x3000)),
        Err(DictionaryWriteError::Payload {
            len: 0x3000,
            chain: 0x4000,
            ..
        })
    ));
    assert!(matches!(
        refused(|m| (m.textures[0].dimension, m.textures[0].depth) = (2, 5)),
        Err(DictionaryWriteError::Shape { .. })
    ));

    let pixels =
        |why: Written<ModelTexture>| matches!(why, Err(DictionaryWriteError::Pixels { .. }));
    let mut volume = three().0.textures.remove(0);
    volume.dimension = 3;
    assert!(pixels(volume.reshaped(4, 4, 71, &levels(4, 4, 4, 8, 1, 0))));
    // A one-level 16x16 of 2-byte elements lies in a tail slot no texture was seen to fill.
    assert!(pixels(ModelTexture::tiled(
        "zq_half",
        16,
        16,
        54,
        &levels(16, 16, 1, 2, 1, 0)
    )));
    assert!(pixels(ModelTexture::tiled(
        "zq_short",
        64,
        64,
        71,
        &[vec![0; 8]]
    )));
    assert!(pixels(ModelTexture::tiled("zq_none", 64, 64, 71, &[])));
}

/// Each slice's levels, for a call that takes them by slice.
fn by_slice(all: &[Vec<Vec<u8>>]) -> Vec<&[Vec<u8>]> {
    all.iter().map(Vec::as_slice).collect()
}

#[test]
fn an_array_and_a_cube_are_tiled_slice_after_slice_and_read_back_each_slice() {
    let slices = |count: u8, width, height, side, element, mips| -> Vec<Vec<Vec<u8>>> {
        (0..count)
            .map(|s| levels(width, height, side, element, mips, 10 * s + 1))
            .collect()
    };
    let one = slices(3, 16, 16, 4, 8, 1);
    let made = ModelTexture::tiled_slices("zq_array", 16, 16, 1, 71, &by_slice(&one));
    let array = slices(3, 64, 32, 4, 8, 4);
    let reshaped = (made
        .expect("tiles")
        .reshaped_slices(64, 32, 71, &by_slice(&array)))
    .expect("tiled anew");
    let shape = (reshaped.depth, reshaped.dimension, reshaped.mip_levels);
    assert_eq!(shape, (3, 1, 4));
    let faces = slices(6, 32, 32, 4, 16, 3);
    let cube = ModelTexture::tiled_slices("zq_cube", 32, 32, 2, 98, &by_slice(&faces));
    let model = DictionaryModel {
        textures: vec![reshaped, cube.expect("tiles")],
        capacity: 2,
    };
    let sections = write_dictionary(&model).expect("planned");
    let read = read_back(&sections);
    for (name, all) in [("zq_array", &array), ("zq_cube", &faces)] {
        let held = read.iter().find(|t| t.0 == name).expect("its entry");
        let want: Vec<u8> = all.iter().flatten().flatten().copied().collect();
        assert!(held.4 == want, "{name} reads back slice after slice");
    }
    assert_eq!(fixed_point(&sections), model);

    let refused =
        |why: Written<ModelTexture>| matches!(why, Err(DictionaryWriteError::Pixels { .. }));
    let fewer = model.textures[0].reshaped_slices(64, 32, 71, &by_slice(&array[..2]));
    assert!(refused(fewer), "a new shape keeps the count of slices");
    let mut volume = model.textures[0].clone();
    volume.dimension = 3;
    assert!(refused(volume.reshaped_slices(
        64,
        32,
        71,
        &by_slice(&array)
    )));
}

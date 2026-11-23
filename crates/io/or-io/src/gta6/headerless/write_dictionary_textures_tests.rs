use super::*;
use crate::gta6::headerless::DictionaryModel;

/// Made-up bytes, `n` of them, from `seed`.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(29) ^ seed | 1)
        .collect()
}

/// A dictionary of `textures`, planned and written.
fn dictionary(textures: Vec<ModelTexture>) -> Rpf9Sections {
    let capacity = textures.len() as u16;
    write_dictionary(&DictionaryModel { textures, capacity }).expect("planned")
}

/// A 64x64 BC1 texture of one level, "zq_paint".
fn paint() -> ModelTexture {
    ModelTexture::tiled("zq_paint", 64, 64, 71, &[pattern(16 * 16 * 8, 1)]).expect("it tiles")
}

/// A volume of one level, 8x16x8 RGBA, its one 4 KiB z-slab zero, "zq_fog".
fn fog(mip_levels: u8, payload: usize) -> ModelTexture {
    ModelTexture {
        name: "zq_fog".to_string(),
        width: 8,
        height: 16,
        depth: 8,
        dimension: 3,
        format: 28,
        mip_levels,
        flags: 0x980,
        byte_43: 0,
        byte_45: 0,
        payload: vec![0; payload],
    }
}

/// Every texture `sections` holds as a save writes it, as read.
fn held(sections: &Rpf9Sections) -> Vec<HeldTexture> {
    let read = read_dictionary(&sections.virtual_section, Some(&sections.physical))
        .expect("the dictionary reads");
    (read.entries.into_iter())
        .map(|(_, entry)| {
            let Ok(Entry::Texture(t)) = entry else {
                panic!("every entry is a texture");
            };
            let x = t.texture.expect("its pixels read");
            HeldTexture {
                name: t.name,
                width: t.width,
                height: t.height,
                format: x.format.format(),
                srgb: x.format.is_srgb(),
                mips: x.levels as u8,
                pixels: x.pixels,
            }
        })
        .collect()
}

#[test]
fn textures_of_their_own_shape_write_in_place_and_read_back_as_held() {
    let before = dictionary(vec![paint()]);
    let mut sections = before.clone();
    let mut textures = held(&sections);
    write_dictionary_textures(&mut sections, &textures).expect("nothing changed");
    assert_eq!(sections, before, "an unchanged texture writes nothing");
    textures[0].pixels = pattern(textures[0].pixels.len(), 7);
    write_dictionary_textures(&mut sections, &textures).expect("written in place");
    assert_eq!(sections.virtual_section, before.virtual_section);
    assert_eq!(held(&sections)[0].pixels, textures[0].pixels);
}

#[test]
fn a_texture_resized_added_and_removed_plans_both_sections_anew() {
    let mut sections = dictionary(vec![paint()]);
    let mut textures = held(&sections);
    textures[0].width = 32;
    textures[0].height = 32;
    textures[0].pixels = pattern(8 * 8 * 8, 3);
    let mut added = textures[0].clone();
    added.name = "ZQ_New".to_string();
    textures.push(added);
    write_dictionary_textures(&mut sections, &textures).expect("planned anew");
    let back = held(&sections);
    let named = |name: &str| back.iter().find(|t| t.name == name).expect("held").clone();
    assert_eq!(back.len(), 2);
    let (paint, new) = (named("zq_paint"), named("zq_new"));
    assert_eq!((paint.width, paint.height), (32, 32));
    assert_eq!(paint.pixels, textures[0].pixels);
    assert_eq!(new.pixels, textures[1].pixels);
    write_dictionary_textures(&mut sections, &[new]).expect("one removed");
    let left = held(&sections);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "zq_new");
}

#[test]
fn a_held_texture_of_no_dictionary_format_or_short_pixels_is_refused_by_kind() {
    let sections = dictionary(vec![paint()]);
    let mut texture = held(&sections).remove(0);
    texture.pixels.truncate(8);
    assert!(matches!(
        texture.tiled(None),
        Err(TexturePlanError::Unread { .. })
    ));
    texture.format = GrcTextureFormat::None;
    assert!(matches!(
        texture.tiled(None),
        Err(TexturePlanError::Format { .. })
    ));
}

#[test]
fn a_volume_writes_each_level_s_planes_from_the_first_slice_that_shows_them() {
    let mut sections = dictionary(vec![fog(1, 0x1000)]);
    let mut textures = held(&sections);
    assert_eq!(textures[0].pixels.len(), 8 * 16 * 8 * 4);
    textures[0].pixels = pattern(textures[0].pixels.len(), 5);
    write_dictionary_textures(&mut sections, &textures).expect("the planes are written");
    assert_eq!(held(&sections)[0].pixels, textures[0].pixels);

    // Level 1's 4x8x4 lies in the tail tile after level 0's slab: each of the 8 slices holds a
    // plane of level 0 and the plane `s >> 1` of level 1.
    let mut sections = dictionary(vec![fog(2, 0x2000)]);
    let mut textures = held(&sections);
    let (level0, level1) = (8 * 16 * 4, 4 * 8 * 4);
    let each = level0 + level1;
    assert_eq!(textures[0].pixels.len(), 8 * each);
    let written = pattern(8 * each, 9);
    textures[0].pixels = written.clone();
    write_dictionary_textures(&mut sections, &textures).expect("every level's planes are written");
    let want: Vec<u8> = (0..8)
        .flat_map(|s| {
            let from = (s & !1) * each + level0;
            let own = &written[s * each..s * each + level0];
            [own, &written[from..from + level1]].concat()
        })
        .collect();
    let back = held(&sections).remove(0);
    assert_eq!(back.mips, 2);
    assert!(
        back.pixels == want,
        "slice s shows level 1 as slice s & !1 gave it"
    );
    let mut again = sections.clone();
    write_dictionary_textures(&mut again, &[back]).expect("nothing changed");
    assert_eq!(again, sections, "what reads back writes nothing");

    // Pixels of another length are refused, and nothing is written.
    let (v, p) = (&sections.virtual_section, &mut sections.physical);
    let why = write_volume_texture(v, p, "zq_fog", &pattern(8 * each - 4, 3)).expect_err("short");
    assert!(why.to_string().contains("planes"), "{why}");
}

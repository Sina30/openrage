//! What the header words of the staged textures hold, which is why a source names one bit of one
//! word and states the rest raw ([the header words](../../../docs/direction/Source_Format_Design.md#what-is-on-disk)).

mod common;

use std::collections::BTreeMap;

use common::{enhanced, legacy};
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::gta5::formats::format;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::Generation;

#[test]
fn the_words_no_declaration_names_a_bit_of_are_zero_in_every_shipped_texture() {
    let Some(files) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ytd")
        .all_targets()
        .find_or_skip("the header words of every corpus texture")
    else {
        return;
    };
    let ytd = format("ytd").unwrap();
    let (mut legacy_textures, mut enhanced_textures) = (0, 0);
    let mut conversion: BTreeMap<u8, usize> = BTreeMap::new();
    let mut tile_modes: BTreeMap<u8, usize> = BTreeMap::new();
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(version) = Rsc7::peek(&bytes).map(|r| r.version()) else {
            continue;
        };
        // A version no layout declares is another suite's to count.
        let Ok(target) = ytd.target_of_version(version) else {
            continue;
        };
        if target == Generation::Gen9 {
            for t in enhanced(&bytes).entries.iter().flatten() {
                enhanced_textures += 1;
                assert_eq!(t.anti_alias_type, 0, "{file}: {}", t.name);
                *tile_modes.entry(t.tile_mode.raw()).or_default() += 1;
            }
        } else {
            for t in legacy(&bytes).entries.iter().flatten() {
                legacy_textures += 1;
                let at = format!("{file}: {}", t.name);
                assert_eq!(t.extra_flags, 0, "{at}");
                assert_eq!(t.handle_index, 0, "{at}");
                assert_eq!(t.info_bits & !1, 0, "{at}: info bits past sRGB");
                assert_eq!(t.stereo_rt_mode, 0, "{at}");
                *conversion
                    .entry(t.resource_type_and_conversion_flags)
                    .or_default() += 1;
            }
        }
    }
    println!(
        "{legacy_textures} Legacy textures state the conversion word as {conversion:#x?}, and \
         {enhanced_textures} Enhanced ones state the tile modes {tile_modes:#x?}"
    );
    assert!(legacy_textures > 0);
    // The one bit the engine's declaration names is in most of them.
    assert!(conversion
        .get(&PROCESSED)
        .is_some_and(|n| *n * 2 > legacy_textures));
}

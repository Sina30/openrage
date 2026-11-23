//! Optimising a textures source: each change is held to a value computed another way, and a texture that needs nothing is left
//! byte for byte.

mod common;

use std::path::Path;

use common::*;
use or_io::authoring::TextureBuildOptions;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage, TextureSource};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::gta5::PgDictionary;
use or_source::optimise::{apply, plan, How, Options, Scale};
use serde_json::json;

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;
const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const BC7: GrcTextureFormat = GrcTextureFormat::BC7;
const BGRA: GrcTextureFormat = GrcTextureFormat::A8R8G8B8;

/// What one texture of a test dictionary is made of: how many mips it stores, and whether its sides need be powers of two.
struct Made {
    name: &'static str,
    format: GrcTextureFormat,
    image: RgbaImage,
    mips: u32,
}

fn made(name: &'static str, format: GrcTextureFormat, image: RgbaImage, mips: u32) -> Made {
    Made {
        name,
        format,
        image,
        mips,
    }
}

/// A Legacy dictionary whose textures store exactly the mips asked for, at any size.
fn dictionary(textures: &[Made]) -> Vec<u8> {
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    for (i, made) in textures.iter().enumerate() {
        let mut texture = GrcTexture {
            name: made.name.to_string(),
            resource_type_and_conversion_flags: PROCESSED,
            extra_flags: 0x10 + i as u32,
            handle_index: 7 + i as u32,
            ..Default::default()
        };
        let options = TextureBuildOptions {
            mip_levels: Some(made.mips),
            allow_npot: true,
            ..TextureBuildOptions::default()
        };
        texture
            .set_pixels(made.format, &made.image, &options)
            .unwrap();
        dictionary.insert(texture);
    }
    dictionary.to_bytes().unwrap()
}

fn dds_of(folder: &Path, name: &str) -> Vec<u8> {
    std::fs::read(folder.join("textures").join(format!("{name}.dds"))).unwrap()
}

fn rules_of(folder: &Path) -> Vec<u8> {
    std::fs::read(folder.join("textures.json")).unwrap()
}

/// What the folder compiles to, as the texture `name` of its dictionary.
fn compiled(folder: &Path, name: &str) -> GrcTexture {
    legacy(&compile(folder))
        .get(name)
        .unwrap_or_else(|| panic!("no {name}"))
        .clone()
}

fn nothing_stops() -> bool {
    false
}

#[test]
fn a_texture_that_needs_nothing_is_left_byte_for_byte() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("fine", DXT5, ramp(128, 128, 1), 6)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let (dds, rules, before) = (dds_of(&folder, "fine"), rules_of(&folder), compile(&folder));

    let found = plan(&folder, &Options::safe(true)).unwrap();
    assert!(
        found.changes.is_empty() && found.left.is_empty(),
        "{found:?}"
    );
    let done = apply(&folder, &Options::safe(true), &nothing_stops).unwrap();

    assert!(done.changes.is_empty());
    assert_eq!(dds_of(&folder, "fine"), dds);
    assert_eq!(rules_of(&folder), rules);
    assert_eq!(compile(&folder), before);
}

#[test]
fn a_texture_that_stores_one_mip_gets_its_chain_and_keeps_the_blocks_it_had() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[
        made("flat", DXT5, ramp(64, 64, 2), 1),
        made("control", DXT1, ramp(64, 64, 3), 5),
    ]);
    let original = legacy(&ytd);
    let level_zero = mips(original.get("flat").unwrap())[0].clone();
    let folder = extract(scratch.path(), "pack", &ytd);
    let control = dds_of(&folder, "control");

    let found = plan(&folder, &Options::safe(true)).unwrap();
    assert_eq!(found.changes.len(), 1, "{found:?}");
    assert_eq!(found.changes[0].texture, "flat");
    assert_eq!(found.changes[0].how, How::Mips);
    let done = apply(&folder, &Options::safe(true), &nothing_stops).unwrap();
    assert_eq!(done.changes.len(), 1);

    let texture = compiled(&folder, "flat");
    // 64, 32, 16, 8 and 4: where shipped textures stop.
    assert_eq!(texture.stored_levels(), 5);
    assert_eq!(texture.stored_size(), (64, 64));
    let stored = mips(&texture);
    assert_eq!(
        stored[0], level_zero,
        "the blocks it had are the blocks it has"
    );
    for (level, bytes) in stored.iter().enumerate() {
        let side = 64 >> level;
        assert_eq!(
            bytes.len(),
            DXT5.mip_size(side, side) as usize,
            "level {level}"
        );
    }
    assert_eq!(
        dds_of(&folder, "control"),
        control,
        "a texture that needed nothing is not touched"
    );
    // A second run finds nothing to do and changes no byte.
    let (dds, rules) = (dds_of(&folder, "flat"), rules_of(&folder));
    assert!(plan(&folder, &Options::safe(true))
        .unwrap()
        .changes
        .is_empty());
    apply(&folder, &Options::safe(true), &nothing_stops).unwrap();
    assert_eq!((dds_of(&folder, "flat"), rules_of(&folder)), (dds, rules));
}

#[test]
fn a_side_that_is_not_a_power_of_two_goes_to_the_nearest() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("odd", DXT1, ramp(100, 60, 4), 1)]);
    let folder = extract(scratch.path(), "pack", &ytd);

    let found = plan(&folder, &Options::safe(false)).unwrap();
    assert_eq!(found.changes.len(), 1, "{found:?}");
    assert_eq!(found.changes[0].how, How::Rebuilt);
    apply(&folder, &Options::safe(false), &nothing_stops).unwrap();

    let texture = compiled(&folder, "odd");
    // 100 is nearer 128 than 64 in ratio, 60 is nearer 64 than 128; the chain stops at 4 on the shorter side.
    assert_eq!(texture.stored_size(), (128, 64));
    assert_eq!(texture.stored_levels(), 5);
    assert_eq!(texture.pixel_format(), DXT1);
    assert!(plan(&folder, &Options::safe(false))
        .unwrap()
        .changes
        .is_empty());
}

#[test]
fn an_uncompressed_texture_is_stored_as_bc1_when_it_is_opaque_and_bc3_when_it_is_not() {
    let scratch = tempfile::tempdir().unwrap();
    let mut opaque = ramp(64, 64, 5);
    for texel in opaque.data.as_chunks_mut::<4>().0 {
        texel[3] = 255;
    }
    let ytd = dictionary(&[
        made("solid", BGRA, opaque, 7),
        made("seethrough", BGRA, quadrants(64, 64), 7),
    ]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let before = (
        dds_of(&folder, "solid").len(),
        dds_of(&folder, "seethrough").len(),
    );

    apply(&folder, &Options::safe(true), &nothing_stops).unwrap();

    assert_eq!(compiled(&folder, "solid").pixel_format(), DXT1);
    assert_eq!(compiled(&folder, "seethrough").pixel_format(), DXT5);
    assert!(
        dds_of(&folder, "solid").len() < before.0,
        "an eighth of the bytes of the first"
    );
    assert!(dds_of(&folder, "seethrough").len() < before.1);
    assert_eq!(
        compiled(&folder, "solid").stored_levels(),
        7,
        "a chain that is there stays as long"
    );
}

#[test]
fn bc7_is_replaced_where_the_platform_cannot_read_it_and_only_there() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("fancy", BC7, ramp(64, 64, 6), 5)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let bytes = dds_of(&folder, "fancy");

    assert!(
        plan(&folder, &Options::safe(false))
            .unwrap()
            .changes
            .is_empty(),
        "Enhanced reads BC7"
    );
    assert_eq!(dds_of(&folder, "fancy"), bytes);

    // Asked to keep the format, it writes no BC7 in a pack built for Legacy: the texture is left, with the way to have it stored as BC1 or BC3.
    let keep_the_format = Options {
        format: false,
        scale: Scale::Ratio(0.5),
        ..Options::safe(true)
    };
    let found = plan(&folder, &keep_the_format).unwrap();
    assert!(found.changes.is_empty(), "{found:?}");
    assert!(
        found.left[0].why.contains("DirectX 10"),
        "{:?}",
        found.left[0]
    );

    apply(&folder, &Options::safe(true), &nothing_stops).unwrap();
    assert!(matches!(
        compiled(&folder, "fancy").pixel_format(),
        DXT1 | DXT5
    ));
}

#[test]
fn a_ratio_or_a_side_drops_the_top_levels_and_keeps_the_others_as_they_were() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("big", DXT5, ramp(256, 256, 7), 7)]);
    let original = mips(legacy(&ytd).get("big").unwrap());
    for (scale, dropped) in [(Scale::Ratio(0.5), 1usize), (Scale::Side(64), 2)] {
        let folder = extract(&scratch.path().join(format!("{dropped}")), "pack", &ytd);
        let options = Options {
            scale,
            ..Options::safe(true)
        };
        let found = plan(&folder, &options).unwrap();
        assert_eq!(
            found.changes[0].how,
            How::Halved(dropped as u32),
            "{found:?}"
        );
        apply(&folder, &options, &nothing_stops).unwrap();

        let texture = compiled(&folder, "big");
        assert_eq!(texture.stored_size(), (256 >> dropped, 256 >> dropped));
        assert_eq!(texture.stored_levels() as usize, 7 - dropped);
        assert_eq!(
            mips(&texture),
            original[dropped..],
            "the levels it keeps are the levels it had, block for block"
        );
    }
}

#[test]
fn a_rule_that_states_the_format_the_texture_had_is_brought_in_line() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("stated", BGRA, quadrants(64, 64), 7)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    edit_rules(&folder, |rules| {
        entry(rules, "stated")["encode"] = json!({ "format": "A8R8G8B8", "mips": "to-1x1" });
    });
    compile(&folder);

    apply(&folder, &Options::safe(true), &nothing_stops).unwrap();

    assert_eq!(compiled(&folder, "stated").pixel_format(), DXT5);
    let text = String::from_utf8(rules_of(&folder)).unwrap();
    assert!(
        text.contains("DXT5") && !text.contains("A8R8G8B8"),
        "{text}"
    );
}

#[test]
fn a_texture_made_from_a_picture_is_left_and_said_so() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("drawn", DXT5, ramp(64, 64, 8), 1)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    save_image(&folder.join("textures/drawn.png"), &ramp(64, 64, 8));
    let rules = rules_of(&folder);

    let found = plan(&folder, &Options::safe(true)).unwrap();
    assert!(found.changes.is_empty(), "{found:?}");
    assert_eq!(found.left.len(), 1);
    assert!(found.left[0].why.contains("picture"), "{:?}", found.left[0]);
    apply(&folder, &Options::safe(true), &nothing_stops).unwrap();
    assert_eq!(rules_of(&folder), rules);
}

#[test]
fn a_texture_packed_from_pictures_is_one_texture_left_and_its_pictures_are_no_textures_of_theirs() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("plain", DXT5, ramp(64, 64, 2), 1)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    for name in ["red.png", "grey.png"] {
        save_image(&folder.join("textures").join(name), &ramp(32, 32, 3));
    }
    edit_rules(&folder, |rules| {
        rules["textures"].as_array_mut().unwrap().push(json!({
            "name": "spec",
            "pack": {
                "r": { "file": "red.png", "channel": "r" },
                "g": { "file": "grey.png", "channel": "g" }
            }
        }));
    });

    let found = plan(&folder, &Options::safe(true)).unwrap();

    assert_eq!(found.changes.len(), 1, "{found:?}");
    assert_eq!(found.changes[0].texture, "plain");
    assert_eq!(found.left.len(), 1, "{found:?}");
    assert_eq!(found.left[0].texture, "spec");
}

#[test]
fn an_enhanced_source_is_optimised_and_compiles_for_enhanced() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = enhanced_dictionary(&[Spec::new("odd", DXT5, ramp(64, 64, 9))]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let options = Options {
        scale: Scale::Ratio(0.5),
        ..Options::safe(false)
    };

    let done = apply(&folder, &options, &nothing_stops).unwrap();

    assert_eq!(done.changes.len(), 1, "{done:?}");
    let reloaded = enhanced(&compile(&folder));
    let texture = reloaded
        .entries
        .iter()
        .flatten()
        .find(|t| t.name == "odd")
        .expect("the texture");
    assert_eq!((texture.width, texture.height), (32, 32));
}

#[test]
fn options_that_cannot_be_carried_out_are_refused_and_a_stop_writes_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = dictionary(&[made("flat", DXT5, ramp(64, 64, 2), 1)]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let (dds, rules) = (dds_of(&folder, "flat"), rules_of(&folder));

    for scale in [Scale::Ratio(1.5), Scale::Ratio(0.0), Scale::Side(2)] {
        let options = Options {
            scale,
            ..Options::safe(true)
        };
        assert!(plan(&folder, &options).is_err(), "{scale:?}");
    }
    assert!(apply(&folder, &Options::safe(true), &|| true).is_err());
    assert_eq!((dds_of(&folder, "flat"), rules_of(&folder)), (dds, rules));
}

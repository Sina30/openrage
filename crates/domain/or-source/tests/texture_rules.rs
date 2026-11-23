//! What `textures.json` asks for beyond the defaults, each held to a value computed another way:
//! channels packed from separate images, only the changed blocks of a DDS re-encoded, and BC7.

mod common;

use std::fs;
use std::path::Path;

use common::*;
use or_io::authoring::{
    encode_texture_with_options, rgba_psnr, EncodeQuality, TextureBuildOptions,
};
use or_io::codec::texture::{decode_level, GrcTextureFormat, MipFilter, RgbaImage, TextureSource};
use serde_json::{json, Value};

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;
const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const DXN: GrcTextureFormat = GrcTextureFormat::DXN;
const BC7: GrcTextureFormat = GrcTextureFormat::BC7;
const BGRA: GrcTextureFormat = GrcTextureFormat::A8R8G8B8;
const L8: GrcTextureFormat = GrcTextureFormat::L8;

/// What a compile is asked for when a rule says nothing: box filter, normal quality, a linear DDS.
fn options(levels: u32, srgb: bool) -> TextureBuildOptions {
    TextureBuildOptions {
        mip_levels: Some(levels),
        filter: MipFilter::Box,
        srgb,
        quality: EncodeQuality::Normal,
        allow_npot: false,
        ..TextureBuildOptions::default()
    }
}

/// The mips a texture of `name` holds, one `Vec` per level.
fn texture_mips(bytes: &[u8], name: &str) -> Vec<Vec<u8>> {
    mips(
        legacy(bytes)
            .get(name)
            .unwrap_or_else(|| panic!("no {name}")),
    )
}

/// Saves the exact decode of mip 0 beside the DDS, as an editor would write an image it never touched.
fn export(folder: &Path, dictionary: &[u8], name: &str) -> RgbaImage {
    let image = level(dictionary, name, 0);
    save_image(&folder.join("textures").join(format!("{name}.png")), &image);
    image
}

/// `image` painted with `colour` over `rect`, which is (x0, y0, x1, y1) and half open.
fn painted(image: &RgbaImage, rect: (u32, u32, u32, u32), colour: [u8; 4]) -> RgbaImage {
    let mut out = image.clone();
    for y in rect.1..rect.3 {
        for x in rect.0..rect.2 {
            out.set_pixel(x, y, colour);
        }
    }
    out
}

#[test]
fn a_save_with_no_change_changes_no_byte() {
    for format in [DXT1, DXT5, DXN, BC7, BGRA, L8] {
        let scratch = tempfile::tempdir().unwrap();
        let original = legacy_dictionary(&[Spec::new("art", format, ramp(64, 64, 3))]);
        let folder = extract(scratch.path(), "pack", &original);
        let from_dds = compile(&folder);
        export(&folder, &original, "art");
        assert_eq!(
            compile(&folder),
            from_dds,
            "{format:?}: an image nobody touched changed bytes"
        );
    }
}

#[test]
fn an_enhanced_texture_saved_with_no_change_changes_no_byte() {
    let scratch = tempfile::tempdir().unwrap();
    let original = enhanced_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    let from_dds = compile(&folder);
    let dictionary = enhanced(&original);
    let texture = dictionary.entries.iter().flatten().next().unwrap();
    let image = decode_level(&**texture, 0).unwrap();
    save_image(&folder.join("textures/art.png"), &image);
    assert_eq!(compile(&folder), from_dds);
}

/// The blocks of `level` that `rect` of level 0 reaches, by the geometry of halving and not by comparing pixels.
fn reached(
    format: GrcTextureFormat,
    side: u32,
    level: u32,
    rect: (u32, u32, u32, u32),
) -> Vec<(u32, u32)> {
    let edge = (side >> level).max(1);
    let block = format.block_size().max(1);
    let across = edge.div_ceil(block);
    let span = |b: u32| {
        // The level-0 texels a block of this level was made from.
        ((b * block) << level, (((b + 1) * block).min(edge)) << level)
    };
    let overlaps = |(a0, a1): (u32, u32), (b0, b1): (u32, u32)| a0 < b1 && b0 < a1;
    let mut blocks = Vec::new();
    for by in 0..across {
        for bx in 0..across {
            // A level narrower than one block is a single block that holds all of the image.
            let whole = block > 1 && edge < block;
            if whole
                || (overlaps(span(bx), (rect.0, rect.2)) && overlaps(span(by), (rect.1, rect.3)))
            {
                blocks.push((bx, by));
            }
        }
    }
    blocks
}

#[test]
fn an_edit_changes_only_the_blocks_it_covers() {
    let rect = (36, 52, 56, 64);
    for format in [DXT1, DXT5, DXN, BC7, BGRA] {
        let scratch = tempfile::tempdir().unwrap();
        let side = 128u32;
        let original = legacy_dictionary(&[Spec::new("art", format, ramp(side, side, 3))]);
        let folder = extract(scratch.path(), "pack", &original);
        let was = texture_mips(&original, "art");
        let edited = painted(&level(&original, "art", 0), rect, [10, 200, 30, 255]);
        save_image(&folder.join("textures/art.png"), &edited);
        let rebuilt = texture_mips(&compile(&folder), "art");

        // What a full encode of the edited image makes of every block; the blocks the edit reaches take that.
        let levels = was.len() as u32;
        let full =
            encode_texture_with_options(format, side, side, &edited, &options(levels, false))
                .unwrap();
        let mut expected = Vec::new();
        let mut untouched = 0usize;
        for (l, bytes) in was.iter().enumerate() {
            let range = format.mip_level_range(side, side, l as u32);
            let (edge, block) = ((side >> l).max(1), format.block_size().max(1));
            let across = edge.div_ceil(block);
            let size = format.bytes_per_block() as usize;
            let mut out = bytes.clone();
            let touched = reached(format, side, l as u32, rect);
            for (bx, by) in &touched {
                let at = ((by * across + bx) as usize) * size;
                out[at..at + size].copy_from_slice(&full.data[range.clone()][at..at + size]);
            }
            untouched += (across * across) as usize - touched.len();
            expected.push(out);
        }
        assert_eq!(rebuilt, expected, "{format:?}");
        assert!(untouched > 0, "{format:?}: the edit reached every block");
        assert_ne!(rebuilt, was, "{format:?}: the edit changed nothing");
    }
}

#[test]
fn a_wider_filter_widens_what_an_edit_reaches_and_leaves_a_distant_block_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(128, 128, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    let edited = painted(
        &level(&original, "art", 0),
        (100, 100, 108, 108),
        [250, 10, 10, 255],
    );
    save_image(&folder.join("textures/art.png"), &edited);
    edit_rules(&folder, |rules| {
        entry(rules, "art")["encode"] = json!({"filter": "lanczos3"});
    });
    let was = texture_mips(&original, "art");
    let rebuilt = texture_mips(&compile(&folder), "art");
    // The first block of each of the first two levels is a long way from the edit, whatever the filter.
    for l in 0..2 {
        assert_eq!(rebuilt[l][..16], was[l][..16], "level {l}");
    }
    assert_ne!(rebuilt, was);
}

#[test]
fn a_rule_the_dds_cannot_honour_is_a_full_encode_of_the_image() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let edited = painted(
        &level(&original, "art", 0),
        (8, 8, 24, 24),
        [10, 200, 30, 255],
    );
    let build = |rule: Value| {
        let folder = extract(scratch.path(), "pack", &original);
        save_image(&folder.join("textures/art.png"), &edited);
        edit_rules(&folder, |rules| entry(rules, "art")["encode"] = rule);
        let bytes = compile(&folder);
        fs::remove_dir_all(folder).unwrap();
        bytes
    };

    // A format the DDS is not in: every block is encoded, and the mips follow the default.
    let bytes = build(json!({"format": "DXT1"}));
    let full = encode_texture_with_options(DXT1, 64, 64, &edited, &options(5, false)).unwrap();
    assert_eq!(texture_mips(&bytes, "art").concat(), full.data);
    assert_eq!(legacy(&bytes).get("art").unwrap().pixel_format(), DXT1);

    // A mip count the DDS does not hold: the DDS has seven levels and the rule says five.
    let bytes = build(json!({"mips": "to-4x4"}));
    let full = encode_texture_with_options(DXT5, 64, 64, &edited, &options(5, false)).unwrap();
    assert_eq!(texture_mips(&bytes, "art").concat(), full.data);

    // The same count as the DDS holds is no reason to throw its blocks away.
    let bytes = build(json!({"mips": "to-1x1"}));
    let kept = texture_mips(&bytes, "art");
    let was = texture_mips(&original, "art");
    assert_eq!(kept.len(), was.len());
    assert_eq!(
        kept[0][..16],
        was[0][..16],
        "a block the edit never reached"
    );
}

#[test]
fn a_colour_space_rule_on_an_untouched_image_changes_the_flag_and_no_block() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    export(&folder, &original, "art");
    edit_rules(&folder, |rules| {
        entry(rules, "art")["encode"] = json!({"colour_space": "srgb"});
    });
    let bytes = compile(&folder);
    assert_eq!(texture_mips(&bytes, "art"), texture_mips(&original, "art"));
    assert_eq!(legacy(&bytes).get("art").unwrap().info_bits & 1, 1);
}

/// A smooth ramp that block compression holds to a high PSNR.
fn gradient(side: u32) -> RgbaImage {
    let mut image = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let v = |k: u32| ((x * k + y * (5 - k)) * 255 / (side * 5)) as u8;
            image.set_pixel(x, y, [v(1), v(2), v(3), 255]);
        }
    }
    image
}

#[test]
fn a_texture_that_asks_for_bc7_compiles_to_bc7_in_every_build() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(
        scratch.path(),
        "pack",
        &legacy_dictionary(&[Spec::new("seed", DXT1, ramp(16, 16, 1))]),
    );
    let image = gradient(64);
    save_image(&folder.join("textures/smooth.png"), &image);
    edit_rules(&folder, |rules| {
        rules["textures"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "smooth", "encode": {"format": "BC7", "colour_space": "linear"}}));
    });
    let bytes = compile(&folder);
    let dictionary = legacy(&bytes);
    let smooth = dictionary.get("smooth").unwrap();
    assert_eq!(smooth.pixel_format(), BC7);
    let psnr = rgba_psnr(&decode_level(smooth, 0).unwrap(), &image).unwrap();
    assert!(psnr.overall >= 40.0, "BC7 held {} dB", psnr.overall);
}

#[test]
fn a_bc7_dds_edited_in_one_corner_keeps_every_other_block() {
    let scratch = tempfile::tempdir().unwrap();
    let side = 64;
    let original = legacy_dictionary(&[Spec::new("art", BC7, ramp(side, side, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    let edited = painted(
        &level(&original, "art", 0),
        (0, 0, 8, 8),
        [200, 30, 90, 255],
    );
    save_image(&folder.join("textures/art.png"), &edited);
    let was = texture_mips(&original, "art");
    let rebuilt = texture_mips(&compile(&folder), "art");
    let across = side / 4;
    let size = BC7.bytes_per_block() as usize;
    let corner = |bx: u32, by: u32| ((by * across + bx) as usize) * size;
    // Blocks (0, 0) to (1, 1) were painted: the four of them differ, and the one beside them does not.
    assert_ne!(
        rebuilt[0][corner(0, 0)..][..size],
        was[0][corner(0, 0)..][..size]
    );
    assert_eq!(
        rebuilt[0][corner(2, 0)..][..size],
        was[0][corner(2, 0)..][..size]
    );
    assert_eq!(
        rebuilt[0][corner(0, 2)..][..size],
        was[0][corner(0, 2)..][..size]
    );
    assert_eq!(
        rebuilt[0][corner(8, 8)..][..size],
        was[0][corner(8, 8)..][..size]
    );
}

/// Three images saved in three formats, each channel of each different from the others.
fn three_inputs(folder: &Path) -> [RgbaImage; 3] {
    let (a, b, c) = (ramp(64, 64, 1), ramp(64, 64, 2), ramp(64, 64, 3));
    let rgba = |i: &RgbaImage| image::RgbaImage::from_raw(64, 64, i.data.clone()).unwrap();
    save_image(&folder.join("textures/intensity.png"), &a);
    rgba(&b).save(folder.join("textures/falloff.tga")).unwrap();
    rgba(&c).save(folder.join("textures/fresnel.tif")).unwrap();
    [a, b, c]
}

/// The image the three inputs make when each output channel takes one channel of one input and the rest keep their default.
fn by_hand(parts: [Option<(&RgbaImage, usize)>; 4]) -> RgbaImage {
    let mut out = RgbaImage::new(64, 64);
    for i in 0..64 * 64 {
        for (channel, part) in parts.iter().enumerate() {
            out.data[i * 4 + channel] = match part {
                Some((image, from)) => image.data[i * 4 + from],
                None if channel == 3 => 255,
                None => 0,
            };
        }
    }
    out
}

fn new_source(scratch: &Path) -> std::path::PathBuf {
    extract(
        scratch,
        "pack",
        &legacy_dictionary(&[Spec::new("other", DXT1, ramp(16, 16, 9))]),
    )
}

fn add_entry(folder: &Path, entry: Value) {
    edit_rules(folder, |rules| {
        rules["textures"].as_array_mut().unwrap().push(entry)
    });
}

#[test]
fn a_packed_specular_map_equals_the_one_built_by_hand_from_the_same_three_images() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = new_source(scratch.path());
    let [a, b, c] = three_inputs(&folder);
    add_entry(
        &folder,
        json!({
            "name": "spec",
            "pack": {
                "r": {"file": "intensity.png", "channel": "r"},
                "g": {"file": "falloff.tga", "channel": "g"},
                "b": {"file": "fresnel.tif", "channel": "b"}
            },
            "encode": {"format": "DXT1"}
        }),
    );
    let bytes = compile(&folder);

    let hand = by_hand([Some((&a, 0)), Some((&b, 1)), Some((&c, 2)), None]);
    let expected = encode_texture_with_options(DXT1, 64, 64, &hand, &options(5, false)).unwrap();
    assert_eq!(texture_mips(&bytes, "spec").concat(), expected.data);
    let dictionary = legacy(&bytes);
    let spec = dictionary.get("spec").unwrap();
    assert_eq!(spec.pixel_format(), DXT1);
    assert_eq!(
        spec.info_bits & 1,
        0,
        "a packed map holds measures, not colour"
    );
    // The three images are inputs of `spec`, and none of them is a texture of its own.
    assert_eq!(dictionary.entries.iter().flatten().count(), 2);
}

#[test]
fn a_pack_may_name_alpha_and_leaves_what_it_does_not_name_at_its_default() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = new_source(scratch.path());
    let [a, _, c] = three_inputs(&folder);
    add_entry(
        &folder,
        json!({
            "name": "mask",
            "pack": {
                "g": {"file": "intensity.png", "channel": "b"},
                "a": {"file": "fresnel.tif", "channel": "a"}
            },
            "encode": {"format": "DXT5"}
        }),
    );
    let bytes = compile(&folder);
    let hand = by_hand([None, Some((&a, 2)), None, Some((&c, 3))]);
    let expected = encode_texture_with_options(DXT5, 64, 64, &hand, &options(5, false)).unwrap();
    assert_eq!(texture_mips(&bytes, "mask").concat(), expected.data);
}

#[test]
fn a_pack_follows_the_format_and_the_colour_space_of_the_dds_beside_it() {
    let scratch = tempfile::tempdir().unwrap();
    let mut base = Spec::new("spec", DXT5, ramp(64, 64, 5));
    base.srgb = true;
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&[base]));
    let [a, b, c] = three_inputs(&folder);
    edit_rules(&folder, |rules| {
        entry(rules, "spec")["pack"] = json!({
            "r": {"file": "intensity.png", "channel": "r"},
            "g": {"file": "falloff.tga", "channel": "r"},
            "b": {"file": "fresnel.tif", "channel": "r"}
        });
    });
    let bytes = compile(&folder);
    let hand = by_hand([Some((&a, 0)), Some((&b, 0)), Some((&c, 0)), None]);
    // The DDS has seven levels, and an image made to its size keeps that count.
    let expected = encode_texture_with_options(DXT5, 64, 64, &hand, &options(7, true)).unwrap();
    assert_eq!(texture_mips(&bytes, "spec").concat(), expected.data);
    assert_eq!(legacy(&bytes).get("spec").unwrap().info_bits & 1, 1);
}

#[test]
fn a_pack_that_cannot_be_built_is_refused_by_name_and_nothing_is_guessed() {
    let scratch = tempfile::tempdir().unwrap();
    let said = |pack: Value, extra: Value| {
        let folder = new_source(scratch.path());
        three_inputs(&folder);
        save_image(&folder.join("textures/small.png"), &ramp(32, 32, 4));
        let mut entry = json!({"name": "spec", "pack": pack});
        for (key, value) in extra.as_object().unwrap() {
            entry[key] = value.clone();
        }
        add_entry(&folder, entry);
        let text = refusal(&folder)
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        fs::remove_dir_all(folder).unwrap();
        text
    };
    let one = |file: &str, channel: &str| json!({"file": file, "channel": channel});

    let text = said(json!({"r": one("missing.png", "r")}), json!({}));
    assert!(text.contains("missing.png"), "{text}");
    let text = said(json!({"r": one("other.dds", "r")}), json!({}));
    assert!(
        text.contains("other.dds") && text.contains("image"),
        "{text}"
    );
    let text = said(
        json!({"r": one("intensity.png", "r"), "g": one("small.png", "r")}),
        json!({}),
    );
    assert!(
        text.contains("intensity.png") && text.contains("small.png") && text.contains("32x32"),
        "{text}"
    );
    let text = said(json!({}), json!({}));
    assert!(text.contains("names no channel"), "{text}");
    let text = said(
        json!({"r": one("intensity.png", "r")}),
        json!({"source": "falloff.tga"}),
    );
    assert!(text.contains("source") && text.contains("pack"), "{text}");
    let text = said(json!({"r": one("../asset.json", "r")}), json!({}));
    assert!(text.contains("asset.json"), "{text}");
    for (pack, word) in [
        (json!({"r": one("intensity.png", "z")}), "z"),
        (json!({"x": one("intensity.png", "r")}), "x"),
    ] {
        let text = said(pack, json!({}));
        assert!(
            text.starts_with("textures.json: ") && text.contains(word),
            "{text}"
        );
    }
}

#[test]
fn a_normal_map_authored_with_green_up_is_flipped_by_a_rule_and_taken_as_it_is_otherwise() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = new_source(scratch.path());
    let authored = ramp(64, 64, 6);
    save_image(&folder.join("textures/bump.png"), &authored);
    add_entry(
        &folder,
        json!({"name": "bump", "encode": {"format": "DXT5", "colour_space": "linear"}}),
    );
    add_entry(
        &folder,
        json!({"name": "bump_flipped", "source": "bump.png",
               "encode": {"format": "DXT5", "colour_space": "linear", "flip_green": true}}),
    );
    let bytes = compile(&folder);

    let mut flipped = authored.clone();
    for texel in flipped.data.as_chunks_mut::<4>().0 {
        texel[1] = 255 - texel[1];
    }
    let encode = |image: &RgbaImage| {
        encode_texture_with_options(DXT5, 64, 64, image, &options(5, false))
            .unwrap()
            .data
    };
    assert_eq!(texture_mips(&bytes, "bump").concat(), encode(&authored));
    assert_eq!(
        texture_mips(&bytes, "bump_flipped").concat(),
        encode(&flipped)
    );
}

#[test]
fn a_flipped_image_made_the_other_way_up_is_the_dds_it_came_from() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    // The decode with its green inverted is the same texture authored the other way up.
    let mut other_way_up = level(&original, "art", 0);
    for texel in other_way_up.data.as_chunks_mut::<4>().0 {
        texel[1] = 255 - texel[1];
    }
    save_image(&folder.join("textures/art.png"), &other_way_up);
    edit_rules(&folder, |rules| {
        entry(rules, "art")["encode"] = json!({"flip_green": true});
    });
    let flipped_back = compile(&folder);
    fs::remove_file(folder.join("textures/art.png")).unwrap();
    // With the image gone the rule has nothing to flip, and a DDS alone refuses it and does not skip it.
    edit_rules(&folder, |rules| {
        entry(rules, "art")
            .as_object_mut()
            .unwrap()
            .remove("encode");
    });
    assert_eq!(
        flipped_back,
        compile(&folder),
        "a map authored the other way up and flipped back is the DDS it came from"
    );
}

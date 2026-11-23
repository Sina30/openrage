//! A rule a texture states is taken or refused by name and never skipped: the rules a DDS alone
//! cannot honour, the header words as named lists, and the image inputs with more than 8 bits a
//! channel (16-bit, a PSD, floating-point light) with the rule that brings them down.

mod common;

use std::fs;
use std::path::Path;

use common::*;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::decl::OpenEnum;
use or_io::gta5::gen9::TileMode;
use serde_json::{json, Value};

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;
const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;

fn said(dir: &Path) -> String {
    refusal(dir)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn rules_of(folder: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(folder.join("textures.json")).unwrap()).unwrap()
}

/// A fresh `art` source of one 64x64 DXT5 texture of seven mips, and the dictionary it came from.
fn art(scratch: &Path) -> (std::path::PathBuf, Vec<u8>) {
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    (extract(scratch, "pack", &original), original)
}

#[test]
fn a_rule_a_dds_alone_cannot_honour_is_refused_by_name_and_one_it_can_is_taken() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, original) = art(scratch.path());
    let rule = |encode: Value| edit_rules(&folder, |rules| entry(rules, "art")["encode"] = encode);

    // The DDS is copied as it is, so each of these would be skipped if it were not refused.
    for (encode, word) in [
        (json!({"format": "DXT1"}), "format"),
        (json!({"mips": "to-4x4"}), "mips"),
        (json!({"flip_green": true}), "flip_green"),
        (json!({"clamp": [0.0, 0.5]}), "clamp"),
        (json!({"tonemap": "reinhard"}), "tonemap"),
        (json!({"exposure": 1.0}), "exposure"),
    ] {
        rule(encode);
        let text = said(&folder);
        assert!(
            text.starts_with("textures.json: texture art: ") && text.contains(word),
            "{word}: {text}"
        );
    }

    // What the DDS already is, and what changes no block, is honoured.
    for encode in [
        json!({"format": "DXT5"}),
        json!({"mips": "to-1x1", "filter": "lanczos3", "quality": "high"}),
        json!({"flip_green": false}),
        json!({"colour_space": "linear"}),
    ] {
        rule(encode);
        assert_eq!(texture_mips(&compile(&folder)), texture_mips(&original));
    }
    // The colour space is a flag of the header, which a rule may state without a block moving.
    rule(json!({"colour_space": "srgb"}));
    let bytes = compile(&folder);
    assert_eq!(texture_mips(&bytes), texture_mips(&original));
    assert_eq!(legacy(&bytes).get("art").unwrap().info_bits & 1, 1);
}

fn texture_mips(bytes: &[u8]) -> Vec<Vec<u8>> {
    mips(legacy(bytes).get("art").expect("art"))
}

#[test]
fn header_words_are_named_lists_with_a_raw_fallback_and_an_integer_still_reads() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, original) = art(scratch.path());
    let words = |bytes: &[u8]| {
        let t = legacy(bytes);
        let t = t.get("art").unwrap();
        (
            t.resource_type_and_conversion_flags,
            t.template_type(),
            t.extra_flags,
            t.info_bits,
            t.handle_index,
            t.stereo_rt_mode,
        )
    };

    // The engine names one bit of the first word; every bit it does not name is raw, in hex.
    let rules = rules_of(&folder);
    let header = &rules["textures"][0]["header"];
    assert_eq!(
        header["resource_type_and_conversion_flags"],
        json!(["processed"])
    );
    assert_eq!(
        header["template_type"],
        json!([format!("0x{:08x}", 0x2000_0054u32)])
    );
    assert_eq!(header["extra_flags"], json!([format!("0x{:08x}", 0x10u32)]));
    assert_eq!(
        header["handle_index"],
        json!(7),
        "a handle is a number, not flags"
    );
    assert_eq!(compile_words(&folder, &words), words(&original));

    // A source written as integers, as every one before this was, reads the same.
    edit_rules(&folder, |rules| {
        entry(rules, "art")["header"] = json!({
            "resource_type_and_conversion_flags": 0x80, "template_type": 0x2000_0054u32,
            "extra_flags": 0x10, "handle_index": 7, "stereo_rt_mode": 2,
        });
    });
    assert_eq!(compile_words(&folder, &words), words(&original));

    // A name and a raw bit together; a raw bit may repeat a named one.
    edit_rules(&folder, |rules| {
        entry(rules, "art")["header"] = json!({
            "resource_type_and_conversion_flags": ["processed", "0x02", "0x80"],
        });
    });
    assert_eq!(compile_words(&folder, &words).0, 0x82);

    // A name the word does not have, a bit it has no room for, and a negative number are refused by name.
    for (value, wanted) in [
        (json!(["fast"]), "fast"),
        (json!(["0x100"]), "0x100"),
        (json!(-1), "resource_type_and_conversion_flags"),
        (json!("processed"), "resource_type_and_conversion_flags"),
    ] {
        edit_rules(&folder, |rules| {
            entry(rules, "art")["header"] = json!({ "resource_type_and_conversion_flags": value });
        });
        let text = said(&folder);
        assert!(
            text.starts_with("textures.json: texture art: header: ") && text.contains(wanted),
            "{text}"
        );
    }
    let text = {
        edit_rules(&folder, |rules| {
            entry(rules, "art")["header"] =
                json!({ "resource_type_and_conversion_flags": ["fast"] });
        });
        said(&folder)
    };
    assert!(
        text.contains("processed"),
        "the refusal lists what the word names: {text}"
    );
}

fn compile_words<T>(folder: &Path, words: &impl Fn(&[u8]) -> T) -> T {
    words(&compile(folder))
}

#[test]
fn an_enhanced_header_names_its_tile_mode_and_states_its_flags_raw() {
    let scratch = tempfile::tempdir().unwrap();
    let original = enhanced_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    let auto = <TileMode as OpenEnum>::describe()
        .name_of(i64::from(TileMode::Auto.to_raw()))
        .expect("the declaration names Auto")
        .to_string();

    let rules = rules_of(&folder);
    let header = &rules["textures"][0]["header"];
    assert_eq!(header["tile_mode"], json!(auto));
    assert_eq!(header["flags"], json!([format!("0x{:08x}", 0x26_0208u32)]));
    assert_eq!(
        header["usage_data"],
        json!([format!("0x{:08x}", 0x80_0254u32)])
    );

    let words = |bytes: &[u8]| {
        let t = enhanced(bytes);
        let t = t.entries.iter().flatten().next().expect("art");
        (t.tile_mode.raw(), t.flags, t.usage_data)
    };
    assert_eq!(words(&compile(&folder)), words(&original));
    // The number reads as the name, and a number no variant has is kept as it is.
    for (state, raw) in [
        (json!(u32::from(TileMode::Auto.to_raw())), 255),
        (json!(7), 7),
    ] {
        edit_rules(&folder, |rules| {
            entry(rules, "art")["header"]["tile_mode"] = state.clone();
        });
        assert_eq!(words(&compile(&folder)).0, raw, "{state}");
    }
    edit_rules(&folder, |rules| {
        entry(rules, "art")["header"]["tile_mode"] = json!("Sideways");
    });
    let text = said(&folder);
    assert!(text.contains("Sideways") && text.contains(&auto), "{text}");
}

/// An image of `side` texels a side, one value per channel from `of(x, y, channel)`.
fn sixteen(
    side: u32,
    of: impl Fn(u32, u32, usize) -> u16,
) -> image::ImageBuffer<image::Rgba<u16>, Vec<u16>> {
    image::ImageBuffer::from_fn(side, side, |x, y| {
        image::Rgba([of(x, y, 0), of(x, y, 1), of(x, y, 2), of(x, y, 3)])
    })
}

/// Uncompressed, so a texture's first mip is its texels.
fn raw_rules(extra: Value) -> Value {
    let mut encode = json!({"format": "A8R8G8B8", "colour_space": "linear"});
    for (key, value) in extra.as_object().unwrap() {
        encode[key] = value.clone();
    }
    json!({"name": "deep", "encode": encode})
}

fn first_mip(bytes: &[u8]) -> RgbaImage {
    level(bytes, "deep", 0)
}

fn fresh(scratch: &Path) -> std::path::PathBuf {
    extract(
        scratch,
        "pack",
        &legacy_dictionary(&[Spec::new("other", DXT1, ramp(16, 16, 9))]),
    )
}

fn push(folder: &Path, entry: Value) {
    edit_rules(folder, |rules| {
        rules["textures"].as_array_mut().unwrap().push(entry)
    });
}

#[test]
fn a_16_bit_image_comes_down_by_its_full_range_and_a_clamp_rule_windows_it() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = fresh(scratch.path());
    // Sixteen steps of 0x1111 across the image, a different phase in each channel.
    let value = |x: u32, y: u32, c: usize| (((x + y * 4 + c as u32 * 5) % 16) * 0x1111) as u16;
    sixteen(4, value)
        .save(folder.join("textures/deep.png"))
        .unwrap();

    // The game's formats hold 8 bits, so how the image comes down is stated and never guessed: with no rule it is refused, with
    // a sentence that names the rule that compiles it.
    push(&folder, raw_rules(json!({})));
    let text = said(&folder);
    assert!(
        text.starts_with("textures.json: texture deep: ")
            && text.contains("textures/deep.png")
            && text.contains("16 bits")
            && text.contains("\"clamp\": [0, 1]"),
        "{text}"
    );
    edit_rules(&folder, |rules| {
        rules["textures"].as_array_mut().unwrap().pop();
    });

    // The whole range is `clamp: [0, 1]`: each value is scaled over the byte.
    push(&folder, raw_rules(json!({"clamp": [0.0, 1.0]})));
    let scaled = first_mip(&compile(&folder));
    for (x, y) in [(0, 0), (1, 0), (3, 3)] {
        for c in 0..3 {
            let want = (f64::from(value(x, y, c)) / 257.0).round() as u8;
            assert_eq!(scaled.get_pixel(x, y)[c], want, "({x}, {y}) channel {c}");
        }
    }

    // A window of a tenth to four fifths of the range: below is 0, above is 255.
    edit_rules(&folder, |rules| {
        rules["textures"].as_array_mut().unwrap().pop();
    });
    push(&folder, raw_rules(json!({"clamp": [0.1, 0.8]})));
    let windowed = first_mip(&compile(&folder));
    for (x, y) in [(0, 0), (1, 0), (2, 1), (3, 3)] {
        for c in 0..3 {
            let seen = f64::from(value(x, y, c)) / 65535.0;
            let want = (((seen - 0.1) / 0.7).clamp(0.0, 1.0) * 255.0).round() as u8;
            assert_eq!(windowed.get_pixel(x, y)[c], want, "({x}, {y}) channel {c}");
        }
    }
    assert_eq!(windowed.get_pixel(0, 0)[0], 0);
    assert_eq!(
        windowed.get_pixel(3, 3)[0],
        255,
        "the top of the range is above the window"
    );
}

#[test]
fn a_rule_for_more_than_8_bits_is_refused_for_a_source_that_has_8() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = fresh(scratch.path());
    save_image(&folder.join("textures/deep.png"), &ramp(16, 16, 4));
    for (extra, word) in [
        (json!({"clamp": [0.0, 0.5]}), "clamp"),
        (json!({"tonemap": "reinhard"}), "tonemap"),
    ] {
        push(&folder, raw_rules(extra));
        let text = said(&folder);
        assert!(
            text.contains("texture deep: ") && text.contains(word) && text.contains("8 bits"),
            "{text}"
        );
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
    }
    // An exposure belongs to a tone map, and a clamp is a range.
    for (extra, wanted) in [
        (json!({"exposure": 1.0}), "exposure"),
        (json!({"clamp": [0.5, 0.5]}), "not a range"),
    ] {
        push(&folder, raw_rules(extra));
        assert!(said(&folder).contains(wanted), "{}", said(&folder));
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
    }
}

/// A 4x4 image of floating-point light, red rising across it, green falling and blue at one.
fn light_image(side: u32) -> image::ImageBuffer<image::Rgb<f32>, Vec<f32>> {
    image::ImageBuffer::from_fn(side, side, |x, _| {
        image::Rgb([x as f32 * 0.5, 2.0 - x as f32 * 0.5, 1.0])
    })
}

#[test]
fn floating_point_light_compiles_only_with_a_rule_that_says_how_it_comes_down() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = fresh(scratch.path());
    for (file, write) in [
        (
            "deep.exr",
            Box::new(|p: &Path| {
                image::DynamicImage::ImageRgb32F(light_image(4))
                    .save(p)
                    .unwrap()
            }) as Box<dyn Fn(&Path)>,
        ),
        (
            "deep.hdr",
            Box::new(|p: &Path| {
                image::DynamicImage::ImageRgb32F(light_image(4))
                    .save(p)
                    .unwrap()
            }),
        ),
    ] {
        let path = folder.join("textures").join(file);
        write(&path);
        push(&folder, raw_rules(json!({})));
        let text = said(&folder);
        assert!(
            text.contains(&format!("textures/{file}"))
                && text.contains("floating-point light")
                && text.contains("clamp"),
            "{text}"
        );

        // A clamp: the light itself is the byte over 0 to 1, so 0.5 is 128 and anything above is 255.
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
        push(&folder, raw_rules(json!({"clamp": [0.0, 1.0]})));
        let clamped = first_mip(&compile(&folder));
        for x in 0..4u32 {
            let light = [x as f64 * 0.5, 2.0 - x as f64 * 0.5, 1.0];
            for (c, light) in light.iter().enumerate() {
                let want = (light.clamp(0.0, 1.0) * 255.0).round() as u8;
                assert_eq!(clamped.get_pixel(x, 0)[c], want, "{file} x {x} channel {c}");
            }
        }

        // A tone map with two stops of exposure: x / (1 + x) of four times the light.
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
        push(
            &folder,
            raw_rules(json!({"tonemap": "reinhard", "exposure": 2.0})),
        );
        let mapped = first_mip(&compile(&folder));
        for x in 0..4u32 {
            let light = [x as f64 * 0.5, 2.0 - x as f64 * 0.5, 1.0];
            for (c, light) in light.iter().enumerate() {
                let bright = light * 4.0;
                let want = (bright / (1.0 + bright) * 255.0).round() as u8;
                assert_eq!(mapped.get_pixel(x, 0)[c], want, "{file} x {x} channel {c}");
            }
        }

        // The same light for a texture stored sRGB is encoded with the curve: brighter bytes.
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
        push(
            &folder,
            raw_rules(json!({"clamp": [0.0, 1.0], "colour_space": "srgb"})),
        );
        let encoded = first_mip(&compile(&folder));
        let curve = |v: f64| {
            let s = if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0).round() as u8
        };
        for x in 1..3u32 {
            let light = [x as f64 * 0.5, 2.0 - x as f64 * 0.5, 1.0];
            for (c, light) in light.iter().enumerate() {
                let want = curve(light.clamp(0.0, 1.0));
                assert!(
                    encoded.get_pixel(x, 0)[c].abs_diff(want) <= 1,
                    "{file} x {x} channel {c}: {} against {want}",
                    encoded.get_pixel(x, 0)[c]
                );
            }
        }
        edit_rules(&folder, |rules| {
            rules["textures"].as_array_mut().unwrap().pop();
        });
        fs::remove_file(path).unwrap();
    }
}

/// A raw 8-bit RGB Photoshop file of one texel a side, planar.
fn psd_of(side: u32, planes: [&[u8]; 3]) -> Vec<u8> {
    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&side.to_be_bytes());
    out.extend_from_slice(&side.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    for _ in 0..3 {
        out.extend_from_slice(&0u32.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    planes.iter().for_each(|plane| out.extend_from_slice(plane));
    out
}

#[test]
fn a_psd_beside_the_dds_is_the_image_it_stores_flattened() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = fresh(scratch.path());
    let red: Vec<u8> = (0..16).map(|i| (i * 16) as u8).collect();
    let green: Vec<u8> = (0..16).map(|i| 255 - (i * 16) as u8).collect();
    let blue = vec![77u8; 16];
    fs::write(
        folder.join("textures/deep.psd"),
        psd_of(4, [&red, &green, &blue]),
    )
    .unwrap();
    push(&folder, raw_rules(json!({})));
    let made = first_mip(&compile(&folder));
    for i in 0..16usize {
        assert_eq!(
            made.get_pixel(i as u32 % 4, i as u32 / 4),
            [red[i], green[i], blue[i], 255],
            "texel {i}"
        );
    }
    // A Photoshop file this build cannot read is refused with the file's name and what it is.
    fs::write(folder.join("textures/deep.psd"), b"8BPS").unwrap();
    let text = said(&folder);
    assert!(
        text.contains("textures/deep.psd") && text.contains("ends before"),
        "{text}"
    );
}

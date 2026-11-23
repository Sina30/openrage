//! A `.ytd` becomes a `.textures.src` folder and compiles back, with no corpus staged.

mod common;

use std::fs;
use std::path::Path;

use common::*;
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage, TextureSource};
use or_io::gta5::gen8::GrcTexture;
use or_source::asset::{sha256_hex, AssetManifest, How};
use serde_json::{json, Value};

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;
const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const BGRA: GrcTextureFormat = GrcTextureFormat::A8R8G8B8;

fn three() -> Vec<Spec> {
    vec![
        Spec::new("Ground_Diff", DXT1, ramp(64, 64, 1)),
        Spec {
            srgb: true,
            template_type: 0x2000_0056,
            ..Spec::new("bark_n", DXT5, ramp(128, 32, 2))
        },
        Spec::new("badge", BGRA, quadrants(16, 16)),
    ]
}

/// Every word and every mip a compile has to give back.
fn assert_same(original: &GrcTexture, rebuilt: &GrcTexture) {
    let name = &original.name;
    assert_eq!(original.name, rebuilt.name);
    let words = |t: &GrcTexture| {
        (
            t.width,
            t.height,
            t.depth,
            t.format,
            t.mip_count,
            t.info_bits,
            t.resource_type_and_conversion_flags,
            t.extra_flags,
            t.handle_index,
            t.stereo_rt_mode,
            t.template_type(),
        )
    };
    assert_eq!(words(original), words(rebuilt), "{name}: header words");
    assert_eq!(mips(original), mips(rebuilt), "{name}: mips");
}

/// Mip `level`'s pixel at (`x`, `y`) of the first texture of `bytes`.
fn pixel(bytes: &[u8], name: &str, level: u32, x: u32, y: u32) -> [u8; 4] {
    let dictionary = legacy(bytes);
    let texture = dictionary.get(name).unwrap_or_else(|| panic!("no {name}"));
    let image = decode_level(texture, level).unwrap();
    let at = ((y * image.width + x) * 4) as usize;
    image.data[at..at + 4].try_into().unwrap()
}

#[test]
fn a_legacy_dictionary_extracts_to_a_folder_and_compiles_back_to_the_same_pixels() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&three());
    let folder = extract(scratch.path(), "pack", &original);
    assert_eq!(folder.file_name().unwrap(), "pack.textures.src");

    let manifest =
        AssetManifest::parse(&fs::read_to_string(folder.join("asset.json")).unwrap()).unwrap();
    assert_eq!(
        (
            manifest.kind.as_str(),
            manifest.name.as_str(),
            manifest.target.as_str()
        ),
        ("textures", "pack", "gen8")
    );
    assert_eq!(manifest.origin.how, How::Extracted);
    assert!(!manifest.origin.from_game);
    assert_eq!(manifest.origin.sha256, Some(sha256_hex(&original)));
    for texture in ["Ground_Diff", "bark_n", "badge"] {
        assert!(
            folder
                .join("textures")
                .join(format!("{texture}.dds"))
                .is_file(),
            "{texture}.dds"
        );
    }

    let rebuilt = compile(&folder);
    let (before, after) = (legacy(&original), legacy(&rebuilt));
    assert_eq!(before.codes, after.codes);
    for texture in before.entries.iter().flatten() {
        assert_same(texture, after.get(&texture.name).unwrap());
    }
}

#[test]
fn an_enhanced_dictionary_extracts_to_a_folder_and_compiles_back_to_the_same_pixels() {
    let scratch = tempfile::tempdir().unwrap();
    let original = enhanced_dictionary(&[
        Spec::new("prop_chair", DXT1, ramp(64, 32, 3)),
        Spec::new("prop_chair_n", DXT5, ramp(32, 32, 4)),
    ]);
    let folder = extract(scratch.path(), "chairs", &original);
    let manifest =
        AssetManifest::parse(&fs::read_to_string(folder.join("asset.json")).unwrap()).unwrap();
    assert_eq!(manifest.target, "gen9");

    let (before, after) = (enhanced(&original), enhanced(&compile(&folder)));
    assert_eq!(before.codes, after.codes);
    for (a, b) in before
        .entries
        .iter()
        .flatten()
        .zip(after.entries.iter().flatten())
    {
        assert_eq!(a.name, b.name);
        let words = |t: &or_io::gta5::gen9::GrcTextureG9| {
            (
                t.width,
                t.height,
                t.depth,
                t.format.raw(),
                t.tile_mode.raw(),
                t.mip_levels,
                t.flags,
                t.usage_data,
                t.anti_alias_type,
            )
        };
        assert_eq!(words(a), words(b), "{}: header words", a.name);
        assert_eq!(mips(&**a), mips(&**b), "{}: mips", a.name);
    }
}

#[test]
fn compiling_an_unchanged_folder_is_byte_identical_twice_and_wherever_the_folder_is() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    let first = compile(&folder);
    assert_eq!(first, compile(&folder));

    let moved = scratch.path().join("elsewhere.textures.src");
    fs::rename(&folder, &moved).unwrap();
    assert_eq!(
        first,
        compile(&moved),
        "a folder's place is not part of what it compiles to"
    );

    // A second extraction differs in its id alone.
    let other = tempfile::tempdir().unwrap();
    let again = extract(other.path(), "pack", &legacy_dictionary(&three()));
    let text = |dir: &Path, file: &str| fs::read(dir.join(file)).unwrap();
    assert_eq!(text(&moved, "textures.json"), text(&again, "textures.json"));
    assert_eq!(
        text(&moved, "textures/badge.dds"),
        text(&again, "textures/badge.dds")
    );
    assert_ne!(text(&moved, "asset.json"), text(&again, "asset.json"));
}

#[test]
fn replacing_one_image_changes_only_that_texture() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    let before = compile(&folder);

    // A different size on purpose: a full encode takes the image's, and the others must not move.
    let replacement = quadrants(32, 32);
    save_image(&folder.join("textures/bark_n.png"), &replacement);
    let after = compile(&folder);

    let (a, b) = (legacy(&before), legacy(&after));
    assert_eq!(a.codes, b.codes);
    for texture in a.entries.iter().flatten() {
        let other = b.get(&texture.name).unwrap();
        if texture.name == "bark_n" {
            continue;
        }
        assert_same(texture, other);
    }

    let new = b.get("bark_n").unwrap();
    let old = a.get("bark_n").unwrap();
    assert_ne!(mips(old), mips(new), "the replaced texture did not change");
    assert_eq!((new.width, new.height), (32, 32));
    assert_eq!(
        new.pixel_format(),
        DXT5,
        "the format follows the DDS the image replaces"
    );
    assert_eq!(new.mip_count, 4, "32x32 down to the 4x4 level is four mips");
    assert_eq!(
        (
            new.resource_type_and_conversion_flags,
            new.extra_flags,
            new.template_type()
        ),
        (
            old.resource_type_and_conversion_flags,
            old.extra_flags,
            old.template_type()
        ),
        "a recompiled texture keeps its header words"
    );
    // BC3 holds flat colour to within a step of 5:6:5 and alpha to within one of eight levels.
    for delta in worst(&decode_level(new, 0).unwrap(), &replacement) {
        assert!(delta <= 8, "the image came back {delta} off");
    }
    assert!(
        new.info_bits & 1 != 0,
        "the DDS said sRGB, so the rebuilt texture does"
    );
}

#[test]
fn a_dds_is_copied_block_for_block_and_a_dds_alone_is_a_texture() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));

    // Blocks no encoder would write, so a decode and an encode cannot give them back.
    let (width, height, mips_declared) = (32u32, 32u32, 4u32);
    let size = DXT1.total_size(width, height, mips_declared) as usize;
    let mut state = 0x9E37_79B9u32;
    let blocks: Vec<u8> = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        })
        .collect();
    let texture = GrcTexture {
        name: "noise".to_string(),
        width: width as u16,
        height: height as u16,
        depth: 1,
        format: DXT1.to_raw_format(),
        mip_count: mips_declared as u8,
        backing_store: blocks.clone(),
        ..Default::default()
    };
    let dds = or_io::codec::texture::to_dds(&texture)
        .unwrap()
        .to_bytes()
        .unwrap();
    fs::write(folder.join("textures/noise.dds"), dds).unwrap();

    let rebuilt = legacy(&compile(&folder));
    assert_eq!(rebuilt.entries.iter().flatten().count(), 4);
    let noise = rebuilt.get("noise").unwrap();
    assert_eq!(mips(noise).concat(), blocks);
    assert_eq!(
        noise.resource_type_and_conversion_flags, 0x80,
        "a new texture is a processed one"
    );
}

#[test]
fn the_source_is_the_named_one_then_an_image_of_the_name_then_the_dds() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    let dds_alone = compile(&folder);
    // Opaque, because BC1 keeps a one-bit alpha and would turn a translucent texel black.
    let a = flat(
        64,
        64,
        [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [128, 128, 128, 255],
        ],
    );
    let b = flat(
        64,
        64,
        [
            [255, 255, 0, 255],
            [0, 255, 255, 255],
            [255, 0, 255, 255],
            [255, 255, 255, 255],
        ],
    );
    save_image(&folder.join("textures/Ground_Diff.png"), &a);
    save_image(&folder.join("textures/other.png"), &b);
    let close_to = |bytes: &[u8], image: &RgbaImage| {
        let dictionary = legacy(bytes);
        let decoded = decode_level(dictionary.get("Ground_Diff").unwrap(), 0).unwrap();
        worst(&decoded, image).into_iter().take(3).max().unwrap()
    };

    // An image of the texture's name beats its DDS.
    assert!(close_to(&compile(&folder), &a) <= 8);
    assert!(close_to(&compile(&folder), &b) > 40);

    // The source a texture names beats an image of its name.
    edit_rules(&folder, |rules| {
        entry(rules, "Ground_Diff")["source"] = json!("other.png");
    });
    assert!(close_to(&compile(&folder), &b) <= 8);
    assert!(
        legacy(&compile(&folder)).get("other").is_none(),
        "a file an entry names is that entry's input, not a texture of its own"
    );

    // No source and no image leaves the DDS, copied as it was.
    edit_rules(&folder, |rules| {
        entry(rules, "Ground_Diff")
            .as_object_mut()
            .unwrap()
            .remove("source");
    });
    fs::remove_file(folder.join("textures/Ground_Diff.png")).unwrap();
    // An image no entry names would be a texture of its own, so it goes too.
    fs::remove_file(folder.join("textures/other.png")).unwrap();
    assert_eq!(compile(&folder), dds_alone);

    // A DDS can be the named source too, and is just as exact.
    let elsewhere = extract(
        scratch.path(),
        "elsewhere",
        &legacy_dictionary(&[Spec::new("Ground_Diff", DXT1, ramp(32, 32, 5))]),
    );
    fs::copy(
        elsewhere.join("textures/Ground_Diff.dds"),
        folder.join("textures/alt.dds"),
    )
    .unwrap();
    edit_rules(&folder, |rules| {
        entry(rules, "Ground_Diff")["source"] = json!("alt.dds");
    });
    let rebuilt = legacy(&compile(&folder));
    let theirs = legacy(&legacy_dictionary(&[Spec::new(
        "Ground_Diff",
        DXT1,
        ramp(32, 32, 5),
    )]));
    assert_eq!(
        mips(rebuilt.get("Ground_Diff").unwrap()),
        mips(theirs.get("Ground_Diff").unwrap())
    );
}

#[test]
fn two_images_of_one_name_are_refused_unless_a_source_picks_one() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    save_image(&folder.join("textures/badge.png"), &quadrants(16, 16));
    fs::copy(
        folder.join("textures/badge.png"),
        folder.join("textures/badge.tga"),
    )
    .unwrap();

    let problems = refusal(&folder);
    let said = problems
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        said.contains("badge.png") && said.contains("badge.tga"),
        "{said}"
    );
    assert_eq!(problems[0].texture.as_deref(), Some("badge"));

    edit_rules(&folder, |rules| {
        entry(rules, "badge")["source"] = json!("badge.png");
    });
    compile(&folder);
}

#[test]
fn png_tga_tiff_bmp_and_jpeg_all_decode_into_a_texture() {
    let scratch = tempfile::tempdir().unwrap();
    let image = quadrants(32, 32);
    let rgba = || image::RgbaImage::from_raw(32, 32, image.data.clone()).unwrap();
    for extension in ["png", "tga", "tif", "tiff", "bmp", "jpg", "jpeg"] {
        let folder = extract(
            scratch.path(),
            extension,
            &legacy_dictionary(&[Spec::new("art", DXT5, ramp(32, 32, 1))]),
        );
        let path = folder.join("textures").join(format!("art.{extension}"));
        match extension {
            "jpg" | "jpeg" => image::DynamicImage::ImageRgba8(rgba())
                .to_rgb8()
                .save(&path),
            _ => rgba().save(&path),
        }
        .unwrap();
        let bytes = compile(&folder);
        // The middle of each quadrant: JPEG blurs an edge, and BC3 is off by a step at most.
        for (x, y, want) in [
            (8, 8, [255, 0, 0]),
            (24, 8, [0, 255, 0]),
            (8, 24, [0, 0, 255]),
        ] {
            let got = pixel(&bytes, "art", 0, x, y);
            for c in 0..3 {
                let slack = if extension.starts_with('j') { 48 } else { 8 };
                assert!(
                    got[c].abs_diff(want[c]) <= slack,
                    ".{extension} ({x},{y}) {got:?}"
                );
            }
        }
    }
}

#[test]
fn mips_go_down_to_the_4x4_level_like_shipped_files_and_a_rule_can_go_further() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(
        scratch.path(),
        "empty",
        &legacy_dictionary(&[Spec::new("seed", DXT1, ramp(16, 16, 1))]),
    );
    // Shipped textures stop when the shorter side reaches 4: 256x256 has seven mips and 1024x64 has five.
    let wanted = [
        ("a", 256, 256, 7),
        ("b", 1024, 64, 5),
        ("c", 4, 4, 1),
        ("d", 8, 4, 1),
        ("e", 16, 8, 2),
    ];
    for (name, w, h, _) in wanted {
        save_image(
            &folder.join("textures").join(format!("{name}.png")),
            &ramp(w, h, 1),
        );
    }
    save_image(&folder.join("textures/f.png"), &ramp(256, 256, 2));
    edit_rules(&folder, |rules| {
        rules["textures"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "f", "encode": {"mips": "to-1x1"}}));
    });
    let rebuilt = legacy(&compile(&folder));
    for (name, _, _, levels) in wanted {
        assert_eq!(rebuilt.get(name).unwrap().mip_count, levels, "{name}");
    }
    assert_eq!(rebuilt.get("f").unwrap().mip_count, 9);
}

#[test]
fn the_colour_space_a_dds_states_is_the_default_and_a_rule_overrides_it() {
    let scratch = tempfile::tempdir().unwrap();
    let mut linear = Spec::new("lin", DXT1, ramp(16, 16, 1));
    let mut colour = Spec::new("col", DXT1, ramp(16, 16, 2));
    (linear.srgb, colour.srgb) = (false, true);
    let folder = extract(
        scratch.path(),
        "pack",
        &legacy_dictionary(&[linear, colour]),
    );

    // A black and white checker: its 4x4 level is the average of the two, 128 averaged as
    // code values and about 188 averaged as light.
    let mut checker = RgbaImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            checker.set_pixel(x, y, [v, v, v, 255]);
        }
    }
    for name in ["lin", "col", "fresh", "forced"] {
        save_image(
            &folder.join("textures").join(format!("{name}.png")),
            &checker,
        );
    }
    edit_rules(&folder, |rules| {
        let textures = rules["textures"].as_array_mut().unwrap();
        textures.push(json!({"name": "fresh"}));
        textures.push(json!({"name": "forced", "encode": {"colour_space": "srgb"}}));
    });
    let bytes = compile(&folder);
    let level_one = |name: &str| pixel(&bytes, name, 1, 0, 0)[0];
    assert!(
        level_one("lin").abs_diff(128) <= 8,
        "lin {}",
        level_one("lin")
    );
    assert!(
        level_one("col").abs_diff(188) <= 8,
        "col {}",
        level_one("col")
    );
    assert!(
        level_one("fresh").abs_diff(188) <= 8,
        "a texture with no DDS is sRGB"
    );
    assert!(level_one("forced").abs_diff(188) <= 8, "the rule wins");
    let rebuilt = legacy(&bytes);
    assert_eq!(rebuilt.get("col").unwrap().info_bits & 1, 1);
    assert_eq!(rebuilt.get("lin").unwrap().info_bits & 1, 0);
}

#[test]
fn what_cannot_be_compiled_is_refused_by_name_and_nothing_is_guessed() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    save_image(&folder.join("textures/badge.png"), &quadrants(16, 16));
    let said = |dir: &Path| {
        refusal(dir)
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let with_rule = |extra: Value| {
        let dir = extract(scratch.path(), "variant", &legacy_dictionary(&three()));
        save_image(&dir.join("textures/badge.png"), &quadrants(16, 16));
        edit_rules(&dir, |rules| {
            let badge = entry(rules, "badge");
            for (key, value) in extra.as_object().unwrap() {
                badge[key] = value.clone();
            }
        });
        let text = said(&dir);
        fs::remove_dir_all(dir).unwrap();
        text
    };

    // A rule this build has no code for must not be skipped over: a sharpening would compile unsharpened.
    assert!(with_rule(json!({"encode": {"sharpen": 1.0}})).contains("sharpen"));
    // A word of the other generation, and a name no format has, are faults in the rules file.
    let header = with_rule(json!({"header": {"usage_data": 1}}));
    assert!(
        header.starts_with("textures.json: ") && header.contains("usage_data"),
        "{header}"
    );
    let format = with_rule(json!({"encode": {"format": "DXT9"}}));
    assert!(
        format.starts_with("textures.json: ") && format.contains("DXT9"),
        "{format}"
    );
    assert!(with_rule(json!({"encode": {"format": "BC6H"}})).contains("BC6"));
    assert!(with_rule(json!({"source": "missing.png"})).contains("missing.png"));
    assert!(with_rule(json!({"source": "../asset.json"})).contains("asset.json"));

    edit_rules(&folder, |rules| {
        rules["textures"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "ghost"}));
    });
    assert!(said(&folder).contains("has no source"), "{}", said(&folder));
    edit_rules(&folder, |rules| {
        rules["textures"].as_array_mut().unwrap().pop();
    });

    save_image(&folder.join("textures/oddly.png"), &ramp(100, 60, 1));
    assert!(said(&folder).contains("power-of-two"), "{}", said(&folder));
    fs::remove_file(folder.join("textures/oddly.png")).unwrap();

    fs::write(folder.join("textures/badge.dds"), b"not a dds").unwrap();
    fs::remove_file(folder.join("textures/badge.png")).unwrap();
    let problems = refusal(&folder);
    assert_eq!(problems[0].file.as_deref(), Some("textures/badge.dds"));
}

#[test]
fn a_texture_that_a_dds_cannot_hold_is_refused_at_extraction_and_no_folder_is_left() {
    let scratch = tempfile::tempdir().unwrap();
    let mut dictionary = legacy(&legacy_dictionary(&three()));
    dictionary.get_mut("badge").unwrap().cut_mip_levels = 1;
    let problems = or_source::textures::extract(
        &or_io::decl::File::to_bytes(&dictionary).unwrap(),
        "pack",
        scratch.path(),
        &or_source::textures::Extraction {
            from_game: false,
            virtual_path: None,
        },
    )
    .unwrap_err();
    assert_eq!(problems[0].texture.as_deref(), Some("badge"));
    assert!(
        problems[0].message.contains("cut_mip_levels"),
        "{}",
        problems[0]
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn a_name_that_is_a_path_is_refused_at_extraction_and_nothing_is_written() {
    let scratch = tempfile::tempdir().unwrap();
    let parent = scratch.path().join("project");
    fs::create_dir(&parent).unwrap();
    let ytd = legacy_dictionary(&three());
    for name in [r"..\escaped", "../escaped", "a/b", r"C:\escaped", "..", ""] {
        let problems = or_source::textures::extract(
            &ytd,
            name,
            &parent,
            &or_source::textures::Extraction {
                from_game: false,
                virtual_path: None,
            },
        )
        .unwrap_err();
        assert!(
            problems[0].message.contains("name: "),
            "{name:?}: {}",
            problems[0]
        );
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0, "{name:?}");
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1, "{name:?}");
    }
}

/// `bark.dds` of a 32x32 DXT5 source with its height, width and mip count words replaced.
fn dds_with(folder: &Path, width: u32, height: u32, mips: u32) {
    let path = folder.join("textures/bark.dds");
    let mut bytes = fs::read(&path).unwrap();
    bytes[12..16].copy_from_slice(&height.to_le_bytes());
    bytes[16..20].copy_from_slice(&width.to_le_bytes());
    bytes[28..32].copy_from_slice(&mips.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_dds_header_that_states_an_absurd_size_is_refused_as_data_and_never_panics() {
    let scratch = tempfile::tempdir().unwrap();
    let spec = [Spec::new("bark", DXT5, ramp(32, 32, 1))];
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&spec));
    let source = or_source::asset::Source::open(&folder).unwrap();
    let whole = fs::read(folder.join("textures/bark.dds")).unwrap();

    // The 65535 case fits 16-bit sides and still needs 2^32 bytes of DXT5.
    let cases = [
        (0x1_0000, 0x1_0000, 6),
        (0x7FFF_FFFF, 0x7FFF_FFFF, 1),
        (u32::MAX, 1, 1),
        (65535, 65535, 1),
        (32, 32, u32::MAX),
        (32, 32, 256),
        (32, 32, 7),
    ];
    for (width, height, mips) in cases {
        fs::write(folder.join("textures/bark.dds"), &whole).unwrap();
        dds_with(&folder, width, height, mips);
        let what = format!("{width}x{height} with {mips} mips");
        let compiled = std::panic::catch_unwind(|| or_source::textures::compile(&source))
            .unwrap_or_else(|_| panic!("{what} panicked"));
        let problems = compiled.expect_err(&what);
        assert_eq!(
            problems[0].file.as_deref(),
            Some("textures/bark.dds"),
            "{what}"
        );
        assert_eq!(problems[0].texture.as_deref(), Some("bark"), "{what}");
    }
}

#[test]
fn a_textures_folder_that_cannot_be_listed_is_refused_and_never_compiled_as_empty() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));

    // Gone: a recook that took this for a source of no textures would publish an empty `.ytd`.
    fs::remove_dir_all(folder.join("textures")).unwrap();
    let problems = refusal(&folder);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].file.as_deref(), Some("textures"));
    assert!(
        problems[0].message.contains("cannot list"),
        "{}",
        problems[0]
    );

    // A file where the folder belongs.
    fs::write(folder.join("textures"), b"not a folder").unwrap();
    let problems = refusal(&folder);
    assert_eq!(problems[0].file.as_deref(), Some("textures"));
}

#[test]
fn a_target_that_is_neither_edition_is_refused_in_the_words_of_the_editions() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), "pack", &legacy_dictionary(&three()));
    let manifest = folder.join("asset.json");
    let text = fs::read_to_string(&manifest).unwrap();
    assert!(text.contains("\"gen8\""), "{text}");
    fs::write(&manifest, text.replace("\"gen8\"", "\"gen7\"")).unwrap();
    let why = refusal(&folder)
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(why.contains("Legacy") && why.contains("Enhanced"), "{why}");
    let lower = why.to_lowercase();
    assert!(!lower.contains("gen8") && !lower.contains("gen9"), "{why}");
}

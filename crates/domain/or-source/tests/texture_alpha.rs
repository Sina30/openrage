//! The alpha rule of `textures.json`: straight or premultiplied colour, and whether a cutout keeps its
//! coverage down the mips. A build that cannot honour a form of it refuses it by name.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use serde_json::{json, Value};

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;
const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;

/// Leaves: a tenth of the texels are solid and the rest are a faint haze, so that an average of boxes takes the share that is solid
/// down to almost none. `premultiplied` writes the colour already multiplied by the alpha, as an exporter that premultiplies does.
fn leaves(side: u32, premultiplied: bool) -> RgbaImage {
    let mut image = RgbaImage::new(side, side);
    for i in 0..side * side {
        let h = i.wrapping_mul(2_654_435_761).rotate_left(13) ^ i.wrapping_mul(40_503);
        let alpha = if h % 10 == 0 {
            200 + (h / 10 % 56) as u8
        } else {
            (h / 7 % 61) as u8
        };
        let colour = |c: u8| match premultiplied {
            true => (u32::from(c) * u32::from(alpha) / 255) as u8,
            false => c,
        };
        image.set_pixel(
            i % side,
            i / side,
            [colour(200), colour(120), colour(40), alpha],
        );
    }
    image
}

/// The share of texels of `image` whose alpha reaches `cutoff`.
fn reaching(image: &RgbaImage, cutoff: f32) -> f32 {
    let bar = (cutoff * 255.0).ceil() as u8;
    let texels = image.data.as_chunks::<4>().0;
    texels.iter().filter(|t| t[3] >= bar).count() as f32 / texels.len() as f32
}

fn said(dir: &Path) -> String {
    refusal(dir)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A source holding the image `cutout.png` and a rule for it, beside a texture that has nothing to do with it.
fn cutout(scratch: &Path, image: &RgbaImage, encode: Value) -> PathBuf {
    static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let name = format!(
        "pack{}",
        MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let folder = extract(
        scratch,
        &name,
        &legacy_dictionary(&[Spec::new("other", DXT1, ramp(16, 16, 9))]),
    );
    save_image(&folder.join("textures/cutout.png"), image);
    edit_rules(&folder, |rules| {
        rules["textures"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "cutout", "encode": encode}));
    });
    folder
}

/// Uncompressed and linear, so that the mips a compile stores are the mips it made.
fn raw(extra: Value) -> Value {
    let mut encode = json!({"format": "A8R8G8B8", "colour_space": "linear"});
    for (key, value) in extra.as_object().unwrap() {
        encode[key] = value.clone();
    }
    encode
}

fn levels_of(bytes: &[u8], count: u32) -> Vec<RgbaImage> {
    (0..count).map(|l| level(bytes, "cutout", l)).collect()
}

#[test]
fn a_premultiplied_cutout_keeps_its_coverage_through_the_mips() {
    let scratch = tempfile::tempdir().unwrap();
    let image = leaves(128, true);
    let top = reaching(&image, 0.5);
    assert!(top > 0.05 && top < 0.3, "the leaves are a minority: {top}");

    let kept = cutout(
        scratch.path(),
        &image,
        raw(json!({"alpha": "premultiplied", "alpha_coverage": 0.5})),
    );
    let chain = levels_of(&compile(&kept), 4);
    assert_eq!(chain[0].data, image.data, "the top level is the artist's");
    for (l, mip) in chain.iter().enumerate() {
        let share = reaching(mip, 0.5);
        assert!(
            (share - top).abs() <= 0.05,
            "mip {l} reaches {share} of its texels, the top level {top}"
        );
        for t in mip.data.as_chunks::<4>().0 {
            assert!(
                t[..3].iter().all(|c| *c <= t[3].saturating_add(1)),
                "mip {l}: colour {t:?} is above its alpha, so it is not premultiplied"
            );
        }
    }

    // The same image without the coverage rule loses the leaves: that is what the rule is for.
    let plain = cutout(
        scratch.path(),
        &image,
        raw(json!({"alpha": "premultiplied"})),
    );
    let lost = reaching(&levels_of(&compile(&plain), 4)[2], 0.5);
    assert!(
        lost < top / 2.0,
        "without the rule {lost} of the texels still reach the cutoff of the {top} the top level had"
    );
}

#[test]
fn a_straight_alpha_is_the_default_and_a_rule_that_states_it_changes_no_byte() {
    let scratch = tempfile::tempdir().unwrap();
    let image = leaves(64, false);
    let none = compile(&cutout(scratch.path(), &image, raw(json!({}))));
    let stated = compile(&cutout(
        scratch.path(),
        &image,
        raw(json!({"alpha": "straight"})),
    ));
    assert_eq!(stated, none);
}

#[test]
fn an_alpha_rule_the_build_cannot_honour_is_refused_by_name_and_never_skipped() {
    let scratch = tempfile::tempdir().unwrap();
    let image = leaves(32, false);
    let try_rule = |extra: Value| {
        let folder = cutout(scratch.path(), &image, raw(extra));
        let text = said(&folder);
        fs::remove_dir_all(folder).unwrap();
        text
    };

    // A word no build has code for names itself, and so does a cutoff that is not one.
    let unknown = try_rule(json!({"alpha": "sideways"}));
    assert!(
        unknown.starts_with("textures.json: ") && unknown.contains("sideways"),
        "{unknown}"
    );
    let rule = try_rule(json!({"sharpen": 2}));
    assert!(rule.contains("sharpen"), "{rule}");
    let range = try_rule(json!({"alpha_coverage": 1.5}));
    assert!(
        range.contains("texture cutout")
            && range.contains("alpha_coverage")
            && range.contains("1.5"),
        "{range}"
    );
    let zero = try_rule(json!({"alpha_coverage": 0.0}));
    assert!(zero.contains("alpha_coverage"), "{zero}");

    // A format that holds no alpha, or one bit of it, cannot keep what the rule asks.
    for (format, rule, word) in [
        ("DXN", json!({"alpha_coverage": 0.5}), "alpha_coverage"),
        ("L8", json!({"alpha": "premultiplied"}), "alpha"),
        ("DXT1", json!({"alpha": "premultiplied"}), "alpha"),
    ] {
        let mut extra = rule;
        extra["format"] = json!(format);
        let text = try_rule(extra);
        assert!(
            text.contains("texture cutout") && text.contains(word) && text.contains(format),
            "{format}: {text}"
        );
    }
    // One bit of alpha is what a cutout of BC1 is, so the coverage of it is kept.
    let folder = cutout(
        scratch.path(),
        &image,
        raw(json!({"format": "DXT1", "alpha_coverage": 0.5})),
    );
    compile(&folder);
}

#[test]
fn a_dds_alone_cannot_keep_a_coverage_and_says_so_and_a_statement_of_its_mode_moves_no_block() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, ramp(64, 64, 3))]);
    let folder = extract(scratch.path(), "pack", &original);
    let rule = |encode: Value| edit_rules(&folder, |rules| entry(rules, "art")["encode"] = encode);

    rule(json!({"alpha_coverage": 0.5}));
    let text = said(&folder);
    assert!(
        text.starts_with("textures.json: texture art: ") && text.contains("alpha_coverage"),
        "{text}"
    );

    for mode in ["straight", "premultiplied"] {
        rule(json!({"alpha": mode}));
        let bytes = compile(&folder);
        assert_eq!(
            mips(legacy(&bytes).get("art").unwrap()),
            mips(legacy(&original).get("art").unwrap()),
            "{mode}: a statement about a DDS moves no block"
        );
    }
}

#[test]
fn an_image_made_from_its_dds_keeps_every_block_unless_an_alpha_rule_asks_for_a_new_chain() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[Spec::new("art", DXT5, leaves(64, false))]);
    let folder = extract(scratch.path(), "pack", &original);
    let exact = level(&original, "art", 0);
    save_image(&folder.join("textures/art.png"), &exact);
    let was = mips(legacy(&original).get("art").unwrap());

    let kept = compile(&folder);
    assert_eq!(
        mips(legacy(&kept).get("art").unwrap()),
        was,
        "an untouched image"
    );

    edit_rules(&folder, |rules| {
        entry(rules, "art")["encode"] = json!({"alpha_coverage": 0.5, "mips": "to-1x1"})
    });
    let rebuilt = mips(legacy(&compile(&folder)).get("art").unwrap());
    assert_eq!(rebuilt.len(), was.len());
    assert_ne!(
        rebuilt, was,
        "a rule about the whole chain makes the whole chain"
    );
}

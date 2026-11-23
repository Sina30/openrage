//! Which file each texture of a source compiles from, said so that a person can see it, and the rule a 16-bit picture is given
//! the moment it is put into a source.

mod common;

use std::path::Path;

use common::*;
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::Source;
use or_source::create::author_textures;
use or_source::textures::{self, InputKind};
use serde_json::{json, Value};

const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;

fn inputs(folder: &Path) -> Vec<textures::Input> {
    textures::inputs_of(&Source::open(folder).unwrap()).unwrap()
}

fn of<'a>(all: &'a [textures::Input], name: &str) -> &'a textures::Input {
    all.iter()
        .find(|i| i.texture == name)
        .unwrap_or_else(|| panic!("no {name} in {all:?}"))
}

#[test]
fn the_inspector_says_which_file_wins_when_a_dds_and_an_image_share_a_name() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[
        Spec::new("both", DXT5, ramp(16, 16, 1)),
        Spec::new("only", DXT5, ramp(16, 16, 2)),
        Spec::new("named", DXT5, ramp(16, 16, 3)),
        Spec::new("mask", DXT5, ramp(16, 16, 4)),
    ]);
    let folder = extract(scratch.path(), "pack", &original);
    let textures_dir = folder.join("textures");
    save_image(&textures_dir.join("both.png"), &ramp(16, 16, 7));
    save_image(&textures_dir.join("elsewhere.png"), &ramp(16, 16, 8));
    save_image(&textures_dir.join("red.png"), &ramp(16, 16, 9));
    edit_rules(&folder, |rules| {
        entry(rules, "named")["source"] = json!("elsewhere.png");
        entry(rules, "mask")["pack"] = json!({"r": {"file": "red.png", "channel": "r"}});
    });

    let all = inputs(&folder);

    let both = of(&all, "both");
    assert_eq!(both.kind, InputKind::Image);
    assert_eq!(both.from, ["both.png"]);
    assert_eq!(both.unused, ["both.dds"], "the DDS beside an image loses");
    let said = both.sentence();
    assert!(
        said.contains("both.png") && said.contains("both.dds") && said.contains("wins"),
        "{said}"
    );

    let only = of(&all, "only");
    assert_eq!(
        (only.kind, only.from.as_slice()),
        (InputKind::Dds, ["only.dds".to_string()].as_slice())
    );
    assert!(only.unused.is_empty());
    assert!(
        only.sentence().contains("copied as it is"),
        "{}",
        only.sentence()
    );

    let named = of(&all, "named");
    assert_eq!(named.from, ["elsewhere.png"]);
    assert_eq!(named.unused, ["named.dds"]);
    assert!(
        named.sentence().contains("names it"),
        "{}",
        named.sentence()
    );

    let mask = of(&all, "mask");
    assert_eq!(mask.kind, InputKind::Pack);
    assert_eq!(mask.from, ["red.png"]);
    assert_eq!(mask.unused, ["mask.dds"]);

    // `elsewhere.png` and `red.png` are the inputs of the entries that name them, and no texture of their own.
    assert_eq!(all.len(), 4, "{all:?}");
}

#[test]
fn two_images_of_one_name_are_said_to_be_unclear_and_not_guessed_between() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(
        scratch.path(),
        "pack",
        &legacy_dictionary(&[Spec::new("other", DXT5, ramp(16, 16, 1))]),
    );
    save_image(&folder.join("textures/twin.png"), &ramp(16, 16, 7));
    std::fs::copy(
        folder.join("textures/twin.png"),
        folder.join("textures/twin.bmp"),
    )
    .unwrap();
    let all = inputs(&folder);
    let twin = of(&all, "twin");
    assert_eq!(twin.kind, InputKind::Unclear);
    assert!(
        twin.sentence().contains("twin.png") && twin.sentence().contains("twin.bmp"),
        "{}",
        twin.sentence()
    );
}

fn sixteen_bit_png(path: &Path) {
    let image: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_fn(8, 8, |x, y| {
            image::Rgba([(x * 8000) as u16, (y * 8000) as u16, 9, 65535])
        });
    image.save(path).unwrap();
}

#[test]
fn a_16_bit_picture_put_into_a_source_is_given_the_whole_range_as_a_stated_rule_that_is_shown() {
    let scratch = tempfile::tempdir().unwrap();
    let pictures = scratch.path().join("pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    sixteen_bit_png(&pictures.join("deep.png"));
    save_image(&pictures.join("plain.png"), &ramp(8, 8, 1));

    let made = author_textures(
        scratch.path(),
        "pics",
        &[
            (pictures.join("deep.png"), "deep.png".to_string()),
            (pictures.join("plain.png"), "plain.png".to_string()),
        ],
    )
    .unwrap();

    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(made.path.join("textures.json")).unwrap())
            .unwrap();
    let list = written["textures"].as_array().unwrap();
    assert_eq!(
        list.len(),
        1,
        "only the 16-bit picture needs a rule: {written}"
    );
    assert_eq!(list[0]["name"], "deep");
    assert_eq!(list[0]["encode"]["clamp"], json!([0.0, 1.0]));

    let bytes = textures::compile(&Source::open(&made.path).unwrap())
        .unwrap()
        .bytes;
    assert!(legacy(&bytes).get("deep").is_some());

    let all = inputs(&made.path);
    let deep = of(&all, "deep");
    assert!(
        deep.rules
            .iter()
            .any(|r| r.contains("clamp") && r.contains('0') && r.contains('1')),
        "the inspector shows the rule: {:?}",
        deep.rules
    );
    assert!(of(&all, "plain").rules.is_empty());
}

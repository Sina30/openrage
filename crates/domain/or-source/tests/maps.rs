//! Normal and specular maps made from a diffuse texture: written as new pictures of the source, never over what is there, and
//! listed before they are made.

mod common;

use common::*;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_source::asset::Source;
use or_source::game_import::tree_digest;
use or_source::maps::{self, Kind, Options};
use or_source::textures;
use serde_json::json;

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;

/// A weave: bright and dark threads, so that the picture has a slope everywhere.
fn weave(side: u32) -> RgbaImage {
    let mut image = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let v = 40 + 150 * (((x / 4 + y / 4) % 2) as u8) + (x % 4) as u8 * 10;
            image.set_pixel(x, y, [v, v / 2 + 20, v / 3, 255]);
        }
    }
    image
}

fn flat_normal() -> [u8; 4] {
    [128, 128, 255, 255]
}

#[test]
fn a_normal_map_slopes_with_the_diffuse_and_a_flat_diffuse_makes_a_flat_one() {
    // A picture that brightens to the right is a surface that rises to the right: it faces left, so red is below the middle.
    let mut ramp_x = RgbaImage::new(16, 16);
    let mut ramp_y = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            ramp_x.set_pixel(x, y, [(x * 12) as u8; 3].into_rgba());
            ramp_y.set_pixel(x, y, [(y * 12) as u8; 3].into_rgba());
        }
    }
    let across = maps::normal_from(&ramp_x, 2.0);
    let down = maps::normal_from(&ramp_y, 2.0);
    // The middle of the picture, away from the edge where the picture wraps.
    assert!(
        across.get_pixel(8, 8)[0] < 120,
        "{:?}",
        across.get_pixel(8, 8)
    );
    assert_eq!(across.get_pixel(8, 8)[1], 128);
    assert!(down.get_pixel(8, 8)[1] < 120, "{:?}", down.get_pixel(8, 8));
    assert_eq!(down.get_pixel(8, 8)[0], 128);

    // A stronger slope tilts further, and a flat picture is flat.
    let steeper = maps::normal_from(&ramp_x, 6.0);
    assert!(steeper.get_pixel(8, 8)[0] < across.get_pixel(8, 8)[0]);
    let plain = maps::normal_from(&flat(16, 16, [[90, 90, 90, 255]; 4]), 2.0);
    assert!(plain
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .all(|t| *t == flat_normal()));

    // The weave is not flat, and every texel is a unit-ish vector pointing out of the picture.
    let woven = maps::normal_from(&weave(32), 2.0);
    assert!(woven
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .any(|t| *t != flat_normal()));
    assert!(woven
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .all(|t| t[2] >= 128 && t[3] == 255));
}

trait IntoRgba {
    fn into_rgba(self) -> [u8; 4];
}

impl IntoRgba for [u8; 3] {
    fn into_rgba(self) -> [u8; 4] {
        [self[0], self[1], self[2], 255]
    }
}

#[test]
fn a_specular_map_follows_the_brightness_of_the_diffuse() {
    let mut dark_to_light = RgbaImage::new(8, 1);
    for x in 0..8 {
        dark_to_light.set_pixel(x, 0, [(x * 30) as u8; 3].into_rgba());
    }
    let spec = maps::specular_from(&dark_to_light);
    let red: Vec<u8> = (0..8).map(|x| spec.get_pixel(x, 0)[0]).collect();
    assert!(
        red.windows(2).all(|w| w[0] <= w[1]) && red[7] > red[0],
        "{red:?}"
    );
    // The other two measures are the same everywhere: they are estimates and an artist replaces them.
    let green: Vec<u8> = (0..8).map(|x| spec.get_pixel(x, 0)[1]).collect();
    assert!(green.windows(2).all(|w| w[0] == w[1]), "{green:?}");
}

#[test]
fn the_maps_of_a_diffuse_are_listed_before_and_written_as_new_pictures_beside_it() {
    let scratch = tempfile::tempdir().unwrap();
    let original = legacy_dictionary(&[
        Spec::new("jbib_diff_000_a_uni", DXT1, weave(64)),
        Spec::new("badge", DXT1, ramp(16, 16, 3)),
    ]);
    let folder = extract(scratch.path(), "pack", &original);
    let before = tree_digest(&folder).unwrap();

    // The plan is made of names, and changes no byte.
    let plan = maps::plan(&folder, &Options::default()).unwrap();
    assert_eq!(plan.generated.len(), 1, "{plan:?}");
    let one = &plan.generated[0];
    assert_eq!(one.diffuse, "jbib_diff_000_a_uni");
    let names: Vec<(Kind, &str)> = one
        .maps
        .iter()
        .map(|m| (m.kind, m.texture.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (Kind::Normal, "jbib_normal_000_a_uni"),
            (Kind::Specular, "jbib_spec_000_a_uni")
        ]
    );
    assert!(
        plan.left
            .iter()
            .any(|l| l.texture == "badge" && l.why.contains("diff")),
        "a texture whose name does not say diffuse is left, with why: {:?}",
        plan.left
    );
    assert_eq!(
        tree_digest(&folder).unwrap(),
        before,
        "a plan writes nothing"
    );

    // Making them adds pictures and entries, and the diffuse is not touched.
    let diffuse_before = std::fs::read(folder.join("textures/jbib_diff_000_a_uni.dds")).unwrap();
    let done = maps::apply(&folder, &Options::default(), &|| false).unwrap();
    assert_eq!(done.generated, plan.generated);
    assert!(folder.join("textures/jbib_normal_000_a_uni.png").is_file());
    assert!(folder.join("textures/jbib_spec_000_a_uni.png").is_file());
    assert_eq!(
        std::fs::read(folder.join("textures/jbib_diff_000_a_uni.dds")).unwrap(),
        diffuse_before
    );

    let bytes = compile(&folder);
    let dictionary = legacy(&bytes);
    let normal = dictionary
        .get("jbib_normal_000_a_uni")
        .expect("the normal map compiles");
    assert_eq!(normal.info_bits & 1, 0, "a normal map is stored linear");
    assert!(dictionary.get("jbib_spec_000_a_uni").is_some());
    let level0 = level(&bytes, "jbib_normal_000_a_uni", 0);
    assert!(
        level0
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|t| t[0].abs_diff(128) > 4 || t[1].abs_diff(128) > 4)
            .count()
            > 100,
        "the compiled normal map is not flat"
    );

    // Run again, it finds them there and writes nothing.
    let after = tree_digest(&folder).unwrap();
    let again = maps::apply(&folder, &Options::default(), &|| false).unwrap();
    assert!(again.generated.is_empty(), "{again:?}");
    assert!(
        again.left.iter().any(|l| l.why.contains("already")),
        "{again:?}"
    );
    assert_eq!(tree_digest(&folder).unwrap(), after);
}

#[test]
fn a_texture_named_in_the_request_is_made_maps_of_whatever_its_name_and_one_kind_can_be_asked() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(
        scratch.path(),
        "pack",
        &legacy_dictionary(&[Spec::new("badge", DXT1, weave(32))]),
    );
    let only_normal = Options {
        specular: false,
        only: vec!["badge".to_string()],
        ..Options::default()
    };
    let done = maps::apply(&folder, &only_normal, &|| false).unwrap();
    assert_eq!(done.generated.len(), 1);
    assert_eq!(done.generated[0].maps.len(), 1);
    assert_eq!(done.generated[0].maps[0].texture, "badge_n");
    let rules: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("textures.json")).unwrap())
            .unwrap();
    let entry = rules["textures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "badge_n")
        .expect("an entry for the new texture");
    assert_eq!(entry["encode"]["colour_space"], json!("linear"));
    assert!(textures::compile(&Source::open(&folder).unwrap()).is_ok());
}

#[test]
fn a_stop_asked_for_leaves_the_source_as_it_was() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(
        scratch.path(),
        "pack",
        &legacy_dictionary(&[Spec::new("jbib_diff_000_a_uni", DXT1, weave(32))]),
    );
    let before = tree_digest(&folder).unwrap();
    assert!(maps::apply(&folder, &Options::default(), &|| true).is_err());
    assert_eq!(tree_digest(&folder).unwrap(), before);
}

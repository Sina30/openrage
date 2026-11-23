//! Putting one image into a `.ytd`: the texture it replaces changes, every other texture keeps its bytes, and an image this build does not read is refused.

mod common;

use common::*;
use or_io::codec::texture::GrcTextureFormat;
use or_source::textures::replace_image;

fn two_textures() -> Vec<u8> {
    legacy_dictionary(&[
        Spec::new("bark", GrcTextureFormat::DXT5, ramp(32, 32, 1)),
        Spec::new("stone", GrcTextureFormat::DXT5, ramp(32, 32, 2)),
    ])
}

fn mips_of(bytes: &[u8], name: &str) -> Vec<Vec<u8>> {
    let dictionary = legacy(bytes);
    let texture = dictionary
        .entries
        .iter()
        .flatten()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} is in the dictionary"));
    mips(&**texture)
}

#[test]
fn an_image_named_for_a_texture_replaces_that_texture_and_leaves_the_others_as_they_were() {
    let ytd = two_textures();
    let folder = tempfile::tempdir().unwrap();
    let image = folder.path().join("stone.png");
    save_image(&image, &quadrants(32, 32));

    let scratch = tempfile::tempdir().unwrap();
    let replaced =
        replace_image(&ytd, &image, scratch.path()).expect("the image replaces a texture");

    assert_ne!(
        mips_of(&replaced, "stone"),
        mips_of(&ytd, "stone"),
        "the image did not reach the texture it is named for"
    );
    assert_eq!(
        mips_of(&replaced, "bark"),
        mips_of(&ytd, "bark"),
        "a texture the image is not named for keeps every mip"
    );
    let shown = level(&replaced, "stone", 0);
    let wanted = quadrants(32, 32);
    let off = worst(&shown, &wanted);
    assert!(
        off.iter().all(|c| *c < 40),
        "the compiled texture shows the image: {off:?}"
    );
}

#[test]
fn an_image_named_for_no_texture_replaces_the_first_of_the_dictionary_and_the_scratch_is_left_clean(
) {
    // Two names whose alphabetical order is not the order the dictionary stores them in, so the first by name is not the first stored.
    let (a, b) = [
        ("bark", "stone"),
        ("stone", "zinc"),
        ("alpha", "omega"),
        ("brick", "glass"),
    ]
    .into_iter()
    .find(|(a, b)| or_io::string_hash_str(a) > or_io::string_hash_str(b))
    .expect("some pair is stored in the other order");
    let ytd = legacy_dictionary(&[
        Spec::new(a, GrcTextureFormat::DXT5, ramp(32, 32, 1)),
        Spec::new(b, GrcTextureFormat::DXT5, ramp(32, 32, 2)),
    ]);
    let stored_first = legacy(&ytd)
        .entries
        .iter()
        .flatten()
        .next()
        .unwrap()
        .name
        .clone();
    assert_eq!(stored_first, b, "the dictionary stores {b} first");
    let folder = tempfile::tempdir().unwrap();
    let image = folder.path().join("the_variant.png");
    save_image(&image, &quadrants(32, 32));

    let scratch = tempfile::tempdir().unwrap();
    let replaced = replace_image(&ytd, &image, scratch.path()).expect("replaced");
    assert_ne!(
        mips_of(&replaced, b),
        mips_of(&ytd, b),
        "the first stored is made from the image"
    );
    assert_eq!(mips_of(&replaced, a), mips_of(&ytd, a));
    assert_eq!(
        std::fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "the folders the replacement made are gone"
    );
}

#[test]
fn a_file_this_build_cannot_read_as_an_image_is_refused_with_a_sentence() {
    let ytd = two_textures();
    let folder = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    for name in ["art.xcf", "art.ytd", "art"] {
        let image = folder.path().join(name);
        std::fs::write(&image, b"not an image").unwrap();
        let problems = replace_image(&ytd, &image, scratch.path()).expect_err(name);
        assert!(
            problems.iter().any(|p| p.message.contains("image")),
            "{name}: {problems:?}"
        );
    }
    let missing = folder.path().join("nothing.png");
    assert!(replace_image(&ytd, &missing, scratch.path()).is_err());
}

#[test]
fn a_16_bit_image_replaces_a_texture_with_the_whole_range_stated_for_it() {
    let ytd = two_textures();
    let folder = tempfile::tempdir().unwrap();
    let image = folder.path().join("stone.png");
    let deep: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_fn(32, 32, |x, y| {
            image::Rgba([(x * 2000) as u16, (y * 2000) as u16, 30_000, 65_535])
        });
    deep.save(&image).unwrap();

    let scratch = tempfile::tempdir().unwrap();
    let replaced = replace_image(&ytd, &image, scratch.path())
        .expect("a picture that is put in is given the rule that compiles it");

    assert_ne!(mips_of(&replaced, "stone"), mips_of(&ytd, "stone"));
    assert_eq!(mips_of(&replaced, "bark"), mips_of(&ytd, "bark"));
}

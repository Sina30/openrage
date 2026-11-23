//! A texture is given once to each variation of a garment: a `.ytd` and a picture, or two pictures, whose names
//! state one variation are one texture, so a folder that keeps a picture beside each texture under its name does not
//! double the textures of its garments. The `.ytd` is kept, else the first picture, and the other is left out by name.

mod common;

use common::{ydd, ytd};
use std::path::{Path, PathBuf};

use or_wardrobe::edit::{add_clothes, undo};
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, bytes).unwrap();
}

/// A 32 by 32 picture in the format the extension says.
fn picture(root: &Path, rel: &str) {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    image::RgbaImage::from_fn(32, 32, |x, y| {
        image::Rgba([(x * 8) as u8, (y * 8) as u8, 128, 255])
    })
    .save(at)
    .unwrap();
}

/// The textures the one garment of the pack ships, as the pack names them.
fn textures(module: &Path) -> Vec<String> {
    let pack = Wardrobe::read(module).unwrap();
    assert_eq!(pack.garments.len(), 1, "{pack:?}");
    (pack.garments[0].variations.iter())
        .map(|v| v.texture.clone().expect("a texture"))
        .collect()
}

#[test]
fn a_ytd_and_the_picture_beside_it_under_its_name_are_one_texture_whichever_sorts_first() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    put(
        &drop,
        &format!("{C}^jbib_diff_001_a_uni.ytd"),
        &ytd("texture a"),
    );
    // `png` sorts before `ytd`, so a rule of the first would keep the picture.
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.png"));
    put(
        &drop,
        &format!("{C}^jbib_diff_001_b_uni.ytd"),
        &ytd("texture b"),
    );

    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop is added");

    assert_eq!(added.garments.len(), 1, "{added:?}");
    assert_eq!(added.textures, 2, "{added:?}");
    assert_eq!(
        textures(&module),
        ["a", "b"].map(|l| format!("textures/{C}^jbib_diff_001_{l}_uni.textures.src"))
    );
    assert_eq!(added.left_out.len(), 1, "{:?}", added.left_out);
    assert!(
        added.left_out[0].path.ends_with("jbib_diff_001_a_uni.png"),
        "{:?}",
        added.left_out
    );
    assert!(
        added.left_out[0].why.contains("jbib_diff_001_a_uni.ytd"),
        "it names the file that holds the variation: {:?}",
        added.left_out
    );
    // The texture kept is the `.ytd` dropped, decompiled, and not a source made of the picture.
    let kept = or_source::asset::Source::open(&module.join(&textures(&module)[0])).unwrap();
    assert_eq!(
        kept.manifest.origin.sha256,
        Some(or_source::asset::sha256_hex(&ytd("texture a")))
    );

    // Dropped again, the garment is the one the pack holds.
    let again = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();
    assert!(again.garments.is_empty(), "{again:?}");
    assert_eq!(again.already_held.len(), 1, "{again:?}");

    undo(&module, &added).expect("the drop is taken back");
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());
}

#[test]
fn two_pictures_of_one_variation_are_one_texture_and_the_first_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.png"));
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.tga"));
    picture(&drop, &format!("{C}^jbib_diff_001_b_uni.bmp"));

    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop is added");

    assert_eq!(added.textures, 2, "{added:?}");
    assert_eq!(
        textures(&module),
        ["a", "b"].map(|l| format!("textures/{C}^jbib_diff_001_{l}_uni.textures.src"))
    );
    assert_eq!(added.left_out.len(), 1, "{:?}", added.left_out);
    assert!(
        added.left_out[0].path.ends_with("jbib_diff_001_a_uni.tga"),
        "{:?}",
        added.left_out
    );
    assert!(
        added.left_out[0].why.contains("jbib_diff_001_a_uni.png"),
        "{:?}",
        added.left_out
    );
}

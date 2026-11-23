//! A garment the pack holds is not added again, and what makes it the same garment is its type (the ped and the slot),
//! its model bytes and, where the drop brings textures, its texture bytes. Two garments with equal models and different
//! textures are not duplicates.

mod common;

use std::path::{Path, PathBuf};

use or_wardrobe::edit::add_clothes;
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";
const F: &str = "mp_f_freemode_01_mp_f_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, bytes).unwrap();
    at
}

/// A folder holding one jacket of `model` bytes with the texture `texture`, as `prefix` states its ped and place.
/// The valid file the words stand for: the same words are the same bytes.
fn tagged(words: &[u8], model: bool) -> Vec<u8> {
    let words = String::from_utf8_lossy(words);
    match model {
        true => common::ydd(&words),
        false => common::ytd(&words),
    }
}

fn drop_of(
    dir: &Path,
    name: &str,
    prefix: &str,
    place: &str,
    model: &[u8],
    texture: &[u8],
) -> PathBuf {
    let drop = dir.join("downloads").join(name);
    put(
        &drop,
        &format!("{prefix}^jbib_{place}_u.ydd"),
        &tagged(model, true),
    );
    put(
        &drop,
        &format!("{prefix}^jbib_diff_{place}_a_uni.ytd"),
        &tagged(texture, false),
    );
    drop
}

#[test]
fn equal_models_with_different_textures_are_added_and_equal_textures_are_the_garment_held() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let first = drop_of(dir.path(), "first", C, "000", b"one jacket", b"red");
    assert_eq!(add_clothes(&module, &[first]).unwrap().garments.len(), 1);

    // The same model with another texture is another garment: a colour that ships as a drawable of its own.
    let blue = drop_of(dir.path(), "blue", C, "001", b"one jacket", b"blue");
    let added = add_clothes(&module, &[blue]).unwrap();
    assert_eq!(added.garments.len(), 1, "{added:?}");
    assert!(added.already_held.is_empty(), "{added:?}");

    // The same model with the texture of the first is the first, whatever its place and file names say.
    let again = drop_of(dir.path(), "again", C, "007", b"one jacket", b"red");
    let added = add_clothes(&module, &[again]).unwrap();
    assert!(added.garments.is_empty(), "{added:?}");
    assert_eq!(added.already_held.len(), 1, "{added:?}");
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 2);
}

#[test]
fn the_same_model_and_texture_for_the_other_ped_or_another_slot_is_another_garment() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let first = drop_of(dir.path(), "first", C, "000", b"one jacket", b"red");
    add_clothes(&module, &[first]).unwrap();

    let female = drop_of(dir.path(), "female", F, "000", b"one jacket", b"red");
    let added = add_clothes(&module, &[female]).unwrap();
    assert_eq!(
        added.garments.len(),
        1,
        "the other ped is another type: {added:?}"
    );

    let trousers = dir.path().join("downloads").join("trousers");
    put(
        &trousers,
        &format!("{C}^lowr_000_u.ydd"),
        &common::ydd("one jacket"),
    );
    put(
        &trousers,
        &format!("{C}^lowr_diff_000_a_uni.ytd"),
        &common::ytd("red"),
    );
    let added = add_clothes(&module, &[trousers]).unwrap();
    assert_eq!(
        added.garments.len(),
        1,
        "another slot is another type: {added:?}"
    );
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 3);
}

#[test]
fn a_drop_of_a_models_alone_is_the_garment_the_pack_holds_of_that_type_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let first = drop_of(dir.path(), "first", C, "000", b"one jacket", b"red");
    add_clothes(&module, &[first]).unwrap();

    let models = dir.path().join("downloads").join("models");
    put(
        &models,
        &format!("{C}^jbib_005_u.ydd"),
        &common::ydd("one jacket"),
    );
    let added = add_clothes(&module, &[models]).unwrap();

    assert!(added.garments.is_empty(), "{added:?}");
    assert_eq!(added.already_held.len(), 1);
}

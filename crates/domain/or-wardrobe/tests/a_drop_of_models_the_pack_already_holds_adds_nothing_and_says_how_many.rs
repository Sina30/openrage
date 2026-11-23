//! Dropping a folder twice adds its garments once: a garment the pack already holds (its ped, its slot,
//! its model bytes and its texture bytes) is not added again, and the drop says how many it left alone and which.

mod common;

use common::{ydd, ytd};
use std::path::{Path, PathBuf};

use or_wardrobe::edit::add_clothes;
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";

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

fn a_pack_folder(drop: &Path) {
    put(drop, &format!("{C}^jbib_000_u.ydd"), &ydd("jacket one"));
    put(drop, &format!("{C}^jbib_diff_000_a_uni.ytd"), &ytd("tex a"));
    put(drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket two"));
    put(drop, &format!("{C}^jbib_diff_001_a_uni.ytd"), &ytd("tex b"));
}

#[test]
fn a_folder_dropped_twice_adds_its_garments_once_and_the_second_drop_says_how_many_it_left() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    a_pack_folder(&drop);

    let first = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();
    assert_eq!(first.garments.len(), 2);
    assert!(first.already_held.is_empty());
    let held = Wardrobe::read(&module).unwrap();

    let again = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();
    assert!(again.garments.is_empty(), "{again:?}");
    assert_eq!(again.already_held.len(), 2, "{:?}", again.already_held);
    assert!(
        again.already_held[0].ends_with("jbib_000_u.ydd"),
        "it names the files it left: {:?}",
        again.already_held
    );
    assert!(
        again.copied.is_empty() && again.made.is_empty(),
        "nothing is copied for a garment that is not added"
    );
    assert_eq!(
        Wardrobe::read(&module).unwrap(),
        held,
        "the pack is as the first drop left it"
    );
}

#[test]
fn a_drop_of_one_new_model_and_one_the_pack_holds_adds_the_new_with_its_textures() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    a_pack_folder(&drop);
    add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    put(&drop, &format!("{C}^lowr_000_r.ydd"), &ydd("trousers"));
    put(
        &drop,
        &format!("{C}^lowr_diff_000_a_whi.ytd"),
        &ytd("tex t"),
    );
    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 1, "{added:?}");
    assert_eq!(added.already_held.len(), 2);
    assert_eq!(added.textures, 1);
    let pack = Wardrobe::read(&module).unwrap();
    assert_eq!(pack.garments.len(), 3);
    assert!(pack.garments[2].variations[0]
        .texture
        .as_deref()
        .is_some_and(|t| t.ends_with("lowr_diff_000_a_whi.textures.src")));
}

#[test]
fn a_model_with_other_bytes_under_a_name_the_pack_holds_is_a_new_garment() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    a_pack_folder(&drop);
    add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    // The same name with a different jacket in it is not the model the pack holds.
    put(
        &drop,
        &format!("{C}^jbib_000_u.ydd"),
        &ydd("a newer jacket one"),
    );
    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 1, "{added:?}");
    assert_eq!(
        added.already_held.len(),
        1,
        "the other jacket is still held"
    );
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 3);
}

#[test]
fn a_model_already_inside_the_packs_folder_is_not_added_a_second_time() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    a_pack_folder(&drop);
    add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    // Dropping the pack's own folder of models names the files where they are, which is the same garment.
    let again = add_clothes(&module, &[module.join("models")]).unwrap();
    assert!(again.garments.is_empty(), "{again:?}");
    assert_eq!(again.already_held.len(), 2);
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 2);
}

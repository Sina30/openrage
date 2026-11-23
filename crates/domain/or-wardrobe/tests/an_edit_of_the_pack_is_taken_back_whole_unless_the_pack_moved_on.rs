//! Every change a screen makes to a pack can be taken back: a revision keeps the pack as it was and
//! as the change left it, so Undo restores exactly that, and refuses once something else has changed
//! the pack since, rather than put back a pack that never existed.

mod common;

use common::{ydd, ytd};
use std::path::{Path, PathBuf};

use or_wardrobe::edit::{
    add_clothes, apply, locate_texture, moved, remedy, remove_texture, revise, undo_revision, Edit,
};
use or_wardrobe::{allocate, check, facts, Depth, Wardrobe};

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

/// A pack of `n` jackets, each a model and a texture, in `dir`'s `clothing` folder.
fn a_pack_of_jackets(dir: &Path, n: u32) -> PathBuf {
    let module = module(dir);
    let drop = dir.join("drop");
    for i in 0..n {
        put(
            &drop,
            &format!("{C}^jbib_{i:03}_u.ydd"),
            &ydd(&format!("jacket {i}")),
        );
        put(
            &drop,
            &format!("{C}^jbib_diff_{i:03}_a_uni.ytd"),
            &ytd(&format!("texture {i}")),
        );
    }
    add_clothes(&module, &[drop]).unwrap();
    module
}

#[test]
fn a_revision_holds_the_pack_before_and_after_and_undo_puts_the_one_back() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 2);
    let id = Wardrobe::read(&module).unwrap().garments[0].id.clone();
    let before = Wardrobe::read(&module).unwrap();

    let ((), revision) = revise(&module, |w| apply(w, &id, Edit::Slot(None))).unwrap();
    let revision = revision.expect("the slot changed, so the pack did");
    assert_eq!(revision.before, before);
    assert_eq!(Wardrobe::read(&module).unwrap().garments[0].slot, None);
    assert_eq!(revision.after, Wardrobe::read(&module).unwrap());

    undo_revision(&module, &revision).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), before);
}

#[test]
fn a_change_that_leaves_the_pack_as_it_was_has_nothing_to_take_back() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 1);
    let id = Wardrobe::read(&module).unwrap().garments[0].id.clone();
    let ped = Wardrobe::read(&module).unwrap().garments[0].ped;

    let ((), revision) = revise(&module, |w| apply(w, &id, Edit::Ped(ped))).unwrap();
    assert!(revision.is_none());
}

#[test]
fn a_change_that_fails_writes_nothing_and_keeps_no_revision() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 1);
    let held = std::fs::read(module.join("wardrobe.json")).unwrap();

    let refused = revise(&module, |w| apply(w, "no-such-id", Edit::Label(None)));
    assert!(refused.is_err());
    assert_eq!(std::fs::read(module.join("wardrobe.json")).unwrap(), held);
}

#[test]
fn undo_after_the_fix_of_a_duplicate_restores_the_garment_the_fix_removed() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("drop");
    for i in 0..2 {
        put(
            &drop,
            &format!("{C}^jbib_{i:03}_u.ydd"),
            &ydd("one jacket twice"),
        );
        put(
            &drop,
            &format!("{C}^jbib_diff_{i:03}_a_uni.ytd"),
            &ytd("the texture of both"),
        );
    }
    add_clothes(&module, &[drop]).unwrap();
    let before = Wardrobe::read(&module).unwrap();
    assert_eq!(before.garments.len(), 2);

    let found = check(
        &before,
        &allocate(&before),
        &facts::inspect(&module, &before, Depth::Digest),
    );
    let duplicate = found
        .iter()
        .find(|f| f.code == "duplicate-content")
        .expect("two garments of one model");
    let fix = remedy(&before, duplicate).expect("a duplicate has a remedy");
    let ((), revision) = revise(&module, |w| fix.apply(w)).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap().garments.len(),
        1,
        "the fix removed one"
    );

    undo_revision(&module, &revision.expect("a change")).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap(),
        before,
        "the garment is back, where it was, with all it stated"
    );
}

#[test]
fn undo_is_refused_once_the_pack_changed_after_the_edit_and_the_pack_is_left_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 2);
    let ids: Vec<String> = Wardrobe::read(&module)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();

    let ((), first) = revise(&module, |w| apply(w, &ids[0], Edit::Position(Some(9)))).unwrap();
    revise(&module, |w| apply(w, &ids[1], Edit::Position(Some(8)))).unwrap();
    let now = Wardrobe::read(&module).unwrap();

    let why = undo_revision(&module, &first.unwrap())
        .unwrap_err()
        .to_string();
    assert!(why.contains("changed after"), "{why}");
    assert_eq!(Wardrobe::read(&module).unwrap(), now);
}

#[test]
fn a_texture_that_is_missing_is_located_by_copying_the_file_in_and_undo_takes_the_copy_away() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 1);
    let pack = Wardrobe::read(&module).unwrap();
    let id = pack.garments[0].id.clone();
    let named = pack.garments[0].variations[0].texture.clone().unwrap();
    std::fs::remove_dir_all(module.join(&named)).unwrap();
    let found = ytd("the texture, found");
    let elsewhere = put(dir.path(), "elsewhere/found.ytd", &found);

    let revision = locate_texture(&module, &id, 0, &named, &elsewhere).unwrap();
    let pack = Wardrobe::read(&module).unwrap();
    let now = module.join(pack.garments[0].variations[0].texture.as_deref().unwrap());
    assert!(
        now.join("asset.json").is_file(),
        "the garment names a source that is there: {now:?}"
    );
    assert_eq!(
        or_source::asset::Source::open(&now)
            .unwrap()
            .manifest
            .origin
            .sha256,
        Some(or_source::asset::sha256_hex(&found)),
        "the file found is decompiled, not copied"
    );
    assert!(
        elsewhere.is_file(),
        "the file it was found in is not touched"
    );

    undo_revision(&module, &revision).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), revision.before);
    assert!(!now.exists(), "the copy the locate made goes with it");
}

#[test]
fn only_a_ytd_file_can_be_located_as_a_texture_and_a_variation_that_is_not_there_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 1);
    let id = Wardrobe::read(&module).unwrap().garments[0].id.clone();
    let text = put(dir.path(), "elsewhere/notes.txt", b"not a texture");
    let ytd = put(dir.path(), "elsewhere/found.ytd", &ytd("a texture"));

    let named = Wardrobe::read(&module).unwrap().garments[0].variations[0]
        .texture
        .clone()
        .unwrap();
    let why = locate_texture(&module, &id, 0, &named, &text)
        .unwrap_err()
        .to_string();
    assert!(why.contains(".ytd"), "{why}");
    let why = locate_texture(&module, &id, 5, &named, &ytd)
        .unwrap_err()
        .to_string();
    assert!(why.contains("1 texture") || why.contains("not 6"), "{why}");
    // A row drawn before the textures moved names another file than the one at its index now.
    let why = locate_texture(&module, &id, 0, "textures/some-other.ytd", &ytd)
        .unwrap_err()
        .to_string();
    assert!(why.contains("look at the problems again"), "{why}");
}

#[test]
fn removing_a_texture_takes_that_variation_out_and_the_later_ones_move_up_a_letter() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("drop");
    put(&drop, &format!("{C}^jbib_000_u.ydd"), &ydd("jacket"));
    for letter in ["a", "b", "c"] {
        put(
            &drop,
            &format!("{C}^jbib_diff_000_{letter}_uni.ytd"),
            &ytd(letter),
        );
    }
    add_clothes(&module, &[drop]).unwrap();
    let pack = Wardrobe::read(&module).unwrap();
    let id = pack.garments[0].id.clone();
    let b = pack.garments[0].variations[1].texture.clone();
    let named = b.clone().unwrap();

    let ((), revision) = revise(&module, |w| remove_texture(w, &id, 1, &named)).unwrap();
    let after = Wardrobe::read(&module).unwrap();
    assert_eq!(after.garments[0].variations.len(), 2);
    assert_ne!(
        after.garments[0].variations[1].texture, b,
        "c took b's letter"
    );
    assert!(revise(&module, |w| remove_texture(w, &id, 7, &named)).is_err());
    // The same row once more names a texture that is not at its index any more, and is refused.
    let again = revise(&module, |w| remove_texture(w, &id, 1, &named));
    assert!(again
        .unwrap_err()
        .to_string()
        .contains("look at the problems again"));

    undo_revision(&module, &revision.unwrap()).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), pack);
}

#[test]
fn taking_a_garment_out_says_which_garments_moved_to_another_place() {
    let dir = tempfile::tempdir().unwrap();
    let module = a_pack_of_jackets(dir.path(), 4);
    let before = Wardrobe::read(&module).unwrap();
    let mut after = before.clone();
    let gone = after.garments[1].id.clone();
    apply(&mut after, &gone, Edit::Remove).unwrap();

    let moves = moved(&before, &after);
    assert_eq!(moves.len(), 2, "the two after it move up one: {moves:?}");
    assert_eq!((moves[0].from, moves[0].to), (2, 1));
    assert_eq!((moves[1].from, moves[1].to), (3, 2));
    assert_eq!(moves[0].garment, before.garments[2].id);
    assert!(moves[0].name.contains("jbib_002"), "{}", moves[0].name);

    // A garment pinned where it is does not move, whatever leaves around it.
    let mut pinned = before.clone();
    let ids: Vec<String> = pinned.garments.iter().map(|g| g.id.clone()).collect();
    for (i, id) in ids.iter().enumerate().skip(2) {
        apply(&mut pinned, id, Edit::Position(Some(100 + i as u32))).unwrap();
    }
    let mut after = pinned.clone();
    apply(&mut after, &gone, Edit::Remove).unwrap();
    assert_eq!(moved(&pinned, &after), Vec::new());
}

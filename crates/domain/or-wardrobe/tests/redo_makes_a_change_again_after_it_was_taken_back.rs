//! Redo makes a change again after Undo took it back: a fix writes the pack it wrote, a drop brings back its garments and the
//! very files it copied, a cloth is linked again, and a pack that something changed since refuses it rather than lose that.

mod common;

use std::path::{Path, PathBuf};

use common::{ydd, ytd};

use or_wardrobe::edit::{
    add_clothes, apply, link_cloth, remedy, revise, undo, undo_cloth, undo_revision, Edit,
};
use or_wardrobe::{allocate, check, facts, purge_held, redo, Depth, Wardrobe};

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

/// A folder of `n` jackets, each a model and a texture, to drop.
fn jackets(dir: &Path, n: u32) -> PathBuf {
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
    drop
}

fn held_files(module: &Path) -> usize {
    let held = module.join(".openrage").join("held");
    walk(&held)
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.path().is_dir() {
            true => walk(&e.path()),
            false => 1,
        })
        .sum()
}

#[test]
fn redo_after_undo_writes_the_pack_a_fix_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("drop");
    for name in ["jbib_000_u.ydd", "jbib_001_u.ydd"] {
        put(&drop, &format!("{C}^{name}"), &ydd("the same jacket"));
    }
    add_clothes(&module, &[drop]).unwrap();
    let finding = {
        let w = Wardrobe::read(&module).unwrap();
        let f = facts::inspect(&module, &w, Depth::Digest);
        check(&w, &allocate(&w), &f)
            .into_iter()
            .find(|f| f.code == "duplicate-content")
            .expect("two jackets of one model are a duplicate")
    };
    let (what, revision) = revise(&module, |w| {
        let fix = remedy(w, &finding).expect("a duplicate has a remedy");
        fix.apply(w)?;
        Ok(fix.what)
    })
    .unwrap();
    assert!(!what.is_empty());
    let revision = revision.expect("the fix changed the pack");
    let fixed = Wardrobe::read(&module).unwrap();
    assert_eq!(fixed.garments.len(), 1);

    let made_again = undo_revision(&module, &revision).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), revision.before);
    assert_eq!(made_again.before, revision.before);
    assert_eq!(made_again.after, fixed);

    redo(&module, &made_again).unwrap();
    assert_eq!(
        Wardrobe::read(&module).unwrap(),
        fixed,
        "the fix is made again"
    );
    undo_revision(&module, &revision).expect("and it can be taken back again");
}

#[test]
fn redo_of_a_drop_brings_back_its_garments_and_the_very_files_it_copied() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let added = add_clothes(&module, &[jackets(dir.path(), 2)]).unwrap();
    let dropped = Wardrobe::read(&module).unwrap();
    assert_eq!(dropped.garments.len(), 2);
    let copies: Vec<(String, Vec<u8>)> = added
        .copied
        .iter()
        .map(|rel| (rel.clone(), std::fs::read(module.join(rel)).unwrap()))
        .collect();
    assert!(
        copies.iter().all(|(rel, _)| rel.contains(".src/")),
        "the models and textures are sources: {copies:?}"
    );

    let made_again = undo(&module, &added).unwrap();
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());
    assert!(copies.iter().all(|(rel, _)| !module.join(rel).exists()));
    assert_eq!(
        held_files(&module),
        copies.len(),
        "the files wait for a Redo and are not deleted"
    );

    redo(&module, &made_again).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), dropped);
    for (rel, bytes) in &copies {
        assert_eq!(&std::fs::read(module.join(rel)).unwrap(), bytes, "{rel}");
    }
    assert_eq!(held_files(&module), 0, "nothing is left waiting");
    undo(&module, &added).expect("the drop is taken back again as it was the first time");
    assert!(copies.iter().all(|(rel, _)| !module.join(rel).exists()));
}

#[test]
fn redo_of_a_linked_cloth_links_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    add_clothes(&module, &[jackets(dir.path(), 1)]).unwrap();
    let id = Wardrobe::read(&module).unwrap().garments[0].id.clone();
    let cloth = put(dir.path(), "outside/jacket.yld", b"cloth bytes");

    let linked = link_cloth(&module, &id, &cloth).unwrap();
    let with_cloth = Wardrobe::read(&module).unwrap();
    let rel = with_cloth.garments[0].cloth.clone().expect("linked");

    let made_again = undo_cloth(&module, &linked).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap().garments[0].cloth, None);
    assert!(!module.join(&rel).exists());

    redo(&module, &made_again).unwrap();
    assert_eq!(Wardrobe::read(&module).unwrap(), with_cloth);
    assert_eq!(std::fs::read(module.join(&rel)).unwrap(), b"cloth bytes");
}

#[test]
fn a_pack_that_changed_since_refuses_redo_and_nothing_moves() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let added = add_clothes(&module, &[jackets(dir.path(), 1)]).unwrap();
    let made_again = undo(&module, &added).unwrap();

    // A new change after the Undo: a second drop.
    let second = dir.path().join("second");
    put(&second, &format!("{C}^lowr_000_u.ydd"), &ydd("trousers"));
    add_clothes(&module, &[second]).unwrap();
    let now = Wardrobe::read(&module).unwrap();

    let refused = redo(&module, &made_again).unwrap_err().to_string();
    assert!(refused.contains("changed after"), "{refused}");
    assert_eq!(Wardrobe::read(&module).unwrap(), now);
    assert_eq!(
        held_files(&module),
        added.copied.len(),
        "the held files stay where they are"
    );
}

#[test]
fn a_file_gone_from_the_held_folder_leaves_its_garment_missing_it_and_does_not_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let added = add_clothes(&module, &[jackets(dir.path(), 1)]).unwrap();
    let made_again = undo(&module, &added).unwrap();
    purge_held(&module);
    assert_eq!(held_files(&module), 0);

    redo(&module, &made_again).unwrap();
    let pack = Wardrobe::read(&module).unwrap();
    assert_eq!(pack.garments.len(), 1, "the garment is back");
    let model = pack.garments[0].model.clone().unwrap();
    assert!(
        !module.join(model).exists(),
        "its file is gone, and the checks say so"
    );
    let facts = facts::inspect(&module, &pack, Depth::Digest);
    assert!(check(&pack, &allocate(&pack), &facts)
        .iter()
        .any(|f| f.code == "model-missing"));
}

#[test]
fn an_edit_that_changes_nothing_is_no_revision_so_no_redo_either() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    add_clothes(&module, &[jackets(dir.path(), 1)]).unwrap();
    let id = Wardrobe::read(&module).unwrap().garments[0].id.clone();
    let ((), revision) = revise(&module, |w| apply(w, &id, Edit::Position(None))).unwrap();
    assert!(revision.is_none());
}

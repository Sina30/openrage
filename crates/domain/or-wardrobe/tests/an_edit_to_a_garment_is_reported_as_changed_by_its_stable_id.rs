//! The diff of two builds says a garment changed when what it writes changed, by its stable id: the
//! bytes of a file it ships, a value of its metadata, or a texture's `texId`; and says nothing of a
//! garment whose files and metadata are the same.
//!
//! The digest the diff compares is read at `Depth::Digest`, which hashes the bytes and never opens
//! them with a reader, so the files below are not game files and no staged file is needed.

mod common;

use std::path::Path;

use common::{garment, JBIB, LOWR};
use or_project::{diff, Item};
use or_wardrobe::{inspect, plan, Depth, Ped, Wardrobe};

fn pack(dir: &Path) -> Wardrobe {
    for (name, bytes) in [
        ("m0.ydd", "model 0"),
        ("m1.ydd", "model 1"),
        ("t0.ytd", "texture 0"),
        ("t1.ytd", "texture 1"),
        ("t2.ytd", "texture 2"),
    ] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let mut w = Wardrobe::new("pack");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "m0.ydd",
        &["t0.ytd", "t1.ytd"],
    ));
    w.garments
        .push(garment(Ped::Male, LOWR, Some(0), "m1.ydd", &["t2.ytd"]));
    w
}

fn items(dir: &Path, w: &Wardrobe) -> Vec<Item> {
    let facts = inspect(dir, w, Depth::Digest);
    plan(w, &facts, "wardrobe", dir).contribution.items
}

/// Only the garment `id` changed between `before` and `after`, and nothing else was added, moved or removed.
fn only_changed(before: &[Item], after: &[Item], id: &str, what: &str) {
    let found = diff(before, after);
    let changed: Vec<&str> = found.changed.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(changed, [id], "{what}");
    assert!(
        found.added.is_empty() && found.moved.is_empty() && found.removed.is_empty(),
        "{what}: {found:?}"
    );
}

#[test]
fn a_garment_that_is_not_edited_is_not_reported_as_changed() {
    let dir = tempfile::tempdir().unwrap();
    let w = pack(dir.path());
    let found = diff(&items(dir.path(), &w), &items(dir.path(), &w));
    assert!(
        found.added.is_empty()
            && found.changed.is_empty()
            && found.moved.is_empty()
            && found.removed.is_empty(),
        "{found:?}"
    );
}

#[test]
fn the_bytes_of_a_file_a_metadata_value_and_a_texture_id_are_each_a_change_of_that_garment() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = pack(dir.path());
    let before = items(dir.path(), &w);
    let jacket = w.garments[0].id.clone();

    // The bytes of one texture, and of the model.
    std::fs::write(dir.path().join("t1.ytd"), "another texture").unwrap();
    only_changed(
        &before,
        &items(dir.path(), &w),
        &jacket,
        "a texture's bytes",
    );
    std::fs::write(dir.path().join("t1.ytd"), "texture 1").unwrap();
    std::fs::write(dir.path().join("m0.ydd"), "another model").unwrap();
    only_changed(
        &before,
        &items(dir.path(), &w),
        &jacket,
        "the model's bytes",
    );
    std::fs::write(dir.path().join("m0.ydd"), "model 0").unwrap();
    assert!(
        diff(&before, &items(dir.path(), &w)).changed.is_empty(),
        "the same bytes again are no change"
    );

    // A value of the metadata, then a variation's `texId`.
    w.garments[0].meta.prop_mask = Some(5);
    only_changed(&before, &items(dir.path(), &w), &jacket, "a metadata value");
    w.garments[0].meta.prop_mask = None;
    w.garments[0].variations[1].tex_id = Some(1);
    only_changed(
        &before,
        &items(dir.path(), &w),
        &jacket,
        "a variation's texId",
    );
}

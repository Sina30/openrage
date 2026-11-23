//! A model or a texture is replaced by pointing the garment at a source made of the new file: the garment keeps everything
//! it states, the file it named is not touched, and Undo names it again where Redo names the source once more.
//!
//! A pack adopted from disk is read in place, so a compiled file in it is never written over; the replacement is a source
//! of the pack, as a dropped file is.

mod common;

use std::path::Path;

use common::{garment, ydd, ytd, JBIB};
use or_project::reference::Reference;
use or_wardrobe::edit::undo_revision;
use or_wardrobe::{redo, replace, Ped, Wardrobe};

fn read(folder: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(folder.join(rel)).unwrap()
}

/// A jacket with a compiled model, two compiled textures that carry their own tint and flags, and a pack that holds it, as an adopted pack does.
fn jacket(folder: &Path) -> String {
    std::fs::create_dir_all(folder.join("models")).unwrap();
    std::fs::create_dir_all(folder.join("textures")).unwrap();
    std::fs::write(folder.join("models/jbib.ydd"), ydd("the first model")).unwrap();
    std::fs::write(
        folder.join("textures/jbib_a.ytd"),
        ytd("the first texture a"),
    )
    .unwrap();
    std::fs::write(
        folder.join("textures/jbib_b.ytd"),
        ytd("the first texture b"),
    )
    .unwrap();
    let mut g = garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jbib.ydd",
        &["textures/jbib_a.ytd", "textures/jbib_b.ytd"],
    );
    g.variations[1].tint = Some("bla".into());
    g.variations[1].tex_id = Some(1);
    g.variations[1].distribution = Some(7);
    g.meta.prop_mask = Some(17);
    g.label = Some("jacket".into());
    let id = g.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(g);
    w.write(folder).unwrap();
    id
}

#[test]
fn a_replaced_model_is_a_source_the_garment_names_and_the_old_file_stays_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = Wardrobe::read(at).unwrap();
    let held = read(at, "models/jbib.ydd");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("better.ydd"), ydd("a better model")).unwrap();

    let change = replace::model(at, &id, &outside.path().join("better.ydd"))
        .expect("a model replaces a model");

    let after = Wardrobe::read(at).unwrap();
    assert_eq!(
        after.garments[0].model.as_deref(),
        Some("models/better.drawables.src"),
        "the model is the source of the file"
    );
    let mut same = after.clone();
    same.garments[0].model = before.garments[0].model.clone();
    assert_eq!(same, before, "the garment states what it stated");
    assert_eq!(
        read(at, "models/jbib.ydd"),
        held,
        "the compiled file is read and never written"
    );
    assert!(at.join("models/better.drawables.src/asset.json").is_file());

    let redo_over = undo_revision(at, &change).expect("the replacement is taken back");
    assert_eq!(Wardrobe::read(at).unwrap(), before);
    assert!(
        !at.join("models/better.drawables.src").exists(),
        "the source the replacement made goes"
    );

    redo(at, &redo_over).expect("and made again");
    assert_eq!(Wardrobe::read(at).unwrap(), after);
    assert!(at.join("models/better.drawables.src/asset.json").is_file());
}

#[test]
fn a_second_replacement_leaves_the_first_and_undo_goes_back_one_step_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = Wardrobe::read(at).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (one, two) = (
        outside.path().join("one.ydd"),
        outside.path().join("two.ydd"),
    );
    std::fs::write(&one, ydd("model one")).unwrap();
    std::fs::write(&two, ydd("model two")).unwrap();

    let first = replace::model(at, &id, &one).unwrap();
    let second = replace::model(at, &id, &two).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap().garments[0].model.as_deref(),
        Some("models/two.drawables.src")
    );

    // The older step cannot be taken back first: the pack is the newer step's now.
    let refused = undo_revision(at, &first).unwrap_err().to_string();
    assert!(refused.contains("changed after"), "{refused}");
    assert!(at.join("models/one.drawables.src").is_dir());

    undo_revision(at, &second).unwrap();
    assert_eq!(
        Wardrobe::read(at).unwrap().garments[0].model.as_deref(),
        Some("models/one.drawables.src")
    );
    undo_revision(at, &first).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), before);
}

#[test]
fn a_replaced_texture_keeps_its_tint_and_flags_and_no_other_texture_changes() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = Wardrobe::read(at).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("new_b.ytd"), ytd("a new texture b")).unwrap();
    let held = (
        read(at, "textures/jbib_a.ytd"),
        read(at, "textures/jbib_b.ytd"),
    );

    let change = replace::texture(at, &id, 1, &outside.path().join("new_b.ytd"))
        .expect("a texture dictionary replaces a texture");

    let after = Wardrobe::read(at).unwrap();
    assert_eq!(
        after.garments[0].variations[1].texture.as_deref(),
        Some("textures/new_b.textures.src")
    );
    assert_eq!(
        after.garments[0].variations[0], before.garments[0].variations[0],
        "no other texture changes"
    );
    assert_eq!(after.garments[0].variations[1].tint.as_deref(), Some("bla"));
    assert_eq!(after.garments[0].variations[1].tex_id, Some(1));
    assert_eq!(after.garments[0].variations[1].distribution, Some(7));
    assert_eq!(
        (
            read(at, "textures/jbib_a.ytd"),
            read(at, "textures/jbib_b.ytd")
        ),
        held,
        "the compiled files are not written"
    );

    undo_revision(at, &change).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), before);
}

#[test]
fn a_picture_replaces_a_texture_as_an_authored_source_with_no_dds() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let outside = tempfile::tempdir().unwrap();
    let picture = outside.path().join("jbib_diff_003_a_uni.png");
    image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 30, 30, 255]))
        .save(&picture)
        .unwrap();

    replace::texture(at, &id, 0, &picture).expect("a picture replaces a texture");

    let named = Wardrobe::read(at).unwrap().garments[0].variations[0]
        .texture
        .clone()
        .unwrap();
    assert_eq!(named, "textures/jbib_diff_003_a_uni.textures.src");
    let source = or_source::asset::Source::open(&at.join(&named)).unwrap();
    assert_eq!(source.manifest.origin.how, or_source::asset::How::Authored);
    assert!(at
        .join(&named)
        .join("textures/jbib_diff_003_a_uni.png")
        .is_file());
    assert!(
        (std::fs::read_dir(at.join(&named).join("textures"))
            .unwrap()
            .flatten())
        .all(|e| !e.file_name().to_string_lossy().ends_with(".dds")),
        "no DDS until it compiles"
    );
}

#[test]
fn a_replacement_that_cannot_be_made_changes_nothing_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("notes.txt"), b"not a model").unwrap();
    std::fs::write(
        outside.path().join("broken.ydd"),
        b"not a drawable dictionary",
    )
    .unwrap();
    std::fs::write(outside.path().join("fine.ydd"), ydd("fine")).unwrap();
    let before = common::snapshot(at);

    for (what, result) in [
        (
            "a file of another kind",
            replace::model(at, &id, &outside.path().join("notes.txt")),
        ),
        (
            "a file no reader opens",
            replace::model(at, &id, &outside.path().join("broken.ydd")),
        ),
        (
            "a file that is not there",
            replace::model(at, &id, &outside.path().join("gone.ydd")),
        ),
        (
            "a garment that is not there",
            replace::model(at, "nobody", &outside.path().join("fine.ydd")),
        ),
        (
            "a texture that is not there",
            replace::texture(at, &id, 9, &outside.path().join("fine.ydd")),
        ),
        (
            "a model as a texture",
            replace::texture(at, &id, 0, &outside.path().join("fine.ydd")),
        ),
    ] {
        assert!(result.is_err(), "{what} was accepted");
    }
    assert_eq!(common::snapshot(at), before, "no refusal wrote anything");
}

#[test]
fn a_model_another_garment_names_too_stays_with_that_garment() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let mut w = Wardrobe::read(at).unwrap();
    let mut twin = w.garments[0].clone();
    twin.id = "twin".into();
    twin.position = Some(4);
    w.garments.push(twin);
    w.write(at).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("better.ydd"), ydd("a better model")).unwrap();

    replace::model(at, &id, &outside.path().join("better.ydd")).expect("nothing is written over");

    let w = Wardrobe::read(at).unwrap();
    assert_eq!(
        w.garments[0].model.as_deref(),
        Some("models/better.drawables.src")
    );
    assert_eq!(
        w.garment("twin").unwrap().model.as_deref(),
        Some("models/jbib.ydd"),
        "the twin still names the file it named"
    );
}

#[test]
fn a_place_held_takes_the_model_it_is_given_and_undo_takes_the_source_out_again() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let mut held = garment(Ped::Female, JBIB, None, "unused", &[]);
    held.model = None;
    let id = held.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(held);
    w.write(at).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dress.ydd"), ydd("a dress")).unwrap();

    let change = replace::model(at, &id, &outside.path().join("dress.ydd")).unwrap();
    let w = Wardrobe::read(at).unwrap();
    let rel = w.garments[0].model.clone().expect("it has a model now");
    assert!(at.join(&rel).join("asset.json").is_file());

    undo_revision(at, &change).unwrap();
    assert!(Wardrobe::read(at).unwrap().garments[0].model.is_none());
    assert!(
        !at.join(&rel).exists(),
        "the source the change made is gone"
    );
}

#[test]
fn a_model_or_a_texture_can_be_replaced_by_a_reference_and_undo_takes_the_reference_out() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let id = jacket(at);
    let before = Wardrobe::read(at).unwrap();

    let model = replace::model_by_reference(
        at,
        &id,
        &Reference::here("sources/dress.drawables.src", None),
    )
    .expect("a reference replaces a model");
    let texture = replace::texture_by_reference(at, &id, 1, &Reference::by_id("jbib_diff_003_b"))
        .expect("an id replaces a texture");

    let w = Wardrobe::read(at).unwrap();
    assert_eq!(
        w.garments[0].model.as_deref(),
        Some("models/dress.ref.json")
    );
    assert_eq!(
        w.garments[0].variations[1].texture.as_deref(),
        Some("textures/jbib_diff_003_b.ref.json")
    );
    assert_eq!(
        Reference::read(&at.join("textures/jbib_diff_003_b.ref.json")).unwrap(),
        Reference::by_id("jbib_diff_003_b")
    );

    undo_revision(at, &texture).unwrap();
    undo_revision(at, &model).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), before);
    assert!(!at.join("models/dress.ref.json").exists());
    assert!(!at.join("textures/jbib_diff_003_b.ref.json").exists());
}

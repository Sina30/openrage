//! A garment leaves a pack as one package and enters another as an equal garment: its models and textures as sources, its
//! settings as data, and never a compiled game file of the pack's.

mod common;

use std::path::Path;

use common::{garment, put_bytes, ydd, ytd, JBIB};
use or_wardrobe::edit::undo_revision;
use or_wardrobe::facts::digest;
use or_wardrobe::package::{export, import, FILE};
use or_wardrobe::{allocate, shop, Expression, Garment, Ped, Wardrobe};

/// A pack of a jacket the way an adopted pack holds one, as compiled files, with every setting a garment states.
fn pack_of_a_jacket(folder: &Path) -> (Wardrobe, String) {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/jacket_1.ydd", &ydd("jacket-alternate"));
    put_bytes(folder, "cloth/jacket.yld", b"the cloth of the jacket");
    put_bytes(folder, "textures/a.ytd", &ytd("a"));
    put_bytes(folder, "textures/b.ytd", &ytd("b"));
    let mut w = Wardrobe::new("origin");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(7),
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    jacket.alternates = vec!["models/jacket_1.ydd".into()];
    jacket.cloth = Some("cloth/jacket.yld".into());
    jacket.label = Some("Wool jacket".into());
    jacket.tags = vec!["wool".into(), "winter".into()];
    jacket.group = Some("Coats".into());
    jacket.meta.flags = Some(3);
    jacket.meta.prop_mask = Some(5);
    jacket.meta.ped_xml_audio_id = Some("SHIRT".into());
    jacket.expressions = vec![Expression {
        index: 4,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    }];
    jacket.variations[1].tint = Some("whi".into());
    jacket.variations[1].tex_id = Some(1);
    jacket.variations[1].distribution = Some(128);
    jacket.variations[1].shop_id = Some("WOOL_JACKET_B".into());
    let id = jacket.id.clone();
    w.garments.push(jacket);
    w.write(folder).unwrap();
    (w, id)
}

/// What a garment states, without the id it is called by and the places its files are.
fn settings(g: &Garment) -> Garment {
    let mut g = g.clone();
    g.id = String::new();
    g.model = g.model.map(|_| "model".into());
    g.alternates = g.alternates.iter().map(|_| "alternate".into()).collect();
    g.cloth = g.cloth.map(|_| "cloth".into());
    for v in &mut g.variations {
        v.texture = v.texture.take().map(|_| "texture".into());
    }
    g
}

fn compiled_outside_sources(folder: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut all = Vec::new();
    walk(folder, folder, &mut all);
    all.into_iter()
        .filter(|f| !f.contains(".src/") && (f.ends_with(".ydd") || f.ends_with(".ytd")))
        .collect()
}

fn source_of_a_game_file(folder: &Path, rel: &str) -> or_source::asset::Source {
    or_source::asset::Source::open(&folder.join(rel))
        .unwrap_or_else(|e| panic!("{rel} is a source: {e}"))
}

#[test]
fn a_garment_is_packaged_as_sources_and_data_and_imports_to_an_equal_garment() {
    let origin = tempfile::tempdir().unwrap();
    let (before, id) = pack_of_a_jacket(origin.path());

    let package = tempfile::tempdir().unwrap();
    let package = package.path().join("jacket.garment");
    let packaged = export(origin.path(), &id, &package).expect("it exports");
    assert_eq!(
        packaged.files.len(),
        5,
        "a model, an alternate, a cloth and two textures: {:?}",
        packaged.files
    );
    assert!(package.join(FILE).is_file());
    assert_eq!(
        compiled_outside_sources(&package),
        Vec::<String>::new(),
        "a package holds sources and settings, never a compiled game file"
    );
    assert!(
        package.join("cloth/jacket.yld").is_file(),
        "a cloth has no source form, so it is the one file a package holds as it is"
    );

    // The pack the package enters has a garment already, at another place, and nothing of the origin's.
    let target = tempfile::tempdir().unwrap();
    Wardrobe::new("other").write(target.path()).unwrap();
    let imported = import(target.path(), &package).expect("it imports");
    let after = Wardrobe::read(target.path()).unwrap();
    assert_eq!(after.garments.len(), 1);
    let made = &after.garments[0];
    assert_eq!(made.id, imported.garment);
    assert_ne!(
        made.id, id,
        "a garment of the pack is never given an id another pack has"
    );
    assert_eq!(
        settings(made),
        settings(&before.garments[0]),
        "every setting comes through: place, pin, label, tags, group, metadata, expression, tint, ids and the shop id"
    );

    // The files are sources of the new pack, and the model is the game file the origin held.
    let model = made.model.as_deref().unwrap();
    assert!(model.ends_with(".drawables.src"), "{model}");
    let asset = source_of_a_game_file(target.path(), model);
    assert_eq!(
        asset.manifest.origin.sha256.as_deref(),
        Some(digest(&ydd("jacket")).as_str()),
        "the source was made from the very file the origin held"
    );
    assert!(made.alternates[0].ends_with(".drawables.src"));
    assert!(made.variations[1]
        .texture
        .as_deref()
        .unwrap()
        .ends_with(".textures.src"));
    assert_eq!(
        std::fs::read(target.path().join(made.cloth.as_deref().unwrap())).unwrap(),
        b"the cloth of the jacket"
    );
    assert!(compiled_outside_sources(target.path()).is_empty());

    // Undo takes the garment and every file it brought out again.
    undo_revision(target.path(), &imported.revision).expect("it can be taken back");
    assert!(Wardrobe::read(target.path()).unwrap().garments.is_empty());
    assert!(
        !target.path().join(model).exists(),
        "the source the import copied is gone"
    );
}

#[test]
fn a_pin_that_is_taken_is_let_go_and_said_and_a_package_that_names_a_missing_file_is_refused() {
    let origin = tempfile::tempdir().unwrap();
    let (_, id) = pack_of_a_jacket(origin.path());
    let package = tempfile::tempdir().unwrap();
    let package = package.path().join("jacket.garment");
    export(origin.path(), &id, &package).unwrap();

    // Importing into the pack it came from: place 7 of the jacket slot is held by the original.
    let imported = import(origin.path(), &package).unwrap();
    let after = Wardrobe::read(origin.path()).unwrap();
    assert_eq!(after.garments.len(), 2);
    let copy = after.garment(&imported.garment).unwrap();
    assert_eq!(copy.position, None, "it takes the lowest free place");
    assert!(
        imported.notes.iter().any(|n| n.contains("7")),
        "{:?}",
        imported.notes
    );

    // The pack writes WOOL_JACKET_B for the original, so the copy does not state it too: two textures with one id clash.
    assert_eq!(copy.variations[1].shop_id, None);
    assert!(
        imported.notes.iter().any(|n| n.contains("WOOL_JACKET_B")),
        "{:?}",
        imported.notes
    );
    let again = import(origin.path(), &package).unwrap();
    assert!(
        again.notes.iter().any(|n| n.contains("WOOL_JACKET_B")),
        "{:?}",
        again.notes
    );
    let after = Wardrobe::read(origin.path()).unwrap();
    assert_eq!(after.garments.len(), 3);
    assert_eq!(
        shop::clashes(&after, &allocate(&after)),
        Vec::<(String, Vec<String>)>::new(),
        "an id is written for one texture only"
    );

    // A package whose garment names a source that is not in it changes nothing.
    let broken = tempfile::tempdir().unwrap();
    let broken = broken.path().join("broken.garment");
    export(origin.path(), &id, &broken).unwrap();
    let model = std::fs::read_dir(broken.join("models"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    std::fs::remove_dir_all(model).unwrap();
    let target = tempfile::tempdir().unwrap();
    Wardrobe::new("other").write(target.path()).unwrap();
    let refused = import(target.path(), &broken).unwrap_err();
    assert!(refused.to_string().contains("models/"), "{refused}");
    assert!(Wardrobe::read(target.path()).unwrap().garments.is_empty());
    assert!(
        std::fs::read_dir(target.path())
            .unwrap()
            .flatten()
            .all(|e| e.file_name() == "wardrobe.json"),
        "nothing was copied before the refusal"
    );
}

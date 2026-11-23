//! The optimiser of a pack: a source is never changed in place, the garments asked for name a copy that is lighter, one copy
//! serves every garment of the same source, what it cannot change is listed with the reason, and one Undo takes it back.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, ytd_of, JBIB};
use or_io::codec::texture::GrcTextureFormat;
use or_project::reference::Reference;
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::optimise::Scale;
use or_wardrobe::edit::{undo_revision, Error, Watch};
use or_wardrobe::textures::{optimise, safe_options};
use or_wardrobe::{Ped, Wardrobe};

const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const BIG: &str = "textures/big.textures.src";

fn a_pack(dir: &Path) -> (PathBuf, Vec<String>) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    decompile(
        &ytd_of(&[("big", DXT5, 256, 256, 7)]),
        "big.ytd",
        &pack.join("textures"),
        &GameFacts::default(),
    )
    .unwrap();
    put_bytes(
        &pack,
        "textures/shipped.ytd",
        &ytd_of(&[("shipped", DXT5, 256, 256, 7)]),
    );
    Reference::here(BIG, None)
        .write(&pack.join("textures/elsewhere.ref.json"))
        .unwrap();
    let mut w = Wardrobe::new("demo");
    let mut ids = Vec::new();
    for (n, texture) in [
        BIG,
        BIG,
        "textures/shipped.ytd",
        "textures/elsewhere.ref.json",
    ]
    .into_iter()
    .enumerate()
    {
        let mut g = garment(Ped::Male, JBIB, Some(n as u32), "models/m.ydd", &[texture]);
        g.label = Some(format!("jacket {n}"));
        ids.push(g.id.clone());
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    (pack, ids)
}

fn textures_of(pack: &Path) -> Vec<String> {
    let w = Wardrobe::read(pack).unwrap();
    w.garments
        .iter()
        .map(|g| g.variations[0].texture.clone().unwrap())
        .collect()
}

#[test]
fn the_garments_asked_for_name_a_lighter_copy_and_the_source_stays_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, ids) = a_pack(dir.path());
    let before = snapshot(&pack);
    let options = or_source::optimise::Options {
        scale: Scale::Ratio(0.5),
        ..safe_options(&Wardrobe::read(&pack).unwrap())
    };

    let (report, revision) =
        optimise(&pack, Some(&ids[..1]), &options, &Watch::default()).expect("it runs");

    // Only the first jacket is in scope: it names the copy, and the second keeps the source it named.
    assert_eq!(
        textures_of(&pack),
        [
            "textures/big-2.textures.src",
            BIG,
            "textures/shipped.ytd",
            "textures/elsewhere.ref.json"
        ]
    );
    assert_eq!(report.changed.len(), 1, "{report:?}");
    assert_eq!(report.changed[0].garments, ["jacket 0"]);
    assert_eq!(report.textures(), 1);
    let after = snapshot(&pack);
    for (path, bytes) in &before {
        if !path.ends_with("wardrobe.json") {
            assert_eq!(after.get(path), Some(bytes), "{path} was changed");
        }
    }
    let copy = or_source::asset::Source::open(&pack.join("textures/big-2.textures.src")).unwrap();
    let old = or_source::asset::Source::open(&pack.join(BIG)).unwrap();
    assert_ne!(
        copy.manifest.id, old.manifest.id,
        "a copy is an asset of its own"
    );
    let compiled = or_source::textures::compile(&copy).unwrap();
    let dictionary = or_io::decl::File::from_bytes(&compiled.bytes)
        .map(|d: or_io::gta5::PgDictionary<or_io::gta5::gen8::GrcTexture>| d)
        .unwrap();
    let texture = dictionary.entries.iter().flatten().next().unwrap();
    assert_eq!(
        or_io::codec::texture::TextureSource::stored_size(&**texture),
        (128, 128)
    );

    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    assert!(!pack.join("textures/big-2.textures.src").exists());
    let undone = snapshot(&pack);
    for (path, bytes) in &before {
        assert_eq!(undone.get(path), Some(bytes), "{path} is not as it was");
    }
}

#[test]
fn one_copy_serves_every_garment_of_the_source_and_what_it_cannot_change_is_listed_with_why() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, _) = a_pack(dir.path());
    let options = or_source::optimise::Options {
        scale: Scale::Side(64),
        ..safe_options(&Wardrobe::read(&pack).unwrap())
    };
    let watch = Watch::default();

    let (report, _) = optimise(&pack, None, &options, &watch).unwrap();

    assert_eq!(
        textures_of(&pack),
        [
            "textures/big-2.textures.src",
            "textures/big-2.textures.src",
            "textures/shipped.ytd",
            "textures/elsewhere.ref.json"
        ],
        "the two jackets of one source share the copy"
    );
    assert_eq!(report.changed[0].garments, ["jacket 0", "jacket 1"]);
    assert!(
        !pack.join("textures/big-3.textures.src").exists(),
        "one copy, not one for each garment"
    );
    let why = |file: &str| {
        (report.left.iter().find(|l| l.file == file))
            .map(|l| l.why.clone())
            .unwrap_or_else(|| panic!("{file} is not listed: {:?}", report.left))
    };
    assert!(why("textures/shipped.ytd").contains("compiled"));
    assert!(why("textures/elsewhere.ref.json").contains("somewhere else"));
    // It counted the textures it looked at, one for each name.
    assert_eq!(watch.progress.snapshot().total, 3);
}

#[test]
fn a_stop_and_an_option_that_cannot_be_carried_out_leave_the_pack_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, _) = a_pack(dir.path());
    let before = snapshot(&pack);
    let options = safe_options(&Wardrobe::read(&pack).unwrap());
    let watch = Watch::default();
    watch.cancel.set();

    let stopped = optimise(&pack, None, &options, &watch);

    assert!(matches!(stopped, Err(Error::Stopped)));
    let bad = or_source::optimise::Options {
        scale: Scale::Ratio(2.0),
        ..options
    };
    assert!(optimise(&pack, None, &bad, &Watch::default()).is_err());
    assert_eq!(snapshot(&pack), before);
}

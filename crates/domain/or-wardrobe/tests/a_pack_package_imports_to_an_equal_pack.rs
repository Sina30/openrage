//! A whole pack travels as one file: its sources and its settings, never a compiled game file, and it imports to an equal pack. The
//! file is the same for the same pack, and an import refuses what leaves its folder or is not a package, leaving nothing behind.

mod common;

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::Path;

use common::{drawables_source, garment, put_bytes, snapshot, ydd, ytd, FEET, JBIB, LOWR};
use or_wardrobe::pack_package::{export, import, HEADER};
use or_wardrobe::query::save;
use or_wardrobe::{freeze, Garment, Ped, Variation, Wardrobe};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// A pack of three garments: a jacket of compiled files with an alternate, a cloth and two textures; trousers whose model is a source
/// folder and that share the jacket's first texture; and shoes that hold a place.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/jacket_1.ydd", &ydd("jacket-alternate"));
    put_bytes(folder, "cloth/jacket.yld", b"the cloth of the jacket");
    put_bytes(folder, "textures/a.ytd", &ytd("a"));
    put_bytes(folder, "textures/b.ytd", &ytd("b"));
    drawables_source(&folder.join("models/trousers.drawables.src"), "trousers");
    put_bytes(
        folder,
        "models/trousers.drawables.src/model.glb",
        b"geometry",
    );
    put_bytes(folder, "shop/extra.meta", b"<meta/>");
    let mut w = Wardrobe::new("travelling");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(3),
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    jacket.label = Some("Wool jacket".into());
    jacket.tags = vec!["wool".into(), "singleplayer".into()];
    jacket.group = Some("Coats".into());
    jacket.alternates = vec!["models/jacket_1.ydd".into()];
    jacket.cloth = Some("cloth/jacket.yld".into());
    jacket.meta.flags = Some(3);
    jacket.variations[1].tint = Some("whi".into());
    let mut trousers = garment(
        Ped::Female,
        LOWR,
        None,
        "models/trousers.drawables.src",
        &["textures/a.ytd"],
    );
    trousers.label = Some("Chinos".into());
    let mut shoes = Garment::new();
    shoes.ped = Some(Ped::Male);
    shoes.slot = Some(FEET);
    shoes.variations = vec![Variation::default()];
    w.garments = vec![jacket, trousers, shoes];
    w.collections.insert(Ped::Male, "mp_m_travel".into());
    w.waivers.push(or_wardrobe::Waiver {
        code: "lod-missing".into(),
        garment: None,
        reason: "the models are small".into(),
    });
    w.shop.carried.push(or_wardrobe::Carried {
        file: "shop/extra.meta".into(),
        directive: "data_file".into(),
        kind: "SHOP_PED_APPAREL_META_FILE".into(),
    });
    save(&mut w, "Singleplayer", "tag:singleplayer").unwrap();
    w.write(folder).unwrap();
    // The lock of the last build and the private folder of this machine are not the pack's to hand over.
    freeze::keep(folder).unwrap();
    put_bytes(folder, ".openrage/versions/note.txt", b"private");
    w
}

fn names_in(file: &Path) -> Vec<String> {
    let zip = ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    zip.file_names().map(String::from).collect()
}

/// What a garment states, without the id it is called by being touched and without the places its files are.
fn settings(g: &Garment) -> Garment {
    let mut g = g.clone();
    g.model = g.model.map(|_| "model".into());
    g.alternates = g.alternates.iter().map(|_| "alternate".into()).collect();
    g.cloth = g.cloth.map(|_| "cloth".into());
    for v in &mut g.variations {
        v.texture = v.texture.take().map(|_| "texture".into());
    }
    g
}

fn bare(w: &Wardrobe) -> Wardrobe {
    let mut w = w.clone();
    w.garments = w.garments.iter().map(settings).collect();
    w.lock = None;
    w
}

#[test]
fn a_pack_leaves_as_one_file_of_sources_and_settings_and_imports_to_an_equal_pack() {
    let origin = tempfile::tempdir().unwrap();
    let w = pack(origin.path());
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("travelling.openrage-pack");

    let packed = export(origin.path(), &file).expect("it exports");
    assert_eq!(packed.garments, 3);
    let names = names_in(&file);
    assert_eq!(names[0], HEADER, "the header comes first");
    assert!(names.contains(&"wardrobe.json".to_string()));
    let compiled: Vec<_> = (names.iter())
        .filter(|n| !n.contains(".src/") && (n.ends_with(".ydd") || n.ends_with(".ytd")))
        .collect();
    assert!(
        compiled.is_empty(),
        "a package holds sources, never a compiled game file: {compiled:?}"
    );
    assert!(names.iter().any(|n| n == "cloth/jacket.yld"), "{names:?}");
    assert!(names.iter().any(|n| n == "shop/extra.meta"), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.contains("lock") || n.contains(".openrage")),
        "the lock and the private folder stay: {names:?}"
    );
    assert_eq!(
        names
            .iter()
            .filter(|n| n.ends_with("a.textures.src/asset.json"))
            .count(),
        1,
        "a texture two garments name is in the file once: {names:?}"
    );
    assert_eq!(
        packed.files,
        names.len() - 1,
        "the header is not a file of the pack"
    );

    // Imported into a folder of its own: the same garments, by id, and the same settings.
    let there = tempfile::tempdir().unwrap();
    let there = there.path().join("travelling");
    let imported = import(&file, &there).expect("it imports");
    assert_eq!(
        (imported.pack.as_str(), imported.garments),
        ("travelling", 3)
    );
    let got = Wardrobe::read(&there).unwrap();
    assert_eq!(
        got.garments.iter().map(|g| &g.id).collect::<Vec<_>>(),
        w.garments.iter().map(|g| &g.id).collect::<Vec<_>>(),
        "a pack that moves keeps the ids its garments are called by"
    );
    assert_eq!(bare(&got), bare(&w));
    assert_eq!(
        got.queries, w.queries,
        "a saved query travels with the pack"
    );
    assert!(got.lock.is_none(), "the lock is that machine's");

    // Everything the garments name is a source or a cloth in the new folder, and the source the pack held is byte for byte there.
    for g in &got.garments {
        for named in g
            .model
            .iter()
            .chain(&g.alternates)
            .chain(g.variations.iter().filter_map(|v| v.texture.as_ref()))
        {
            assert!(
                named.ends_with(".src") && there.join(named).is_dir(),
                "{named}"
            );
        }
    }
    let was = snapshot(&origin.path().join("models/trousers.drawables.src"));
    let now = snapshot(&there.join("models/trousers.drawables.src"));
    assert_eq!(now, was);

    // A pack of sources is a fixed point: once it has travelled, it travels again as the same pack, and the same pack is the same file.
    let second = out.path().join("second.openrage-pack");
    export(&there, &second).unwrap();
    let third = out.path().join("third.openrage-pack");
    export(&there, &third).unwrap();
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&third).unwrap()
    );
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere.path().join("pack");
    import(&second, &elsewhere).unwrap();
    assert_eq!(Wardrobe::read(&elsewhere).unwrap(), got);
    assert_eq!(
        snapshot(&elsewhere).keys().collect::<Vec<_>>(),
        snapshot(&there).keys().collect::<Vec<_>>()
    );
}

/// A zip with the named entries.
fn zipped(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

const HEAD: &[u8] = b"{\"format\":\"openrage-pack\",\"schema\":1}";

#[test]
fn an_import_refuses_what_leaves_its_folder_or_is_not_a_package_and_leaves_nothing_behind() {
    let work = tempfile::tempdir().unwrap();
    let wardrobe = Wardrobe::new("p").to_json().into_bytes();
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "slip",
            zipped(&[(HEADER, HEAD), ("wardrobe.json", &wardrobe), ("../evil.txt", b"x")]),
            "leaves the folder",
        ),
        (
            "absolute",
            zipped(&[(HEADER, HEAD), ("wardrobe.json", &wardrobe), ("/evil.txt", b"x")]),
            "leaves the folder",
        ),
        (
            "compiled",
            zipped(&[(HEADER, HEAD), ("wardrobe.json", &wardrobe), ("models/x.ydd", b"x")]),
            "compiled game file",
        ),
        (
            "no header",
            zipped(&[("wardrobe.json", &wardrobe)]),
            "not a pack package",
        ),
        (
            "newer",
            zipped(&[(HEADER, b"{\"format\":\"openrage-pack\",\"schema\":9}"), ("wardrobe.json", &wardrobe)]),
            "schema 9",
        ),
        ("no settings", zipped(&[(HEADER, HEAD)]), "no wardrobe.json"),
        ("not a zip", b"just text".to_vec(), "not a pack package"),
        (
            "names a file it lacks",
            zipped(&[
                (HEADER, HEAD),
                (
                    "wardrobe.json",
                    br#"{"format":"openrage-wardrobe","schema":3,"pack":"p","garments":[{"id":"g","model":"models/gone.drawables.src"}]}"#,
                ),
            ]),
            "does not hold",
        ),
    ];
    for (what, bytes, says) in cases {
        let file = work
            .path()
            .join(format!("{}.openrage-pack", what.replace(' ', "-")));
        std::fs::write(&file, bytes).unwrap();
        let into = work.path().join(format!("into-{}", what.replace(' ', "-")));
        let why = import(&file, &into).expect_err(what).to_string();
        assert!(why.contains(says), "{what}: {why}");
        assert!(!into.exists(), "{what}: nothing is left behind");
        assert!(!work.path().join("evil.txt").exists());

        // Into a folder that was there and empty, it stays empty.
        std::fs::create_dir_all(&into).unwrap();
        import(&file, &into).expect_err(what);
        assert_eq!(std::fs::read_dir(&into).unwrap().count(), 0, "{what}");
    }

    // A folder that holds something is not unpacked into.
    let origin = tempfile::tempdir().unwrap();
    pack(origin.path());
    let file = work.path().join("good.openrage-pack");
    export(origin.path(), &file).unwrap();
    let busy = work.path().join("busy");
    put_bytes(&busy, "mine.txt", b"mine");
    let why = import(&file, &busy).unwrap_err().to_string();
    assert!(why.contains("not empty"), "{why}");
    assert_eq!(
        snapshot(&busy)
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .len(),
        1
    );
}

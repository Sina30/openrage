//! A `dlc.rpf` or an `.oiv` a build wrote hands back every file in it, one at a time, by the path the plan named it by,
//! with the archives inside the pack opened and the files of a package that are not the pack's told apart from the pack's.

use std::collections::BTreeMap;
use std::path::Path;

use or_project::built::{unpack, BuiltKind};
use or_project::{
    build, plan_with, Container, Contribution, ManifestEntry, Module, Origin, Output, OutputKind,
    Plan, PlannedFile,
};

fn document(path: &str, text: &str) -> PlannedFile {
    PlannedFile {
        path: path.to_string(),
        origin: Origin::Document(text.to_string()),
    }
}

/// A pack of a shop file and two files inside an archive, and, for a package, a readme that is no pack's.
fn planned(kind: OutputKind) -> Plan {
    let take: &[&str] = match kind {
        OutputKind::OivPackage => &["files", "wardrobe"],
        _ => &["wardrobe"],
    };
    let container = Container {
        modules: vec![
            Module::files("files"),
            Module::clothing("wardrobe", "wardrobe"),
        ],
        outputs: vec![Output::of(kind, "out", "uniforms", take)],
    };
    let mut files = Contribution::of("files");
    files.files = vec![document("readme.txt", "read me")];
    let mut pack = Contribution::of("wardrobe");
    pack.pack = true;
    pack.files = vec![
        document("x64/models/cdimages/u_female.rpf/stem/a.txt", "a"),
        document("x64/models/cdimages/u_female.rpf/stem.txt", "stem"),
        document("common/data/u_shop.meta", "<shop/>"),
    ];
    pack.manifest = vec![ManifestEntry {
        key: "data_file".into(),
        values: vec![
            "SHOP_PED_APPAREL_META_FILE".into(),
            "common/data/u_shop.meta".into(),
        ],
    }];
    plan_with(
        &container,
        "out",
        Path::new("/projects/uniforms"),
        &[files, pack],
    )
    .expect("the output is declared")
}

/// Every file `unpack` hands over, by path, with whether it is the pack's.
fn unpacked(at: &Path) -> (BuiltKind, BTreeMap<String, (bool, Vec<u8>)>) {
    let mut held = BTreeMap::new();
    let kind = unpack(at, &mut |path, in_pack, bytes| {
        held.insert(path.to_string(), (in_pack, bytes));
        Ok(())
    })
    .expect("a built pack unpacks");
    (kind, held)
}

#[test]
fn a_dlc_rpf_hands_back_the_files_of_the_archives_in_it_by_the_path_the_plan_gave_them() {
    let plan = planned(OutputKind::SingleplayerDlc);
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let dir = tempfile::tempdir().unwrap();
    let built = build(&plan, dir.path()).expect("it builds");

    // Both the folder and the file name the same pack.
    let (kind, held) = unpacked(&built.folder);
    let (same, _) = unpacked(&built.folder.join("dlc.rpf"));
    assert_eq!((kind, same), (BuiltKind::Dlc, BuiltKind::Dlc));
    for file in &plan.files {
        let Origin::Document(text) = &file.origin else {
            continue;
        };
        let (in_pack, bytes) = held
            .get(&file.path)
            .unwrap_or_else(|| panic!("{} in {:?}", file.path, held.keys().collect::<Vec<_>>()));
        assert!(in_pack, "every file of a pack is the pack's");
        assert_eq!(bytes, text.as_bytes(), "{}", file.path);
    }
    assert!(
        held.keys().all(|p| !p.ends_with(".rpf")),
        "an archive inside the pack is opened and its files handed back, not the archive: {:?}",
        held.keys().collect::<Vec<_>>()
    );
}

#[test]
fn an_oiv_hands_back_what_it_installs_and_says_which_of_it_is_the_pack() {
    let plan = planned(OutputKind::OivPackage);
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let dir = tempfile::tempdir().unwrap();
    let built = build(&plan, dir.path()).expect("it builds");

    let (kind, held) = unpacked(&built.folder.join("uniforms.oiv"));
    assert_eq!(kind, BuiltKind::Oiv);
    // The pack's files are told by the path they have inside the pack; the readme is the game folder's.
    for want in [
        "x64/models/cdimages/u_female.rpf/stem/a.txt",
        "common/data/u_shop.meta",
        "content.xml",
        "setup2.xml",
    ] {
        let (in_pack, _) = held.get(want).unwrap_or_else(|| panic!("{want}"));
        assert!(in_pack, "{want}");
    }
    let (in_pack, bytes) = &held["readme.txt"];
    assert!(!in_pack, "a readme is installed in the game folder");
    assert_eq!(bytes, b"read me");
    assert!(
        !held.contains_key("assembly.xml"),
        "the manifest is the package's and no file of it"
    );
}

#[test]
fn something_that_is_no_built_pack_is_refused_with_a_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let nothing = unpack(dir.path(), &mut |_, _, _| Ok(())).unwrap_err();
    assert!(
        nothing.contains("dlc.rpf") && nothing.contains(".oiv"),
        "{nothing}"
    );
    let junk = dir.path().join("junk.rpf");
    std::fs::write(&junk, b"not an archive").unwrap();
    assert!(unpack(&junk, &mut |_, _, _| Ok(())).is_err());
}

//! `Gta5` over a synthetic install built here, and once over a real one.
//!
//! The synthetic install is a scratch directory holding an unencrypted
//! `common.rpf`, so it needs no keys and runs on a clean checkout. The real
//! run needs `OPENRAGE_GAME_ROOT` and records its skip without it.

use std::path::{Path, PathBuf};

use or_io::container::rpf::RpfBuilder;
use or_io::decl::{File, Open};
use or_io::gta5::*;
use or_test_support::{game_roots, skip_install};
use or_title::{Gta5, InstallError};

fn filter(weight: f32) -> Option<Box<CrFrameFilterAny>> {
    Some(Box::new(CrFrameFilterAny::CrFrameFilterMultiWeight(
        CrFrameFilterMultiWeight {
            base: CrFrameFilter {
                r#type: Open::new(EFrameFilterType::MultiWeight),
                ..Default::default()
            },
            track_id_indices: vec![TrackIdIndex::new(0, 3, 0)],
            weights: vec![weight],
            ..Default::default()
        },
    )))
}

fn authored() -> CrFrameFilterDictionary {
    CrFrameFilterDictionary {
        codes: vec![0x100],
        entries: vec![filter(0.5)],
        ..Default::default()
    }
}

/// A fresh directory under the test target's scratch space.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gta5_install")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// An install whose `common.rpf` holds `data/filters.yfd`.
fn synthetic_install(name: &str) -> PathBuf {
    let root = scratch(name).join("install");
    std::fs::create_dir_all(&root).expect("install root");
    RpfBuilder::default()
        .insert_with(
            "data/filters.yfd",
            authored()
                .to_bytes()
                .expect("the authored dictionary saves"),
            false,
        )
        .expect("add the file")
        .save(root.join("common.rpf"))
        .expect("save the archive");
    root
}

#[test]
fn a_synthetic_install_opens_a_declared_file_by_name_and_saves_to_a_mod_folder() {
    let root = synthetic_install("opens_and_saves");
    let gta = Gta5::at(&root).expect("the synthetic install opens");

    let mut dict: CrFrameFilterDictionary = gta.open("filters.yfd").expect("opened by name");
    assert_eq!(dict, authored());
    assert_eq!(
        dict.save().unwrap_err().to_string(),
        or_io::Error::NoPath.to_string(),
        "a file opened out of an archive has no path to save back to"
    );

    if let Some(CrFrameFilterAny::CrFrameFilterMultiWeight(w)) = dict.entries[0].as_deref_mut() {
        w.weights[0] = 0.25;
    }
    let out = root.parent().unwrap().join("mods").join("filters.yfd");
    gta.save_to_mod(&dict, &out)
        .expect("saved outside the install");

    let saved = CrFrameFilterDictionary::open(&out).expect("the mod file reopens");
    assert_eq!(saved, dict);
    assert_eq!(
        gta.open::<CrFrameFilterDictionary>("FILTERS.YFD")
            .expect("a name is matched without case"),
        authored(),
        "the install is unchanged"
    );
}

#[test]
fn save_to_mod_refuses_a_path_inside_the_install() {
    let root = synthetic_install("refuses_inside");
    let gta = Gta5::at(&root).expect("the synthetic install opens");
    let dict = authored();

    let outside = root.parent().unwrap().join("elsewhere");
    for path in [
        root.join("filters.yfd"),
        root.join("mods").join("filters.yfd"),
        outside
            .join("..")
            .join("install")
            .join("x")
            .join("filters.yfd"),
    ] {
        match gta.save_to_mod(&dict, &path) {
            Err(InstallError::InsideInstall { .. }) => {}
            other => panic!("{} was not refused: {other:?}", path.display()),
        }
        assert!(!path.exists(), "{} was written", path.display());
    }
    assert!(
        !root.join("mods").exists(),
        "a refused save made a directory"
    );
}

#[test]
fn open_gives_the_wrong_kind_message_the_type_gives() {
    let root = synthetic_install("wrong_kind");
    let gta = Gta5::at(&root).expect("the synthetic install opens");

    let through_install = gta
        .open::<CrFrameFilterDictionary>("filters.ytd")
        .unwrap_err();
    let direct = CrFrameFilterDictionary::open(root.join("filters.ytd")).unwrap_err();
    assert!(matches!(
        through_install,
        InstallError::File(or_io::Error::WrongKind { .. })
    ));
    assert_eq!(through_install.to_string(), direct.to_string());
}

#[test]
fn a_name_the_install_does_not_hold_is_refused_by_name() {
    let root = synthetic_install("missing_name");
    let gta = Gta5::at(&root).expect("the synthetic install opens");
    let err = gta
        .open::<CrFrameFilterDictionary>("absent.yfd")
        .unwrap_err();
    assert!(matches!(err, InstallError::NotInInstall { .. }), "{err:?}");
    assert!(err.to_string().contains("absent.yfd"), "{err}");
}

#[test]
fn a_directory_without_common_rpf_is_not_an_install() {
    let dir = scratch("not_an_install");
    match Gta5::at(&dir) {
        Err(InstallError::NotAnInstall { .. }) => {}
        other => panic!("expected NotAnInstall, got {:?}", other.map(|_| ())),
    }
}

/// Every `.yfd` a real install resolves, opened by name through `Gta5::find`.
#[test]
fn gta5_find_opens_every_frame_filter_dictionary_by_name() {
    const WHAT: &str = "gta5_find_opens_every_frame_filter_dictionary_by_name";
    if game_roots().is_empty() {
        skip_install(WHAT, "Gta5::find needs an install to find");
        return;
    }
    let gta = Gta5::find().expect("OPENRAGE_GAME_ROOT names an install");
    let names = gta.names().expect("the install mounts");
    let yfd: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| n.ends_with(".yfd"))
        .collect();
    let mut opened = 0usize;
    let mut refused = Vec::new();
    for name in &yfd {
        match gta.open::<CrFrameFilterDictionary>(name) {
            Ok(_) => opened += 1,
            Err(e) => refused.push(format!("{name}: {e}")),
        }
    }
    eprintln!(
        "\n=== {WHAT} ===\ninstall: {}\n{} names resolved, {} .yfd, {} opened, {} refused",
        gta.root().display(),
        names.len(),
        yfd.len(),
        opened,
        refused.len()
    );
    for line in refused.iter().take(12) {
        eprintln!("  {line}");
    }
    assert!(!yfd.is_empty(), "the install resolves no .yfd name");
    assert!(refused.is_empty(), "{} refused", refused.len());
    assert_eq!(opened, yfd.len());
}

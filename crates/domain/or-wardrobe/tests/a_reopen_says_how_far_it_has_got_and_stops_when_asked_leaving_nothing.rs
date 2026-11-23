//! Reopening a built pack or a grzyClothTool project tells how many files it has read and which one it is on, and a stop asked
//! for before it is done leaves nothing behind: no project folder, and the pack it read is not touched.
//!
//! ⚠ Reads files; it says nothing about the game loading the pack.

mod common;

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use common::{garment, put_bytes, ydd, ytd, JBIB};
use or_project::{build_all, Container, Module, Output, OutputKind, Project};
use or_wardrobe::edit::{Error as EditError, Watch};
use or_wardrobe::gctproject::{self, XOR};
use or_wardrobe::reopen::{reopen_watched, Error, Options};
use or_wardrobe::{plans, Ped, Wardrobe};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// The folder a build of a pack of one jacket wrote as a singleplayer DLC.
fn a_built_dlc(dir: &Path) -> PathBuf {
    put_bytes(dir, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(dir, "textures/a.ytd", &ytd("a"));
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jacket.ydd",
        &["textures/a.ytd"],
    ));
    w.write(dir).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::of(
            OutputKind::SingleplayerDlc,
            "out",
            "demo",
            &["wardrobe"],
        )],
    };
    let path = dir.join("pack.openrage-proj");
    Project::create(&path, &container, &[]).unwrap();
    let plans = plans(&Project::load(&path).unwrap(), "out").unwrap();
    let out = dir.join("build");
    let (built, stopped) = build_all(&plans, &out);
    assert!(stopped.is_none(), "{stopped:?}");
    built[0].folder.clone()
}

fn stopped() -> Watch {
    let watch = Watch::default();
    watch.cancel.set();
    watch
}

#[test]
fn a_reopen_counts_the_files_it_reads_and_a_stop_asked_for_makes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let built = a_built_dlc(dir.path());
    let into = dir.path().join("reopened");

    let watch = Watch::default();
    let made = reopen_watched(&built, &into, &Options::default(), &watch).expect("it reopens");
    assert_eq!(made.garments, 1);
    let seen = watch.progress.snapshot();
    assert!(
        seen.attempted > 0 && !seen.item.is_empty(),
        "the files it read were counted and the last one named: {seen:?}"
    );

    let asked = dir.path().join("asked");
    let refused = reopen_watched(&built, &asked, &Options::default(), &stopped());
    assert!(
        matches!(refused, Err(Error::Edit(EditError::Stopped))),
        "{refused:?}"
    );
    assert!(!asked.exists(), "a stop leaves no project folder");
}

/// A project file of `entries`, the zip of a resource with every byte XORed as the tool does.
fn a_project_file(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        zip.start_file(*path, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut bytes = zip.finish().unwrap().into_inner();
    bytes.iter_mut().for_each(|b| *b ^= XOR);
    bytes
}

#[test]
fn a_grzy_project_counts_the_files_it_reads_and_a_stop_asked_for_makes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let c = "mp_m_freemode_01_mp_m_demo";
    let file = dir.path().join("demo.gctproject");
    std::fs::write(
        &file,
        a_project_file(&[
            (&format!("demo/stream/{c}^jbib_000_u.ydd"), ydd("jacket")),
            (
                &format!("demo/stream/{c}^jbib_diff_000_a_uni.ytd"),
                ytd("a"),
            ),
        ]),
    )
    .unwrap();

    let watch = Watch::default();
    let into = dir.path().join("reopened");
    gctproject::reopen_watched(&file, &into, &Options::default(), &watch).expect("it reopens");
    let seen = watch.progress.snapshot();
    assert_eq!(
        (seen.attempted, seen.total),
        (2, 2),
        "every entry of the zip was read and counted: {seen:?}"
    );

    let asked = dir.path().join("asked");
    let refused = gctproject::reopen_watched(&file, &asked, &Options::default(), &stopped());
    assert!(
        matches!(refused, Err(Error::Edit(EditError::Stopped))),
        "{refused:?}"
    );
    assert!(!asked.exists(), "a stop leaves no project folder");
}

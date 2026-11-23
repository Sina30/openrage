//! Reopening a built pack runs the rules of the engine over the pack it makes and puts the errors and warnings in its report,
//! so a server owner sees what to repair in a pack they did not make without opening the Problems list first.
//!
//! The pack is a grzyClothTool project laid out by the test, which the reopening reads as it reads a built pack: no game file,
//! no install and no corpus.

mod common;

use std::io::{Cursor, Write};

use common::{ydd, ytd};
use or_io::decl::File;
use or_wardrobe::gctproject::{self, XOR};
use or_wardrobe::reopen::Options;
use or_wardrobe::rules::Severity;
use or_wardrobe::{allocate, metadata, Garment, Ped, Slot, Wardrobe};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

const STEM: &str = "mp_m_freemode_01_mp_m_p";
const JBIB_DIR: &str = "demo/stream/p/mp_m_freemode_01_mp_m_p/jbib";

/// The variation metadata the engine writes for one jacket, which states `ownsCloth` when `claims_cloth`.
fn variation_metadata(claims_cloth: bool) -> Vec<u8> {
    let mut w = Wardrobe::new("p");
    let mut g = Garment::new();
    g.ped = Some(Ped::Male);
    g.slot = Some("jbib".parse::<Slot>().unwrap());
    g.position = Some(0);
    g.model = Some("m.ydd".into());
    g.variations = vec![Default::default()];
    g.missing_cloth = claims_cloth;
    w.garments.push(g);
    metadata::variation_info(&w, &allocate(&w), Ped::Male)
        .to_bytes()
        .unwrap()
}

fn project_file(claims_cloth: bool) -> Vec<u8> {
    let entries: Vec<(String, Vec<u8>)> = vec![
        (
            "demo/fxmanifest.lua".into(),
            b"fx_version 'cerulean'\ngame 'gta5'\n".to_vec(),
        ),
        (format!("{JBIB_DIR}/{STEM}^jbib_000_u.ydd"), ydd("jacket")),
        (
            format!("{JBIB_DIR}/{STEM}^jbib_diff_000_a_uni.ytd"),
            ytd("a"),
        ),
        (
            format!("demo/stream/p/{STEM}.ymt"),
            variation_metadata(claims_cloth),
        ),
    ];
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        zip.start_file(path, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let mut bytes = zip.finish().unwrap().into_inner();
    bytes.iter_mut().for_each(|b| *b ^= XOR);
    bytes
}

#[test]
fn a_pack_whose_metadata_states_a_cloth_no_file_is_there_for_says_so_in_its_report() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("demo.gctproject");
    std::fs::write(&file, project_file(true)).unwrap();

    let reopened = gctproject::reopen(&file, &dir.path().join("made"), &Options::default())
        .expect("it reopens");

    let cloth: Vec<_> = reopened
        .broken
        .iter()
        .filter(|b| b.message.to_lowercase().contains("cloth"))
        .collect();
    assert_eq!(cloth.len(), 1, "{:#?}", reopened.broken);
    assert_eq!(cloth[0].severity, Severity::Error);
    assert_eq!(
        cloth[0].garment.as_deref(),
        Some(format!("{STEM}^jbib_000_u.drawables.src").as_str()),
        "the garment is named as the screens name it"
    );
    assert!(
        reopened.broken.iter().all(|b| b.severity != Severity::Info),
        "a note is not something broken: {:#?}",
        reopened.broken
    );
}

#[test]
fn a_pack_with_nothing_wrong_in_it_reports_nothing_broken() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("demo.gctproject");
    std::fs::write(&file, project_file(false)).unwrap();

    let reopened = gctproject::reopen(&file, &dir.path().join("made"), &Options::default())
        .expect("it reopens");

    assert_eq!(reopened.garments, 1);
    assert!(reopened.broken.is_empty(), "{:#?}", reopened.broken);
}

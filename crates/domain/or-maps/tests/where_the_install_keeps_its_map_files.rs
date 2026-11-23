//! Where a `.ymap` and a `.ytyp` live in the packs of an install, and what `content.xml` registers of them: the measurement a build cell
//! waits for before it says *Yes* for a singleplayer DLC.
//!
//! Point `OPENRAGE_GAME_ROOT` at an install root. Without one this skips, which is not a pass.
//!
//! ⚠ The install is opened read-only and nothing here writes into one.

mod common;

use std::collections::BTreeMap;

use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};
use or_title::dlc::ContentXml;

const WHAT: &str = "where_the_install_keeps_its_map_files";

/// The first segment of a logical path that tells a base archive, an update, a patch or a DLC pack: the device before `:/`, else the path's head.
fn place_of(logical: &str) -> String {
    let (device, rest) = logical.split_once(":/").unwrap_or(("", logical));
    let head = rest.split('/').next().unwrap_or("");
    match device {
        "platform" => format!("platform:/{head}"),
        d if d.starts_with("dlc") => "a DLC pack".to_string(),
        "update" => format!("update:/{head}"),
        other => other.to_string(),
    }
}

#[test]
fn a_map_file_lives_in_an_archive_under_levels_and_content_xml_registers_a_type_file_and_never_a_map_file(
) {
    let Some(root) = game_roots().into_iter().next() else {
        skip_install(WHAT, format!("{GAME_ROOT_ENV} names no install"));
        return;
    };
    let game = common::mount(&root).expect("the install mounts");
    let (mut ymaps, mut ytyps, mut contents) = (Vec::new(), Vec::new(), Vec::new());
    game.for_each_file(|logical, _, leaf| {
        let leaf = leaf.to_ascii_lowercase();
        if leaf.ends_with(".ymap") {
            ymaps.push(logical.to_string());
        } else if leaf.ends_with(".ytyp") {
            ytyps.push(logical.to_string());
        } else if leaf == "content.xml" {
            contents.push(logical.to_string());
        }
    });
    let mut places: BTreeMap<(&str, String), usize> = BTreeMap::new();
    for (ext, list) in [("ymap", &ymaps), ("ytyp", &ytyps)] {
        for logical in list {
            *places.entry((ext, place_of(logical))).or_default() += 1;
        }
    }
    println!(
        "{} .ymap and {} .ytyp files in {} content.xml documents",
        ymaps.len(),
        ytyps.len(),
        contents.len()
    );
    for ((ext, place), n) in &places {
        println!("  .{ext} {n:6} {place}");
    }

    // What the content.xml documents register.
    let mut registered: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    let mut ityp = Vec::new();
    for logical in &contents {
        let Some(bytes) = game.vfs.open(logical) else {
            continue;
        };
        let content = ContentXml::parse(&bytes).expect("a shipped content.xml parses");
        let pack = logical.split(":/").next().unwrap_or("").to_string();
        for entry in &content.data_files {
            let ext = entry
                .filename
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if ["ymap", "imap", "ytyp", "ityp", "rpf"].contains(&ext.as_str()) {
                *registered
                    .entry((ext.clone(), entry.file_type.clone(), entry.contents.clone()))
                    .or_default() += 1;
            }
            if ext == "ityp" {
                ityp.push((pack.clone(), entry.filename.clone()));
            }
        }
    }
    for ((ext, file_type, contents), n) in &registered {
        println!("  content.xml: .{ext} {file_type} {contents:?} {n}");
    }

    // No document registers a map file by itself.
    assert!(
        registered
            .keys()
            .all(|(ext, ..)| ext != "ymap" && ext != "imap"),
        "a content.xml registers a map file: {registered:?}"
    );
    // A type file is registered under one type, and the archive holds it under the other spelling of its extension.
    let types: Vec<_> = registered
        .iter()
        .filter(|((ext, ..), _)| ext == "ityp")
        .collect();
    assert!(!types.is_empty());
    assert!(
        types
            .iter()
            .all(|((_, file_type, _), _)| file_type == "DLC_ITYP_REQUEST"),
        "{types:?}"
    );
    // The map data a pack ships is a nested archive registered as such.
    let nested = registered
        .iter()
        .filter(|((ext, file_type, contents), _)| {
            ext == "rpf" && file_type == "RPF_FILE" && contents == "CONTENTS_DLC_MAP_DATA"
        })
        .map(|(_, n)| *n)
        .sum::<usize>();
    assert!(nested > 0, "no nested archive of map data is registered");

    // Every `.ityp` a document registers is the `.ytyp` of that name inside a nested archive of the same folder: `props/a.ityp` is `props/<any>.rpf/a.ytyp`.
    let in_an_archive: std::collections::BTreeSet<(String, String)> = ytyps
        .iter()
        .filter_map(|logical| {
            let (archive, leaf) = logical.rsplit_once('/')?;
            let (folder, _) = archive.rsplit_once('/')?;
            let (_, folder) = folder.split_once(":/").unwrap_or(("", folder));
            let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
            Some((folder.to_ascii_lowercase(), stem.to_ascii_lowercase()))
        })
        .collect();
    let mut held = 0usize;
    let mut absent = Vec::new();
    for (pack, filename) in &ityp {
        let path = filename
            .split_once(":/")
            .map_or(filename.as_str(), |(_, rest)| rest)
            .replace("%PLATFORM%", "x64");
        let (folder, leaf) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
        let found =
            in_an_archive.contains(&(folder.to_ascii_lowercase(), stem.to_ascii_lowercase()));
        if found {
            held += 1;
        } else {
            absent.push(format!("{pack}: {filename}"));
        }
    }
    println!(
        "  {held} of {} registered .ityp files are the .ytyp of a nested archive of their folder",
        ityp.len()
    );
    assert!(
        held * 100 >= ityp.len() * 95,
        "{} of {} registered type files are the .ytyp of a nested archive of their folder: {:?}",
        held,
        ityp.len(),
        &absent[..absent.len().min(5)]
    );
}

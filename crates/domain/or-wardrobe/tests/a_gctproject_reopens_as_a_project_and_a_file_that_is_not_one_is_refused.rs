//! grzyClothTool's project file is the one foreign project format the Wardrobe reads: a zip, every byte of it XORed with
//! `0x2B`, that holds a resource. It reopens as a project of sources with what could not be placed named, as a built pack does.
//!
//! ⚠ No `.gctproject` of grzyClothTool is staged here, so these hold the reader to the format as it was described to this
//! build (a zip XORed with `0x2B` holding a resource), and the files inside are made by the tests. What a real file holds
//! that this reader does not place is listed, never dropped.

mod common;

use std::io::{Cursor, Write};
use std::path::Path;

use common::{ydd, ytd};
use or_project::{ModuleKind, OutputKind, Project};
use or_wardrobe::gctproject::{self, XOR};
use or_wardrobe::reopen::Options;
use or_wardrobe::Wardrobe;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const C: &str = "mp_m_freemode_01_mp_m_demo";

/// The zip of a resource, each entry `(path, bytes, deflated)`, with every byte XORed as the project file is.
fn project_file(entries: &[(String, Vec<u8>, bool)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes, deflated) in entries {
        let method = match deflated {
            true => CompressionMethod::Deflated,
            false => CompressionMethod::Stored,
        };
        zip.start_file(
            path.as_str(),
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut bytes = zip.finish().unwrap().into_inner();
    bytes.iter_mut().for_each(|b| *b ^= XOR);
    bytes
}

fn resource() -> Vec<(String, Vec<u8>, bool)> {
    let entry = |path: String, bytes: Vec<u8>, deflated| (path, bytes, deflated);
    vec![
        entry(
            "demo/fxmanifest.lua".into(),
            b"fx_version 'cerulean'
game 'gta5'
client_script 'client.lua'
"
            .to_vec(),
            true,
        ),
        entry("demo/client.lua".into(), b"print('hi')".to_vec(), true),
        entry(
            "demo/what?.txt".into(),
            b"a name no host takes".to_vec(),
            false,
        ),
        entry(
            format!("demo/stream/{C}^jbib_000_u.ydd"),
            ydd("jacket"),
            true,
        ),
        entry(
            format!("demo/stream/{C}^jbib_diff_000_a_uni.ytd"),
            ytd("a"),
            false,
        ),
        entry(
            format!("demo/stream/{C}^jbib_001_u.ydd"),
            b"this is not a drawable".to_vec(),
            false,
        ),
    ]
}

fn said<'a>(unplaced: &'a [or_wardrobe::import::LeftOut], path: &str) -> &'a str {
    unplaced
        .iter()
        .find(|l| l.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed in {unplaced:#?}"))
        .why
        .as_str()
}

#[test]
fn a_gctproject_reopens_as_sources_and_lists_the_files_it_could_not_place() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("demo.gctproject");
    std::fs::write(&file, project_file(&resource())).unwrap();

    let into = dir.path().join("reopened");
    let reopened = gctproject::reopen(&file, &into, &Options::default()).expect("it reopens");

    assert_eq!(reopened.pack, "demo");
    let project = Project::load(&reopened.project).unwrap();
    assert_eq!(project.container.outputs[0].kind, OutputKind::FivemResource);
    let kinds: Vec<&ModuleKind> = project.container.modules.iter().map(|m| &m.kind).collect();
    assert_eq!(kinds, [&ModuleKind::Clothing, &ModuleKind::Files]);
    assert_eq!(
        project.container.outputs[0].take,
        ["wardrobe", "files"],
        "a resource ships its other files"
    );

    // The folder the project file sat in is not part of the resource's paths.
    let wardrobe = Wardrobe::read(&into.join("clothing")).unwrap();
    let placed = wardrobe
        .garments
        .iter()
        .find(|g| g.position == Some(0))
        .expect("the jacket");
    assert!(placed.model.as_deref().unwrap().ends_with(".drawables.src"));
    assert!(placed.variations[0]
        .texture
        .as_deref()
        .unwrap()
        .ends_with(".textures.src"));

    // A file that opens as nothing keeps the place the name states and stays a file, named with the reason.
    let broken = wardrobe
        .garments
        .iter()
        .find(|g| g.position == Some(1))
        .expect("the place the broken model names");
    assert!(broken.is_reserved());
    let why = said(&reopened.unplaced, &format!("stream/{C}^jbib_001_u.ydd"));
    assert!(
        why.contains("could not be turned into a source") && why.contains("files/stream/"),
        "{why}"
    );
    assert_eq!(
        std::fs::read(into.join(format!("files/stream/{C}^jbib_001_u.ydd"))).unwrap(),
        b"this is not a drawable"
    );

    // The scripts of the resource are not lost, and the manifest is named as one the build writes its own of.
    assert_eq!(
        std::fs::read(into.join("files/client.lua")).unwrap(),
        b"print('hi')"
    );
    assert!(said(&reopened.unplaced, "fxmanifest.lua").contains("files/fxmanifest.lua"));

    // A name no host can create is not a reason to lose the file: it is kept under another and named as it was.
    assert!(said(&reopened.unplaced, "what?.txt").contains("files/what_.txt"));
    assert_eq!(
        std::fs::read(into.join("files/what_.txt")).unwrap(),
        b"a name no host takes"
    );
}

#[test]
fn a_file_that_is_not_a_gctproject_is_refused_with_the_reason_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let into = dir.path().join("reopened");

    // The bytes of a zip that were not XORed.
    let plain = {
        let mut bytes = project_file(&resource());
        bytes.iter_mut().for_each(|b| *b ^= XOR);
        bytes
    };
    let file = dir.path().join("plain.gctproject");
    std::fs::write(&file, plain).unwrap();
    let refused = gctproject::reopen(&file, &into, &Options::default()).unwrap_err();
    assert!(refused.to_string().contains("0x2B"), "{refused}");

    let empty = dir.path().join("empty.gctproject");
    std::fs::write(&empty, b"").unwrap();
    assert!(gctproject::reopen(&empty, &into, &Options::default()).is_err());
    assert!(Path::new(&into).read_dir().is_err(), "nothing was made");
}

/// The path under `files/` that `unplaced` says the file `path` was kept as.
fn kept_as(unplaced: &[or_wardrobe::import::LeftOut], path: &str) -> String {
    let why = said(unplaced, path);
    let (_, kept) = why.rsplit_once("kept as ").expect(why);
    kept.to_string()
}

#[test]
fn files_whose_staged_names_collide_are_each_kept_with_their_own_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("demo.gctproject");
    // Each is `(path in the pack, bytes)`: a name no host takes beside the real name it becomes, the same name in two cases,
    // and the place of a model with the case of its name changed, which the model of `resource` already holds.
    let wanted: Vec<(String, Vec<u8>)> = [
        ("what?.txt".to_string(), b"a name no host takes".to_vec()),
        (
            "what_.txt".to_string(),
            b"the file really called what_".to_vec(),
        ),
        ("Readme.txt".to_string(), b"upper".to_vec()),
        ("readme.txt".to_string(), b"lower".to_vec()),
        (format!("stream/{C}^JBIB_000_U.ydd"), b"the second".to_vec()),
        (format!("stream/{C}^jbib_000_u.YDD"), b"the third".to_vec()),
    ]
    .into();
    let mut entries: Vec<_> = resource()
        .into_iter()
        .filter(|(path, _, _)| path != "demo/what?.txt")
        .collect();
    entries.extend(
        (wanted.iter()).map(|(path, bytes)| (format!("demo/{path}"), bytes.clone(), false)),
    );
    std::fs::write(&file, project_file(&entries)).unwrap();

    let into = dir.path().join("reopened");
    let reopened = gctproject::reopen(&file, &into, &Options::default()).expect("it reopens");

    let mut seen = std::collections::BTreeSet::new();
    for (path, bytes) in &wanted {
        let kept = kept_as(&reopened.unplaced, path);
        assert_eq!(
            &std::fs::read(into.join(&kept)).unwrap(),
            bytes,
            "{path} was kept as {kept} with the bytes of another file"
        );
        // A host that tells `Readme.txt` from `readme.txt` keeps both names.
        let name = if cfg!(target_os = "linux") {
            kept.clone()
        } else {
            kept.to_lowercase()
        };
        assert!(seen.insert(name), "{kept} is the name of two kept files");
    }
}

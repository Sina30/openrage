//! A fragment (`.yft`) is a source of its own: its drawable is a glb that a compile puts back into
//! the fragment, and it is never read as a `.ydr` or a `.ydd`.

mod model_fixtures;

use std::fs;
use std::path::{Path, PathBuf};

use model_fixtures::{drawable, geometry, meshes, same_positions, ydd, ydr, yft};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{DrawableTarget, GtaFragType, SpdAABB};
use or_source::asset::{sha256_hex, Source};
use or_source::cook::{cook_source, describe, Cooker, Recook};
use or_source::fragment;
use or_source::game_import::{import, kind_of, Facts, Input, Options, Report, Verdict};
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};

fn how(file: &str) -> Extraction<'_> {
    Extraction {
        from_game: true,
        virtual_path: Some(file),
        textures: false,
        skeleton: None,
    }
}

fn game(virtual_path: &str) -> Facts {
    Facts {
        from_game: true,
        virtual_path: Some(virtual_path.to_string()),
        game_build: None,
        ..Facts::default()
    }
}

fn lamp() -> Vec<u8> {
    yft(drawable(
        "lamp",
        vec![geometry(6, false, 0.0), geometry(9, false, 3.0)],
        None,
    ))
}

fn extract_lamp(parent: &Path) -> PathBuf {
    fragment::extract(&lamp(), "lamp", parent, &how("x64a.rpf/lamp.yft"))
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
}

fn compile(dir: &Path) -> Vec<u8> {
    fragment::compile(&Source::open(dir).unwrap())
        .unwrap_or_else(|problems| panic!("{}: {problems:#?}", dir.display()))
        .bytes
}

fn box_bits(b: &SpdAABB) -> [u32; 8] {
    [
        b.min.x.to_bits(),
        b.min.y.to_bits(),
        b.min.z.to_bits(),
        b.min.w.to_bits(),
        b.max.x.to_bits(),
        b.max.y.to_bits(),
        b.max.z.to_bits(),
        b.max.w.to_bits(),
    ]
}

fn made(report: &Report) -> PathBuf {
    assert_eq!(report.files.len(), 1, "{report:#?}");
    match &report.files[0].verdict {
        Verdict::Made { folder } => folder.clone(),
        other => panic!("expected a source to be made, got {other:?}: {report:#?}"),
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_yft_has_a_kind_of_its_own_and_is_never_a_drawable_or_a_dictionary() {
    assert_eq!(fragment::KIND, "fragment");
    for extension in ["yft", "YFT"] {
        let row = kind_of(extension).expect("a .yft has a row in the kind table");
        assert_eq!(row.kind, fragment::KIND);
        assert_ne!(row.kind, models::DRAWABLE);
        assert_ne!(row.kind, models::DRAWABLES);
    }
    for extension in ["ydr", "ydd"] {
        assert_ne!(kind_of(extension).unwrap().kind, fragment::KIND);
    }
}

#[test]
fn a_yft_is_refused_by_the_drawable_readers_and_a_drawable_by_the_fragment_reader() {
    let (fragment_bytes, drawable_bytes, dictionary_bytes) = (
        lamp(),
        ydr(drawable("lamp", vec![geometry(6, false, 0.0)], None)),
        ydd(vec![(
            "lamp",
            drawable("lamp", vec![geometry(6, false, 0.0)], None),
        )]),
    );
    let scratch = tempfile::tempdir().unwrap();
    let here = scratch.path();
    let one =
        models::extract_drawable(&fragment_bytes, "lamp", here, &how("lamp.yft")).unwrap_err();
    let many =
        models::extract_drawables(&fragment_bytes, "lamp", here, &how("lamp.yft")).unwrap_err();
    for (refused, extension) in [(one, "ydr"), (many, "ydd")] {
        let why = &refused[0].message;
        assert!(
            why.contains(&format!("not a .{extension} this build reads"))
                && why.contains("RSC7 version"),
            "{why}"
        );
    }
    for (bytes, file) in [
        (&drawable_bytes, "lamp.ydr"),
        (&dictionary_bytes, "lamp.ydd"),
    ] {
        let refused = fragment::extract(bytes, "lamp", here, &how(file)).unwrap_err();
        let why = &refused[0].message;
        assert!(
            why.contains("not a .yft this build reads") && why.contains("RSC7 version"),
            "{file}: {why}"
        );
    }
    assert_eq!(names(here), Vec::<String>::new(), "a refusal left a folder");
}

#[test]
fn a_fragment_extracts_to_a_folder_of_its_own_and_compiles_back_to_the_same_drawable() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = extract_lamp(scratch.path());
    assert_eq!(dir.file_name().unwrap(), "lamp.fragment.src");
    assert_eq!(
        names(&dir),
        ["asset.json", "base.yft", "drawable"],
        "{dir:?}"
    );
    assert_eq!(names(&dir.join("drawable")), ["drawable.json", "model.glb"]);

    let source = Source::open(&dir).unwrap();
    assert_eq!(source.manifest.kind, "fragment");
    assert_eq!(source.manifest.target, "gen8");
    assert_eq!(source.manifest.origin.sha256, Some(sha256_hex(&lamp())));

    let bytes = compile(&dir);
    assert_eq!(bytes, compile(&dir), "two compiles of one folder differ");
    let before = GtaFragType::<Gen8>::from_bytes(&lamp()).unwrap();
    let after = GtaFragType::<Gen8>::from_bytes(&bytes).unwrap();
    let (was, is) = (
        &before.common_drawable.as_ref().unwrap().base,
        &after.common_drawable.as_ref().unwrap().base,
    );
    let (was_meshes, is_meshes) = (meshes(was), meshes(is));
    assert!(same_positions(&was_meshes.positions, &is_meshes.positions));
    assert_eq!(was_meshes.indices, is_meshes.indices);
    assert_eq!(
        box_bits(&was.lod_group.bounds_box),
        box_bits(&is.lod_group.bounds_box)
    );
}

#[test]
fn a_glb_put_into_a_fragment_changes_its_drawable_and_nothing_else_of_the_fragment() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = extract_lamp(scratch.path());
    // The same two geometries, moved: the glb a drawable of that shape extracts to.
    let moved = ydr(drawable(
        "lamp",
        vec![geometry(6, false, 40.0), geometry(9, false, 50.0)],
        None,
    ));
    let elsewhere = tempfile::tempdir().unwrap();
    let donor = models::extract_drawable(&moved, "lamp", elsewhere.path(), &how("lamp.ydr"))
        .unwrap_or_else(|problems| panic!("{problems:#?}"));
    fs::copy(donor.join("model.glb"), dir.join("drawable/model.glb")).unwrap();

    let after = GtaFragType::<Gen8>::from_bytes(&compile(&dir)).unwrap();
    let drawable = &after.common_drawable.as_ref().unwrap().base;
    let want = meshes(&model_fixtures::read_ydr(&moved).base);
    assert!(same_positions(&meshes(drawable).positions, &want.positions));

    let before = GtaFragType::<Gen8>::from_bytes(&lamp()).unwrap();
    assert_eq!(before.tune_name, after.tune_name);
    assert_eq!(before.drawables().len(), after.drawables().len());
    assert_eq!(before.child_count(), after.child_count());
}

#[test]
fn a_fragment_whose_drawable_holds_nothing_to_edit_is_said_so_and_leaves_no_folder() {
    let empty = yft(drawable("lamp", Vec::new(), None));
    let scratch = tempfile::tempdir().unwrap();
    let problems = fragment::extract(&empty, "lamp", scratch.path(), &how("lamp.yft")).unwrap_err();
    assert!(problems[0].message.contains("no geometry"), "{problems:#?}");
    assert_eq!(names(scratch.path()), Vec::<String>::new());
}

#[test]
fn importing_a_yft_makes_a_fragment_source_and_never_a_drawable_or_a_dictionary() {
    let project = tempfile::tempdir().unwrap();
    let bytes = lamp();
    let report = import(
        project.path(),
        &[Input::Bytes {
            name: "lamp.yft",
            bytes: &bytes,
            facts: game("x64a.rpf/lamp.yft"),
        }],
        &Options::default(),
    );
    let folder = made(&report);
    assert_eq!(folder, project.path().join("sources/lamp.fragment.src"));
    assert_eq!(
        names(&project.path().join("sources")),
        ["lamp.fragment.src"]
    );
    assert!(!project.path().join("files").exists());

    let source = Source::open(&folder).unwrap();
    assert!(source.manifest.origin.from_game);
    assert_eq!(
        source.manifest.origin.cooked_sha256,
        Some(sha256_hex(&compile(&folder))),
        "an import records what the cook made, so a build can tell the file is still the game's"
    );
}

#[test]
fn a_yft_with_no_source_form_is_kept_as_a_compiled_file_and_the_report_says_why() {
    let project = tempfile::tempdir().unwrap();
    let empty = yft(drawable("hull", Vec::new(), None));
    let report = import(
        project.path(),
        &[
            Input::Bytes {
                name: "hull.yft",
                bytes: &empty,
                facts: Facts::default(),
            },
            Input::Bytes {
                name: "junk.yft",
                bytes: b"not a fragment",
                facts: Facts::default(),
            },
        ],
        &Options::default(),
    );
    for file in &report.files {
        assert!(
            matches!(file.verdict, Verdict::Kept { copied_to: Some(_) }),
            "{file:#?}"
        );
    }
    assert!(
        report.files[0].reason.contains("no geometry"),
        "{report:#?}"
    );
    assert_eq!(
        names(&project.path().join("files")),
        ["hull.yft", "junk.yft"]
    );
    assert!(!project.path().join("sources").exists());
    assert!(
        report.notes.iter().any(|n| n.contains("2 .yft")),
        "the report names the kind it kept: {report:#?}"
    );
}

#[test]
fn a_fragment_source_cooks_to_a_yft_a_build_and_a_saved_glb_can_both_ask_for() {
    let project = tempfile::tempdir().unwrap();
    let dir = extract_lamp(project.path());
    let said = describe(&dir).expect("a fragment source is described");
    assert_eq!(said.kind, "fragment");
    assert_eq!(said.name, "lamp");
    assert_eq!(said.extension, "yft");
    assert!(said.bytes > 0);

    let scratch = tempfile::tempdir().unwrap();
    let cooked = cook_source(&dir, scratch.path()).unwrap();
    assert_eq!(cooked.file_name().unwrap(), "lamp.yft");
    assert_eq!(fs::read(&cooked).unwrap(), compile(&dir));

    let shadow = project.path().join("build/shadow");
    let id = Source::open(&dir).unwrap().manifest.id;
    let mut cooker = Cooker::new(&shadow);
    for input in ["drawable/model.glb", "drawable/drawable.json", "base.yft"] {
        let event = cooker.recook(&dir.join(input));
        assert!(
            matches!(event, Recook::Cooked { .. } | Recook::Unchanged { .. }),
            "{input}: {event:?}"
        );
    }
    assert_eq!(
        fs::read(shadow.join(&id).join("lamp.yft")).unwrap(),
        compile(&dir)
    );
    fs::write(dir.join("notes.txt"), b"mine").unwrap();
    assert!(matches!(
        cooker.recook(&dir.join("notes.txt")),
        Recook::Ignored(_)
    ));
}

/// What a corpus run counts.
#[derive(Default)]
struct Tally {
    fragments: usize,
    no_drawable_geometry: usize,
    sources: usize,
    geometries: usize,
}

fn every_yft<T: Extracted + Imported + DrawableTarget>(target: &str) -> Option<Tally>
where
    GtaFragType<T>: File,
{
    let files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(target)
        .find_or_skip(&format!("every {target} corpus .yft comes back"))?;
    let scratch = tempfile::tempdir().unwrap();
    let mut tally = Tally::default();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".yft");
        let bytes = fs::read(&path).unwrap();
        tally.fragments += 1;
        let dir = match fragment::extract(&bytes, name, scratch.path(), &how(&file)) {
            Ok(dir) => dir,
            Err(problems)
                if problems[0].message.contains("no geometry")
                    || problems[0].message.contains("no common drawable") =>
            {
                tally.no_drawable_geometry += 1;
                continue;
            }
            Err(problems) => panic!("{file}: extract: {problems:#?}"),
        };
        tally.sources += 1;
        let compiled = compile(&dir);
        assert_eq!(compiled, compile(&dir), "{file}: two compiles differ");

        let (before, after) = (
            GtaFragType::<T>::from_bytes(&bytes).unwrap(),
            GtaFragType::<T>::from_bytes(&compiled).unwrap(),
        );
        let (was, is) = (
            &before.common_drawable.as_ref().unwrap().base,
            &after.common_drawable.as_ref().unwrap().base,
        );
        let (was_meshes, is_meshes) = (meshes(was), meshes(is));
        assert!(
            same_positions(&was_meshes.positions, &is_meshes.positions),
            "{file}: a position moved"
        );
        assert_eq!(was_meshes.indices, is_meshes.indices, "{file}: indices");
        assert_eq!(
            was_meshes.skin, is_meshes.skin,
            "{file}: the skinning changed"
        );
        assert_eq!(
            box_bits(&was.lod_group.bounds_box),
            box_bits(&is.lod_group.bounds_box),
            "{file}: the drawable box moved"
        );
        // The physics, the other drawables and the cloth are the base's.
        assert_eq!(before.drawables().len(), after.drawables().len(), "{file}");
        assert_eq!(before.child_count(), after.child_count(), "{file}");
        assert_eq!(before.tune_name, after.tune_name, "{file}");
        assert_eq!(before.env_cloth.len(), after.env_cloth.len(), "{file}");
        tally.geometries += was_meshes.positions.len();
        fs::remove_dir_all(&dir).unwrap();
    }
    Some(tally)
}

#[test]
fn every_corpus_yft_extracts_and_compiles_back_to_the_same_drawable() {
    let gen8 = every_yft::<Gen8>(TARGET_GEN8);
    let gen9 = every_yft::<Gen9>(TARGET_GEN9);
    for (what, tally) in [("Legacy", gen8), ("Enhanced", gen9)] {
        if let Some(t) = tally {
            println!(
                "{what}: {} fragments, {} sources written, {} with nothing to edit in the common \
                 drawable; {} geometries came back the same",
                t.fragments, t.sources, t.no_drawable_geometry, t.geometries
            );
            assert!(t.sources > 0, "{what}: no fragment became a source");
        }
    }
}

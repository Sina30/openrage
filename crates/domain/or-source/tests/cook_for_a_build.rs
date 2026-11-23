//! What a build asks of a source folder: what it will cook to, before it cooks, and the cook itself.

mod common;
mod model_fixtures;

use std::fs;
use std::path::Path;

use common::*;
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::{sha256_hex, Source, MANIFEST};
use or_source::cook::{cook_source, describe};

fn bark(parent: &Path) -> std::path::PathBuf {
    let spec = vec![Spec::new("bark", GrcTextureFormat::DXT5, ramp(32, 32, 1))];
    extract(parent, "bark_set", &legacy_dictionary(&spec))
}

fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = dir.join(MANIFEST);
    let mut text: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut text);
    fs::write(path, serde_json::to_string_pretty(&text).unwrap()).unwrap();
}

#[test]
fn a_textures_source_says_what_it_cooks_to_before_it_cooks() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    let said = describe(&dir).expect("a textures source is described");
    assert_eq!(said.kind, "textures");
    assert_eq!(said.name, "bark_set");
    assert_eq!(said.extension, "ytd");
    let size = |p: std::path::PathBuf| fs::metadata(p).unwrap().len();
    let payload: u64 = fs::read_dir(dir.join("textures"))
        .unwrap()
        .flatten()
        .map(|e| e.metadata().unwrap().len())
        .sum::<u64>()
        + size(dir.join(MANIFEST))
        + size(dir.join("textures.json"));
    assert!(payload > 0);
    assert_eq!(
        said.bytes, payload,
        "the plan counts the files the cook reads"
    );
    assert!(
        said.game.is_empty(),
        "an authored source is no file of the game's"
    );
}

#[test]
fn a_source_from_the_game_names_the_file_it_was_taken_from() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    let original = sha256_hex(b"the compiled original");
    edit_manifest(&dir, |m| {
        m["origin"]["from_game"] = true.into();
        m["origin"]["sha256"] = original.clone().into();
    });
    assert_eq!(describe(&dir).unwrap().game, vec![original]);
}

#[test]
fn a_platform_but_win64_and_a_kind_nobody_cooks_are_each_refused_in_one_sentence() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    edit_manifest(&dir, |m| m["platform"] = "xbox360".into());
    let why = describe(&dir).unwrap_err();
    assert!(why.contains("xbox360") && why.contains("win64"), "{why}");
    assert!(!why.contains('\n'));

    edit_manifest(&dir, |m| {
        m["platform"] = "win64".into();
        m["kind"] = "bounds".into();
    });
    let why = describe(&dir).unwrap_err();
    assert!(why.contains("bounds"), "{why}");
}

#[test]
fn a_cook_writes_the_file_a_compile_makes_into_the_scratch_folder() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    let scratch = work.path().join("scratch");
    let output = cook_source(&dir, &scratch).expect("the source cooks");
    let id = Source::open(&dir).unwrap().manifest.id;
    assert_eq!(output, scratch.join(id).join("bark_set.ytd"));
    assert_eq!(fs::read(&output).unwrap(), compile(&dir));
}

#[test]
fn a_broken_image_fails_the_cook_and_names_itself_and_writes_nothing() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    fs::write(dir.join("textures").join("bark.png"), b"not an image").unwrap();
    let scratch = work.path().join("scratch");
    let why = cook_source(&dir, &scratch).unwrap_err();
    assert!(why.iter().any(|w| w.contains("bark")), "{why:?}");
    assert!(
        !scratch.exists()
            || fs::read_dir(&scratch)
                .unwrap()
                .flatten()
                .all(|e| { fs::read_dir(e.path()).unwrap().flatten().next().is_none() }),
        "a failed cook left a file"
    );
}

#[test]
fn a_rule_the_texture_cannot_honour_fails_the_cook_and_names_the_texture_and_the_rule() {
    let work = tempfile::tempdir().unwrap();
    let dir = bark(work.path());
    // The DDS is DXT5 and has no image beside it, so a format of another kind would be skipped if it were not refused.
    edit_rules(&dir, |rules| {
        entry(rules, "bark")["encode"] = serde_json::json!({"format": "DXT1"});
    });
    let scratch = work.path().join("scratch");
    let why = cook_source(&dir, &scratch).unwrap_err();
    assert!(
        why.iter().any(|w| w.contains("texture bark")
            && w.contains("format")
            && w.contains("textures.json")),
        "{why:?}"
    );
    assert!(!scratch.exists() || fs::read_dir(&scratch).unwrap().flatten().next().is_none());
}

/// How many files of each extension the corpus check imports, spread over the staged ones.
const SAMPLE: usize = 6;

/// The cook of the source in `folder`, as the bytes a build would ship.
fn cooked(folder: &Path, scratch: &Path) -> Vec<u8> {
    let output =
        cook_source(folder, scratch).unwrap_or_else(|why| panic!("{}: {why:?}", folder.display()));
    fs::read(output).unwrap()
}

/// Imports `bytes` as the game's file `name` into a project of its own and returns the source folder.
fn import_from_the_game(project: &Path, name: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    use or_source::game_import::{import, Facts, Input, Options, Verdict};
    let facts = Facts {
        from_game: true,
        virtual_path: Some(format!("x64a.rpf/{name}")),
        game_build: None,
        ..Facts::default()
    };
    let report = import(
        project,
        &[Input::Bytes { name, bytes, facts }],
        &Options::default(),
    );
    match &report.files[0].verdict {
        Verdict::Made { folder } => Some(folder.clone()),
        _ => None,
    }
}

/// What a build asks of one imported file: is its cook the game's file, before an edit and after.
fn check_the_games_file_until_it_is_edited(file: &str, bytes: &[u8]) -> Option<bool> {
    let project = tempfile::tempdir().unwrap();
    let folder = import_from_the_game(project.path(), file, bytes)?;
    let said = describe(&folder).unwrap_or_else(|why| panic!("{file}: {why}"));
    assert!(
        said.game.contains(&sha256_hex(bytes)),
        "{file}: the game's own file is not one the build leaves out"
    );
    let first = cooked(&folder, &project.path().join("first"));
    assert!(
        said.game.contains(&sha256_hex(&first)),
        "{file}: an import nobody touched cooks to a file the build does not take for the game's, \
         so it would ship"
    );
    if !edit_the_source(&folder) {
        return Some(false);
    }
    let second = cooked(&folder, &project.path().join("second"));
    assert_ne!(first, second, "{file}: the edit did not reach the cook");
    let after = describe(&folder).unwrap();
    assert!(
        !after.game.contains(&sha256_hex(&second)),
        "{file}: an edited source still cooks to what the build takes for the game's file"
    );
    Some(true)
}

#[test]
fn a_source_is_the_games_file_to_a_build_until_it_is_edited_whatever_bytes_the_game_ships() {
    use model_fixtures::{drawable, geometry, ydd, ydr};
    let ytd = legacy_dictionary(&[Spec::new("bark", GrcTextureFormat::DXT5, ramp(32, 32, 1))]);
    let ydr = ydr(drawable("jacket", vec![geometry(6, false, 0.0)], None));
    let ydd = ydd(vec![(
        "jbib_000_u",
        drawable("jbib_000_u", vec![geometry(6, false, 0.0)], None),
    )]);
    for (file, bytes) in [("bark.ytd", ytd), ("jacket.ydr", ydr), ("jbib_u.ydd", ydd)] {
        let shipped = as_the_game_ships_it(&bytes);
        assert_eq!(
            check_the_games_file_until_it_is_edited(file, &shipped),
            Some(true),
            "{file} was not imported and edited"
        );
    }
}

#[test]
fn a_corpus_file_taken_from_the_game_is_the_games_file_to_a_build_until_it_is_edited() {
    let (mut untouched, mut edited) = (0, 0);
    for extension in ["ytd", "ydr", "ydd"] {
        let Some(files) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
            .all_targets()
            .find_or_skip(&format!(
                "a corpus .{extension} is the game's file until it is edited"
            ))
        else {
            continue;
        };
        let step = (files.len() / SAMPLE).max(1);
        for path in files.iter().step_by(step).take(SAMPLE) {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            if let Some(was_edited) =
                check_the_games_file_until_it_is_edited(&file, &fs::read(path).unwrap())
            {
                untouched += 1;
                edited += usize::from(was_edited);
            }
        }
    }
    eprintln!("{untouched} corpus file(s) cook to the game's file untouched; {edited} edited");
    assert!(
        untouched == 0 || edited > 0,
        "no corpus file could be edited"
    );
}

//! A source folder given to an import is taken as it is: an `asset.json`, a `.textures.src`, or a folder that holds sources
//! among its game files, and one step says how many came in. Nothing the import reads is changed.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{extract, legacy_dictionary, ramp, Spec};
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::{Source, MANIFEST};
use or_source::game_import::{import, tree_digest, Facts, Input, Options, Report, Verdict};
use or_source::textures;

fn ytd(texture: &'static str, salt: u32) -> Vec<u8> {
    legacy_dictionary(&[Spec::new(
        texture,
        GrcTextureFormat::DXT5,
        ramp(16, 16, salt),
    )])
}

fn run(root: &Path, inputs: &[Input<'_>]) -> Report {
    import(root, inputs, &Options::default())
}

fn path(path: &Path) -> Input<'_> {
    Input::Path {
        path,
        facts: Facts::default(),
    }
}

fn counts(report: &Report) -> [usize; 4] {
    let mut counts = [0; 4];
    for file in &report.files {
        counts[match file.verdict {
            Verdict::Made { .. } => 0,
            Verdict::AlreadyASource { .. } => 1,
            Verdict::Kept { .. } => 2,
            Verdict::Refused => 3,
        }] += 1;
    }
    counts
}

fn made(report: &Report) -> Vec<PathBuf> {
    (report.files.iter())
        .filter_map(|f| match &f.verdict {
            Verdict::Made { folder } => Some(folder.clone()),
            _ => None,
        })
        .collect()
}

fn id_of(folder: &Path) -> String {
    Source::open(folder).unwrap().manifest.id
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
fn a_textures_src_given_alone_comes_in_whole_with_its_id_and_the_original_is_not_touched() {
    let elsewhere = tempfile::tempdir().unwrap();
    let source = extract(elsewhere.path(), "road", &ytd("road", 3));
    let before = tree_digest(&source).unwrap();
    let project = tempfile::tempdir().unwrap();

    let report = run(project.path(), &[path(&source)]);

    assert_eq!(counts(&report), [1, 0, 0, 0], "{report:#?}");
    let folder = made(&report).remove(0);
    assert_eq!(folder, project.path().join("sources/road.textures.src"));
    assert_eq!(
        tree_digest(&folder).unwrap(),
        before,
        "taken as it is, id and all"
    );
    assert_eq!(tree_digest(&source).unwrap(), before, "the original stays");
    assert_eq!(report.created.len(), 1);
    assert!(report.created[0].untouched());
    assert!(
        report.files[0].reason.contains("as it is"),
        "{}",
        report.files[0].reason
    );
}

#[test]
fn an_asset_json_given_alone_imports_the_folder_it_is_in() {
    let elsewhere = tempfile::tempdir().unwrap();
    let source = extract(elsewhere.path(), "road", &ytd("road", 3));
    let project = tempfile::tempdir().unwrap();
    let marker = source.join(MANIFEST);

    let report = run(project.path(), &[path(&marker)]);

    assert_eq!(counts(&report), [1, 0, 0, 0], "{report:#?}");
    assert_eq!(
        id_of(&made(&report)[0]),
        id_of(&source),
        "the asset.json named its folder"
    );
}

#[test]
fn the_same_source_twice_is_already_a_source_and_a_source_of_the_project_is_one_too() {
    let elsewhere = tempfile::tempdir().unwrap();
    let source = extract(elsewhere.path(), "road", &ytd("road", 3));
    let project = tempfile::tempdir().unwrap();
    let first = run(project.path(), &[path(&source)]);
    let folder = made(&first).remove(0);

    let again = run(project.path(), &[path(&source), path(&folder)]);

    assert_eq!(counts(&again), [0, 2, 0, 0], "{again:#?}");
    assert!(again.created.is_empty(), "nothing was written");
    assert_eq!(
        names(&project.path().join("sources")),
        ["road.textures.src"]
    );
}

#[test]
fn a_copy_that_was_edited_comes_in_beside_the_original_with_an_id_of_its_own() {
    let elsewhere = tempfile::tempdir().unwrap();
    let source = extract(elsewhere.path(), "road", &ytd("road", 3));
    let project = tempfile::tempdir().unwrap();
    let first = made(&run(project.path(), &[path(&source)])).remove(0);
    // Someone changed the copy they hold, and the id went with it.
    fs::write(source.join("notes.txt"), b"my edit").unwrap();

    let report = run(project.path(), &[path(&source)]);

    assert_eq!(counts(&report), [1, 0, 0, 0], "{report:#?}");
    let second = made(&report).remove(0);
    assert_eq!(second, project.path().join("sources/road_2.textures.src"));
    assert_ne!(
        id_of(&second),
        id_of(&first),
        "two sources never share an id"
    );
    assert!(
        report.files[0].reason.contains("id of its own"),
        "{}",
        report.files[0].reason
    );
    assert_eq!(
        Source::open(&second).unwrap().manifest.name,
        "road",
        "the name inside is the source's own, and the folder takes the number"
    );
}

#[test]
fn a_source_whose_kind_is_a_path_is_refused_and_nothing_is_written_outside_the_project() {
    let world = tempfile::tempdir().unwrap();
    let source = extract(&world.path().join("downloads"), "road", &ytd("road", 3));
    let manifest = source.join(MANIFEST);
    let text = fs::read_to_string(&manifest).unwrap();
    let hostile = text.replace(
        "\"kind\": \"textures\"",
        "\"kind\": \"x/../../../../../escaped\"",
    );
    assert_ne!(hostile, text, "the kind was rewritten");
    fs::write(&manifest, hostile).unwrap();
    let project = world.path().join("a/b/proj");
    fs::create_dir_all(&project).unwrap();

    let report = run(&project, &[path(&source)]);

    assert_eq!(counts(&report), [0, 0, 0, 1], "{report:#?}");
    assert!(
        report.files[0].reason.contains("kind"),
        "{}",
        report.files[0].reason
    );
    assert_eq!(
        names(world.path()),
        ["a", "downloads"],
        "the import wrote outside the project"
    );
    assert!(!project.join("sources").exists(), "nothing is left behind");
}

#[test]
fn a_source_the_project_holds_outside_sources_is_already_a_source_and_not_a_second_asset_of_its_id()
{
    let project = tempfile::tempdir().unwrap();
    // A clothing content's own folder, which is not `sources/`.
    let held = extract(
        &project.path().join("clothing/models"),
        "road",
        &ytd("road", 3),
    );

    let by_folder = run(project.path(), &[path(project.path())]);
    let by_itself = run(project.path(), &[path(&held)]);

    for report in [&by_folder, &by_itself] {
        assert_eq!(counts(report), [0, 1, 0, 0], "{report:#?}");
        assert_eq!(
            report.files[0].verdict,
            Verdict::AlreadyASource {
                folder: held.clone()
            }
        );
        assert!(report.created.is_empty(), "nothing was written");
    }
    assert!(!project.path().join("sources").exists());
}

#[test]
fn a_folder_scan_takes_the_sources_and_the_game_files_in_it_and_the_count_says_so() {
    let held = tempfile::tempdir().unwrap();
    let scan = held.path().join("pack");
    let (one, two) = (scan.join("a/one"), scan.join("b/deep/two"));
    extract(&one, "one", &ytd("one", 1));
    extract(&two, "two", &ytd("two", 2));
    fs::create_dir_all(scan.join("c")).unwrap();
    fs::write(scan.join("c/road.ytd"), ytd("road", 3)).unwrap();
    fs::write(scan.join("c/readme.txt"), b"hello").unwrap();
    let project = tempfile::tempdir().unwrap();

    let report = run(project.path(), &[path(&scan)]);

    assert_eq!(
        counts(&report),
        [3, 0, 1, 0],
        "two sources and a ytd made, a text file kept: {report:#?}"
    );
    assert_eq!(
        names(&project.path().join("sources")),
        ["one.textures.src", "road.textures.src", "two.textures.src"]
    );
    // The sources of the walk were taken whole, never walked into as game files.
    assert!(
        (report.files.iter()).all(|f| !f.input.ends_with(".dds")),
        "{report:#?}"
    );
    for folder in made(&report) {
        assert!(Source::open(&folder).is_ok());
    }
}

#[test]
fn a_folder_that_says_it_is_a_source_and_is_not_one_is_refused_with_the_reason() {
    let held = tempfile::tempdir().unwrap();
    let broken = held.path().join("broken.textures.src");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join(MANIFEST), b"{ not json").unwrap();
    let project = tempfile::tempdir().unwrap();

    let report = run(project.path(), &[path(&broken)]);

    assert_eq!(counts(&report), [0, 0, 0, 1], "{report:#?}");
    assert!(
        report.files[0].reason.contains("not an OpenRage source"),
        "{}",
        report.files[0].reason
    );
    assert!(
        !project.path().join("sources").exists(),
        "nothing is left behind"
    );
}

#[test]
fn a_source_that_came_in_cooks_like_the_one_it_was() {
    let elsewhere = tempfile::tempdir().unwrap();
    let source = extract(elsewhere.path(), "road", &ytd("road", 3));
    let wanted = textures::compile(&Source::open(&source).unwrap())
        .unwrap()
        .bytes;
    let project = tempfile::tempdir().unwrap();

    let folder = made(&run(project.path(), &[path(&source)])).remove(0);

    let cooked = textures::compile(&Source::open(&folder).unwrap())
        .unwrap()
        .bytes;
    assert_eq!(cooked, wanted);
}

#[test]
fn the_one_door_decompiles_what_has_a_source_form_and_says_why_for_what_has_none() {
    use or_source::game_import::decompile;
    use or_source::sample;
    let scratch = tempfile::tempdir().unwrap();

    let textures_folder = decompile(
        &sample::texture_dictionary("road", 3),
        "road.ytd",
        scratch.path(),
        &Facts::default(),
    )
    .expect("a texture dictionary decompiles");
    assert_eq!(textures_folder, scratch.path().join("road.textures.src"));
    let models_folder = decompile(
        &sample::drawable_dictionary("jbib_000_u", 6, 0.0),
        "jbib_000_u.ydd",
        scratch.path(),
        &Facts::default(),
    )
    .expect("a drawable dictionary decompiles");
    assert_eq!(
        models_folder,
        scratch.path().join("jbib_000_u.drawables.src")
    );
    assert!(
        or_source::cook::cook_source(&models_folder, &scratch.path().join("cooked")).is_ok(),
        "and it cooks back"
    );

    let cloth = decompile(
        b"cloth",
        "jbib_000_u.yld",
        scratch.path(),
        &Facts::default(),
    );
    assert!(cloth.unwrap_err().contains("no source form"));
    let broken = decompile(b"not a ytd", "x.ytd", scratch.path(), &Facts::default());
    assert!(broken.is_err());
    assert!(
        !scratch.path().join("x.textures.src").exists(),
        "a refusal leaves nothing behind"
    );
}

//! Saving a file inside a source recooks that one asset into a shadow output, and nothing else.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::Source;
use or_source::cook::{Cooker, Ignored, Recook};

/// A project folder holding two sources and, beside them, the shadow folder a build reads.
struct Project {
    root: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
    id_a: String,
    id_b: String,
}

impl Project {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let spec = |name| vec![Spec::new(name, GrcTextureFormat::DXT5, ramp(32, 32, 1))];
        let a = extract(root.path(), "a", &legacy_dictionary(&spec("bark")));
        let b = extract(root.path(), "b", &legacy_dictionary(&spec("stone")));
        let id = |dir: &Path| Source::open(dir).unwrap().manifest.id;
        Self {
            id_a: id(&a),
            id_b: id(&b),
            a,
            b,
            root,
        }
    }

    fn shadow(&self) -> PathBuf {
        self.root.path().join("build").join("shadow")
    }

    fn output_of_a(&self) -> PathBuf {
        self.shadow().join(&self.id_a).join("a.ytd")
    }

    fn cooker(&self) -> Cooker {
        Cooker::new(self.shadow())
    }
}

fn problems(event: Recook) -> Vec<or_source::Problem> {
    match event {
        Recook::Failed { problems, .. } => problems,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn saving_an_image_recooks_that_one_asset_and_no_other() {
    let project = Project::new();
    let mut cooker = project.cooker();

    let first = cooker.recook(&project.a.join("asset.json"));
    assert_eq!(
        first,
        Recook::Cooked {
            source: project.a.clone(),
            output: project.output_of_a()
        }
    );
    let before = fs::read(project.output_of_a()).unwrap();
    assert_eq!(before, compile(&project.a));

    save_image(&project.a.join("textures/bark.png"), &quadrants(32, 32));
    let saved = cooker.recook(&project.a.join("textures/bark.png"));
    assert!(matches!(saved, Recook::Cooked { .. }), "{saved:?}");
    let after = fs::read(project.output_of_a()).unwrap();
    assert_ne!(before, after, "the saved image did not reach the output");
    assert_eq!(after, compile(&project.a));

    // The other asset was never read into the shadow, and the shadow holds one asset's folder.
    assert!(!project.shadow().join(&project.id_b).exists());
    assert_eq!(fs::read_dir(project.shadow()).unwrap().count(), 1);
    assert!(
        project.b.join("asset.json").is_file(),
        "its source is where it was"
    );
    let _ = &project.b;
}

#[test]
fn what_the_cook_wrote_and_what_changed_nothing_are_ignored() {
    let project = Project::new();
    let mut cooker = project.cooker();
    cooker.recook(&project.a.join("textures.json"));
    let output = project.output_of_a();

    // A file watcher reports the shadow folder's own files, the output and the temporary one beside it.
    let temporary = output.with_extension("ytd.tmp");
    assert_eq!(cooker.recook(&output), Recook::Ignored(Ignored::OwnWrite));
    assert_eq!(
        cooker.recook(&temporary),
        Recook::Ignored(Ignored::OwnWrite)
    );

    // A save that changes no input cooks nothing: the sentinel would be gone if it had.
    fs::write(&output, b"SENTINEL").unwrap();
    let again = cooker.recook(&project.a.join("textures.json"));
    assert_eq!(
        again,
        Recook::Unchanged {
            source: project.a.clone()
        }
    );
    assert_eq!(fs::read(&output).unwrap(), b"SENTINEL");

    // The same image saved twice is one change.
    let image = project.a.join("textures/bark.png");
    save_image(&image, &quadrants(32, 32));
    assert!(matches!(cooker.recook(&image), Recook::Cooked { .. }));
    fs::write(&output, b"SENTINEL").unwrap();
    save_image(&image, &quadrants(32, 32));
    assert!(matches!(cooker.recook(&image), Recook::Unchanged { .. }));
    assert_eq!(fs::read(&output).unwrap(), b"SENTINEL");

    // A deleted output is cooked again even though the inputs are as they were.
    fs::remove_file(&output).unwrap();
    assert!(matches!(cooker.recook(&image), Recook::Cooked { .. }));
    assert_eq!(fs::read(&output).unwrap(), compile(&project.a));
}

#[test]
fn a_failure_is_data_and_the_previous_output_stays_in_place() {
    let project = Project::new();
    let mut cooker = project.cooker();
    assert!(matches!(
        cooker.recook(&project.a.join("asset.json")),
        Recook::Cooked { .. }
    ));
    let good = fs::read(project.output_of_a()).unwrap();

    // A DDS cut short.
    let dds = project.a.join("textures/bark.dds");
    let whole = fs::read(&dds).unwrap();
    fs::write(&dds, &whole[..whole.len() / 2]).unwrap();
    let listed = problems(cooker.recook(&dds));
    assert_eq!(listed[0].file.as_deref(), Some("textures/bark.dds"));
    assert_eq!(listed[0].texture.as_deref(), Some("bark"));
    assert_eq!(fs::read(project.output_of_a()).unwrap(), good);

    // Two images of one name.
    fs::write(&dds, &whole).unwrap();
    save_image(&project.a.join("textures/bark.png"), &quadrants(32, 32));
    fs::copy(
        project.a.join("textures/bark.png"),
        project.a.join("textures/bark.tga"),
    )
    .unwrap();
    let listed = problems(cooker.recook(&project.a.join("textures/bark.tga")));
    assert!(listed[0].message.contains("bark.png") && listed[0].message.contains("bark.tga"));
    assert_eq!(fs::read(project.output_of_a()).unwrap(), good);

    // A textures.json that no longer parses, and an asset.json that is no longer a source's.
    fs::remove_file(project.a.join("textures/bark.tga")).unwrap();
    fs::remove_file(project.a.join("textures/bark.png")).unwrap();
    fs::write(project.a.join("textures.json"), "{ not json").unwrap();
    let listed = problems(cooker.recook(&project.a.join("textures.json")));
    assert_eq!(listed[0].file.as_deref(), Some("textures.json"));
    fs::write(project.a.join("asset.json"), "{}").unwrap();
    let listed = problems(cooker.recook(&project.a.join("asset.json")));
    assert_eq!(listed[0].file.as_deref(), Some("asset.json"));
    assert_eq!(fs::read(project.output_of_a()).unwrap(), good);
}

#[test]
fn a_file_that_is_no_input_of_a_source_is_ignored_and_a_deleted_image_is_not() {
    let project = Project::new();
    let mut cooker = project.cooker();
    let ignored = |cooker: &mut Cooker, path: PathBuf| match cooker.recook(&path) {
        Recook::Ignored(why) => why,
        other => panic!("{}: {other:?}", path.display()),
    };
    let outside = project.root.path().join("readme.txt");
    assert_eq!(ignored(&mut cooker, outside), Ignored::OutsideSource);
    for not_an_input in [
        "textures/notes.txt",
        "textures/bark.png.tmp",
        "textures/nested/deep.png",
        "build.log",
    ] {
        assert_eq!(
            ignored(&mut cooker, project.a.join(not_an_input)),
            Ignored::NotAnInput,
            "{not_an_input}"
        );
    }
    assert!(!project.shadow().exists(), "nothing was cooked");

    // An image that was there and is not: the texture goes back to its DDS.
    let image = project.a.join("textures/bark.png");
    save_image(&image, &quadrants(32, 32));
    assert!(matches!(cooker.recook(&image), Recook::Cooked { .. }));
    let with_image = fs::read(project.output_of_a()).unwrap();
    fs::remove_file(&image).unwrap();
    assert!(matches!(cooker.recook(&image), Recook::Cooked { .. }));
    assert_ne!(fs::read(project.output_of_a()).unwrap(), with_image);
}

/// Every path under `dir`, so a test can tell that a cook added nothing anywhere.
fn tree(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap().flatten() {
            if entry.path().is_dir() {
                pending.push(entry.path());
            }
            found.push(entry.path());
        }
    }
    found.sort();
    found
}

fn set_in_manifest(source: &Path, key: &str, value: &str) {
    let path = source.join("asset.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest[key] = value.into();
    fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

#[test]
fn an_id_or_a_name_that_is_a_path_is_refused_and_nothing_is_written_outside_the_shadow() {
    let project = Project::new();
    let elsewhere = tempfile::tempdir().unwrap();
    let absolute = elsewhere.path().to_string_lossy().into_owned();
    let original = fs::read_to_string(project.a.join("asset.json")).unwrap();
    let mut cooker = project.cooker();
    let before = tree(project.root.path());

    let hostile = [
        ("id", r"..\..\escaped"),
        ("id", "../../escaped"),
        ("id", absolute.as_str()),
        ("id", ".."),
        ("id", "aux"),
        ("name", "com1.backup"),
        ("id", ""),
        ("name", r"..\..\..\victim"),
        ("name", "../../victim"),
        ("name", "sub/dir"),
        ("name", r"C:\victim"),
        ("name", ".."),
        ("name", ""),
    ];
    for (key, value) in hostile {
        fs::write(project.a.join("asset.json"), &original).unwrap();
        set_in_manifest(&project.a, key, value);
        let listed = problems(cooker.recook(&project.a.join("asset.json")));
        assert_eq!(
            listed[0].file.as_deref(),
            Some("asset.json"),
            "{key} {value:?}"
        );
        assert!(
            listed[0].message.contains(&format!("{key}: ")),
            "{key} {value:?}: {}",
            listed[0]
        );
        assert_eq!(
            tree(project.root.path()),
            before,
            "{key} {value:?} wrote inside the project"
        );
        assert_eq!(
            fs::read_dir(elsewhere.path()).unwrap().count(),
            0,
            "{key} {value:?} wrote into a folder of its own choosing"
        );
    }
}

#[test]
fn a_header_that_is_hostile_fails_as_data_and_the_previous_output_stays() {
    let project = Project::new();
    let mut cooker = project.cooker();
    assert!(matches!(
        cooker.recook(&project.a.join("asset.json")),
        Recook::Cooked { .. }
    ));
    let good = fs::read(project.output_of_a()).unwrap();

    let dds = project.a.join("textures/bark.dds");
    let whole = fs::read(&dds).unwrap();
    for (width, height, mips) in [(0x7FFF_FFFFu32, 0x7FFF_FFFFu32, 1u32), (32, 32, u32::MAX)] {
        let mut hostile = whole.clone();
        hostile[12..16].copy_from_slice(&height.to_le_bytes());
        hostile[16..20].copy_from_slice(&width.to_le_bytes());
        hostile[28..32].copy_from_slice(&mips.to_le_bytes());
        fs::write(&dds, &hostile).unwrap();
        let listed = problems(cooker.recook(&dds));
        assert_eq!(listed[0].file.as_deref(), Some("textures/bark.dds"));
        assert_eq!(listed[0].texture.as_deref(), Some("bark"));
        assert_eq!(fs::read(project.output_of_a()).unwrap(), good);
    }
}

#[test]
fn a_source_of_a_kind_this_build_does_not_cook_is_a_failure_and_not_a_silence() {
    let project = Project::new();
    let text = fs::read_to_string(project.a.join("asset.json")).unwrap();
    fs::write(
        project.a.join("asset.json"),
        text.replace("\"textures\"", "\"cloth\""),
    )
    .unwrap();
    let listed = problems(project.cooker().recook(&project.a.join("asset.json")));
    assert!(listed[0].message.contains("cloth"), "{}", listed[0]);
}

/// `from` and everything in it, copied to `to`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn two_sources_with_one_id_do_not_share_a_shadow_slot() {
    let project = Project::new();
    // A folder copied in a file manager keeps its `asset.json`, and so its id.
    let copy = project.root.path().join("copy of a.textures.src");
    copy_tree(&project.a, &copy);
    let mut cooker = project.cooker();

    assert!(matches!(
        cooker.recook(&project.a.join("asset.json")),
        Recook::Cooked { .. }
    ));
    let output = project.output_of_a();
    let of_a = fs::read(&output).unwrap();

    // The copy is edited and saved: its cook would land in a's slot, so it is refused and names both.
    save_image(&copy.join("textures/bark.png"), &quadrants(32, 32));
    let listed = problems(cooker.recook(&copy.join("textures/bark.png")));
    assert_eq!(listed[0].file.as_deref(), Some("asset.json"));
    let said = listed[0].message.as_str();
    assert!(
        said.contains(&project.id_a) && said.contains(&project.a.display().to_string()),
        "{said}"
    );
    assert_eq!(
        fs::read(&output).unwrap(),
        of_a,
        "a's output was overwritten"
    );

    // The first source keeps its slot, and a recook of it is as before.
    assert!(matches!(
        cooker.recook(&project.a.join("textures.json")),
        Recook::Unchanged { .. }
    ));

    // Once the first is gone the slot is free, and the copy takes it.
    fs::remove_dir_all(&project.a).unwrap();
    let taken = cooker.recook(&copy.join("textures/bark.png"));
    assert!(matches!(taken, Recook::Cooked { .. }), "{taken:?}");
    assert_eq!(fs::read(&output).unwrap(), compile(&copy));
    assert_ne!(fs::read(&output).unwrap(), of_a);
}

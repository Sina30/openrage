//! What the Project and Wardrobe workspaces do with a source, a file of the project or a pack's textures runs headless over a staged project:
//! `source compile`, `project recook`, `remove`, `history` and `rollback`, `wardrobe maps` and `wardrobe budget`, and `project import --skeleton`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::authoring::TextureBuildOptions;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::gta5::PgDictionary;
use or_project::{Container, Module, Output, OutputKind, Project};
use or_source::game_import::{decompile, tree_digest, Facts};
use or_wardrobe::{Component, Garment, Ped, Slot, Wardrobe};

fn cli(args: &[&str]) -> ExitCode {
    let all: Vec<String> = args.iter().map(ToString::to_string).collect();
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// A `.ytd` of one texture of `side` by `side`, DXT1, with a weave in it so that nothing about it is flat.
fn ytd(name: &str, side: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let v = (40 + 150 * ((x / 4 + y / 4) % 2) + x % 4 * 10) as u8;
            image.set_pixel(x, y, [v, v / 2 + 20, (x * 3 % 256) as u8, 255]);
        }
    }
    let mut texture = GrcTexture {
        name: name.to_string(),
        resource_type_and_conversion_flags: PROCESSED,
        ..Default::default()
    };
    let options = TextureBuildOptions {
        mip_levels: Some(1),
        ..TextureBuildOptions::default()
    };
    texture
        .set_pixels(GrcTextureFormat::DXT1, &image, &options)
        .unwrap();
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(texture);
    dictionary.to_bytes().unwrap()
}

fn files_project(dir: &Path) -> PathBuf {
    let file = dir.join("harbor.openrage-proj");
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::of(
            OutputKind::FivemResource,
            "fivem",
            "harbor",
            &["files"],
        )],
    };
    Project::create(&file, &container, &[]).unwrap();
    file
}

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => walk(base, &path, out),
                false => {
                    let rel = path
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out.retain(|rel, _| !rel.starts_with(".openrage/"));
    out
}

#[test]
fn one_source_is_compiled_on_its_own_into_a_folder_and_a_source_that_does_not_compile_writes_nothing(
) {
    let work = tempfile::tempdir().unwrap();
    let sources = work.path().join("sources");
    std::fs::create_dir_all(&sources).unwrap();
    let source = decompile(&ytd("logo", 32), "logo.ytd", &sources, &Facts::default()).unwrap();
    let before = tree_digest(&source).unwrap();
    let out = work.path().join("compiled");

    assert_eq!(
        cli(&["source", "compile", text(&source), "--out", text(&out)]),
        ExitCode::SUCCESS
    );
    let made = std::fs::read(out.join("logo.ytd")).expect("the game file is in --out");
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(&made).expect("it reloads");
    assert!(dictionary.get("logo").is_some());
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        1,
        "nothing but the file is left in --out"
    );
    assert_eq!(
        tree_digest(&source).unwrap(),
        before,
        "the source is as it was"
    );

    // A texture file that is no DDS stops the compile: no file is written and the source is not touched.
    std::fs::write(source.join("textures/logo.dds"), b"not a dds").unwrap();
    let broken = tree_digest(&source).unwrap();
    let other = work.path().join("other");
    assert_eq!(
        cli(&["source", "compile", text(&source), "--out", text(&other)]),
        ExitCode::FAILURE
    );
    assert!(!other.join("logo.ytd").exists());
    assert_eq!(tree_digest(&source).unwrap(), broken);

    assert_eq!(
        cli(&["source", "compile", text(&source)]),
        ExitCode::from(2),
        "--out is required"
    );
}

#[test]
fn a_file_is_taken_out_of_the_project_and_the_history_brings_it_back() {
    let work = tempfile::tempdir().unwrap();
    let file = files_project(work.path());
    let notes = work.path().join("notes.txt");
    std::fs::write(&notes, b"hello").unwrap();
    assert_eq!(
        cli(&["project", "add-content", text(&file), text(&notes)]),
        ExitCode::SUCCESS
    );
    let members = |file: &Path| {
        Project::load(file)
            .unwrap()
            .members
            .iter()
            .map(|m| m.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(members(&file), ["notes.txt"]);

    assert_eq!(
        cli(&["project", "remove", text(&file), text(&notes)]),
        ExitCode::SUCCESS
    );
    assert!(members(&file).is_empty());
    assert!(notes.is_file(), "the file stays on disk");
    assert_eq!(
        cli(&[
            "project",
            "remove",
            text(&file),
            text(&work.path().join("nothing.txt"))
        ]),
        ExitCode::FAILURE,
        "a file the project does not hold is refused"
    );

    // The history lists the step the removal kept, and a rollback to the step before it brings the file back.
    let steps = or_project::snapshot::Snapshots::open(&or_project::snapshot::history_dir(&file));
    let labels: Vec<&str> = steps.steps().iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "The project as it was opened",
            "Removed notes.txt from the project"
        ]
    );
    assert_eq!(cli(&["project", "history", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        cli(&["project", "history", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    let first = steps.steps()[0].n.to_string();
    assert_eq!(
        cli(&["project", "rollback", text(&file), &first]),
        ExitCode::SUCCESS
    );
    assert_eq!(members(&file), ["notes.txt"]);
    assert_eq!(
        cli(&["project", "rollback", text(&file), "99"]),
        ExitCode::FAILURE,
        "a step that is not kept is refused"
    );
}

#[test]
fn a_rollback_over_an_import_takes_back_its_sources_unless_one_was_changed() {
    let work = tempfile::tempdir().unwrap();
    let file = files_project(work.path());
    let incoming = work.path().join("incoming");
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::write(incoming.join("logo.ytd"), ytd("logo", 32)).unwrap();
    std::fs::write(incoming.join("badge.ytd"), ytd("badge", 32)).unwrap();
    let steps = || {
        or_project::snapshot::Snapshots::open(&or_project::snapshot::history_dir(&file))
            .steps()
            .iter()
            .map(|s| (s.n, s.label.clone()))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        cli(&[
            "project",
            "import",
            text(&file),
            text(&incoming.join("logo.ytd"))
        ]),
        ExitCode::SUCCESS
    );
    let logo = work.path().join("sources/logo.textures.src");
    assert!(logo.is_dir());
    assert_eq!(
        steps().iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        ["The project as it was opened", "Imported 1 source(s)"],
        "the import keeps the step the editor keeps"
    );

    // Back to the project as it was opened: the source the import made is taken back, and the file the import read is not touched.
    assert_eq!(
        cli(&[
            "project",
            "rollback",
            text(&file),
            &steps()[0].0.to_string()
        ]),
        ExitCode::SUCCESS
    );
    assert!(!logo.exists());
    assert!(incoming.join("logo.ytd").is_file());

    // A source somebody changed since its import stops the rollback with nothing removed.
    assert_eq!(
        cli(&[
            "project",
            "rollback",
            text(&file),
            &steps()[1].0.to_string()
        ]),
        ExitCode::SUCCESS,
        "forward again: the project moves and the source is not made again"
    );
    assert_eq!(
        cli(&[
            "project",
            "import",
            text(&file),
            text(&incoming.join("badge.ytd"))
        ]),
        ExitCode::SUCCESS
    );
    let badge = work.path().join("sources/badge.textures.src");
    std::fs::write(badge.join("notes.txt"), b"mine").unwrap();
    let first = steps()[0].0.to_string();
    assert_eq!(
        cli(&["project", "rollback", text(&file), &first]),
        ExitCode::FAILURE
    );
    assert!(badge.join("notes.txt").is_file(), "nothing was removed");
}

#[test]
fn one_source_is_recooked_into_the_shadow_folder_and_a_failure_leaves_the_last_output() {
    let work = tempfile::tempdir().unwrap();
    let file = files_project(work.path());
    let sources = work.path().join("sources");
    std::fs::create_dir_all(&sources).unwrap();
    let source = decompile(&ytd("logo", 32), "logo.ytd", &sources, &Facts::default()).unwrap();

    assert_eq!(
        cli(&["project", "recook", text(&file), text(&source)]),
        ExitCode::SUCCESS
    );
    let shadow = or_project::shadow_root(&file);
    let cooked: Vec<PathBuf> = walk_files(&shadow);
    assert_eq!(cooked.len(), 1, "{cooked:?}");
    assert!(cooked[0].ends_with("logo.ytd"));
    let kept = std::fs::read(&cooked[0]).unwrap();

    std::fs::write(source.join("textures/logo.dds"), b"not a dds").unwrap();
    assert_eq!(
        cli(&["project", "recook", text(&file), text(&source)]),
        ExitCode::FAILURE
    );
    assert_eq!(
        std::fs::read(&cooked[0]).unwrap(),
        kept,
        "the last output is as it was"
    );
    assert_eq!(
        cli(&[
            "project",
            "recook",
            text(&file),
            text(&work.path().join("nowhere"))
        ]),
        ExitCode::FAILURE
    );
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// A project whose one clothing content holds two jackets, the first with a diffuse of 128 by 128 and the second with one of 64 by 64.
fn a_pack(dir: &Path) -> (PathBuf, PathBuf) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    for (name, side) in [("jbib_diff_000_a_uni", 128), ("jbib_diff_001_a_uni", 64)] {
        decompile(
            &ytd(name, side),
            &format!("{name}.ytd"),
            &pack.join("textures"),
            &Facts::default(),
        )
        .unwrap();
    }
    let mut w = Wardrobe::new("demo");
    for (n, name) in ["jbib_diff_000_a_uni", "jbib_diff_001_a_uni"]
        .iter()
        .enumerate()
    {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.position = Some(n as u32);
        g.model = Some(format!("models/m{n}.ydd"));
        g.variations = vec![or_wardrobe::Variation {
            texture: Some(format!("textures/{name}.textures.src")),
            ..Default::default()
        }];
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "clothing")],
        outputs: vec![Output::fivem_resource("fivem", "demo", &["wardrobe"])],
    };
    let file = dir.join("demo.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    (file, pack)
}

#[test]
fn maps_are_listed_by_a_dry_run_and_made_in_copies_the_garments_name() {
    let dir = tempfile::tempdir().unwrap();
    let (file, pack) = a_pack(dir.path());
    let file = text(&file);
    let before = tree(&pack);

    assert_eq!(
        cli(&["wardrobe", "maps", file, "--dry-run", "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(tree(&pack), before, "a dry run writes nothing");

    assert_eq!(cli(&["wardrobe", "maps", file]), ExitCode::SUCCESS);
    let after = Wardrobe::read(&pack).unwrap();
    let named: Vec<String> = after
        .garments
        .iter()
        .map(|g| g.variations[0].texture.clone().unwrap())
        .collect();
    assert_eq!(
        named,
        [
            "textures/jbib_diff_000_a_uni-2.textures.src",
            "textures/jbib_diff_001_a_uni-2.textures.src"
        ]
    );
    let copy = pack.join(&named[0]);
    assert!(copy.join("textures/jbib_normal_000_a_uni.png").is_file());
    assert!(copy.join("textures/jbib_spec_000_a_uni.png").is_file());
    assert!(
        pack.join("textures/jbib_diff_000_a_uni.textures.src")
            .is_dir(),
        "the source it was is still there"
    );
    assert_eq!(
        cli(&["wardrobe", "maps", file, "--no-normal", "--no-specular"]),
        ExitCode::from(1)
    );
    assert_eq!(
        cli(&["wardrobe", "maps", file, "--strength", "0"]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_budget_is_a_dry_run_until_it_is_applied_and_a_target_it_cannot_reach_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let (file, pack) = a_pack(dir.path());
    let file = text(&file);
    let before = tree(&pack);

    // 128 by 128 and 64 by 64 in DXT1 with one mip are 8,192 and 2,048 bytes: 6 KB asks for a cut.
    assert_eq!(
        cli(&["wardrobe", "budget", file, "--target", "6KB", "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(tree(&pack), before, "a dry run writes nothing");
    assert_eq!(
        cli(&["wardrobe", "budget", file, "--target", "6KB", "--apply"]),
        ExitCode::SUCCESS
    );
    let after = Wardrobe::read(&pack).unwrap();
    assert!(
        after.garments.iter().any(|g| g.variations[0]
            .texture
            .as_deref()
            .is_some_and(|t| t.contains("-2"))),
        "a garment names a lighter copy"
    );
    assert_eq!(
        cli(&["wardrobe", "budget", file, "--target", "1"]),
        ExitCode::FAILURE,
        "a target below what the textures can come to"
    );
    assert_eq!(
        cli(&["wardrobe", "budget", file]),
        ExitCode::from(2),
        "a target is required"
    );
    assert_eq!(
        cli(&["wardrobe", "budget", file, "--target", "lots"]),
        ExitCode::from(2)
    );
}

#[test]
fn a_folder_is_bound_to_a_slot_listed_and_let_go_and_the_newest_picture_is_put_in_now() {
    let dir = tempfile::tempdir().unwrap();
    let (file, pack) = a_pack(dir.path());
    let file = text(&file);
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    let garment = Wardrobe::read(&pack).unwrap().garments[0].id.clone();

    assert_eq!(
        cli(&[
            "wardrobe",
            "slot",
            file,
            "--garment",
            &garment,
            "--folder",
            text(&art),
            "--now"
        ]),
        ExitCode::SUCCESS,
        "a folder with no picture yet is bound all the same"
    );
    assert_eq!(
        cli(&["wardrobe", "slot", file, "--list", "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(or_wardrobe::slots::bindings(&pack).len(), 1);

    let mut picture = RgbaImage::new(128, 128);
    for y in 0..128 {
        for x in 0..128 {
            picture.set_pixel(x, y, [(x * 2) as u8, 40, (y * 2) as u8, 255]);
        }
    }
    or_io::codec::texture::save_png(art.join("saved.png"), &picture).unwrap();
    assert_eq!(
        cli(&[
            "wardrobe",
            "slot",
            file,
            "--garment",
            &garment,
            "--folder",
            text(&art),
            "--now"
        ]),
        ExitCode::SUCCESS
    );
    assert!(
        pack.join("textures/jbib_diff_000_a_uni.textures.src/textures/jbib_diff_000_a_uni.png")
            .is_file(),
        "the picture is the diffuse of the texture"
    );
    assert_eq!(
        cli(&[
            "wardrobe",
            "slot",
            file,
            "--garment",
            &garment,
            "--folder",
            text(&art),
            "--unbind"
        ]),
        ExitCode::SUCCESS
    );
    assert!(or_wardrobe::slots::bindings(&pack).is_empty());
    assert_eq!(
        cli(&[
            "wardrobe",
            "slot",
            file,
            "--garment",
            &garment,
            "--folder",
            text(&art),
            "--slot",
            "sideways"
        ]),
        ExitCode::from(2)
    );
    assert_eq!(
        cli(&[
            "wardrobe",
            "slot",
            file,
            "--garment",
            "nobody",
            "--folder",
            text(&art)
        ]),
        ExitCode::FAILURE
    );
}

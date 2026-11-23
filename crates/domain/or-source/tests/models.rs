//! A drawable or a drawable dictionary extracted to a source folder and compiled back, on drawables the suite builds.

mod model_fixtures;

use std::path::{Path, PathBuf};

use model_fixtures::{
    drawable, geometry, meshes, read_ydd, read_ydr, reference, same_positions, ydd, ydr,
};
use or_interchange::gltf::{self, Container};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
use or_source::asset::{sha256_hex, AssetManifest, Source, MANIFEST};
use or_source::models::{self, Compiled, Extraction};
use or_source::Problem;
use serde_json::{json, Value};

fn how() -> Extraction<'static> {
    Extraction {
        from_game: false,
        virtual_path: None,
        textures: false,
        skeleton: None,
    }
}

fn extract_ydr(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    models::extract_drawable(bytes, name, parent, &how())
        .unwrap_or_else(|problems| panic!("extract: {problems:#?}"))
}

fn extract_ydd(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    models::extract_drawables(bytes, name, parent, &how())
        .unwrap_or_else(|problems| panic!("extract: {problems:#?}"))
}

fn compile(dir: &Path) -> Compiled {
    models::compile(&Source::open(dir).unwrap())
        .unwrap_or_else(|problems| panic!("compile: {problems:#?}"))
}

fn refusal(dir: &Path) -> Vec<Problem> {
    match models::compile(&Source::open(dir).unwrap()) {
        Ok(_) => panic!("{} compiled and was expected to be refused", dir.display()),
        Err(problems) => problems,
    }
}

fn said(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Two geometries, so a pairing that confuses them shows.
fn two_geometries() -> Vec<u8> {
    ydr(drawable(
        "subject",
        vec![geometry(6, false, 0.0), geometry(9, false, 40.0)],
        None,
    ))
}

/// The scene of `bytes`' first drawable, all its levels, as an export of it would be.
fn scene_of(bytes: &[u8]) -> or_interchange::Scene {
    drawable_scene(
        "subject",
        &read_ydr(bytes).base,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    )
}

fn lifted(mut scene: or_interchange::Scene, by: f32) -> or_interchange::Scene {
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                for position in &mut primitive.positions {
                    position[2] += by;
                }
            }
        }
    }
    scene
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn set_json(path: &Path, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

#[test]
fn a_drawable_extracts_to_the_folder_the_design_names() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = extract_ydr(scratch.path(), "subject", &bytes);

    assert_eq!(folder, scratch.path().join("subject.drawable.src"));
    assert_eq!(
        files_in(&folder),
        ["asset.json", "base.ydr", "drawable.json", "model.glb"]
    );
    let manifest =
        AssetManifest::parse(&std::fs::read_to_string(folder.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.kind, "drawable");
    assert_eq!(manifest.name, "subject");
    assert_eq!(manifest.target, "gen8");
    assert_eq!(manifest.origin.sha256, Some(sha256_hex(&bytes)));
    // The base is the file itself, and the manifest says which one it is by its hash.
    assert_eq!(std::fs::read(folder.join("base.ydr")).unwrap(), bytes);
    assert_eq!(
        manifest.extra["compile"],
        json!({"base": sha256_hex(&bytes), "mode": "edit"})
    );
    assert!(Source::open(&folder).is_ok());
}

#[test]
fn a_source_compiles_back_to_the_same_meshes() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = extract_ydr(scratch.path(), "subject", &bytes);
    let compiled = compile(&folder);

    assert_eq!(compiled.file_name, "subject.ydr");
    let (before, after) = (
        meshes(&read_ydr(&bytes).base),
        meshes(&read_ydr(&compiled.bytes).base),
    );
    assert!(same_positions(&before.positions, &after.positions));
    assert_eq!(before.indices, after.indices);
    assert_eq!(before.skin, after.skin);
    assert!(
        compiled.report.iter().any(|l| l.contains("2 geometries")),
        "{:?}",
        compiled.report
    );
}

#[test]
fn compiling_an_unchanged_source_twice_is_byte_identical() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    assert_eq!(compile(&folder).bytes, compile(&folder).bytes);
}

/// ⚠ A round trip of geometry nobody changed passes when the compile writes nothing, so this one moves it.
#[test]
fn an_edit_to_the_glb_lands_in_the_compiled_file() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = extract_ydr(scratch.path(), "subject", &bytes);

    let moved = lifted(scene_of(&bytes), 2.0);
    std::fs::write(
        folder.join("model.glb"),
        gltf::write(&moved, "subject", Container::Glb).primary,
    )
    .unwrap();

    let after = meshes(&read_ydr(&compile(&folder).bytes).base);
    let before = meshes(&read_ydr(&bytes).base);
    let expected: Vec<Vec<[f32; 3]>> = before
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + 2.0]).collect())
        .collect();
    assert!(same_positions(&expected, &after.positions));
    assert_eq!(after.indices, before.indices);
}

#[test]
fn a_base_that_is_not_the_one_the_manifest_names_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    let other = ydr(drawable("subject", vec![geometry(3, false, 1.0)], None));
    std::fs::write(folder.join("base.ydr"), other).unwrap();

    let why = said(&refusal(&folder));
    assert!(why.contains("base.ydr") && why.contains("compile"), "{why}");
}

#[test]
fn a_rule_this_build_has_no_code_for_is_refused_by_name() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    set_json(&folder.join("drawable.json"), |v| {
        v["shaders"] = json!([{"name": "ped_default"}]);
    });
    let why = said(&refusal(&folder));
    assert!(
        why.contains("drawable.json") && why.contains("shaders"),
        "{why}"
    );
}

#[test]
fn author_mode_is_refused_by_name_and_edit_is_the_only_mode() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    set_json(&folder.join(MANIFEST), |v| {
        v["compile"]["mode"] = json!("author")
    });
    let why = said(&refusal(&folder));
    assert!(why.contains("author") && why.contains("edit"), "{why}");
}

#[test]
fn a_source_with_no_glb_says_which_file_is_missing() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    std::fs::remove_file(folder.join("model.glb")).unwrap();
    let problems = refusal(&folder);
    assert_eq!(
        problems[0].file.as_deref(),
        Some("model.glb"),
        "{problems:?}"
    );
}

#[test]
fn a_geometry_count_the_base_does_not_have_is_refused_not_guessed() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = extract_ydr(scratch.path(), "subject", &bytes);
    // A glb of one geometry, for a base of two.
    let one = ydr(drawable("subject", vec![geometry(6, false, 0.0)], None));
    std::fs::write(
        folder.join("model.glb"),
        gltf::write(&scene_of(&one), "subject", Container::Glb).primary,
    )
    .unwrap();
    let why = said(&refusal(&folder));
    assert!(
        why.contains("2 geometries") && why.contains("brings 1"),
        "{why}"
    );
}

#[test]
fn a_drawable_with_no_geometry_is_not_a_source() {
    let scratch = tempfile::tempdir().unwrap();
    let empty = ydr(drawable("empty", Vec::new(), None));
    let problems = models::extract_drawable(&empty, "empty", scratch.path(), &how()).unwrap_err();
    assert!(said(&problems).contains("no geometry"), "{problems:?}");
    assert!(
        !scratch.path().join("empty.drawable.src").exists(),
        "nothing is left behind"
    );
}

#[test]
fn a_dictionary_extracts_a_folder_per_drawable_and_compiles_back_to_the_same_meshes() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydd(vec![
        (
            "alpha",
            drawable("alpha", vec![geometry(6, false, 0.0)], None),
        ),
        (
            "beta",
            drawable(
                "beta",
                vec![geometry(3, false, 10.0), geometry(9, false, 20.0)],
                None,
            ),
        ),
    ]);
    let folder = extract_ydd(scratch.path(), "pair", &bytes);

    assert_eq!(folder, scratch.path().join("pair.drawables.src"));
    assert_eq!(
        files_in(&folder),
        ["asset.json", "base.ydd", "dictionary.json", "drawables"]
    );
    assert_eq!(files_in(&folder.join("drawables")), ["alpha", "beta"]);
    assert_eq!(
        files_in(&folder.join("drawables").join("beta")),
        ["drawable.json", "model.glb"]
    );
    let dictionary: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("dictionary.json")).unwrap())
            .unwrap();
    let code = |name: &str| format!("0x{:08x}", or_io::string_hash_str(name));
    let listed: Vec<(String, String)> = dictionary["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["code"].as_str().unwrap().to_string(),
                e["folder"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    for name in ["alpha", "beta"] {
        assert!(
            listed.contains(&(code(name), name.to_string())),
            "{listed:?}"
        );
    }

    let compiled = compile(&folder);
    assert_eq!(compiled.file_name, "pair.ydd");
    let (before, after) = (read_ydd(&bytes), read_ydd(&compiled.bytes));
    assert_eq!(
        before.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        after.iter().map(|(c, _)| *c).collect::<Vec<_>>()
    );
    for ((_, was), (_, is)) in before.iter().zip(&after) {
        let (was, is) = (meshes(was), meshes(is));
        assert!(same_positions(&was.positions, &is.positions));
        assert_eq!(was.indices, is.indices);
    }
}

#[test]
fn an_edit_to_one_entry_of_a_dictionary_changes_that_entry_and_no_other() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = drawable("alpha", vec![geometry(6, false, 0.0)], None);
    let beta = drawable("beta", vec![geometry(9, false, 20.0)], None);
    let bytes = ydd(vec![("alpha", alpha.clone()), ("beta", beta.clone())]);
    let folder = extract_ydd(scratch.path(), "pair", &bytes);

    let lifted_beta = lifted(
        drawable_scene(
            "beta",
            &beta,
            &ExtractOptions {
                lods: LodSelection::All,
                textures: false,
            },
        ),
        3.0,
    );
    std::fs::write(
        folder.join("drawables").join("beta").join("model.glb"),
        gltf::write(&lifted_beta, "beta", Container::Glb).primary,
    )
    .unwrap();

    let after = read_ydd(&compile(&folder).bytes);
    let find = |name: &str| {
        let code = or_io::string_hash_str(name);
        meshes(&after.iter().find(|(c, _)| *c == code).unwrap().1)
    };
    assert!(same_positions(
        &find("alpha").positions,
        &meshes(&alpha).positions
    ));
    let expected: Vec<Vec<[f32; 3]>> = meshes(&beta)
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + 3.0]).collect())
        .collect();
    assert!(same_positions(&find("beta").positions, &expected));
}

#[test]
fn two_drawables_of_one_name_are_told_apart_by_their_codes() {
    let scratch = tempfile::tempdir().unwrap();
    // The shipped ig_claypain_p.ydd holds two drawables that both state one name.
    let a = drawable("same", vec![geometry(6, false, 0.0)], None);
    let b = drawable("same", vec![geometry(6, false, 5.0)], None);
    let bytes = ydd(vec![("first", a), ("second", b)]);
    let folder = extract_ydd(scratch.path(), "twins", &bytes);

    let folders = files_in(&folder.join("drawables"));
    let named = |name: &str| format!("entry_{:08x}", or_io::string_hash_str(name));
    assert!(
        folders.contains(&named("first")) && folders.contains(&named("second")),
        "{folders:?}"
    );

    // Each came back as itself, not as its twin.
    let (before, after) = (read_ydd(&bytes), read_ydd(&compile(&folder).bytes));
    for ((code, was), (again, is)) in before.iter().zip(&after) {
        assert_eq!(code, again);
        assert!(same_positions(
            &meshes(was).positions,
            &meshes(is).positions
        ));
    }
    assert!(!same_positions(
        &meshes(&before[0].1).positions,
        &meshes(&before[1].1).positions
    ));
}

#[test]
fn a_dictionary_entry_whose_folder_is_a_path_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydd(vec![(
        "alpha",
        drawable("alpha", vec![geometry(6, false, 0.0)], None),
    )]);
    let folder = extract_ydd(scratch.path(), "one", &bytes);
    for hostile in ["../outside", "a/b", "..", "", "x "] {
        set_json(&folder.join("dictionary.json"), |v| {
            v["entries"][0]["folder"] = json!(hostile);
        });
        let why = said(&refusal(&folder));
        assert!(why.contains("dictionary.json"), "{hostile:?}: {why}");
    }
}

#[test]
fn a_ped_component_without_its_skeleton_keeps_its_skinning_by_keeping_the_weights_it_has() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    let folder = extract_ydr(scratch.path(), "jbib", &bytes);

    // glTF cannot say what the palette indexes, so the glb states no weights, and the compile's
    // default rule keeps the ones the base holds.
    let scene = gltf::read::load(
        Path::new("model.glb"),
        &std::fs::read(folder.join("model.glb")).unwrap(),
    )
    .unwrap();
    assert!(scene.objects[0].bones.is_empty());
    assert!(scene.primitives().all(|p| p.joints.is_empty()));

    let (before, after) = (
        meshes(&read_ydr(&bytes).base),
        meshes(&read_ydr(&compile(&folder).bytes).base),
    );
    assert_eq!(before.skin, after.skin);
    assert!(
        before.skin[0][0] == vec![(2, 55), (5, 200)],
        "{:?}",
        before.skin[0][0]
    );
}

#[test]
fn a_ped_component_with_the_reference_skeleton_carries_its_weights_in_the_glb() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    let skeleton = reference(8);
    let folder = models::extract_drawable(
        &bytes,
        "jbib",
        scratch.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap_or_else(|p| panic!("{p:#?}"));

    let scene = gltf::read::load(
        Path::new("model.glb"),
        &std::fs::read(folder.join("model.glb")).unwrap(),
    )
    .unwrap();
    assert_eq!(scene.objects[0].bones.len(), 8);
    assert_eq!(
        scene.objects[0]
            .bones
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        skeleton.iter().map(|b| b.name.as_str()).collect::<Vec<_>>()
    );
    let first = scene.primitives().next().unwrap();
    assert_eq!(first.joints.len(), 6);
    assert_eq!(
        &first.joints[0][..2],
        &[5, 2].map(|b| b as u16)[..],
        "heaviest first"
    );

    // The weights come from the glb, not from the base: the rule that keeps the base's is off.
    set_json(&folder.join("drawable.json"), |v| {
        v["import"]["keep_skinning"] = json!(false);
    });
    let (before, after) = (
        meshes(&read_ydr(&bytes).base),
        meshes(&read_ydr(&compile(&folder).bytes).base),
    );
    assert_eq!(before.skin, after.skin);
}

#[test]
fn a_reference_skeleton_the_palette_outgrows_is_refused_naming_the_bone() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    // The palette names bone 5.
    let skeleton = reference(5);
    let problems = models::extract_drawable(
        &bytes,
        "jbib",
        scratch.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap_err();
    let why = said(&problems);
    assert!(why.contains("bone 5") && why.contains("5 bones"), "{why}");
}

#[test]
fn a_drawable_with_a_skeleton_of_its_own_ignores_the_reference() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], Some(6)));
    let skeleton = reference(9);
    let folder = models::extract_drawable(
        &bytes,
        "jbib",
        scratch.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap();
    let scene = gltf::read::load(
        Path::new("model.glb"),
        &std::fs::read(folder.join("model.glb")).unwrap(),
    )
    .unwrap();
    assert_eq!(scene.objects[0].bones.len(), 6);
}

#[test]
fn a_file_that_is_not_a_drawable_is_refused_as_data() {
    let scratch = tempfile::tempdir().unwrap();
    let why = said(
        &models::extract_drawable(b"not a resource", "x", scratch.path(), &how()).unwrap_err(),
    );
    assert!(why.contains("not a .ydr"), "{why}");
    let why = said(&models::extract_drawables(b"RSC7", "x", scratch.path(), &how()).unwrap_err());
    assert!(why.contains("not a .ydd"), "{why}");
    // A .ydd is no .ydr: the version a file states is read against the format it is offered as.
    let dictionary = ydd(vec![(
        "alpha",
        drawable("alpha", vec![geometry(6, false, 0.0)], None),
    )]);
    assert!(models::extract_drawable(&dictionary, "x", scratch.path(), &how()).is_err());
    assert_eq!(
        files_in(scratch.path()),
        Vec::<String>::new(),
        "a refusal left a folder"
    );
}

/// The glb of `folder` with its skin's bones listed in the order `reorder` makes of them.
fn relist_the_bones(folder: &Path, reorder: impl Fn(&mut Vec<or_interchange::Bone>)) {
    let path = folder.join("model.glb");
    let mut scene =
        gltf::read::load(Path::new("model.glb"), &std::fs::read(&path).unwrap()).unwrap();
    let before: Vec<String> = scene.objects[0]
        .bones
        .iter()
        .map(|b| b.name.clone())
        .collect();
    reorder(&mut scene.objects[0].bones);
    let after: Vec<String> = scene.objects[0]
        .bones
        .iter()
        .map(|b| b.name.clone())
        .collect();
    for primitive in scene.objects[0]
        .meshes
        .iter_mut()
        .flat_map(|m| m.primitives.iter_mut())
    {
        for joints in &mut primitive.joints {
            for joint in joints.iter_mut() {
                let name = &before[usize::from(*joint)];
                *joint = after.iter().position(|n| n == name).unwrap_or(0) as u16;
            }
        }
    }
    // A bone's parent is an index too.
    for bone in &mut scene.objects[0].bones {
        bone.parent = None;
    }
    std::fs::write(&path, gltf::write(&scene, "jbib", Container::Glb).primary).unwrap();
}

#[test]
fn a_glb_that_lists_the_bones_in_another_order_is_bound_by_the_bones_names() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    let skeleton = reference(8);
    let folder = models::extract_drawable(
        &bytes,
        "jbib",
        scratch.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap();
    set_json(&folder.join("drawable.json"), |v| {
        v["import"]["keep_skinning"] = json!(false);
    });
    // The drawable states the bones it binds to, by name, in the order its palette indexes them.
    let rules: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("drawable.json")).unwrap())
            .unwrap();
    assert_eq!(rules["skeleton"].as_array().unwrap().len(), 8);
    assert_eq!(rules["skeleton"][5], "ref5");

    // A tool that writes the bones back in the opposite order, as one that sorts them might.
    relist_the_bones(&folder, |bones| bones.reverse());
    let (before, after) = (
        meshes(&read_ydr(&bytes).base),
        meshes(&read_ydr(&compile(&folder).bytes).base),
    );
    assert_eq!(
        before.skin, after.skin,
        "the weights followed the position and not the name"
    );
}

#[test]
fn a_skin_imported_into_a_source_extracted_without_the_skeleton_is_refused_and_not_bound_by_position(
) {
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    let skeleton = reference(8);
    let beside_parent = tempfile::tempdir().unwrap();
    let beside = models::extract_drawable(
        &bytes,
        "jbib",
        beside_parent.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap();
    let bare_parent = tempfile::tempdir().unwrap();
    let bare = extract_ydr(bare_parent.path(), "jbib", &bytes);

    // A tool's file: the same skin with its bones listed the other way round, so a joint's
    // position is no longer the bone it names.
    let tool = tempfile::tempdir().unwrap();
    std::fs::copy(beside.join("model.glb"), tool.path().join("model.glb")).unwrap();
    relist_the_bones(tool.path(), |bones| bones.reverse());
    let file = tool.path().join("model.glb");

    let before = meshes(&read_ydr(&bytes).base);
    models::import(&Source::open(&beside).unwrap(), None, &file)
        .unwrap_or_else(|p| panic!("beside the skeleton: {p:#?}"));
    assert_eq!(
        meshes(&read_ydr(&compile(&beside).bytes).base).skin,
        before.skin,
        "beside the skeleton the names put every weight back on its bone"
    );

    let kept = std::fs::read(bare.join("model.glb")).unwrap();
    let why = said(&models::import(&Source::open(&bare).unwrap(), None, &file).unwrap_err());
    assert!(
        why.contains("drawable.json lists no bone names")
            && why.contains("reference_skeleton")
            && why.contains("keep_skinning"),
        "{why}"
    );
    assert_eq!(
        std::fs::read(bare.join("model.glb")).unwrap(),
        kept,
        "a refused import left the model changed"
    );
    assert_eq!(
        meshes(&read_ydr(&compile(&bare).bytes).base).skin,
        before.skin
    );
}

#[test]
fn a_weight_on_a_bone_the_skeleton_does_not_have_is_refused_naming_it() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = ydr(drawable("jbib", vec![geometry(6, true, 0.0)], None));
    let skeleton = reference(8);
    let folder = models::extract_drawable(
        &bytes,
        "jbib",
        scratch.path(),
        &Extraction {
            skeleton: Some(&skeleton),
            ..how()
        },
    )
    .unwrap();
    // The bone a vertex is weighted to is renamed in the file, as an artist's own would be.
    let path = folder.join("model.glb");
    let mut scene =
        gltf::read::load(Path::new("model.glb"), &std::fs::read(&path).unwrap()).unwrap();
    scene.objects[0].bones[5].name = "my_extra_bone".to_string();
    std::fs::write(&path, gltf::write(&scene, "jbib", Container::Glb).primary).unwrap();
    let why = said(&refusal(&folder));
    assert!(
        why.contains("my_extra_bone") && why.contains("8 bones"),
        "{why}"
    );
}

#[test]
fn a_reference_skeleton_is_read_from_a_drawable_that_has_one_and_refused_from_one_that_has_none() {
    let ped = ydr(drawable("ped", vec![geometry(6, true, 0.0)], Some(6)));
    let bones = models::reference_skeleton("ydr", &ped).unwrap();
    assert_eq!(
        bones.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["bone0", "bone1", "bone2", "bone3", "bone4", "bone5"]
    );
    assert_eq!(bones[1].parent, Some(0));

    let prop = ydr(drawable("prop", vec![geometry(6, false, 0.0)], None));
    let why = models::reference_skeleton("ydr", &prop)
        .unwrap_err()
        .message;
    assert!(why.contains("no skeleton"), "{why}");
    let why = models::reference_skeleton("ydd", &ped).unwrap_err().message;
    assert!(
        why.contains(".yft or a .ydr") && why.contains("this is a .ydd"),
        "{why}"
    );
    assert!(models::reference_skeleton("yft", b"junk").is_err());
}

#[test]
fn a_protected_asset_is_refused_with_a_message_that_says_what_it_is() {
    let scratch = tempfile::tempdir().unwrap();
    // Not an RSC7 resource: the file opens with FXAP, and what follows is its author's.
    let mut protected = b"FXAP".to_vec();
    protected.extend(std::iter::repeat_n(0x5Au8, 200));

    let why =
        said(&models::extract_drawables(&protected, "x", scratch.path(), &how()).unwrap_err());
    assert!(
        why.contains("protected asset") && why.contains("FXAP") && why.contains(".ydd"),
        "{why}"
    );
    let why = said(&models::extract_drawable(&protected, "x", scratch.path(), &how()).unwrap_err());
    assert!(
        why.contains("protected asset") && why.contains(".ydr"),
        "{why}"
    );
    let why = models::reference_skeleton("yft", &protected)
        .unwrap_err()
        .message;
    assert!(
        why.contains("protected asset") && why.contains(".yft"),
        "{why}"
    );
    assert_eq!(files_in(scratch.path()), Vec::<String>::new());

    // The same bytes without the marker fail as the unreadable file they are.
    let mut plain = protected;
    plain[..4].copy_from_slice(b"JUNK");
    let why = said(&models::extract_drawables(&plain, "x", scratch.path(), &how()).unwrap_err());
    assert!(!why.contains("protected"), "{why}");
}

#[test]
fn a_target_that_is_neither_edition_is_refused_in_the_words_of_the_editions() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract_ydr(scratch.path(), "subject", &two_geometries());
    set_json(&folder.join(MANIFEST), |v| v["target"] = json!("gen7"));
    let why = said(&refusal(&folder));
    assert!(why.contains("Legacy") && why.contains("Enhanced"), "{why}");
    let lower = why.to_lowercase();
    assert!(!lower.contains("gen8") && !lower.contains("gen9"), "{why}");
}

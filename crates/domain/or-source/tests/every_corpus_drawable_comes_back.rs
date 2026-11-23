//! Every staged `.ydr` and `.ydd` of both targets extracts to a source folder and compiles back to the same meshes.
//!
//! A ped component is held to more: its skinning has to survive twice over, once by the weights
//! the base keeps and once by the weights a glb carries beside the ped's skeleton.

mod model_fixtures;

use std::path::{Path, PathBuf};

use model_fixtures::{meshes, same_positions, Meshes};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{DrawableTarget, GtaDrawable, PgDictionary, RmcDrawable, SpdAABB};
use or_source::asset::Source;
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::{json, Value};

/// A quantised weight and the float it came from can land one byte apart.
const WEIGHT_SLACK: u8 = 1;

#[derive(Default)]
struct Tally {
    drawables: usize,
    no_geometry: usize,
    geometries: usize,
    vertices: usize,
    /// Skinned against a skeleton the file does not carry.
    ped_components: usize,
    /// A ped component whose weights a glb carried, beside the largest ped skeleton staged.
    carried: usize,
    folders: usize,
}

fn corpus(extension: &str, target: &str, what: &str) -> Option<Vec<PathBuf>> {
    Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(target)
        .find_or_skip(what)
}

/// The staged skeleton with the most bones, which is the one every ped component's palette fits.
fn largest_skeleton() -> Option<Vec<or_interchange::Bone>> {
    let files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(TARGET_GEN8)
        .find();
    files
        .iter()
        .filter_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            models::reference_skeleton("yft", &bytes).ok()
        })
        .max_by_key(Vec::len)
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

/// Whether two skins agree vertex by vertex: the same bones, and weights within `slack` of each other.
fn same_skin(a: &Meshes, b: &Meshes, slack: u8) -> Result<(), String> {
    if a.skin.len() != b.skin.len() {
        return Err("the geometry count changed".to_string());
    }
    for (slot, (a, b)) in a.skin.iter().zip(&b.skin).enumerate() {
        if a.len() != b.len() {
            return Err(format!("geometry {slot} changed its vertex count"));
        }
        for (vertex, (was, is)) in a.iter().zip(b).enumerate() {
            let bones = |lanes: &[(u16, u8)]| lanes.iter().map(|l| l.0).collect::<Vec<_>>();
            // A weight the slack takes to nothing is not a bone the vertex lost.
            let heavy = |lanes: &[(u16, u8)]| {
                lanes
                    .iter()
                    .copied()
                    .filter(|l| l.1 > slack)
                    .collect::<Vec<_>>()
            };
            let (was, is) = (heavy(was), heavy(is));
            if bones(&was) != bones(&is)
                || was.iter().zip(&is).any(|(x, y)| x.1.abs_diff(y.1) > slack)
            {
                return Err(format!(
                    "geometry {slot} vertex {vertex} was {was:?} and is {is:?}"
                ));
            }
        }
    }
    Ok(())
}

fn same_meshes<T: DrawableTarget>(
    label: &str,
    before: &RmcDrawable<T>,
    after: &RmcDrawable<T>,
    skin_slack: u8,
    tally: &mut Tally,
) {
    let (was, is) = (meshes(before), meshes(after));
    assert!(
        same_positions(&was.positions, &is.positions),
        "{label}: a position moved"
    );
    assert_eq!(was.indices, is.indices, "{label}: the index runs changed");
    if let Err(why) = same_skin(&was, &is, skin_slack) {
        panic!("{label}: the skinning changed: {why}");
    }
    // The drawable's own box is authored, so a compile must not move it; compared by bits
    // because a shipped box carries a NaN, which `==` calls unequal to itself.
    assert_eq!(
        box_bits(&before.lod_group.bounds_box),
        box_bits(&after.lod_group.bounds_box),
        "{label}: the drawable box moved"
    );
    tally.geometries += was.positions.len();
    tally.vertices += was.positions.iter().map(Vec::len).sum::<usize>();
}

/// Whether a drawable is skinned and has no skeleton to say what its palette indexes.
fn is_ped_component<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> bool {
    drawable.skeleton.is_none() && meshes(drawable).skin.iter().any(|g| !g.is_empty())
}

/// Sets `import.keep_skinning` to false in every `drawable.json` under `folder`.
fn let_the_glb_carry_the_weights(folder: &Path) {
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "drawable.json") {
                let mut rules: Value =
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                rules["import"]["keep_skinning"] = json!(false);
                std::fs::write(&path, serde_json::to_string_pretty(&rules).unwrap()).unwrap();
            }
        }
    }
}

fn compile(folder: &Path, label: &str) -> models::Compiled {
    models::compile(&Source::open(folder).unwrap())
        .unwrap_or_else(|problems| panic!("{label}: compile: {problems:#?}"))
}

fn how<'a>(file: &'a str, skeleton: Option<&'a [or_interchange::Bone]>) -> Extraction<'a> {
    Extraction {
        from_game: true,
        virtual_path: Some(file),
        textures: false,
        skeleton,
    }
}

fn every_ydr<T: Extracted + Imported>(
    target: &str,
    what: &str,
    skeleton: Option<&[or_interchange::Bone]>,
) -> Option<Tally>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let files = corpus("ydr", target, what)?;
    let scratch = tempfile::tempdir().unwrap();
    let mut tally = Tally::default();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydr");
        let bytes = std::fs::read(&path).unwrap();
        let before = GtaDrawable::<T>::from_bytes(&bytes).unwrap();
        tally.drawables += 1;

        let folder = match models::extract_drawable(&bytes, name, scratch.path(), &how(&file, None))
        {
            Ok(folder) => folder,
            Err(problems) if problems[0].message.contains("no geometry") => {
                tally.no_geometry += 1;
                continue;
            }
            Err(problems) => panic!("{file}: extract: {problems:#?}"),
        };
        tally.folders += 1;
        let compiled = compile(&folder, &file);
        assert_eq!(
            compiled.bytes,
            compile(&folder, &file).bytes,
            "{file}: two compiles of one folder differ"
        );
        let after = GtaDrawable::<T>::from_bytes(&compiled.bytes).unwrap();
        same_meshes(&file, &before.base, &after.base, 0, &mut tally);
        std::fs::remove_dir_all(&folder).unwrap();

        if is_ped_component(&before.base) {
            tally.ped_components += 1;
            tally.carried += carry(&file, name, &bytes, &before.base, skeleton, scratch.path());
        }
    }
    Some(tally)
}

/// A ped component extracted beside `skeleton`, whose glb states the weights, put back without the base's.
fn carry<T: Extracted + Imported>(
    file: &str,
    name: &str,
    bytes: &[u8],
    before: &RmcDrawable<T>,
    skeleton: Option<&[or_interchange::Bone]>,
    scratch: &Path,
) -> usize
where
    GtaDrawable<T>: File,
{
    let Some(skeleton) = skeleton else { return 0 };
    let folder = models::extract_drawable(bytes, name, scratch, &how(file, Some(skeleton)))
        .unwrap_or_else(|problems| panic!("{file}: extract beside the skeleton: {problems:#?}"));
    let_the_glb_carry_the_weights(&folder);
    let after = GtaDrawable::<T>::from_bytes(&compile(&folder, file).bytes).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    let (was, is) = (meshes(before), meshes(&after.base));
    if let Err(why) = same_skin(&was, &is, WEIGHT_SLACK) {
        panic!("{file}: the weights a glb carries came back changed: {why}");
    }
    1
}

fn every_ydd<T: Extracted + Imported>(
    target: &str,
    what: &str,
    skeleton: Option<&[or_interchange::Bone]>,
) -> Option<Tally>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let files = corpus("ydd", target, what)?;
    let scratch = tempfile::tempdir().unwrap();
    let mut tally = Tally::default();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydd");
        let bytes = std::fs::read(&path).unwrap();
        let before = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes).unwrap();
        tally.drawables += before.entries.iter().flatten().count();

        let folder =
            match models::extract_drawables(&bytes, name, scratch.path(), &how(&file, None)) {
                Ok(folder) => folder,
                Err(problems) if problems[0].message.contains("no geometry") => {
                    tally.no_geometry += 1;
                    continue;
                }
                Err(problems) => panic!("{file}: extract: {problems:#?}"),
            };
        tally.folders += 1;
        let compiled = compile(&folder, &file);
        assert_eq!(
            compiled.bytes,
            compile(&folder, &file).bytes,
            "{file}: two compiles of one folder differ"
        );
        let after = PgDictionary::<GtaDrawable<T>>::from_bytes(&compiled.bytes).unwrap();
        assert_eq!(before.codes, after.codes, "{file}: the keys changed");
        for (index, (was, is)) in before.entries.iter().zip(&after.entries).enumerate() {
            match (was, is) {
                (Some(was), Some(is)) => same_meshes(
                    &format!("{file}[{index}]"),
                    &was.base,
                    &is.base,
                    0,
                    &mut tally,
                ),
                (None, None) => {}
                _ => panic!("{file}[{index}]: an entry appeared or went"),
            }
        }
        std::fs::remove_dir_all(&folder).unwrap();

        // Every entry of a dictionary of ped components is carried at once.
        let components = before
            .entries
            .iter()
            .flatten()
            .filter(|e| is_ped_component(&e.base))
            .count();
        if components > 0 {
            tally.ped_components += components;
            if let Some(skeleton) = skeleton {
                let folder = models::extract_drawables(
                    &bytes,
                    name,
                    scratch.path(),
                    &how(&file, Some(skeleton)),
                )
                .unwrap_or_else(|p| panic!("{file}: extract beside the skeleton: {p:#?}"));
                let_the_glb_carry_the_weights(&folder);
                let after =
                    PgDictionary::<GtaDrawable<T>>::from_bytes(&compile(&folder, &file).bytes)
                        .unwrap();
                std::fs::remove_dir_all(&folder).unwrap();
                for (was, is) in before.entries.iter().zip(&after.entries) {
                    if let (Some(was), Some(is)) = (was, is) {
                        if let Err(why) =
                            same_skin(&meshes(&was.base), &meshes(&is.base), WEIGHT_SLACK)
                        {
                            panic!("{file}: the weights a glb carries came back changed: {why}");
                        }
                    }
                }
                tally.carried += components;
            }
        }
    }
    Some(tally)
}

fn report(what: &str, tally: &Tally) {
    println!(
        "{what}: {} drawables, {} sources written, {} with no geometry; {} geometries and {} \
         vertices came back the same; {} ped components kept their skinning from the base and {} \
         again from a glb",
        tally.drawables,
        tally.folders,
        tally.no_geometry,
        tally.geometries,
        tally.vertices,
        tally.ped_components,
        tally.carried
    );
    assert!(tally.vertices > 0, "{what} compared nothing");
}

#[test]
fn every_corpus_ydr_extracts_and_compiles_back_to_the_same_meshes() {
    let skeleton = largest_skeleton();
    let what = "every_corpus_ydr_extracts_and_compiles_back_to_the_same_meshes";
    if let Some(tally) = every_ydr::<Gen8>(TARGET_GEN8, what, skeleton.as_deref()) {
        report(what, &tally);
    }
}

#[test]
fn every_corpus_ydd_extracts_and_compiles_back_to_the_same_meshes() {
    let skeleton = largest_skeleton();
    let what = "every_corpus_ydd_extracts_and_compiles_back_to_the_same_meshes";
    if let Some(tally) = every_ydd::<Gen8>(TARGET_GEN8, what, skeleton.as_deref()) {
        report(what, &tally);
        assert!(
            tally.ped_components == 0
                || skeleton.is_none()
                || tally.carried == tally.ped_components,
            "a ped component's weights were not carried through its glb"
        );
    }
}

#[test]
fn every_enhanced_ydr_extracts_and_compiles_back_to_the_same_meshes() {
    let skeleton = largest_skeleton();
    let what = "every_enhanced_ydr_extracts_and_compiles_back_to_the_same_meshes";
    if let Some(tally) = every_ydr::<Gen9>(TARGET_GEN9, what, skeleton.as_deref()) {
        report(what, &tally);
    }
}

#[test]
fn every_enhanced_ydd_extracts_and_compiles_back_to_the_same_meshes() {
    let skeleton = largest_skeleton();
    let what = "every_enhanced_ydd_extracts_and_compiles_back_to_the_same_meshes";
    if let Some(tally) = every_ydd::<Gen9>(TARGET_GEN9, what, skeleton.as_deref()) {
        report(what, &tally);
    }
}

/// Every vertex of every level of `scene`, moved up by `by`.
fn lifted(mut scene: or_interchange::Scene, by: f32) -> or_interchange::Scene {
    for primitive in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        for position in &mut primitive.positions {
            position[2] += by;
        }
    }
    scene
}

/// The scene a source's glb is written from: every level, no textures.
fn scene_of<T: Extracted>(name: &str, drawable: &RmcDrawable<T>) -> or_interchange::Scene {
    or_interchange::drawable_scene(
        name,
        drawable,
        &or_interchange::ExtractOptions {
            lods: or_interchange::LodSelection::All,
            textures: false,
        },
    )
}

fn write_glb(path: &Path, scene: &or_interchange::Scene, name: &str) {
    let files = or_interchange::gltf::write(scene, name, or_interchange::gltf::Container::Glb);
    std::fs::write(path, files.primary).unwrap();
}

/// Each vertex `by` higher than in `before`, in every geometry.
fn moved_by<T: DrawableTarget>(
    label: &str,
    before: &RmcDrawable<T>,
    after: &RmcDrawable<T>,
    by: f32,
) {
    let (was, is) = (meshes(before), meshes(after));
    let expected: Vec<Vec<[f32; 3]>> = was
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + by]).collect())
        .collect();
    assert!(
        was.positions.iter().any(|run| !run.is_empty()),
        "{label} has no vertex to move"
    );
    assert!(
        same_positions(&expected, &is.positions),
        "{label}: the edit did not land {by} up in every geometry"
    );
    assert_eq!(was.indices, is.indices, "{label}: the index runs changed");
}

/// ⚠ A round trip of geometry nobody changed passes when a compile writes nothing, so this one moves it.
fn an_edit_lands_in_every_ydr<T: Extracted + Imported>(target: &str, what: &str)
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let Some(files) = corpus("ydr", target, what) else {
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydr");
        let bytes = std::fs::read(&path).unwrap();
        let before = GtaDrawable::<T>::from_bytes(&bytes).unwrap();
        let folder = models::extract_drawable(&bytes, name, scratch.path(), &how(&file, None))
            .unwrap_or_else(|p| panic!("{file}: extract: {p:#?}"));
        write_glb(
            &folder.join(models::MODEL),
            &lifted(scene_of(name, &before.base), 2.0),
            name,
        );
        let after = GtaDrawable::<T>::from_bytes(&compile(&folder, &file).bytes).unwrap();
        moved_by(&file, &before.base, &after.base, 2.0);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}

/// Entry `k` of a dictionary is lifted `k + 1` up, so a glb that went to the wrong entry shows.
fn an_edit_lands_in_its_own_entry<T: Extracted + Imported>(target: &str, what: &str)
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let Some(files) = corpus("ydd", target, what) else {
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let mut edited = 0;
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydd");
        let bytes = std::fs::read(&path).unwrap();
        let before = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes).unwrap();
        let folder = models::extract_drawables(&bytes, name, scratch.path(), &how(&file, None))
            .unwrap_or_else(|p| panic!("{file}: extract: {p:#?}"));
        let listed: models::Dictionary = serde_json::from_str(
            &std::fs::read_to_string(folder.join(models::DICTIONARY)).unwrap(),
        )
        .unwrap();
        let mut lifts = Vec::new();
        for (k, entry) in listed.entries.iter().enumerate() {
            let code = u32::from_str_radix(entry.code.trim_start_matches("0x"), 16).unwrap();
            let at = before.codes.iter().position(|c| *c == code).unwrap();
            let held = before.entries[at].as_deref().unwrap();
            let by = (k + 1) as f32;
            write_glb(
                &folder
                    .join(models::DIR)
                    .join(&entry.folder)
                    .join(models::MODEL),
                &lifted(scene_of(&entry.folder, &held.base), by),
                &entry.folder,
            );
            lifts.push((at, by));
        }
        let after =
            PgDictionary::<GtaDrawable<T>>::from_bytes(&compile(&folder, &file).bytes).unwrap();
        for (at, by) in lifts {
            let (was, is) = (
                before.entries[at].as_deref().unwrap(),
                after.entries[at].as_deref().unwrap(),
            );
            moved_by(&format!("{file}[{at}]"), &was.base, &is.base, by);
            edited += 1;
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }
    assert!(edited > 0, "{what} edited no entry");
}

#[test]
fn an_edit_to_a_corpus_ydr_lands_in_the_compiled_file() {
    an_edit_lands_in_every_ydr::<Gen8>(
        TARGET_GEN8,
        "an_edit_to_a_corpus_ydr_lands_in_the_compiled_file",
    );
}

#[test]
fn an_edit_to_an_enhanced_ydr_lands_in_the_compiled_file() {
    an_edit_lands_in_every_ydr::<Gen9>(
        TARGET_GEN9,
        "an_edit_to_an_enhanced_ydr_lands_in_the_compiled_file",
    );
}

#[test]
fn an_edit_to_each_entry_of_a_corpus_ydd_lands_in_that_entry() {
    an_edit_lands_in_its_own_entry::<Gen8>(
        TARGET_GEN8,
        "an_edit_to_each_entry_of_a_corpus_ydd_lands_in_that_entry",
    );
}

#[test]
fn an_edit_to_each_entry_of_an_enhanced_ydd_lands_in_that_entry() {
    an_edit_lands_in_its_own_entry::<Gen9>(
        TARGET_GEN9,
        "an_edit_to_each_entry_of_an_enhanced_ydd_lands_in_that_entry",
    );
}

#[test]
fn a_source_can_carry_the_drawables_own_textures_for_a_person_to_look_at() {
    let what = "a_source_can_carry_the_drawables_own_textures_for_a_person_to_look_at";
    let Some(files) = corpus("ydr", TARGET_GEN8, what) else {
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let mut with_textures = 0;
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydr");
        let bytes = std::fs::read(&path).unwrap();
        let plain =
            models::extract_drawable(&bytes, name, scratch.path(), &how(&file, None)).unwrap();
        let plain_glb = std::fs::read(plain.join(models::MODEL)).unwrap();
        std::fs::remove_dir_all(&plain).unwrap();

        let folder = models::extract_drawable(
            &bytes,
            name,
            scratch.path(),
            &Extraction {
                textures: true,
                ..how(&file, None)
            },
        )
        .unwrap();
        let glb = std::fs::read(folder.join(models::MODEL)).unwrap();
        let scene = or_interchange::gltf::read::load(Path::new("model.glb"), &glb).unwrap();
        // The textures are for looking at: a compile reads the geometry and says it kept none.
        let compiled = compile(&folder, &file);
        let before = GtaDrawable::<Gen8>::from_bytes(&bytes).unwrap();
        let after = GtaDrawable::<Gen8>::from_bytes(&compiled.bytes).unwrap();
        same_meshes(&file, &before.base, &after.base, 0, &mut Tally::default());
        if glb.len() > plain_glb.len() {
            with_textures += 1;
            assert!(
                scene.dropped.iter().any(|d| d.what == "textures"),
                "{file}: the glb carries images and its reader did not say it drops them"
            );
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }
    assert!(with_textures > 0, "no staged drawable carries a texture");
}

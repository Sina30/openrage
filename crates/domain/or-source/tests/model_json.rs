//! What `drawable.json` says that a glb cannot: the base's shaders, distances, flags, bounds and bone tags, read out and put back.

mod model_fixtures;

use std::path::{Path, PathBuf};

use model_fixtures::{drawable, geometry, meshes, read_ydr, ydr};
use or_interchange::extract::{effect_name, parameter_name};
use or_interchange::gltf::{self, Container};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection, Scene};
use or_io::decl::{File, Gen8, Hash32, Vec4};
use or_io::gta5::drawable_codecs::TextureSlot;
use or_io::gta5::gen8::{EntryData, GrcInstanceDataEntry, GrcTextureReference};
use or_io::gta5::{GrmModel, GtaDrawable, RmcDrawable, RmcLod, SpdAABB};
use or_source::asset::Source;
use or_source::models::{self, Compiled, Extraction};
use or_source::Problem;
use serde_json::{json, Value};

const FLOAT_PARAMETER: &str = "specularFalloffMult";
const TEXTURE_PARAMETER: &str = "DiffuseSampler";
const TEXTURE: &str = "jbib_diff_000";

fn how() -> Extraction<'static> {
    Extraction {
        from_game: false,
        virtual_path: None,
        textures: false,
        skeleton: None,
    }
}

/// A drawable of two geometries whose one shader states a texture and a constant, and that has levels, boxes and four bones.
fn shaded() -> RmcDrawable<Gen8> {
    let mut out = drawable(
        "subject",
        vec![geometry(6, false, 0.0), geometry(9, false, 40.0)],
        Some(4),
    );
    let group = out.shader_group.as_mut().unwrap();
    let bound = group.binding(TEXTURE);
    let shader = &mut group.shaders[0];
    shader.entries = vec![
        GrcInstanceDataEntry {
            register: 0,
            name_hash: Hash32::of(TEXTURE_PARAMETER),
            data: EntryData::Texture(bound),
        },
        GrcInstanceDataEntry {
            register: 1,
            name_hash: Hash32::of(FLOAT_PARAMETER),
            data: EntryData::Float(vec![Vec4 {
                x: 0.5,
                y: 2.0,
                z: 3.0,
                w: 4.0,
            }]),
        },
    ];
    shader.draw_bucket = 3;
    shader.flags = 5;
    out.lod_group.lod_distances = [10.0, 20.0, 30.0, 40.0];
    out.lod_group.bucket_masks = [1, 2, 3, 4];
    out.lod_group.bounds_box = SpdAABB {
        min: model_fixtures::vec4(-1.0, -2.0, -3.0, 0.0),
        max: model_fixtures::vec4(1.0, 2.0, 3.0, 0.0),
        ..Default::default()
    };
    out.lod_group.bounds_sphere.center_and_radius = model_fixtures::vec4(0.25, 0.5, 0.75, 7.0);
    out
}

fn bytes_of(drawable: RmcDrawable<Gen8>) -> Vec<u8> {
    ydr(drawable)
}

fn extract(parent: &Path, bytes: &[u8]) -> PathBuf {
    models::extract_drawable(bytes, "subject", parent, &how())
        .unwrap_or_else(|problems| panic!("extract: {problems:#?}"))
}

fn compile(dir: &Path) -> Compiled {
    models::compile(&Source::open(dir).unwrap())
        .unwrap_or_else(|problems| panic!("compile: {problems:#?}"))
}

fn refusal(dir: &Path) -> String {
    match models::compile(&Source::open(dir).unwrap()) {
        Ok(_) => panic!("{} compiled and was expected to be refused", dir.display()),
        Err(problems) => problems
            .iter()
            .map(Problem::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn rules(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("drawable.json")).unwrap()).unwrap()
}

fn edit(dir: &Path, change: impl FnOnce(&mut Value)) {
    let mut value = rules(dir);
    change(&mut value);
    std::fs::write(
        dir.join("drawable.json"),
        serde_json::to_string_pretty(&value).unwrap(),
    )
    .unwrap();
}

/// The key a parameter is written under, computed the way an extraction computes it.
fn key_of(drawable: &RmcDrawable<Gen8>, name: &str) -> String {
    let effect = drawable.shader_group.as_ref().unwrap().shaders[0]
        .basis_hash
        .get();
    parameter_name(effect, Hash32::of(name).get())
}

fn drawable_of(bytes: &[u8]) -> GtaDrawable<Gen8> {
    read_ydr(bytes)
}

#[test]
fn drawable_json_states_what_the_base_holds() {
    let scratch = tempfile::tempdir().unwrap();
    let base = shaded();
    let folder = extract(scratch.path(), &bytes_of(base.clone()));
    let rules = rules(&folder);

    let shader = &base.shader_group.as_ref().unwrap().shaders[0];
    let material = &rules["materials"][0];
    assert_eq!(material["shader"], effect_name(shader.basis_hash.get()));
    assert_eq!(
        material["name"], material["shader"],
        "an export names a material for its shader"
    );
    assert_eq!(material["draw_bucket"], shader.draw_bucket);
    assert_eq!(material["flags"], shader.flags);
    assert_eq!(
        material["parameters"][key_of(&base, FLOAT_PARAMETER)],
        json!([[0.5, 2.0, 3.0, 4.0]])
    );
    assert_eq!(
        material["parameters"][key_of(&base, TEXTURE_PARAMETER)],
        TEXTURE
    );

    assert_eq!(rules["lod_distances"], json!([10.0, 20.0, 30.0, 40.0]));
    assert_eq!(rules["bucket_masks"], json!([1, 2, 3, 4]));
    assert_eq!(
        rules["bounds_box"],
        json!({"min": [-1.0, -2.0, -3.0], "max": [1.0, 2.0, 3.0]})
    );
    assert_eq!(
        rules["bounds_sphere"],
        json!({"center": [0.25, 0.5, 0.75], "radius": 7.0})
    );
    assert_eq!(
        rules["models"][0],
        json!({"at": "lod0/model0", "name": "subject_lod0_model0", "flags": 0, "mask": 0})
    );
    let bones = base.skeleton.as_ref().unwrap();
    assert_eq!(rules["bones"].as_array().unwrap().len(), bones.bones.len());
    assert_eq!(
        rules["bones"][2],
        json!({"name": bones.bones[2].name, "tag": bones.bones[2].bone_id})
    );
}

/// ⚠ A round trip of values nobody changed passes when a compile writes nothing, so the next tests change one.
#[test]
fn an_unedited_drawable_json_changes_no_byte() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    let with = compile(&folder).bytes;

    edit(&folder, |v| {
        let rules = v.as_object_mut().unwrap();
        for key in [
            "materials",
            "models",
            "lod_distances",
            "bucket_masks",
            "bounds_box",
            "bounds_sphere",
            "bones",
        ] {
            rules.remove(key);
        }
    });
    assert_eq!(
        compile(&folder).bytes,
        with,
        "the values drawable.json states are the base's, so putting them back changes nothing"
    );
}

#[test]
fn editing_a_shader_parameter_changes_that_parameter_and_nothing_else() {
    let scratch = tempfile::tempdir().unwrap();
    let base = shaded();
    let folder = extract(scratch.path(), &bytes_of(base.clone()));
    let unedited = compile(&folder).bytes;
    let key = key_of(&base, FLOAT_PARAMETER);

    edit(&folder, |v| {
        v["materials"][0]["parameters"][&key] = json!([[0.5, 9.0, 3.0, 4.0]]);
    });
    let compiled = compile(&folder);

    // The expectation is the unedited compile with that one number moved, saved by the same code.
    let mut expected = drawable_of(&unedited);
    let shader = &mut expected.base.shader_group.as_mut().unwrap().shaders[0];
    let Some(EntryData::Float(rows)) = shader
        .entry_mut(Hash32::of(FLOAT_PARAMETER).get())
        .map(|e| &mut e.data)
    else {
        panic!("the parameter is a constant");
    };
    assert_eq!(rows[0].y, 2.0);
    rows[0].y = 9.0;
    assert_eq!(compiled.bytes, expected.to_bytes().unwrap());
    assert_ne!(compiled.bytes, unedited);
    assert!(
        compiled
            .report
            .iter()
            .any(|l| l.contains("material 0") && l.contains(&key)),
        "{:?}",
        compiled.report
    );
}

#[test]
fn rebinding_a_texture_names_the_dds_the_material_uses() {
    let scratch = tempfile::tempdir().unwrap();
    let base = shaded();
    let folder = extract(scratch.path(), &bytes_of(base.clone()));
    let unedited = compile(&folder).bytes;
    let key = key_of(&base, TEXTURE_PARAMETER);

    edit(&folder, |v| {
        v["materials"][0]["parameters"][&key] = json!("jbib_diff_001");
    });
    let compiled = compile(&folder);

    let mut expected = drawable_of(&unedited);
    let group = expected.base.shader_group.as_mut().unwrap();
    let slot: TextureSlot<_, _> =
        TextureSlot::Reference(Box::new(GrcTextureReference::named("jbib_diff_001")));
    let Some(entry) = group.shaders[0].entry_mut(Hash32::of(TEXTURE_PARAMETER).get()) else {
        panic!("the parameter is a texture");
    };
    entry.data = EntryData::Texture(slot);
    assert_eq!(compiled.bytes, expected.to_bytes().unwrap());
}

#[test]
fn distances_masks_bounds_flags_and_bone_tags_are_set_where_they_differ() {
    let scratch = tempfile::tempdir().unwrap();
    let base = shaded();
    let folder = extract(scratch.path(), &bytes_of(base.clone()));
    let unedited = compile(&folder).bytes;

    edit(&folder, |v| {
        v["lod_distances"] = json!([11.0, 22.0, 33.0, 44.0]);
        v["bucket_masks"] = json!([4, 3, 2, 1]);
        v["bounds_box"]["max"] = json!([1.5, 2.0, 3.0]);
        v["bounds_sphere"]["radius"] = json!(8.0);
        v["models"][0]["flags"] = json!(2);
        v["models"][0]["mask"] = json!(7);
        v["materials"][0]["draw_bucket"] = json!(1);
        v["materials"][0]["flags"] = json!(6);
        v["bones"][1]["tag"] = json!(900);
    });
    let compiled = compile(&folder);

    let mut expected = drawable_of(&unedited);
    let d = &mut expected.base;
    d.lod_group.lod_distances = [11.0, 22.0, 33.0, 44.0];
    d.lod_group.bucket_masks = [4, 3, 2, 1];
    d.lod_group.bounds_box.max.x = 1.5;
    d.lod_group.bounds_sphere.center_and_radius.w = 8.0;
    let model = &mut d.lod_group.lods[0].models[0];
    model.flags = 2;
    model.mask = 7;
    let shader = &mut d.shader_group.as_mut().unwrap().shaders[0];
    shader.draw_bucket = 1;
    shader.flags = 6;
    d.skeleton.as_mut().unwrap().bones[1].bone_id = 900;
    assert_eq!(compiled.bytes, expected.to_bytes().unwrap());

    // The tag is what the skeleton's table finds the bone by, which a reopened file shows.
    let back = drawable_of(&compiled.bytes);
    assert_eq!(back.base.skeleton.as_ref().unwrap().bones[1].bone_id, 900);
}

#[test]
fn a_shader_drawable_json_changes_is_refused_by_name() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    edit(&folder, |v| {
        v["materials"][0]["shader"] = json!("vehicle_paint1")
    });
    let why = refusal(&folder);
    assert!(
        why.contains("drawable.json") && why.contains("materials[0]") && why.contains("authoring"),
        "{why}"
    );
}

#[test]
fn a_parameter_the_shader_does_not_have_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    edit(&folder, |v| {
        v["materials"][0]["parameters"]["notAParameter"] = json!([[1.0, 1.0, 1.0, 1.0]]);
    });
    let why = refusal(&folder);
    assert!(
        why.contains("notAParameter") && why.contains("authoring"),
        "{why}"
    );
}

#[test]
fn a_constant_of_the_wrong_shape_and_a_list_of_the_wrong_length_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let base = shaded();
    let folder = extract(scratch.path(), &bytes_of(base.clone()));
    let key = key_of(&base, FLOAT_PARAMETER);

    edit(&folder, |v| {
        v["materials"][0]["parameters"][&key] = json!([[1.0, 2.0]])
    });
    assert!(refusal(&folder).contains("four numbers"));

    edit(&folder, |v| {
        v["materials"][0]["parameters"][&key] = json!([[0.5, 2.0, 3.0, 4.0]]);
        let again = v["materials"][0].clone();
        v["materials"].as_array_mut().unwrap().push(again);
    });
    let why = refusal(&folder);
    assert!(
        why.contains("2") && why.contains("1 shaders") || why.contains("shader"),
        "{why}"
    );
}

#[test]
fn two_bones_that_share_a_tag_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    edit(&folder, |v| {
        let first = v["bones"][0]["tag"].clone();
        v["bones"][1]["tag"] = first;
    });
    let why = refusal(&folder);
    assert!(why.contains("share the tag"), "{why}");
}

#[test]
fn a_bone_renamed_in_the_json_is_refused_and_not_followed() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    edit(&folder, |v| v["bones"][1]["name"] = json!("SKEL_Head"));
    assert!(refusal(&folder).contains("bones[1]"));
}

#[test]
fn a_model_the_base_does_not_hold_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = extract(scratch.path(), &bytes_of(shaded()));
    edit(&folder, |v| v["models"][0]["at"] = json!("lod0/model7"));
    assert!(refusal(&folder).contains("lod0/model7"));
}

/// Two models, each of one geometry, in the first level, so which mesh went where shows.
fn two_models() -> RmcDrawable<Gen8> {
    let mut out = shaded();
    let model = |shift: f32| {
        Box::new(GrmModel {
            aabbs: vec![SpdAABB::default(); or_io::codec::mesh::aabb_slots(1)],
            shader_indices: vec![0],
            geometries: vec![Box::new(geometry(6, false, shift))],
            ..Default::default()
        })
    };
    out.lod_group.lods[0] = RmcLod {
        models: vec![model(0.0), model(40.0)],
        ..Default::default()
    }
    .into();
    out
}

fn scene_of(bytes: &[u8]) -> Scene {
    drawable_scene(
        "subject",
        &read_ydr(bytes).base,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    )
}

fn lifted(scene: &mut Scene, mesh: usize, by: f32) {
    for primitive in &mut scene.objects[0].meshes[mesh].primitives {
        for position in &mut primitive.positions {
            position[2] += by;
        }
    }
}

fn first_z(drawable: &RmcDrawable<Gen8>, model: usize) -> f32 {
    meshes(drawable).positions[model][0][2]
}

#[test]
fn a_glb_whose_meshes_are_renamed_and_reordered_pairs_by_the_names_drawable_json_states() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = bytes_of(two_models());
    let folder = extract(scratch.path(), &bytes);
    let before = read_ydr(&bytes).base;
    assert_eq!(rules(&folder)["models"].as_array().unwrap().len(), 2);

    // A tool exports "Hair" first and "Torso" second, and the artist says which is which.
    let mut scene = scene_of(&bytes);
    lifted(&mut scene, 1, 100.0);
    scene.objects[0].meshes[0].name = "Torso".into();
    scene.objects[0].meshes[1].name = "Hair".into();
    scene.objects[0].meshes.reverse();
    for mesh in &mut scene.objects[0].meshes {
        for primitive in &mut mesh.primitives {
            primitive.stated_address = None;
        }
    }
    std::fs::write(
        folder.join("model.glb"),
        gltf::write(&scene, "subject", Container::Glb).primary,
    )
    .unwrap();
    edit(&folder, |v| {
        v["models"][0]["name"] = json!("subject_Torso");
        v["models"][1]["name"] = json!("subject_Hair");
    });

    let after = read_ydr(&compile(&folder).bytes).base;
    assert_eq!(
        first_z(&after, 0),
        first_z(&before, 0),
        "Torso is model0 and was not moved"
    );
    assert_eq!(
        first_z(&after, 1),
        first_z(&before, 1) + 100.0,
        "Hair is model1"
    );
}

#[test]
fn a_mesh_past_the_vertex_limit_is_split_by_an_import_and_the_report_says_so() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = bytes_of(shaded());
    let folder = extract(scratch.path(), &bytes);

    let mut scene = scene_of(&bytes);
    let wide = 70_002usize;
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    primitive.positions = (0..wide).map(|i| [i as f32, 0.0, 0.0]).collect();
    primitive.normals = vec![[0.0, 0.0, 1.0]; wide];
    primitive.uvs[0] = vec![[0.0; 2]; wide];
    primitive.indices = (0..wide as u32).collect();
    let file = scratch.path().join("wide.glb");
    std::fs::write(&file, gltf::write(&scene, "wide", Container::Glb).primary).unwrap();

    let source = Source::open(&folder).unwrap();
    let imported = models::import(&source, None, &file)
        .unwrap_or_else(|problems| panic!("import: {problems:#?}"));
    assert!(
        imported
            .report
            .iter()
            .any(|l| l.contains("split into 2 geometries")),
        "{:?}",
        imported.report
    );
    let after = read_ydr(&compile(&folder).bytes).base;
    assert_eq!(
        after.lod_group.lods[0].models[0].geometries.len(),
        3,
        "the first geometry became two, and the second stayed"
    );
}

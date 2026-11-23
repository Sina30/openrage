//! A material that lacks constants its shader declares is a finding of the rules, with a fix that states them at the values the
//! shader declares, in a new source of the model, as one step of Undo; a sampler the material lacks is not added.
//!
//! ⚠ Reads a ped component staged in the corpus; a machine without it skips, and says so. The shader database is made here, from
//! the shader the staged model uses and two parameters named for this test, so no install is read.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use common::{garment, put_bytes, snapshot, JBIB};
use or_io::decl::{File, Gen8};
use or_io::gta5::shaders::shader_defs::{
    self, ShaderDef, ShaderDefinitions, ShaderParamDef, ShaderParamKind, ShaderParamValue,
};
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_source::asset::Source;
use or_source::models;
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::mesh::{run, run_many, Op};
use or_wardrobe::{allocate, check, inspect, Depth, Ped, Wardrobe};

/// The shader database belongs to the process, so a test that installs or clears it takes this first and keeps it to its end.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|held| held.into_inner())
}

fn staged(name: &str) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the ped component {name}"),
            "not staged".to_string(),
        );
    }
    found
}

fn param(name: &str, kind: ShaderParamKind, register: u8, floats: Vec<f32>) -> ShaderParamDef {
    ShaderParamDef {
        name: name.to_string(),
        name_hash: or_io::string_hash_str(name),
        kind,
        array_len: 1,
        register,
        cbuffer: None,
        drawable_place: None,
        semantic: None,
        semantic_hash: None,
        ui_name: None,
        default: Some(ShaderParamValue::Floats(floats)),
        preset_default: None,
    }
}

/// A database that declares, for the shader the first drawable of `bytes` uses, one constant and one sampler it does not carry.
fn declare_for(bytes: &[u8]) -> (u32, u32) {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).unwrap();
    let entry = dictionary.entries.iter().flatten().next().unwrap();
    let shader = &entry.base.shader_group.as_deref().unwrap().shaders[0];
    let effect = shader.basis_hash.get();
    let mut defs = ShaderDefinitions::new("gta5", "gen8");
    defs.shaders.push(ShaderDef {
        name: "test_shader".to_string(),
        name_hash: effect,
        effect: "test_shader".to_string(),
        draw_bucket: None,
        vertex_type: 0,
        traits: Vec::new(),
        parameters: vec![
            param("zzTestFactor", ShaderParamKind::Float, 200, vec![0.5]),
            param("zzTestSampler", ShaderParamKind::Texture, 201, Vec::new()),
        ],
    });
    shader_defs::install(defs);
    (effect, or_io::string_hash_str("zzTestFactor"))
}

fn first_shader_has(bytes: &[u8], hash: u32) -> Option<f32> {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).unwrap();
    let entry = dictionary.entries.iter().flatten().next().unwrap();
    let shader = &entry.base.shader_group.as_deref().unwrap().shaders[0];
    match &shader.entry(hash)?.data {
        or_io::gta5::gen8::EntryData::Float(rows) => rows.first().map(|v| v.x),
        _ => None,
    }
}

fn findings(pack: &Path) -> Vec<String> {
    let w = Wardrobe::read(pack).unwrap();
    let facts = inspect(pack, &w, Depth::Measure);
    check(&w, &allocate(&w), &facts)
        .into_iter()
        .filter(|f| f.code == "material-parameters")
        .map(|f| f.message)
        .collect()
}

#[test]
fn a_material_that_lacks_declared_constants_is_found_and_completed_and_a_sampler_is_left() {
    let _alone = alone();
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    let bytes = std::fs::read(&file).unwrap();
    let (_, factor) = declare_for(&bytes);
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    std::fs::create_dir_all(&pack).unwrap();
    put_bytes(&pack, "models/m.ydd", &bytes);
    let mut w = Wardrobe::new("demo");
    let g = garment(Ped::Male, JBIB, None, "models/m.ydd", &["textures/t.ytd"]);
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    let before = snapshot(&pack);

    let found = findings(&pack);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("zzTestFactor") && !found[0].contains("zzTestSampler"),
        "the constant is named and the sampler is not: {}",
        found[0]
    );

    let (report, revision) = run(&pack, &id, &Op::Materials, None, &Watch::default()).unwrap();
    assert_eq!(report.models.len(), 1, "{report:?}");
    let lines = report.lines().join("\n");
    assert!(
        lines.contains("zzTestFactor") && lines.contains("zzTestSampler") && lines.contains("left"),
        "{lines}"
    );
    let now = Wardrobe::read(&pack).unwrap().garments[0]
        .model
        .clone()
        .unwrap();
    assert_ne!(now, "models/m.ydd");
    let compiled = models::compile(&Source::open(&pack.join(&now)).unwrap()).unwrap();
    assert_eq!(
        first_shader_has(&compiled.bytes, factor),
        Some(0.5),
        "the constant is stated at the value the shader declares"
    );
    assert_eq!(
        first_shader_has(&compiled.bytes, or_io::string_hash_str("zzTestSampler")),
        None,
        "a sampler is not added"
    );
    assert!(findings(&pack).is_empty(), "the finding is gone");

    // A second run has nothing to state.
    let (again, none) = run(&pack, &id, &Op::Materials, None, &Watch::default()).unwrap();
    assert!(again.models.is_empty() && none.is_none(), "{again:?}");

    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    let undone = snapshot(&pack);
    for (path, bytes) in &before {
        assert_eq!(undone.get(path), Some(bytes), "{path} is not as it was");
    }
    shader_defs::clear();
}

#[test]
fn without_a_shader_database_no_material_is_judged_and_the_fix_says_why() {
    let _alone = alone();
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    shader_defs::clear();
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    std::fs::create_dir_all(&pack).unwrap();
    put_bytes(&pack, "models/m.ydd", &std::fs::read(&file).unwrap());
    let mut w = Wardrobe::new("demo");
    let g = garment(Ped::Male, JBIB, None, "models/m.ydd", &["textures/t.ytd"]);
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    assert!(findings(&pack).is_empty());
    let (report, revision) = run(&pack, &id, &Op::Materials, None, &Watch::default()).unwrap();
    assert!(revision.is_none());
    assert!(
        report
            .refused
            .iter()
            .any(|r| r.why.contains("shader database")),
        "{report:?}"
    );
}

#[test]
fn a_whole_pack_is_completed_as_one_step_and_a_model_two_garments_name_becomes_one_source() {
    let _alone = alone();
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    let bytes = std::fs::read(&file).unwrap();
    declare_for(&bytes);
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    std::fs::create_dir_all(&pack).unwrap();
    put_bytes(&pack, "models/m.ydd", &bytes);
    put_bytes(&pack, "models/other.ydd", &bytes);
    let mut w = Wardrobe::new("demo");
    for (n, model) in ["models/m.ydd", "models/m.ydd", "models/other.ydd"]
        .iter()
        .enumerate()
    {
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            Some(n as u32),
            model,
            &["textures/t.ytd"],
        ));
    }
    w.write(&pack).unwrap();
    let before = snapshot(&pack);
    assert_eq!(
        findings(&pack).len(),
        3,
        "each garment's model lacks the constant"
    );

    let (report, revision) =
        run_many(&pack, None, &Op::Materials, None, &Watch::default()).unwrap();
    assert_eq!(
        report.models.len(),
        2,
        "the model two garments name is made once: {report:?}"
    );
    let after = Wardrobe::read(&pack).unwrap();
    let names: Vec<String> = after
        .garments
        .iter()
        .map(|g| g.model.clone().unwrap())
        .collect();
    assert_eq!(names[0], names[1]);
    assert_ne!(names[0], names[2]);
    assert!(names.iter().all(|n| !n.ends_with(".ydd")));
    assert!(findings(&pack).is_empty());

    // A garment asked for alone is the only one changed.
    undo_revision(&pack, &revision.expect("one step of Undo")).unwrap();
    let ids: Vec<String> = after.garments.iter().map(|g| g.id.clone()).collect();
    let (one, _) = run_many(
        &pack,
        Some(&ids[2..]),
        &Op::Materials,
        None,
        &Watch::default(),
    )
    .unwrap();
    assert_eq!(one.models.len(), 1);
    assert_eq!(findings(&pack).len(), 2);
    let _ = before;
    shader_defs::clear();
}

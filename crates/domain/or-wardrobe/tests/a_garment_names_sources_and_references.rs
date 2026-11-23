//! What a garment may name besides a file of its pack: a source folder, which a build cooks for the platform the build states,
//! and a reference to a source or file of the project, of another project, or an id alone.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{garment, JBIB};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::reference::Reference;
use or_project::{build_all_with, Container, Cook, Module, Origin, Output, Plan, Project, Target};
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::sample;
use or_wardrobe::measure::{measure, Measure};
use or_wardrobe::{allocate, check, inspect, plans, Depth, Fact, Ped, Wardrobe};

const MODEL: &str = "jbib_000_u";
const TEXTURE: &str = "jbib_diff_000_a_uni";

/// Decompile the game file `bytes` named `name` into `parent`; the folder.
fn source(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    fs::create_dir_all(parent).unwrap();
    decompile(bytes, name, parent, &GameFacts::default()).expect("a source")
}

fn model_source(parent: &Path) -> PathBuf {
    source(
        parent,
        &format!("{MODEL}.ydd"),
        &sample::drawable_dictionary(MODEL, 6, 0.0),
    )
}

fn texture_source(parent: &Path) -> PathBuf {
    source(
        parent,
        &format!("{TEXTURE}.ytd"),
        &sample::texture_dictionary(TEXTURE, 3),
    )
}

/// A project in `dir` whose one clothing content is the folder `clothing`, with a build for each target in `targets`.
fn project(dir: &Path, wardrobe: &Wardrobe, targets: &[Option<Target>]) -> (Project, PathBuf) {
    let pack = dir.join("clothing");
    fs::create_dir_all(&pack).unwrap();
    wardrobe.write(&pack).unwrap();
    let outputs = targets
        .iter()
        .enumerate()
        .map(|(n, target)| Output {
            target: *target,
            ..Output::fivem_resource(&format!("out{n}"), &format!("demo_pack_{n}"), &["wardrobe"])
        })
        .collect();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "clothing")],
        outputs,
    };
    let file = dir.join("demo.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    (Project::load(&file).unwrap(), pack)
}

fn one_garment(model: &str, texture: &str) -> Wardrobe {
    let mut w = Wardrobe::new("demo");
    w.garments
        .push(garment(Ped::Male, JBIB, Some(12), model, &[texture]));
    w
}

fn cooks(plan: &Plan) -> Vec<(String, String, PathBuf)> {
    plan.files
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::Cook { kind, source, .. } => {
                Some((f.path.clone(), kind.clone(), source.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_texture_source_and_a_model_source_of_the_pack_are_read_as_the_files_they_become_and_cooked() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    let (model, texture) = (
        format!("models/{MODEL}.drawables.src"),
        format!("textures/{TEXTURE}.textures.src"),
    );
    model_source(&pack.join("models"));
    texture_source(&pack.join("textures"));
    let w = one_garment(&model, &texture);

    let facts = inspect(&pack, &w, Depth::Open);

    for path in [&model, &texture] {
        let Some(Fact::File { bytes, read, .. }) = facts.get(path) else {
            panic!("{path}: {:?}", facts.get(path));
        };
        assert!(*bytes > 0, "{path}");
        assert!(matches!(read, Some(Ok(_))), "{path}: {read:?}");
    }
    let counted = Measure::of_fact(facts.get(&texture)).expect("a texture source is counted");
    assert_eq!(
        (counted.textures.len(), counted.textures[0].width),
        (1, 16),
        "the rules that hold a texture to a limit see a texture of a source"
    );
    let found: Vec<_> = check(&w, &allocate(&w), &facts)
        .into_iter()
        // ⚠ `lod-missing` is a note on the levels of the model's drawable, which a source is held to as the file it cooks into is.
        .filter(|f| f.code != "lod-missing")
        .filter(|f| f.code.contains("missing") || f.code.contains("unreadable"))
        .collect();
    assert!(found.is_empty(), "{found:?}");

    let (project, _) = project(dir.path(), &w, &[None]);
    let plans = plans(&project, "out0").unwrap();
    assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);
    let cooked = cooks(&plans[0]);
    assert_eq!(cooked.len(), 2, "{cooked:?}");
    assert!(
        cooked
            .iter()
            .any(|(p, k, s)| p.ends_with(".ydd") && k == "drawables" && s == &pack.join(&model)),
        "{cooked:?}"
    );
    assert!(
        cooked
            .iter()
            .any(|(p, k, s)| p.ends_with(".ytd") && k == "textures" && s == &pack.join(&texture)),
        "{cooked:?}"
    );
}

#[test]
fn a_source_of_the_wrong_kind_for_its_place_does_not_open_and_says_which_kind_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    model_source(&pack.join("models"));
    model_source(&pack.join("textures"));
    texture_source(&pack.join("textures"));
    // A drawables source named as a texture, and a textures source as a cloth.
    let mut w = one_garment(
        &format!("models/{MODEL}.drawables.src"),
        &format!("textures/{MODEL}.drawables.src"),
    );
    w.garments[0].cloth = Some(format!("textures/{TEXTURE}.textures.src"));

    let facts = inspect(&pack, &w, Depth::Open);
    let said: Vec<String> = check(&w, &allocate(&w), &facts)
        .into_iter()
        .filter(|f| f.code.ends_with("unreadable"))
        .map(|f| f.message)
        .collect();

    assert!(
        said.iter()
            .any(|m| m.contains("a texture is a textures source")),
        "{said:?}"
    );
    assert!(
        said.iter().any(|m| m.contains("a cloth is a .yld file")),
        "{said:?}"
    );
}

#[test]
fn a_reference_leads_a_model_and_a_texture_to_sources_of_the_project_and_follows_a_rename() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    let model = model_source(&dir.path().join("sources"));
    let texture = texture_source(&dir.path().join("sources"));
    let id_of = |folder: &Path| or_project::reference::asset_id(folder).unwrap();
    Reference::here("sources/jbib_000_u.drawables.src", Some(&id_of(&model)))
        .write(&pack.join("models/jacket.ref.json"))
        .unwrap();
    Reference::here(
        "sources/jbib_diff_000_a_uni.textures.src",
        Some(&id_of(&texture)),
    )
    .write(&pack.join("textures/jacket.ref.json"))
    .unwrap();
    let w = one_garment("models/jacket.ref.json", "textures/jacket.ref.json");
    let (project, _) = project(dir.path(), &w, &[None]);

    let plan = &plans(&project, "out0").unwrap()[0];
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    let cooked = cooks(plan);
    assert_eq!(cooked.len(), 2, "{cooked:?}");
    assert!(cooked.iter().any(|(_, _, s)| s == &model), "{cooked:?}");
    assert!(cooked.iter().any(|(_, _, s)| s == &texture), "{cooked:?}");

    // The person renames and moves the model's folder; the reference follows its id.
    fs::create_dir_all(dir.path().join("art")).unwrap();
    let moved = dir.path().join("art/blazer.drawables.src");
    fs::rename(&model, &moved).unwrap();
    let plan = &plans(&project, "out0").unwrap()[0];
    assert!(plan.is_buildable(), "{:?}", plan.problems);
    assert!(
        cooks(plan).iter().any(|(_, _, s)| s == &moved),
        "{:?}",
        cooks(plan)
    );
}

#[test]
fn a_reference_to_a_source_that_is_gone_is_a_problem_that_names_where_it_looked() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    Reference::here("sources/gone.drawables.src", Some("no-such-id"))
        .write(&pack.join("models/jacket.ref.json"))
        .unwrap();
    let texture = texture_source(&pack.join("textures"));
    let w = one_garment(
        "models/jacket.ref.json",
        &format!(
            "textures/{}",
            texture.file_name().unwrap().to_string_lossy()
        ),
    );
    let (project, _) = project(dir.path(), &w, &[None]);

    let plan = &plans(&project, "out0").unwrap()[0];

    assert!(!plan.is_buildable());
    let problem = plan
        .problems
        .iter()
        .find(|p| p.contains("model"))
        .unwrap_or_else(|| panic!("{:?}", plan.problems));
    assert!(problem.contains("leads nowhere"), "{problem}");
    let facts = inspect(&pack, &w, Depth::Open);
    let found = check(&w, &allocate(&w), &facts);
    assert!(
        found.iter().any(|f| f.code == "model-missing"),
        "a missing file has a Locate: {found:?}"
    );
}

#[test]
fn an_id_alone_builds_with_a_plain_sentence_and_ships_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    let model = model_source(&pack.join("models"));
    let texture = texture_source(&pack.join("textures"));
    Reference::by_id("cloth_jbib_000_u")
        .write(&pack.join("cloth/jacket.ref.json"))
        .unwrap();
    let mut w = one_garment(
        &format!("models/{}", model.file_name().unwrap().to_string_lossy()),
        &format!(
            "textures/{}",
            texture.file_name().unwrap().to_string_lossy()
        ),
    );
    w.garments[0].cloth = Some("cloth/jacket.ref.json".into());
    let (project, _) = project(dir.path(), &w, &[None]);

    let plan = &plans(&project, "out0").unwrap()[0];

    assert!(plan.is_buildable(), "{:?}", plan.problems);
    assert!(
        (plan.files.iter()).all(|f| !f.path.ends_with(".yld")),
        "no cloth file is shipped"
    );
    let note = plan
        .notes
        .iter()
        .find(|n| n.contains("cloth_jbib_000_u"))
        .unwrap_or_else(|| panic!("{:?}", plan.notes));
    assert!(note.contains("not in the project"), "{note}");
    assert!(note.contains("by its id"), "{note}");
}

#[test]
fn an_export_follows_a_reference_to_what_it_leads_to_and_names_the_one_that_leads_to_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    let model = model_source(&dir.path().join("sources"));
    fs::create_dir_all(dir.path().join("loose")).unwrap();
    fs::write(dir.path().join("loose/jacket.yld"), b"CLOTH").unwrap();
    let id = or_project::reference::asset_id(&model).unwrap();
    Reference::here("sources/jbib_000_u.drawables.src", Some(&id))
        .write(&pack.join("models/jacket.ref.json"))
        .unwrap();
    Reference::here("loose/jacket.yld", None)
        .write(&pack.join("cloth/jacket.ref.json"))
        .unwrap();
    Reference::by_id("jbib_diff_000_a_uni_by_hand")
        .write(&pack.join("textures/jacket.ref.json"))
        .unwrap();
    let mut w = one_garment("models/jacket.ref.json", "textures/jacket.ref.json");
    w.garments[0].cloth = Some("cloth/jacket.ref.json".into());
    project(dir.path(), &w, &[None]);
    let out = dir.path().join("exported");

    let done = or_wardrobe::export::garments(&pack, &[w.garments[0].id.clone()], &out).unwrap();

    let ydd = done.written.iter().find(|n| n.ends_with(".ydd")).unwrap();
    let yld = done.written.iter().find(|n| n.ends_with(".yld")).unwrap();
    assert_eq!(
        fs::read(out.join(yld)).unwrap(),
        b"CLOTH",
        "the cloth the reference leads to"
    );
    let model_bytes = fs::read(out.join(ydd)).unwrap();
    assert!(
        !model_bytes.starts_with(b"{") && !model_bytes.is_empty(),
        "the model is the file the source cooks into, not the reference's text"
    );
    assert!(
        done.written.iter().all(|n| !n.ends_with(".ytd")),
        "{done:?}"
    );
    let said = done
        .left_out
        .iter()
        .find(|l| l.why.contains("jbib_diff_000_a_uni_by_hand"))
        .unwrap_or_else(|| panic!("{done:?}"));
    assert!(said.path.contains("textures/jacket.ref.json"), "{said:?}");
    for name in fs::read_dir(&out).unwrap().flatten() {
        let bytes = fs::read(name.path()).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("openrage-reference"),
            "{:?} holds a reference file's text under a game's name",
            name.file_name()
        );
    }
}

#[test]
fn a_dropped_source_of_a_kind_a_garment_does_not_take_is_listed_as_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("clothing");
    fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    let drop = dir.path().join("downloads").join("pack");
    let foreign = drop.join("mp_m_freemode_01_mp_m_x^jbib_000_u.fragments.src");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("asset.json"), b"{}").unwrap();

    let added = or_wardrobe::edit::add_clothes(&module, &[drop]).unwrap();

    assert!(added.garments.is_empty(), "{added:?}");
    let said = added
        .left_out
        .iter()
        .find(|l| l.path.ends_with(".fragments.src"))
        .unwrap_or_else(|| panic!("a folder of the drop vanished with no reason: {added:?}"));
    assert!(
        said.why.contains(".drawables.src") && said.why.contains(".textures.src"),
        "it says which kinds a garment takes: {said:?}"
    );
}

/// A cooker that makes the file of the source as the build's own does.
fn cook() -> impl Fn(&Path, &Path) -> Result<PathBuf, Vec<String>> {
    |source: &Path, scratch: &Path| or_source::cook::cook_source(source, scratch)
}

#[test]
fn the_same_garment_is_built_for_legacy_and_for_enhanced_from_one_source() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("clothing");
    model_source(&pack.join("models"));
    texture_source(&pack.join("textures"));
    let w = one_garment(
        &format!("models/{MODEL}.drawables.src"),
        &format!("textures/{TEXTURE}.textures.src"),
    );
    let (project, _) = project(
        dir.path(),
        &w,
        &[Some(Target::Legacy), Some(Target::Enhanced)],
    );
    let root = tempfile::tempdir().unwrap();
    let cook = cook();

    let mut layouts = Vec::new();
    for output in ["out0", "out1"] {
        let plans = plans(&project, output).unwrap();
        assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);
        let (built, stopped) = build_all_with(
            &plans,
            root.path(),
            &ParamRemap::default(),
            Some(&cook as &Cook),
        );
        assert!(stopped.is_none(), "{output}: {stopped:?}");
        let mut seen = Vec::new();
        for (path, bytes) in common::snapshot(&built[0].folder) {
            if path.ends_with(".ytd") || path.ends_with(".ydd") {
                let counted = measure(&path, &bytes)
                    .and_then(|m| Measure::of(&m))
                    .unwrap_or_else(|| panic!("{path} does not open"));
                seen.push((
                    path.rsplit('.').next().unwrap().to_string(),
                    counted.enhanced,
                ));
            }
        }
        seen.sort();
        layouts.push(seen);
    }

    let (legacy, enhanced) = (&layouts[0], &layouts[1]);
    assert_eq!(legacy.len(), 2, "{legacy:?}");
    assert!(
        legacy.iter().all(|(_, enhanced)| !enhanced),
        "the Legacy build: {legacy:?}"
    );
    assert!(
        enhanced.iter().all(|(_, enhanced)| *enhanced) && enhanced.len() == 2,
        "the Enhanced build, from the same two sources: {enhanced:?}"
    );
}

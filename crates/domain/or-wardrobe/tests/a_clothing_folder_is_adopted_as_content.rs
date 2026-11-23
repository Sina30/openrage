//! Adopting the folder of a resource that exists: its clothing becomes a clothing content, what the
//! engine writes again is not carried twice, and what it could not place is listed and still ships.
//!
//! The pack is laid out from the engine's own layout functions with files that only have names:
//! an import reads names and metadata, and a plan at `Depth::Stat` reads neither a model nor a texture.

use std::path::{Path, PathBuf};

use or_io::decl::File;
use or_io::meta::CPedVariationInfo;
use or_project::adopt::{analyse, Adoption, Recogniser};
use or_project::{Project, PROJECT_EXTENSION};
use or_wardrobe::adopt::Clothing;
use or_wardrobe::{layout, Component, Ped, Skin, Slot, Wardrobe};

const JBIB: Slot = Slot::Component(Component::Jbib);

fn put(folder: &Path, rel: &str, body: &[u8]) {
    let path = folder.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A resource of the pack `harbor`: two jackets of the male ped, each with a texture, beside the files a resource has.
fn resource(dir: &Path) -> PathBuf {
    let folder = dir.join("harbor_resource");
    let collection = "mp_m_harbor";
    let directory = layout::directory("harbor", Ped::Male, collection, JBIB);
    for position in 0..2 {
        let model = layout::model_name(Ped::Male, collection, JBIB, position, Skin::Universal);
        let texture = layout::texture_name(Ped::Male, collection, JBIB, position, 0, "uni");
        put(&folder, &format!("{directory}/{model}"), b"model");
        put(&folder, &format!("{directory}/{texture}"), b"texture");
    }
    put(
        &folder,
        "fxmanifest.lua",
        b"fx_version 'cerulean'\nclient_script 'client/main.lua'\n",
    );
    put(&folder, "client/main.lua", b"print('hi')");
    put(&folder, "html/index.html", b"<html></html>");
    // Written again by a build, at the path the engine writes it.
    put(
        &folder,
        &layout::shop_path(Ped::Male, collection),
        b"<shop/>",
    );
    // Named so that nothing states the slot, a cloth file, and a metadata file nothing here reads.
    put(&folder, "stream/harbor/odd.ydd", b"odd");
    put(&folder, "stream/harbor/cape.yld", b"cloth");
    put(&folder, "stream/harbor/other.ymt", b"not metadata");
    // Never adopted.
    put(&folder, ".git/config", b"x");
    put(&folder, "build/old/a.ydr", b"x");
    folder
}

fn rel(folder: &Path, files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|f| {
            f.strip_prefix(folder)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn unplaced<'a>(adoption: &'a Adoption, path: &str) -> &'a str {
    adoption
        .unplaced
        .iter()
        .find(|u| u.path == path)
        .map(|u| u.why.as_str())
        .unwrap_or_else(|| panic!("{path} is not listed: {:#?}", adoption.unplaced))
}

#[test]
fn a_resource_with_clothing_is_clothing_content_and_files_and_a_list_of_what_could_not_be_placed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = resource(dir.path());
    let adoption = analyse(&folder, &[&Clothing]);

    assert_eq!(adoption.found.len(), 1, "{adoption:#?}");
    let module = &adoption.found[0].module;
    assert_eq!((module.id.as_str(), module.path()), ("clothing", Some(".")));
    assert!(
        adoption.found[0].summary.contains("2 model(s)"),
        "{}",
        adoption.found[0].summary
    );

    // What stays Files: the scripts and the page, and what the pack could not place, which ships as it is.
    assert_eq!(
        rel(&folder, &adoption.files),
        [
            "client/main.lua",
            "html/index.html",
            "stream/harbor/cape.yld",
            "stream/harbor/odd.ydd",
            "stream/harbor/other.ymt",
        ]
    );
    assert!(unplaced(&adoption, "stream/harbor/odd.ydd").contains("name does not state"));
    assert!(unplaced(&adoption, "stream/harbor/cape.yld").contains("ships as it is"));
    assert!(unplaced(&adoption, "stream/harbor/other.ymt").contains("neither variation metadata"));
    assert!(
        unplaced(&adoption, "fxmanifest.lua").contains("writes its own"),
        "the manifest is the build's, and what the old one stated is said to be compared by hand"
    );
    assert!(
        unplaced(&adoption, &layout::shop_path(Ped::Male, "mp_m_harbor"))
            .contains("writes it again")
    );

    // The pack is a file the adoption adds, and nothing else is.
    assert_eq!(adoption.writes.len(), 1);
    assert_eq!(adoption.writes[0].path, folder.join("wardrobe.json"));
    adoption.write().expect("it is added beside the files");
    let wardrobe = Wardrobe::read(&folder).expect("it reads back");
    assert_eq!(wardrobe.pack, "harbor");
    let positions: Vec<Option<u32>> = wardrobe.garments.iter().map(|g| g.position).collect();
    assert_eq!(
        positions,
        [Some(0), Some(1)],
        "pinned at the numbers the names carry"
    );
}

#[test]
fn an_adopted_resource_plans_what_the_engine_writes_and_carries_nothing_twice() {
    let dir = tempfile::tempdir().unwrap();
    let folder = resource(dir.path());
    let adoption = analyse(&folder, &[&Clothing]);
    adoption.write().unwrap();

    let file = folder.join(format!("harbor.{PROJECT_EXTENSION}"));
    let documents: Vec<PathBuf> = adoption.files.clone();
    Project::create(&file, &adoption.container("harbor"), &documents).unwrap();
    let project = Project::load(&file).unwrap();
    let plans = or_wardrobe::plans(&project, "fivem").expect("the output is declared");

    assert_eq!(plans.len(), 1);
    let plan = &plans[0];
    assert!(plan.is_buildable(), "{:#?}", plan.problems);
    let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", JBIB);
    let model = layout::model_name(Ped::Male, "mp_m_harbor", JBIB, 0, Skin::Universal);
    for kept in [
        "client/main.lua",
        "html/index.html",
        "stream/harbor/odd.ydd",
        "fxmanifest.lua",
        &format!("{directory}/{model}"),
        &layout::variation_path("harbor", Ped::Male, "mp_m_harbor"),
        &layout::shop_path(Ped::Male, "mp_m_harbor"),
    ] {
        assert!(paths.contains(&kept), "{kept} is not in {paths:#?}");
    }
    let mut seen = paths.clone();
    seen.sort_by_key(|p| p.to_lowercase());
    seen.dedup_by_key(|p| p.to_lowercase());
    assert_eq!(
        seen.len(),
        paths.len(),
        "no path is written twice: {paths:#?}"
    );
}

#[test]
fn a_folder_that_names_no_clothing_is_not_clothing() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("scripts_only");
    put(&folder, "client/main.lua", b"x");
    put(&folder, "stream/prop.ydr", b"x");
    put(&folder, "stream/nameless.ydd", b"x");

    let adoption = analyse(&folder, &[&Clothing]);
    assert!(!adoption.recognised(), "{adoption:#?}");
    assert_eq!(
        adoption.files.len(),
        3,
        "every file is Files, as adopting always was"
    );
    assert!(adoption.writes.is_empty());
}

#[test]
fn a_folder_that_already_holds_a_pack_is_read_and_its_pack_is_not_written_over() {
    let dir = tempfile::tempdir().unwrap();
    let folder = resource(dir.path());
    let first = analyse(&folder, &[&Clothing]);
    first.write().unwrap();
    let written = std::fs::read(folder.join("wardrobe.json")).unwrap();

    let again = analyse(&folder, &[&Clothing]);
    assert!(again.recognised());
    assert!(again.writes.is_empty(), "the pack there is the pack");
    assert!(Clothing.holds(&folder));
    assert!(
        !again.files.iter().any(|f| f.ends_with("wardrobe.json")),
        "the pack's own file is not shipped as a loose one"
    );
    assert_eq!(
        std::fs::read(folder.join("wardrobe.json")).unwrap(),
        written
    );
}

#[test]
fn an_empty_pack_is_made_for_a_content_a_template_declares_and_never_over_one() {
    let dir = tempfile::tempdir().unwrap();
    let module = or_project::Module::clothing("uniforms", "modules/uniforms");
    or_wardrobe::adopt::start_module(dir.path(), &module, "harbor").expect("a folder and a pack");
    let wardrobe = Wardrobe::read(&dir.path().join("modules").join("uniforms")).unwrap();
    assert_eq!(
        (wardrobe.pack.as_str(), wardrobe.garments.len()),
        ("harbor", 0)
    );

    let kept = std::fs::read(dir.path().join("modules/uniforms/wardrobe.json")).unwrap();
    or_wardrobe::adopt::start_module(dir.path(), &module, "other").expect("it is already there");
    assert_eq!(
        std::fs::read(dir.path().join("modules/uniforms/wardrobe.json")).unwrap(),
        kept
    );
    let escaping = or_project::Module::clothing("up", "../outside");
    assert!(or_wardrobe::adopt::start_module(dir.path(), &escaping, "harbor").is_err());
    assert!(or_wardrobe::adopt::start_module(dir.path(), &module, "Not Lower").is_err());
}

#[test]
fn a_pack_larger_than_the_file_cap_keeps_its_scripts_and_its_interface() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("big_resource");
    // Both peds, every component, 250 positions, a model and a texture each: 12,000 files, past the cap.
    for (ped, collection) in [(Ped::Male, "mp_m_big"), (Ped::Female, "mp_f_big")] {
        for component in Component::ALL {
            let slot = Slot::Component(component);
            let directory = layout::directory("big", ped, collection, slot);
            for position in 0..250 {
                let model = layout::model_name(ped, collection, slot, position, Skin::Universal);
                let texture = layout::texture_name(ped, collection, slot, position, 0, "uni");
                put(&folder, &format!("{directory}/{model}"), b"model");
                put(&folder, &format!("{directory}/{texture}"), b"texture");
            }
        }
    }
    put(&folder, "fxmanifest.lua", b"fx_version 'cerulean'\n");
    put(&folder, "client/main.lua", b"print('client')");
    put(&folder, "server/main.lua", b"print('server')");
    put(&folder, "html/index.html", b"<html></html>");

    let adoption = analyse(&folder, &[&Clothing]);

    assert!(adoption.recognised(), "{:#?}", adoption.notes);
    assert_eq!(
        rel(&folder, &adoption.files),
        ["client/main.lua", "html/index.html", "server/main.lua"],
        "the clothing takes its own files first, so the cap never reaches the scripts and the page"
    );
    assert!(!adoption.truncated, "{:#?}", adoption.notes);
    assert!(adoption.notes.is_empty(), "{:#?}", adoption.notes);
}

#[test]
fn a_variation_file_that_names_no_ped_stays_a_loose_file_and_one_that_names_one_is_the_packs() {
    let dir = tempfile::tempdir().unwrap();
    let folder = resource(dir.path());
    let bytes = CPedVariationInfo::default()
        .to_bytes()
        .expect("an empty variation info writes");
    // The import cannot place the first (no ped in its name), and reads the second into the pack.
    put(&folder, "stream/addon/cs_addon.ymt", &bytes);
    put(&folder, "data/mp_m_freemode_01_mp_m_harbor.ymt", &bytes);

    let adoption = analyse(&folder, &[&Clothing]);
    let files = rel(&folder, &adoption.files);

    assert!(
        files.contains(&"stream/addon/cs_addon.ymt".to_string()),
        "the build never writes it, so it ships as it is: {files:#?}"
    );
    assert!(
        unplaced(&adoption, "stream/addon/cs_addon.ymt").contains("ships as it is"),
        "{:#?}",
        adoption.unplaced
    );
    assert!(
        !files.contains(&"data/mp_m_freemode_01_mp_m_harbor.ymt".to_string()),
        "the pack states it and the build writes it again: {files:#?}"
    );
}

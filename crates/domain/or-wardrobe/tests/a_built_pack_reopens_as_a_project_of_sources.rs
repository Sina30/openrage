//! A singleplayer DLC or an `.oiv` that a build wrote is reopened as a project: the pack's garments with everything their
//! metadata states, every model and texture as a source and no compiled file of the pack's, and what could not be placed
//! named, with the file kept under `files/` in the Files content.
//!
//! ⚠ Nobody here can run GTA V: this holds that what a build wrote reopens and builds again, not that the game mounts it.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::{garment, put_bytes, ydd, ytd, FEET, JBIB, LOWR, P_HEAD};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{
    build_all, build_all_with, Container, Cook, Module, ModuleKind, Output, OutputKind, Project,
};
use or_wardrobe::reopen::{reopen, Options};
use or_wardrobe::{plans, Expression, Ped, Wardrobe};

const HANDLING: &[u8] = b"<?xml version=\"1.0\"?>\n<CHandlingDataMgr/>\n";
const README: &[u8] = b"installed beside the game, not inside the pack";

/// A pack of a male jacket with an alternate and two textures, a hat, a pair of heels with an expression and a female pair of trousers.
fn pack(folder: &Path) {
    put_bytes(folder, "models/jacket.ydd", &ydd("jacket"));
    put_bytes(folder, "models/jacket_1.ydd", &ydd("jacket-alternate"));
    put_bytes(folder, "models/hat.ydd", &ydd("hat"));
    put_bytes(folder, "models/heels.ydd", &ydd("heels"));
    put_bytes(folder, "models/trousers.ydd", &ydd("trousers"));
    for name in ["a", "b", "hat", "heels", "t"] {
        put_bytes(folder, &format!("textures/{name}.ytd"), &ytd(name));
    }
    let mut w = Wardrobe::new("demo");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        Some(0),
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    jacket.alternates = vec!["models/jacket_1.ydd".to_string()];
    let hat = garment(
        Ped::Male,
        P_HEAD,
        Some(0),
        "models/hat.ydd",
        &["textures/hat.ytd"],
    );
    let mut heels = garment(
        Ped::Male,
        FEET,
        Some(0),
        "models/heels.ydd",
        &["textures/heels.ytd"],
    );
    heels.expressions = vec![Expression {
        index: 4,
        tracks: vec![33],
        ids: vec![28462],
        types: vec![2],
        components: vec![1],
    }];
    let trousers = garment(
        Ped::Female,
        LOWR,
        Some(0),
        "models/trousers.ydd",
        &["textures/t.ytd"],
    );
    w.garments.extend([jacket, hat, heels, trousers]);
    w.write(folder).unwrap();
}

/// The project of `dir`: the pack, and a handling file the game is told of by type, which no clothing engine places.
fn project(dir: &Path, kind: OutputKind) -> Project {
    pack(dir);
    put_bytes(dir, "data/handling.meta", HANDLING);
    let mut files = Module::files("files");
    files
        .set_file_type("data/handling.meta", Some("HANDLING_FILE"))
        .unwrap();
    // A package also puts a readme in the game folder; a pack registers a file only under a type, so it holds the handling file alone.
    let mut documents = vec![dir.join("data/handling.meta")];
    if kind == OutputKind::OivPackage {
        put_bytes(dir, "readme.txt", README);
        documents.push(dir.join("readme.txt"));
    }
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "."), files],
        outputs: vec![Output::of(kind, "out", "demo", &["wardrobe", "files"])],
    };
    let path = dir.join("pack.openrage-proj");
    Project::create(&path, &container, &documents).unwrap();
    Project::load(&path).unwrap()
}

fn built(project: &Project, out: &Path) -> PathBuf {
    let plans = plans(project, "out").unwrap();
    assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);
    let (built, stopped) = build_all(&plans, out);
    assert!(stopped.is_none(), "{stopped:?}");
    built[0].folder.clone()
}

fn files_under(dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => walk(base, &path, out),
                false => {
                    let rel = path.strip_prefix(base).unwrap();
                    out.insert(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

fn said<'a>(reopened: &'a or_wardrobe::reopen::Reopened, path: &str) -> &'a str {
    reopened
        .unplaced
        .iter()
        .find(|l| l.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed in {:#?}", reopened.unplaced))
        .why
        .as_str()
}

#[test]
fn a_dlc_reopens_as_sources_and_names_what_it_could_not_place() {
    let dir = tempfile::tempdir().unwrap();
    let original = project(dir.path(), OutputKind::SingleplayerDlc);
    let out = tempfile::tempdir().unwrap();
    let folder = built(&original, out.path());

    let into = tempfile::tempdir().unwrap();
    let into = into.path().join("reopened");
    let reopened = reopen(&folder, &into, &Options::default()).expect("a built pack reopens");

    assert_eq!(reopened.pack, "demo");
    assert_eq!(reopened.garments, 4);
    // The pack is a project with a clothing content of sources and a Files content of what it could not place.
    let project = Project::load(&reopened.project).expect("the project file reads");
    let kinds: Vec<(&str, &ModuleKind)> = project
        .container
        .modules
        .iter()
        .map(|m| (m.id.as_str(), &m.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("wardrobe", &ModuleKind::Clothing),
            ("files", &ModuleKind::Files)
        ]
    );
    let output = &project.container.outputs[0];
    assert_eq!(output.kind, OutputKind::SingleplayerDlc);
    assert_eq!(output.name, "demo");
    assert_eq!(
        output.take,
        ["wardrobe"],
        "a file of another kind needs a type the game is told, so a build takes the pack alone until somebody states one"
    );

    // Every model and texture is a source folder, and the project holds no compiled file of the pack's.
    let pack_folder = into.join("clothing");
    let wardrobe = Wardrobe::read(&pack_folder).unwrap();
    assert_eq!(wardrobe.pack, "demo");
    for g in &wardrobe.garments {
        let model = g.model.as_deref().expect("a model");
        assert!(model.ends_with(".drawables.src"), "{model}");
        assert!(pack_folder.join(model).is_dir(), "{model}");
        for texture in g.variations.iter().filter_map(|v| v.texture.as_deref()) {
            assert!(texture.ends_with(".textures.src"), "{texture}");
            assert!(pack_folder.join(texture).is_dir(), "{texture}");
        }
    }
    let compiled: Vec<String> = files_under(&pack_folder)
        .into_iter()
        .filter(|f| !f.contains(".src/"))
        .filter(|f| [".ydd", ".ytd"].iter().any(|e| f.ends_with(e)))
        .collect();
    assert!(compiled.is_empty(), "{compiled:?}");

    // What the metadata states comes back with the pack.
    let find = |ped, slot| {
        wardrobe
            .garments
            .iter()
            .find(|g| g.ped == Some(ped) && g.slot == Some(slot))
            .unwrap_or_else(|| panic!("{ped:?} {slot:?}"))
    };
    let jacket = find(Ped::Male, JBIB);
    assert_eq!((jacket.position, jacket.alternates.len()), (Some(0), 1));
    assert_eq!(jacket.variations.len(), 2);
    assert_eq!(find(Ped::Male, FEET).expressions.len(), 1);
    assert_eq!(find(Ped::Male, P_HEAD).variations.len(), 1);
    assert_eq!(find(Ped::Female, LOWR).variations.len(), 1);

    // What could not be placed is named, and the file the pack does not write again is kept.
    assert!(said(&reopened, "content.xml").contains("writes it again"));
    assert!(said(&reopened, "setup2.xml").contains("writes it again"));
    assert!(said(&reopened, "data/handling.meta").contains("files/data/handling.meta"));
    assert_eq!(
        std::fs::read(into.join("files/data/handling.meta")).unwrap(),
        HANDLING
    );
    assert!(!into.join("files/content.xml").exists());
    let text = std::fs::read_to_string(&reopened.project).unwrap();
    assert!(
        text.contains("\"adopted\"") && text.contains("data/handling.meta"),
        "the project file records what was not placed: {text}"
    );
}

#[test]
fn a_reopened_dlc_builds_the_pack_it_was_and_every_file_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let original = project(dir.path(), OutputKind::SingleplayerDlc);
    let out = tempfile::tempdir().unwrap();
    let folder = built(&original, out.path());
    let before: BTreeSet<String> = plans(&original, "out").unwrap()[0]
        .files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| p != "data/handling.meta")
        .collect();

    let into = tempfile::tempdir().unwrap();
    let into = into.path().join("reopened");
    let reopened = reopen(&folder, &into, &Options::default()).unwrap();
    let again = Project::load(&reopened.project).unwrap();
    let planned = plans(&again, "dlc").unwrap();
    assert!(planned[0].is_buildable(), "{:?}", planned[0].problems);
    let after: BTreeSet<String> = planned[0].files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(
        after, before,
        "the reopened pack ships the files the built one held"
    );

    let root = tempfile::tempdir().unwrap();
    let cook = |source: &Path, scratch: &Path| or_source::cook::cook_source(source, scratch);
    let (made, stopped) = build_all_with(
        &planned,
        root.path(),
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );
    assert!(stopped.is_none(), "{stopped:?}");
    assert!(
        made[0].verification.problems.is_empty(),
        "{:?}",
        made[0].verification.problems
    );
}

#[test]
fn an_oiv_reopens_the_pack_it_installs_and_keeps_what_it_puts_in_the_game_folder() {
    let dir = tempfile::tempdir().unwrap();
    let original = project(dir.path(), OutputKind::OivPackage);
    let out = tempfile::tempdir().unwrap();
    let folder = built(&original, out.path());

    let into = tempfile::tempdir().unwrap();
    let into = into.path().join("reopened");
    let reopened = reopen(&folder.join("demo.oiv"), &into, &Options::default()).unwrap();

    assert_eq!(reopened.garments, 4);
    let project = Project::load(&reopened.project).unwrap();
    assert_eq!(project.container.outputs[0].kind, OutputKind::OivPackage);
    assert!(said(&reopened, "readme.txt").contains("game folder"));
    assert_eq!(
        std::fs::read(into.join("files/readme.txt")).unwrap(),
        README
    );
    let wardrobe = Wardrobe::read(&into.join("clothing")).unwrap();
    assert!(wardrobe.garments.iter().all(|g| g
        .model
        .as_deref()
        .is_some_and(|m| m.ends_with(".drawables.src"))));
}

#[test]
fn a_folder_that_holds_no_clothing_is_refused_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    // A pack of one file nothing here reads: a DLC of a handling file only.
    put_bytes(dir.path(), "data/handling.meta", HANDLING);
    let mut files = Module::files("files");
    files
        .set_file_type("data/handling.meta", Some("HANDLING_FILE"))
        .unwrap();
    let container = Container {
        modules: vec![files],
        outputs: vec![Output::of(
            OutputKind::SingleplayerDlc,
            "out",
            "plain",
            &["files"],
        )],
    };
    let path = dir.path().join("plain.openrage-proj");
    Project::create(&path, &container, &[dir.path().join("data/handling.meta")]).unwrap();
    let project = Project::load(&path).unwrap();
    let plans = or_project::plans(
        &project.container,
        "out",
        &project.folder(),
        &project.sources(),
    )
    .unwrap();
    let (built, stopped) = build_all(&plans, out.path());
    assert!(stopped.is_none());

    let into = tempfile::tempdir().unwrap();
    let into = into.path().join("reopened");
    let refused = reopen(&built[0].folder, &into, &Options::default()).unwrap_err();
    assert!(
        refused.to_string().contains("clothing"),
        "it says why: {refused}"
    );
    assert!(!into.exists(), "a refusal makes no folder");
}

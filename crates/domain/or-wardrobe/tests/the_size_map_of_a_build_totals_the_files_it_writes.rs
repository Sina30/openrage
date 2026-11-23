//! The size map ranks the files a build writes by content, by garment and by slot, and its figures total the build's files: the
//! plan's before the build and the bytes the cooked files came out at after it. The dependencies list what each garment names,
//! once each, with the garments that share it and what leads outside the pack.

mod common;

use std::path::{Path, PathBuf};

use common::{drawables_source, garment, project, put_bytes, snapshot, ytd, JBIB, LOWR, P_HEAD};
use or_project::reference::Reference;
use or_project::{build_all_with, Cook};
use or_source::sample::drawable_dictionary_with_levels as drawable_dictionary;
use or_wardrobe::sizes::{dependencies, sizes, Leads};
use or_wardrobe::{inspect, planned, plans, Depth, Ped, Wardrobe};

fn bytes(vertices: usize, tag: f32) -> Vec<u8> {
    drawable_dictionary("x", vertices, tag, 1)
}

/// A jacket and a hat of compiled files, trousers whose model is a source folder, and the jacket's first texture worn by the trousers too.
fn pack(folder: &Path) -> Wardrobe {
    put_bytes(folder, "models/jacket.ydd", &bytes(900, 1.0));
    put_bytes(folder, "models/hat.ydd", &bytes(30, 2.0));
    put_bytes(folder, "textures/a.ytd", &ytd("a"));
    put_bytes(folder, "textures/b.ytd", &ytd("b"));
    put_bytes(folder, "textures/hat.ytd", &ytd("hat"));
    drawables_source(&folder.join("models/trousers.drawables.src"), "trousers");
    put_bytes(
        folder,
        "models/trousers.drawables.src/model.glb",
        b"geometry",
    );
    let mut w = Wardrobe::new("weighed");
    let mut jacket = garment(
        Ped::Male,
        JBIB,
        None,
        "models/jacket.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    jacket.label = Some("Wool jacket".into());
    let mut trousers = garment(
        Ped::Female,
        LOWR,
        None,
        "models/trousers.drawables.src",
        &["textures/a.ytd"],
    );
    trousers.label = Some("Chinos".into());
    let mut hat = garment(
        Ped::Male,
        P_HEAD,
        None,
        "models/hat.ydd",
        &["textures/hat.ytd"],
    );
    hat.label = Some("Hat".into());
    w.garments = vec![jacket, trousers, hat];
    w.write(folder).unwrap();
    w
}

/// A cooker that makes a model of its own size, whatever the folder holds, so the cooked file is not the payload the plan counted.
fn cooker() -> impl Fn(&Path, &Path) -> Result<PathBuf, Vec<String>> {
    move |_source: &Path, scratch: &Path| {
        let at = scratch.join("cooked").join("trousers.ydd");
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, bytes(300, 3.0)).map_err(|e| vec![e.to_string()])?;
        Ok(at)
    }
}

#[test]
fn the_size_map_totals_the_files_of_the_plan_and_of_the_build_that_wrote_them() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let project = project(at, &w, "weighed_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    assert!(plan.is_buildable(), "{:?}", plan.problems);

    // What the pack's own files weigh, from the disk and not from the plan.
    let len = |rel: &str| std::fs::metadata(at.join(rel)).unwrap().len();
    let jacket = len("models/jacket.ydd") + len("textures/a.ytd") + len("textures/b.ytd");
    let hat = len("models/hat.ydd") + len("textures/hat.ytd");

    let module = &project.container.modules[0];
    let pack_plan = planned(at, module, Depth::Measure).unwrap();
    let size = sizes(&w, &pack_plan, &[]);
    let weight = |rows: &[or_project::sizes::Weight], name: &str| {
        rows.iter()
            .find(|r| r.name == name)
            .map(|r| (r.files, r.bytes))
    };
    assert_eq!(weight(&size.garments, "Wool jacket"), Some((3, jacket)));
    assert_eq!(weight(&size.garments, "Hat"), Some((2, hat)));
    assert_eq!(
        weight(&size.garments, "Chinos").map(|(files, _)| files),
        Some(2),
        "the trousers' model is cooked and their texture is the jacket's, copied for them"
    );
    assert!(
        size.garments.windows(2).all(|p| p[0].bytes >= p[1].bytes),
        "the heaviest first: {:?}",
        size.garments
    );
    assert_eq!(size.garments[0].name, "Wool jacket");
    let slot = size
        .slots
        .iter()
        .find(|s| (s.ped, s.slot) == (Ped::Male, JBIB))
        .unwrap();
    assert_eq!((slot.files, slot.bytes), (3, jacket));
    assert!(
        size.slots.windows(2).all(|p| p[0].bytes >= p[1].bytes),
        "{:?}",
        size.slots
    );
    // The weights of the garments and of what none owns are the files of the contribution.
    let owned: u64 = size.garments.iter().map(|g| g.bytes).sum();
    assert_eq!(owned + size.rest.bytes, size.bytes);
    assert_eq!(
        size.bytes,
        pack_plan
            .contribution
            .files
            .iter()
            .map(|f| f.bytes())
            .sum::<u64>()
    );
    assert_eq!(size.files, pack_plan.contribution.files.len() as u64);
    assert!(
        size.rest.files >= 3,
        "the metadata of the pack are files no garment owns"
    );

    // By content, the plan's totals: the manifest the build writes is the only file no content made.
    let by_content = or_project::sizes::by_content(&plan, &[]);
    assert_eq!(by_content.total, plan.totals);
    assert_eq!(by_content.rows.len(), 1);
    assert_eq!(by_content.rows[0].name, module.id);
    assert_eq!(by_content.own.files, 1, "fxmanifest.lua");

    // After the build: the cooked file counts the bytes it came out at, and the whole is what is on the disk.
    let cook = cooker();
    let root = tempfile::tempdir().unwrap();
    let (built, stopped) = build_all_with(
        std::slice::from_ref(&plan),
        root.path(),
        &Default::default(),
        Some(&cook as &Cook),
    );
    assert!(stopped.is_none(), "{stopped:?}");
    let built = &built[0];
    assert_eq!(built.cooked.len(), 1);
    let after = or_project::sizes::by_content(&plan, &built.cooked);
    assert_eq!(after.total, built.written, "what the build says it wrote");
    let written = snapshot(&built.folder);
    let on_disk: u64 = plan
        .files
        .iter()
        .map(|f| written[&f.path].len() as u64)
        .sum();
    assert_eq!(after.total.bytes, on_disk, "and what is on the disk");
    assert_ne!(
        after.total.bytes, plan.totals.bytes,
        "the cook came out at another size than the payload the plan counted"
    );
    let size_after = sizes(&w, &pack_plan, &built.cooked);
    let trousers = |s: &or_wardrobe::sizes::Sizes| weight(&s.garments, "Chinos").unwrap().1;
    let payload = pack_plan
        .contribution
        .files
        .iter()
        .find_map(|f| match f.origin {
            or_project::Origin::Cook { bytes, .. } => Some(bytes),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        trousers(&size_after) + payload,
        trousers(&size) + built.cooked[0].bytes,
        "the trousers weigh the cooked model in place of the payload the plan counted"
    );
    assert_eq!(size_after.bytes, after.total.bytes - after.own.bytes);
}

#[test]
fn the_dependencies_list_what_each_garment_names_once_with_those_that_share_it() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let mut w = pack(at);
    // A garment that names a file that is not there, and one that names the game file of an id.
    let mut lost = garment(Ped::Male, JBIB, None, "models/gone.ydd", &[]);
    lost.label = Some("Lost".into());
    let mut elsewhere = garment(Ped::Male, JBIB, None, "models/by_id.ref.json", &[]);
    elsewhere.label = Some("By id".into());
    Reference::ById {
        id: "jbib_000_u".into(),
    }
    .write(&at.join("models/by_id.ref.json"))
    .unwrap();
    w.garments.extend([lost, elsewhere]);
    w.write(at).unwrap();

    let facts = inspect(at, &w, Depth::Open);
    let deps = dependencies(&w, at, &facts);
    let by = |path: &str| {
        deps.iter()
            .find(|d| d.path == path)
            .unwrap_or_else(|| panic!("{path} in {deps:?}"))
    };
    assert_eq!(
        by("textures/a.ytd").used_by,
        ["Wool jacket", "Chinos"],
        "named by two garments, listed once"
    );
    assert_eq!(by("textures/a.ytd").leads, Leads::Inside);
    assert_eq!(by("models/trousers.drawables.src").leads, Leads::Inside);
    assert_eq!(by("models/gone.ydd").leads, Leads::Missing);
    assert_eq!(
        by("models/by_id.ref.json").leads,
        Leads::GameId("jbib_000_u".into())
    );
    let distinct: std::collections::BTreeSet<_> = deps.iter().map(|d| &d.path).collect();
    assert_eq!(distinct.len(), deps.len());
    assert_eq!(
        deps.len(),
        8,
        "{:?}",
        deps.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
    assert!(
        deps.windows(2).all(|p| p[0].bytes >= p[1].bytes),
        "the heaviest first"
    );
    assert_eq!(deps[0].path, "models/jacket.ydd");
    assert_eq!(
        deps[0].bytes,
        std::fs::metadata(at.join("models/jacket.ydd"))
            .unwrap()
            .len()
    );
}

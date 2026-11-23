//! A garment whose model is a `.drawables.src` folder of the pack is planned as a cook, never as a copy of the folder: the
//! plan ships the file the build cooks it into, under the name any other model of its place has, and an edit of the
//! source is an edit of the garment for the build's diff.

mod common;

use std::path::{Path, PathBuf};

use common::{drawables_source, garment, project, put, put_bytes, JBIB};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{build_all_with, Cook, Origin};
use or_wardrobe::{allocate, check, inspect, plan, plans, Depth, Fact, Ped, Wardrobe};

const WHAT: &str = "a_garment_that_names_a_source_folder_ships_the_cooked_file";
const MODEL: &str = "mp_m_freemode_01_mp_m_demo^jbib_012_u.ydd";

/// A pack of one jacket pinned at 12 whose model is the source folder `models/jacket.drawables.src`.
fn pack(folder: &Path) -> Wardrobe {
    drawables_source(&folder.join("models/jacket.drawables.src"), "jacket");
    put_bytes(folder, "models/jacket.drawables.src/model.glb", b"geometry");
    put_bytes(folder, "textures/a.ytd", b"texture");
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(12),
        "models/jacket.drawables.src",
        &["textures/a.ytd"],
    ));
    w.write(folder).unwrap();
    w
}

#[test]
fn the_source_is_looked_at_as_the_file_it_becomes_and_the_plan_cooks_it_under_the_models_name() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);

    let facts = inspect(at, &w, Depth::Open);
    let Some(Fact::File {
        bytes,
        digest,
        read,
        ..
    }) = facts.get("models/jacket.drawables.src")
    else {
        panic!("a source is a file to look at, not a folder");
    };
    assert!(*bytes > 0 && digest.is_some());
    assert!(matches!(read, Some(Ok(_))), "{read:?}");
    let found: Vec<_> = check(&w, &allocate(&w), &facts)
        .into_iter()
        .filter(|f| f.message.contains("model"))
        .map(|f| f.message)
        .collect();
    assert!(
        found
            .iter()
            .all(|m| !m.contains("folder") && !m.contains("not there")),
        "{found:?}"
    );

    let planned = plan(&w, &facts, "pack", at);
    let model = planned
        .contribution
        .files
        .iter()
        .find(|f| f.path.ends_with(MODEL))
        .expect("the garment ships its model");
    match &model.origin {
        Origin::Cook {
            source, kind, game, ..
        } => {
            assert_eq!(source, &at.join("models").join("jacket.drawables.src"));
            assert_eq!(kind, "drawables");
            assert!(
                game.is_empty(),
                "the pack's metadata states this drawable, so its file always ships"
            );
        }
        other => panic!("a source is cooked, not copied: {other:?}"),
    }
    assert!(
        planned
            .contribution
            .files
            .iter()
            .all(|f| !matches!(&f.origin, Origin::Copy { from, .. } if from.to_string_lossy().contains(".src"))),
        "nothing inside the folder is copied"
    );
}

#[test]
fn an_edit_of_the_source_changes_the_garments_digest_for_the_next_builds_diff() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let digest_of = |w: &Wardrobe| {
        let facts = inspect(at, w, Depth::Digest);
        plan(w, &facts, "pack", at).contribution.items[0]
            .digest
            .clone()
    };
    let first = digest_of(&w);
    assert_eq!(digest_of(&w), first, "the same folder, the same digest");

    put_bytes(
        at,
        "models/jacket.drawables.src/model.glb",
        b"geometry, edited",
    );
    assert_ne!(digest_of(&w), first);
}

#[test]
fn a_source_a_build_cannot_cook_is_a_model_that_does_not_open_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);

    // Not a source at all.
    std::fs::remove_file(at.join("models/jacket.drawables.src/asset.json")).unwrap();
    let facts = inspect(at, &w, Depth::Digest);
    let found = check(&w, &allocate(&w), &facts);
    let model = found
        .iter()
        .find(|f| f.message.contains("does not open"))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert!(
        model.message.contains("not an OpenRage source"),
        "{}",
        model.message
    );
    assert!(model.blocks(), "a build stops for a model it cannot cook");

    // A source of another kind.
    drawables_source(&at.join("models/jacket.drawables.src"), "jacket");
    let manifest = at.join("models/jacket.drawables.src/asset.json");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("\"drawables\"", "\"textures\"");
    std::fs::write(&manifest, text).unwrap();
    std::fs::write(at.join("models/jacket.drawables.src/textures.json"), b"{}").unwrap();
    let found = check(&w, &allocate(&w), &inspect(at, &w, Depth::Digest));
    assert!(
        found.iter().any(|f| f.message.contains("textures source")),
        "{found:?}"
    );
}

/// A cooker that makes the file a stand-in source stands for: the staged model, whatever the folder holds.
fn cooker(model: PathBuf) -> impl Fn(&Path, &Path) -> Result<PathBuf, Vec<String>> {
    move |source: &Path, scratch: &Path| {
        let at = scratch.join("cooked").join("jacket.ydd");
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::copy(&model, &at).map_err(|e| vec![format!("{}: {e}", source.display())])?;
        Ok(at)
    }
}

#[test]
fn a_build_ships_the_cooked_file_where_the_game_names_it_and_never_the_folder() {
    let (Some(ydds), Some(ytds)) = (
        common::staged("ydd", 1, WHAT),
        common::staged("ytd", 1, WHAT),
    ) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    drawables_source(&at.join("models/jacket.drawables.src"), "jacket");
    put(at, "textures/a.ytd", &ytds[0]);
    let mut w = Wardrobe::new("demo");
    w.garments.push(garment(
        Ped::Male,
        JBIB,
        Some(12),
        "models/jacket.drawables.src",
        &["textures/a.ytd"],
    ));
    let project = project(at, &w, "demo_pack");
    let plans = plans(&project, "fivem").unwrap();
    assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);

    let cook = cooker(ydds[0].clone());
    let root = tempfile::tempdir().unwrap();
    let (built, stopped) = build_all_with(
        &plans,
        root.path(),
        &ParamRemap::default(),
        Some(&cook as &Cook),
    );
    assert!(stopped.is_none(), "{stopped:?}");

    let written = common::snapshot(&built[0].folder);
    let shipped: Vec<_> = written.keys().filter(|p| p.ends_with(MODEL)).collect();
    assert_eq!(shipped.len(), 1, "{:?}", written.keys().collect::<Vec<_>>());
    assert_eq!(
        written[shipped[0]],
        std::fs::read(&ydds[0]).unwrap(),
        "the file the cook made"
    );
    assert!(
        written.keys().all(|p| !p.contains(".src")),
        "the folder never ships"
    );
    assert_eq!(built[0].cooked.len(), 1);
}

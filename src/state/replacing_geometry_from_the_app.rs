//! Replacing the geometry of a drawable source from the Assets table: the model goes into the source,
//! the cook is made again, the Model workspace draws the cook, and the report says what the drawable had no place for.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, over the smallest staged
//! drawable that extracts: no install and no keys. A suite that finds no corpus has skipped.

use std::path::{Path, PathBuf};

use or_interchange::gltf::{self, Container};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
use or_io::decl::{File, Gen8};
use or_io::gta5::GtaDrawable;
use or_source::models::{self, Extraction};
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8};

use super::importing_game_files_from_the_app::{imported, project, row, until};
use super::opening_a_ydd::finish_load;
use super::workbench::{AssetState, Question};
use super::AppState;
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::viewport::MeshCacheKey;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
/// How far an edit in another tool moves every vertex.
const LIFT: f32 = 3.0;

fn how() -> Extraction<'static> {
    Extraction {
        from_game: false,
        virtual_path: None,
        textures: false,
        skeleton: None,
    }
}

/// The smallest staged `.ydr` that becomes a source.
fn a_drawable(scratch: &Path) -> Option<PathBuf> {
    let mut files = Corpus::new(MANIFEST, "ydr").target(TARGET_GEN8).find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let found = files.into_iter().find(|path| {
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let into = scratch.join("probe");
        let _ = std::fs::remove_dir_all(&into);
        std::fs::create_dir_all(&into).unwrap();
        models::extract_drawable(&bytes, "probe", &into, &how()).is_ok()
    });
    if found.is_none() {
        skip_corpus(
            "replacing the geometry of a drawable source",
            "no staged .ydr",
        );
    }
    found
}

/// A glb of the drawable in `ydr` with every vertex lifted, as a modeller's tool would hand it back.
fn lifted_glb(ydr: &Path, into: &Path) -> PathBuf {
    let bytes = std::fs::read(ydr).unwrap();
    let document = GtaDrawable::<Gen8>::from_bytes(&bytes).expect("the drawable reads");
    let options = ExtractOptions {
        lods: LodSelection::All,
        textures: false,
    };
    let mut scene = drawable_scene("subject", &document.base, &options);
    for position in (scene.objects.iter_mut())
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
        .flat_map(|p| p.positions.iter_mut())
    {
        position[2] += LIFT;
    }
    let file = into.join("lifted.glb");
    std::fs::write(&file, gltf::write(&scene, "lifted", Container::Glb).primary).unwrap();
    file
}

/// The first vertex the viewport draws of the active model.
fn first_vertex_drawn(state: &AppState) -> [f32; 3] {
    let meshes = build_meshes_for_active_document(state);
    let first = meshes.iter().find_map(|mesh| mesh.vertices.first());
    first.expect("the viewport draws a vertex of it").position
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

/// A project that imported `ydr` as a source, cooked, and its cook open in the Model workspace.
struct Imported {
    state: AppState,
    key: String,
    source: PathBuf,
    ydr: PathBuf,
    work: tempfile::TempDir,
}

fn a_drawable_source_open_in_the_model_workspace() -> Option<Imported> {
    let work = tempfile::tempdir().unwrap();
    let ydr = a_drawable(work.path())?;
    let (mut state, _) = project(work.path());
    state.import_game_files(vec![ydr.clone()]);
    imported(&mut state);
    let name = ydr.file_stem().unwrap().to_string_lossy().into_owned();
    until(&mut state, "the cook of the import", |s| {
        s.workbench.rows().iter().any(|r| r.name == name)
            && s.workbench.state_of(row(s, &name)) == AssetState::Cooked
    });
    let (key, source) = (
        row(&state, &name).key.clone(),
        row(&state, &name).path.clone(),
    );
    state.open_asset(&key, None);
    finish_load(&mut state).expect("the cooked drawable opens");
    assert_eq!(state.active_workspace, Workspace::Model);
    Some(Imported {
        state,
        key,
        source,
        ydr,
        work,
    })
}

#[test]
fn a_model_put_into_a_source_is_cooked_again_and_the_model_workspace_draws_the_cook() {
    let Some(mut c) = a_drawable_source_open_in_the_model_workspace() else {
        return;
    };
    let before = first_vertex_drawn(&c.state);
    let (recooked, drawn) = (
        c.state.recooked,
        MeshCacheKey::of(&c.state, c.state.active_document.expect("a tab"), 0),
    );
    let external = lifted_glb(&c.ydr, c.work.path());

    c.state.start_replacing_geometry(&c.key, external.clone());
    until(&mut c.state, "the report", |s| {
        matches!(s.workbench.asking, Some(Question::Geometry { .. }))
    });
    let Some(Question::Geometry {
        replaced, lines, ..
    }) = c.state.workbench.asking.clone()
    else {
        unreachable!()
    };
    assert!(replaced, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("geometr")),
        "what the compile did: {lines:?}"
    );
    assert_eq!(
        std::fs::read(c.source.join("model.glb")).unwrap(),
        std::fs::read(&external).unwrap(),
        "the glb is stored as it came"
    );

    until(&mut c.state, "the recook to reach the tab", |s| {
        s.recooked > recooked
    });
    let after = first_vertex_drawn(&c.state);
    assert!(
        (distance(before, after) - LIFT).abs() < 0.01,
        "the Model workspace draws the cook of the new geometry: {before:?} then {after:?}"
    );
    assert!(
        MeshCacheKey::of(&c.state, c.state.active_document.expect("a tab"), 0) != drawn,
        "the viewport builds its meshes again"
    );
    let history: Vec<_> = (c.state.workbench.history().iter())
        .filter(|e| e.text.starts_with("Replaced the geometry"))
        .collect();
    assert_eq!(history.len(), 1, "one History line: {history:?}");
}

#[test]
fn a_model_the_source_refuses_changes_nothing_and_the_window_says_why() {
    let Some(mut c) = a_drawable_source_open_in_the_model_workspace() else {
        return;
    };
    let before = std::fs::read(c.source.join("model.glb")).unwrap();
    let drawn = first_vertex_drawn(&c.state);
    let obj = c.work.path().join("mesh.obj");
    std::fs::write(&obj, "v 0 0 0").unwrap();

    c.state.start_replacing_geometry(&c.key, obj);
    until(&mut c.state, "the report", |s| {
        matches!(s.workbench.asking, Some(Question::Geometry { .. }))
    });

    let Some(Question::Geometry {
        replaced, lines, ..
    }) = c.state.workbench.asking.clone()
    else {
        unreachable!()
    };
    assert!(!replaced);
    assert!(lines.iter().any(|l| l.contains("mesh.obj")), "{lines:?}");
    assert_eq!(std::fs::read(c.source.join("model.glb")).unwrap(), before);
    assert_eq!(first_vertex_drawn(&c.state), drawn);
}

#[test]
fn the_row_offers_replace_geometry_only_on_a_model_source() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = project(dir.path());
    let ctx = egui::Context::default();
    super::importing_game_files_from_the_app::write(&folder.join("client.lua"), b"print(1)");
    state.add_files_to_project(vec![folder.join("client.lua")]);
    until(&mut state, "the row", |s| {
        s.workbench.rows().iter().any(|r| r.name == "client.lua")
    });

    let key = row(&state, "client.lua").key.clone();
    state.select_asset(Some(key));
    let command = commands::of(&state, Action::ReplaceGeometry).expect("a command");
    assert_eq!(
        command.enabled,
        Err("Only a drawable or a drawables source has geometry to replace")
    );
    commands::run_action(&mut state, &ctx, Action::ReplaceGeometry);
    assert!(state.workbench.asking.is_none(), "a file takes no model");
}

//! The two paths of the owner's words, walked end to end over a pack somebody built for a server: Open versus Import, and the short path of a
//! clothing creator who comes from Durty Cloth Tool. Each step is the app's own, on the app's workers, with nothing stubbed.
//!
//! ⚠ Reads the files of the pack `OPENRAGE_REFERENCE_PACK` names (the one that holds `stream/`), which is in no repository and is never written or
//! copied into one: a case copies the few files it needs under the scratch folder it makes. Without it a case skips, which is not a pass.
//! "Install" is the copy the Workbench guide describes: the resource folder of `build/` into the `resources` folder of a server.

use std::path::{Path, PathBuf};

use or_project::{project_folder, Project};
use or_test_support::skip_corpus;
use or_wardrobe::measure::{measure, Measure};
use or_wardrobe::Wardrobe;

use super::fix_all_safe_and_the_optimiser_in_the_app::ytd;
use super::front_door::{NewProject, Template};
use super::importing_game_files_from_the_app::{imported, row};
use super::opening_a_ydd::finish_load;
use super::os_drop::DropTarget;
use super::the_dressed_ped::until;
use super::workbench::{AssetState, LastBuild};
use super::{AppState, DocumentKind, Prefs};
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

const WHAT: &str = "the_two_paths_of_the_workbench_end_to_end";
/// The collection the garments of a drop are named for.
const C: &str = "mp_m_freemode_01_mp_m_t";

/// The distances the edit states, which no drawable of the game states.
const EDITED: [f32; 4] = [111.0, 222.0, 333.0, 444.0];

/// A jacket of the reference: its model and its first diffuse texture, both of which the readers open.
struct Jacket {
    model: PathBuf,
    texture: PathBuf,
}

/// The `n` smallest jackets of the reference pack that open, or none (and a skip) without the pack.
fn jackets(n: usize) -> Option<Vec<Jacket>> {
    let root = std::env::var_os("OPENRAGE_REFERENCE_PACK")
        .map(PathBuf::from)
        .filter(|p| p.join("stream").is_dir());
    let Some(root) = root else {
        skip_corpus(
            WHAT,
            "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
        );
        return None;
    };
    let mut models = Vec::new();
    let mut folders = vec![root.join("stream")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let size = entry.metadata().map_or(u64::MAX, |m| m.len());
            if path.is_dir() {
                folders.push(path);
            } else if name.ends_with("_u.ydd") && name.contains("^jbib_") && size < 400_000 {
                models.push((size, path));
            }
        }
        if models.len() > 400 {
            break;
        }
    }
    models.sort();
    let found: Vec<Jacket> = models
        .into_iter()
        .filter_map(|(_, model)| {
            let name = model.file_name()?.to_str()?;
            // `x^jbib_002_u.ydd` is worn with `x^jbib_diff_002_a_uni.ytd`.
            let texture = model.with_file_name(
                name.replace("jbib_", "jbib_diff_")
                    .replace("_u.ydd", "_a_uni.ytd"),
            );
            let asset = or_io::open(&model).ok()?;
            let usable = !crate::render::meshes::meshes_of_asset(&asset).is_empty()
                && or_io::open(&texture).is_ok();
            usable.then_some(Jacket { model, texture })
        })
        .take(n)
        .collect();
    if found.len() < n {
        skip_corpus(
            WHAT,
            format!("fewer than {n} jackets of the reference open"),
        );
        return None;
    }
    Some(found)
}

/// A clothing project `tshirts` in a scratch folder, open on the Wardrobe.
fn a_clothing_project(dir: &Path) -> AppState {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(created.file);
    until(&mut state, "the project's scan", |s| {
        s.workbench.is_scanned()
    });
    state.active_workspace = Workspace::Wardrobe;
    state
}

/// The pack's garments once it holds `n`.
fn garments(state: &AppState, n: usize) -> Option<Vec<(String, Option<String>)>> {
    let read = state.wardrobe_pack()?.read.as_ref().ok()?;
    (read.wardrobe.garments.len() == n).then(|| {
        (read.wardrobe.garments.iter())
            .map(|g| (g.id.clone(), g.model.clone()))
            .collect()
    })
}

fn wait_for_garments(state: &mut AppState, n: usize) -> Vec<(String, Option<String>)> {
    until(state, "the garments", |s| garments(s, n).is_some());
    garments(state, n).expect("the garments stand")
}

fn select(state: &mut AppState, id: &str) {
    let module = state.wardrobe_module_id().expect("a clothing content");
    state.select_garment(&module, Some(id.to_string()));
}

/// Poll until the preview has drawn `parts` parts of a scene it is not waiting to replace.
fn shows(state: &mut AppState, parts: usize) {
    until(state, "the preview", |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading() && d.scene().is_some_and(|scene| scene.parts.len() == parts)
    });
}

/// What the first mesh of the first part of the scene is made of.
fn first_mesh(state: &AppState) -> (usize, usize) {
    let scene = state.workbench.wardrobe.dressed.scene().expect("a scene");
    let mesh = &scene.parts[0].geometry[0];
    (mesh.vertices.len(), mesh.indices.len())
}

/// The vertices and indices of the first mesh of the high level of the model at `path`, counted by the reader.
fn counted(path: &Path) -> (usize, usize) {
    let asset = or_io::open(path).expect("the model opens");
    let meshes = crate::render::meshes::meshes_of_asset_at(&asset, 0);
    (meshes[0].vertices.len(), meshes[0].indices.len())
}

/// The folder the built resource is in, once the build of the project ended well.
fn built(state: &mut AppState) -> crate::state::workbench::BuiltPart {
    state.build_all();
    until(state, "the build to end", |s| {
        !s.workbench.builds().is_running()
    });
    match state.workbench.builds().last("fivem") {
        Some(LastBuild::Built(summary)) => summary.parts.first().cloned().expect("a resource"),
        other => panic!("the build did not go through: {other:?}"),
    }
}

/// Every file under `dir`, relative, with its bytes.
fn files_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn open_a_game_file_in_place_import_it_edit_its_source_build_and_check_it_on_the_ped() {
    let Some(mut jackets) = jackets(2) else {
        return;
    };
    let (one, other) = (jackets.remove(0), jackets.remove(0));
    let dir = tempfile::tempdir().unwrap();
    // The pack holds one jacket already, dropped as clothes; the other is a game file that sits where a download would.
    let drop = dir.path().join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    std::fs::copy(&one.model, drop.join(format!("{C}^jbib_000_u.ydd"))).unwrap();
    std::fs::copy(
        &one.texture,
        drop.join(format!("{C}^jbib_diff_000_a_uni.ytd")),
    )
    .unwrap();
    let file = dir.path().join("downloads/other_jacket.ydd");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::copy(&other.model, &file).unwrap();
    let original = std::fs::read(&file).unwrap();

    let mut state = a_clothing_project(dir.path());
    state.handle_os_drop(vec![drop], DropTarget::Wardrobe);
    let held = wait_for_garments(&mut state, 1);
    let jacket = held[0].0.clone();
    select(&mut state, &jacket);
    shows(&mut state, 1);
    let own = first_mesh(&state);
    let project = state.project_path.clone().expect("a project");
    let folder = project_folder(&project);
    let sources = folder.join("sources");
    let container = state.project_carried.container.clone();

    // OPEN: the file is shown where it is. Nothing is copied, no source is made, the project is as it was.
    state.load_file(file.clone());
    finish_load(&mut state).expect("the file opens where it is");
    let shown = state.active_document().expect("a document");
    assert_eq!(shown.kind, DocumentKind::Ydd);
    assert_eq!(
        shown.path.as_deref(),
        Some(file.as_path()),
        "opened in place"
    );
    assert!(!sources.exists() || std::fs::read_dir(&sources).unwrap().next().is_none());
    assert_eq!(
        state.project_carried.container, container,
        "Open changes no project"
    );
    assert_eq!(std::fs::read(&file).unwrap(), original);
    state.active_workspace = Workspace::Wardrobe;

    // IMPORT: the file becomes a source of the project, and only adds. The file it came from is as it was.
    state.import_game_files(vec![file.clone()]);
    imported(&mut state);
    let made = sources.join("other_jacket.drawables.src");
    assert!(
        made.join("asset.json").is_file(),
        "the import made {made:?}"
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        original,
        "an import never touches the file"
    );
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.action.is_some()
                && t.message.starts_with("Imported 1 file: 1 became a source")),
        "the import says what it did in a toast that has Undo import"
    );
    until(&mut state, "the cook of the import", |s| {
        s.workbench.rows().iter().any(|r| r.name == "other_jacket")
            && s.workbench.state_of(row(s, "other_jacket")) == AssetState::Cooked
    });

    // EDIT its source: the distances the levels of detail switch at are drawable.json's, and a recook puts them in the file the source cooks into.
    let key = row(&state, "other_jacket").key.clone();
    let rules: Vec<PathBuf> = std::fs::read_dir(made.join("drawables"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path().join("drawable.json"))
        .collect();
    assert!(
        !rules.is_empty(),
        "the source holds a drawable.json for each drawable"
    );
    for path in &rules {
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        value["lod_distances"] = serde_json::json!(EDITED);
        std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    state.recook_asset(&key);
    until(&mut state, "the recook", |s| {
        let cooked = s.workbench.row(&key).and_then(|r| r.output.clone());
        cooked.is_some_and(|output| lod_distances(&output) == Some(EDITED))
    });

    // CHECK it on the ped: the garment takes the source for its model, and the ped wears it.
    state.replace_model(jacket.clone(), made);
    until(&mut state, "the garment to name the source", |s| {
        garments(s, 1).is_some_and(|g| {
            g[0].1
                .as_deref()
                .is_some_and(|m| m.ends_with(".drawables.src"))
        })
    });
    select(&mut state, &jacket);
    until(&mut state, "the other jacket on the ped", |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading()
            && d.scene().is_some_and(|scene| scene.parts.len() == 1)
            && first_mesh(s) != own
    });
    assert_eq!(
        first_mesh(&state),
        counted(&other.model),
        "the ped wears the model that was imported"
    );

    // BUILD: the resource holds the edited model, every file it wrote was read back and reloads through a reader.
    let part = built(&mut state);
    assert!(part.checked > 0 && !part.reloaded.is_empty(), "{part:?}");
    let shipped: Vec<_> = files_under(&part.folder)
        .into_iter()
        .filter(|(name, _)| name.ends_with(".ydd"))
        .collect();
    assert_eq!(
        shipped.len(),
        1,
        "{:?}",
        shipped.iter().map(|s| &s.0).collect::<Vec<_>>()
    );
    assert!(
        part.reloaded.iter().any(|(name, _)| name.ends_with(".ydd")),
        "the model reloads through a reader: {:?}",
        part.reloaded
    );
    let at = dir.path().join("shipped.ydd");
    std::fs::write(&at, &shipped[0].1).unwrap();
    assert_eq!(lod_distances(&at), Some(EDITED), "the edit is in the build");
    assert!(Project::load(&project).is_ok());
}

/// The distances the levels of detail of the first drawable of the `.ydd` at `path` switch at, none while the file is not a Legacy drawable dictionary.
fn lod_distances(path: &Path) -> Option<[f32; 4]> {
    use or_io::decl::{File, Gen8};
    use or_io::gta5::{GtaDrawable, PgDictionary};
    let bytes = std::fs::read(path).ok()?;
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes).ok()?;
    Some(
        dictionary
            .entries
            .iter()
            .flatten()
            .next()?
            .base
            .lod_group
            .lod_distances,
    )
}

#[test]
fn a_folder_is_dropped_its_problems_are_seen_fixed_checked_on_the_ped_built_and_installed() {
    let Some(mut jackets) = jackets(2) else {
        return;
    };
    let (first, second) = (jackets.remove(0), jackets.remove(0));
    let dir = tempfile::tempdir().unwrap();
    // A folder somebody keeps: two jackets of a server pack, and the texture of the first one is a picture of 100 by 60 with one mip.
    let drop = dir.path().join("my clothes");
    std::fs::create_dir_all(&drop).unwrap();
    std::fs::copy(&first.model, drop.join(format!("{C}^jbib_000_u.ydd"))).unwrap();
    std::fs::write(
        drop.join(format!("{C}^jbib_diff_000_a_uni.ytd")),
        ytd("jbib_diff_000_a_uni", 100, 60),
    )
    .unwrap();
    std::fs::copy(&second.model, drop.join(format!("{C}^jbib_001_u.ydd"))).unwrap();
    std::fs::copy(
        &second.texture,
        drop.join(format!("{C}^jbib_diff_001_a_uni.ytd")),
    )
    .unwrap();

    let mut state = a_clothing_project(dir.path());

    // DROP the folder on the Wardrobe.
    state.handle_os_drop(vec![drop], DropTarget::Wardrobe);
    let held = wait_for_garments(&mut state, 2);
    let problem = |state: &AppState, code: &str| {
        state
            .workbench
            .findings()
            .iter()
            .any(|f| f.code == Some(code))
    };

    // SEE the problems: the odd texture is found, in the words of the rules.
    until(&mut state, "the problems", |s| {
        problem(s, "texture-not-power-of-two") && problem(s, "texture-mips")
    });

    // FIX ALL SAFE: one command, one step of Undo, and what it could change is gone.
    let steps = state.workbench.wardrobe.undo.len();
    let command = commands::of(&state, Action::FixAllSafe).expect("a command");
    assert!(command.enabled.is_ok(), "{:?}", command.enabled);
    commands::run(&mut state, &egui::Context::default(), &command);
    until(&mut state, "the worker to finish", |s| s.wardrobe_is_idle());
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        steps + 1,
        "one step for the pipeline"
    );
    assert!(
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text.contains("Textures: 1 change(s)")),
        "{:?}",
        state.workbench.history()
    );
    until(&mut state, "the problems to be read again", |s| {
        !problem(s, "texture-not-power-of-two") && !problem(s, "texture-mips")
    });

    // CHECK it on the ped: the first jacket is worn.
    let jacket = held
        .iter()
        .find(|(_, model)| model.as_deref().is_some_and(|m| m.contains("jbib_000")))
        .expect("the first jacket")
        .0
        .clone();
    select(&mut state, &jacket);
    shows(&mut state, 1);
    assert_eq!(
        first_mesh(&state),
        counted(&first.model),
        "the ped wears the model of the folder"
    );

    // BUILD: every file it wrote was read back, and the texture that was fixed is a power of two with its mips.
    let part = built(&mut state);
    assert!(part.checked > 0 && part.reloaded.len() >= 4, "{part:?}");
    let files = files_under(&part.folder);
    let odd: Vec<Measure> = files
        .iter()
        .filter(|(name, _)| name.contains("jbib_diff_000") && name.ends_with(".ytd"))
        .filter_map(|(name, bytes)| Measure::of(&measure(name, bytes)?))
        .collect();
    assert_eq!(
        odd.len(),
        1,
        "{:?}",
        files.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    let texture = &odd[0].textures[0];
    assert!(
        texture.width.is_power_of_two() && texture.height.is_power_of_two(),
        "{texture:?}"
    );
    assert!(texture.mips > 1, "{texture:?}");

    // INSTALL, as the guide says: the resource folder is copied into the resources folder of a server, and the copy is the build.
    let server = dir.path().join("server/resources");
    let installed = server.join(part.folder.file_name().expect("a resource name"));
    copy_folder(&part.folder, &installed);
    assert_eq!(
        files_under(&installed),
        files,
        "the installed copy is the build, byte for byte"
    );
    assert!(installed.join("fxmanifest.lua").is_file());
    for (name, bytes) in &files {
        if name.ends_with(".ydd") || name.ends_with(".ytd") {
            let at = installed.join(name);
            assert!(
                or_io::open(&at).is_ok(),
                "{name} reloads from the server's folder ({} bytes)",
                bytes.len()
            );
        }
    }
    let pack = Wardrobe::read(&folder_of_pack(&state)).expect("the pack still reads");
    assert_eq!(pack.garments.len(), 2);
}

/// The folder of the pack the Wardrobe shows.
fn folder_of_pack(state: &AppState) -> PathBuf {
    state.wardrobe_pack().expect("a pack").folder.clone()
}

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

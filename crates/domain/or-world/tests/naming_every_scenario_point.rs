//! Every scenario region's points named, and a point re-typed by name saved
//! back to its region's own layout.
//!
//! The staged regions name each point's type through their own look-ups; an
//! install, under `OPENRAGE_GAME_ROOT` (`;`-separated roots), also names each
//! type and model set against the scenario and model-set manifests it ships.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::RpfCryptoProvider;
use or_io::decl::File;
use or_io::hash::literal_string_hash;
use or_io::meta::{CScenarioPointRegion, MetaDocument, MetaInstance, Value};
use or_io::Error;
use or_test_support::{game_roots, skip_install, Corpus, CorpusOrigin, GAME_ROOT_ENV};
use or_title::game::Game;
use or_world::field;
use or_world::scenario::{set_scenario_type, PointAt, ScenarioManifests};

/// The staged regions: `.ymt` in the metadata corpus whose root is a scenario region.
fn staged_regions(what: &str) -> Option<Vec<(String, Vec<u8>, CScenarioPointRegion)>> {
    support::schema();
    let origin = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    if matches!(origin, CorpusOrigin::Absent(_)) {
        or_test_support::skip_corpus(what, "no metadata corpus is staged");
        return None;
    }
    let files: Vec<PathBuf> = Corpus::with_root(origin.path(), "ymt")
        .all_targets()
        .find_or_skip(what)?;
    let regions: Vec<_> = files
        .iter()
        .filter_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            match CScenarioPointRegion::from_bytes(&bytes) {
                Err(Error::WrongType { .. }) => None,
                region => {
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some((name, bytes, region.unwrap_or_else(|e| panic!("{e}"))))
                }
            }
        })
        .collect();
    if regions.is_empty() {
        or_test_support::skip_corpus(what, "no staged .ymt is a scenario region");
        return None;
    }
    Some(regions)
}

/// Gives the region's first own point type `index`, in the value tree.
fn retype_in_tree(root: &mut MetaInstance, index: u64) -> bool {
    let key = |name: &[u8]| literal_string_hash(name);
    let point = field::get_mut(root, key(b"Points"))
        .and_then(as_struct_mut)
        .and_then(|c| field::get_mut(c, key(b"MyPoints")))
        .and_then(|items| match items {
            Value::Array(items) => items.first_mut().and_then(as_struct_mut),
            _ => None,
        });
    match point.and_then(|p| field::get_mut(p, key(b"iType"))) {
        Some(Value::UInt(v)) => *v = index,
        Some(Value::Int(v)) => *v = index as i64,
        _ => return false,
    }
    true
}

fn as_struct_mut(value: &mut Value) -> Option<&mut MetaInstance> {
    match value {
        Value::Struct(inst) | Value::Pointer(Some(inst)) => Some(inst),
        _ => None,
    }
}

#[test]
fn every_staged_points_type_is_named_and_a_retyped_point_keeps_its_layout() {
    const WHAT: &str = "every_staged_points_type_is_named_and_a_retyped_point_keeps_its_layout";
    let Some(regions) = staged_regions(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut retyped = 0usize;
    for (file, bytes, region) in &regions {
        for (cluster, point) in region.all_points() {
            if region.scenario_type(point).is_none() {
                failures.push(format!(
                    "{file}: a point of cluster {cluster:?} names type {} of {}",
                    point.i_type,
                    region.look_ups.type_names.len()
                ));
            }
        }
        let Some(first) = region.points.my_points.first() else {
            continue;
        };
        let names = &region.look_ups.type_names;
        let Some(other) = names.iter().find(|n| {
            region
                .scenario_type(first)
                .is_some_and(|t| t.hash != n.hash)
        }) else {
            continue;
        };
        let at = PointAt {
            cluster: None,
            index: 0,
        };
        let mut edited = region.clone();
        set_scenario_type(&mut edited, at, other.clone()).expect("the point exists");
        retyped += 1;
        let index = names
            .iter()
            .position(|n| n.hash == other.hash)
            .expect("named");
        let mut tree = MetaDocument::parse(support::schema(), bytes.clone()).expect("reads");
        assert!(retype_in_tree(&mut tree.instance, index as u64), "{file}");
        let (saved, want) = match (edited.to_bytes(), tree.to_bytes()) {
            (Ok(saved), Ok(want)) => (saved, want),
            (Err(e), _) => {
                failures.push(format!("{file}: the re-typed region does not save: {e}"));
                continue;
            }
            (_, Err(e)) => {
                failures.push(format!("{file}: the re-typed tree does not save: {e}"));
                continue;
            }
        };
        if saved != want {
            failures.push(format!(
                "{file}: re-typing a point saved otherwise than the tree's own layout does"
            ));
        }
        match CScenarioPointRegion::from_bytes(&saved) {
            Ok(back) if back == edited => {
                let named = back.scenario_type(&back.points.my_points[0]);
                if named.map(|n| n.hash) != Some(other.hash) {
                    failures.push(format!("{file}: the point reopens as another type"));
                }
            }
            Ok(_) => failures.push(format!(
                "{file}: the re-typed region does not reopen as saved"
            )),
            Err(e) => failures.push(format!("{file}: the re-typed region does not reopen: {e}")),
        }
    }
    assert!(retyped > 0, "no staged region has a point to re-type");
    assert!(
        failures.is_empty(),
        "{} problems over {} regions:\n  {}",
        failures.len(),
        regions.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

fn crypto_for(root: &Path) -> Option<Arc<dyn RpfCryptoProvider>> {
    let exe = Game::detect_from_root(root).find_executable(root)?;
    let keys = extract_keys(&exe, &|_| {}).ok()?;
    Some(Arc::new(Gta5RetailCryptoProvider::new(Arc::new(keys))))
}

/// The base game's scenario-info and model-set manifests; a DLC pack names its own by `fileType`.
const BASE_MANIFESTS: [&str; 3] = [
    "common:/data/ai/scenarios.meta",
    "common:/data/ai/ambientpedmodelsets.meta",
    "common:/data/ai/vehiclemodelsets.meta",
];
const MANIFEST_FILE_TYPES: [&str; 3] = [
    "SCENARIO_INFO_FILE",
    "AMBIENT_PED_MODEL_SET_FILE",
    "AMBIENT_VEHICLE_MODEL_SET_FILE",
];

struct Named {
    label: String,
    regions: usize,
    points: usize,
    with_model_set: usize,
    problems: Vec<String>,
}

fn name_install(root: &Path) -> Option<Named> {
    let library = support::schema();
    let install = or_title::GtaInstall::from_path(root)?;
    let game =
        or_title::build_mounted_game(&install, crypto_for(root)?, or_title::DlcCutoff::All).ok()?;
    let pack_manifests = game
        .packs
        .iter()
        .flat_map(|pack| &pack.data_files)
        .filter(|df| {
            MANIFEST_FILE_TYPES
                .iter()
                .any(|t| df.file_type.eq_ignore_ascii_case(t))
        })
        .map(|df| df.filename.clone());
    let mut manifests = ScenarioManifests::default();
    for path in BASE_MANIFESTS
        .map(String::from)
        .into_iter()
        .chain(pack_manifests)
    {
        let Some(bytes) = game.vfs.open(&path) else {
            continue;
        };
        if let Ok(doc) = MetaDocument::parse(library, bytes) {
            manifests.add(&doc.instance);
        }
    }
    let mut ymt: BTreeMap<String, String> = BTreeMap::new();
    game.for_each_file(|logical, _container, leaf| {
        if leaf.to_ascii_lowercase().ends_with(".ymt") {
            ymt.insert(logical.to_ascii_lowercase(), logical.to_string());
        }
    });
    let mut out = Named {
        label: root.display().to_string(),
        regions: 0,
        points: 0,
        with_model_set: 0,
        problems: Vec::new(),
    };
    for path in ymt.values() {
        let Some(bytes) = game.vfs.open(path) else {
            continue;
        };
        let region = match CScenarioPointRegion::from_bytes(&bytes) {
            Ok(region) => region,
            Err(Error::WrongType { .. }) => continue,
            Err(e) => {
                out.problems.push(format!("{path}: {e}"));
                continue;
            }
        };
        out.regions += 1;
        match manifests.name_points(&region) {
            Ok(points) => {
                out.points += points.len();
                out.with_model_set += points.iter().filter(|p| p.model_set.is_some()).count();
            }
            Err(e) => out.problems.push(format!("{path}: {e}")),
        }
    }
    Some(out)
}

#[test]
fn every_installed_points_type_and_model_set_is_named_by_a_manifest() {
    const WHAT: &str = "every_installed_points_type_and_model_set_is_named_by_a_manifest";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(
            WHAT,
            format_args!(
                "no install root; set {GAME_ROOT_ENV} to a ;-separated list of install directories"
            ),
        );
        return;
    }
    let installs: Vec<Named> = roots.iter().filter_map(|r| name_install(r)).collect();
    if installs.is_empty() {
        skip_install(WHAT, "no install root mounted");
        return;
    }
    for install in &installs {
        let report = format!(
            "{}: {} regions, {} points named, {} with a model set, {} problems",
            install.label,
            install.regions,
            install.points,
            install.with_model_set,
            install.problems.len()
        );
        eprintln!("{report}");
        assert!(install.regions >= 100, "{report}");
        assert!(
            install.problems.is_empty(),
            "{report}\n  {}",
            install
                .problems
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
}

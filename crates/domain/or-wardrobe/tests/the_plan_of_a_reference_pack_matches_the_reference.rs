//! The planner's output for a reference pack, held to the reference: the structure the resource has,
//! the variation metadata its own `.ymt` states, and the creature metadata it ships.
//!
//! The reference is a clothing resource somebody built and a server runs: it is not in the repository
//! and not in the corpus, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds
//! `stream/` and `data/`. Without it this suite skips, which is not a pass. The pack is only read.
//!
//! ⚠ The pack is imported in place and planned at `Depth::Stat`, because what is held to the
//! reference is the layout and the metadata, and its 3.8 GB of drawables and textures are not read.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::compare::{expressions, same_meaning};
use common::shop;
use or_io::decl::File;
use or_io::meta::{CCreatureMetaData, CPedVariationInfo};
use or_project::{Origin, PlannedFile};
use or_wardrobe::import::{import, Options};
use or_wardrobe::{inspect, layout, plan, Depth, Ped};

const WHAT: &str = "the_plan_of_a_reference_pack_matches_the_reference";

fn reference() -> Option<PathBuf> {
    let path = std::env::var_os("OPENRAGE_REFERENCE_PACK").map(PathBuf::from);
    match path.filter(|p| p.join("stream").is_dir()) {
        Some(path) => Some(path),
        None => {
            or_test_support::skip_corpus(
                WHAT,
                "OPENRAGE_REFERENCE_PACK names no clothing resource; it is a pack of the owner's server, in no repository",
            );
            None
        }
    }
}

/// Every file under `dir` with one of the extensions, by its `/` path from `base`.
fn files_under(base: &Path, dir: &Path, extensions: &[&str], out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(base, &path, extensions, out);
        } else if path
            .extension()
            .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
        {
            out.insert(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

/// The folder of the pack's streamed files under `stream/`: the one that holds the collections' folders.
fn stream_folder(root: &Path) -> PathBuf {
    std::fs::read_dir(root.join("stream"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            std::fs::read_dir(p).is_ok_and(|mut d| {
                d.any(|e| {
                    e.is_ok_and(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("mp_m_freemode_01_")
                    })
                })
            })
        })
        .expect("a folder of streamed collections")
}

#[test]
fn the_plan_has_the_files_the_reference_has_where_the_reference_has_them() {
    let Some(root) = reference() else {
        return;
    };
    let stream = stream_folder(&root);
    let imported = import(&root, &Options::default()).expect("the reference imports");
    eprintln!(
        "{} models, {} textures, {} places held, {} left out",
        imported.report.models,
        imported.report.textures,
        imported.report.reserved,
        imported.report.left_out.len()
    );
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    assert!(
        planned.contribution.problems.is_empty(),
        "{:?}",
        &planned.contribution.problems[..planned.contribution.problems.len().min(5)]
    );

    // The reference's drawables, their cloth and their textures, relative to the folder of the pack; the planned ones the same.
    let mut shipped = BTreeSet::new();
    files_under(&stream, &stream, &["ydd", "ytd", "yld"], &mut shipped);
    // A replacement of the game's own files is no part of a pack of collections.
    shipped.retain(|p| !p.starts_with("replace/"));
    let prefix = format!("stream/{}/", imported.wardrobe.pack);
    let made: BTreeSet<String> = planned
        .contribution
        .files
        .iter()
        .filter(|f| [".ydd", ".ytd", ".yld"].iter().any(|e| f.path.ends_with(e)))
        .map(|f| f.path.strip_prefix(&prefix).unwrap_or(&f.path).to_string())
        .collect();

    // What the import itself left out is the explanation of a shipped file with no planned one.
    let left_out: BTreeSet<String> = imported
        .report
        .left_out
        .iter()
        .filter_map(|l| {
            l.path.strip_prefix(&format!(
                "stream/{}/",
                stream.file_name().unwrap().to_string_lossy()
            ))
        })
        .map(String::from)
        .collect();
    for l in imported
        .report
        .left_out
        .iter()
        .filter(|l| l.path.contains("/unity/") && !l.path.contains("/replace/"))
    {
        eprintln!("left out {}: {}", l.path, l.why);
    }
    let missing: Vec<&String> = shipped
        .iter()
        .filter(|p| !made.contains(*p) && !left_out.contains(*p))
        .collect();
    let extra: Vec<&String> = made.iter().filter(|p| !shipped.contains(*p)).collect();
    eprintln!(
        "shipped {}, planned {}, left out by the import {}, shipped with no plan {}, planned with no shipped file {}",
        shipped.len(),
        made.len(),
        left_out.len(),
        missing.len(),
        extra.len()
    );
    assert!(
        missing.is_empty(),
        "shipped, not planned and not left out: {:?}",
        &missing[..missing.len().min(6)]
    );
    assert!(
        extra.is_empty(),
        "planned, not shipped: {:?}",
        &extra[..extra.len().min(6)]
    );
}

#[test]
fn the_plan_states_the_variation_metadata_the_reference_states() {
    let Some(root) = reference() else {
        return;
    };
    let stream = stream_folder(&root);
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let mut compared = 0;
    for ped in Ped::ALL {
        let collection = imported.wardrobe.collection(ped);
        let name = format!("{}_{collection}.ymt", ped.model());
        let Some(path) = find(&stream, &name) else {
            continue;
        };
        let shipped = CPedVariationInfo::from_bytes(&std::fs::read(&path).unwrap())
            .expect("the reference reopens");
        let made = planned
            .documents
            .variations
            .get(&ped)
            .expect("the ped was planned");
        let wrong = same_meaning(&shipped, made);
        assert!(
            wrong.is_empty(),
            "{ped}: {} differences, first: {:?}",
            wrong.len(),
            &wrong[..wrong.len().min(3)]
        );
        assert_eq!(
            made.dlc_name.hash,
            or_io::string_hash(collection.as_bytes())
        );
        compared += 1;
    }
    assert_eq!(
        compared, 2,
        "both peds of the reference are held to their own file"
    );
}

#[test]
fn the_plan_states_the_creature_metadata_the_reference_ships() {
    let Some(root) = reference() else {
        return;
    };
    let stream = stream_folder(&root);
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let path = find_creature(&stream).expect("the reference ships creature metadata");
    let shipped = CCreatureMetaData::from_bytes(&std::fs::read(path).unwrap()).expect("it reopens");
    let made = planned
        .documents
        .creature
        .as_ref()
        .expect("the plan writes creature metadata");
    assert_eq!(expressions(made), expressions(&shipped));
    let file: &PlannedFile = planned
        .contribution
        .files
        .iter()
        .find(|f| f.path.contains("creaturemetadata/"))
        .unwrap();
    assert!(matches!(file.origin, Origin::Generated(_)));
}

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        {
            return Some(path);
        }
    }
    None
}

fn find_creature(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_creature(&path) {
                return Some(found);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ymt"))
            && std::fs::read(&path).is_ok_and(|b| b.starts_with(b"RBF0"))
        {
            return Some(path);
        }
    }
    None
}

/// Of the Durty-named entries of a reference the plan names, all but a few: the reference lists some
/// entries for a texture it has no file for. The shop metadata itself is held to the reference entry for
/// entry, ids included, in `a_server_pack_with_its_own_shop_metadata_is_imported_and_written_back`.
const SHARE_OF_THE_REFERENCE: usize = 98;

#[test]
fn the_plan_states_the_shop_metadata_the_reference_registers_its_collections_with() {
    let Some(root) = reference() else {
        return;
    };
    let imported = import(&root, &Options::default()).expect("the reference imports");
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let files: Vec<shop::Shop> = Ped::ALL
        .map(|ped| {
            let path = root.join(layout::shop_path(ped, &imported.wardrobe.collection(ped)));
            shop::read(&std::fs::read_to_string(path).expect("the reference ships it"))
        })
        .into();
    for (ped, shipped) in Ped::ALL.into_iter().zip(&files) {
        let made = shop::read(&planned.documents.shops[&ped]);

        // The elements outside the entries, by name and value: the ped, the collection, the character and the creature metadata.
        assert_eq!(made.header, shipped.header, "{ped}: the header");

        for (section, made, shipped, both, unmodelled) in [
            (
                "pedComponents",
                &made.components,
                &shipped.components,
                files
                    .iter()
                    .flat_map(|s| s.components.clone())
                    .collect::<Vec<_>>(),
                Vec::<&str>::new(),
            ),
            (
                "pedProps",
                &made.props,
                &shipped.props,
                files
                    .iter()
                    .flat_map(|s| s.props.clone())
                    .collect::<Vec<_>>(),
                Vec::<&str>::new(),
            ),
        ] {
            // The elements of an entry: the plan states every element the reference's entries have (an import keeps what it does not name).
            // The name the game hashes (`uniqueNameHash`) is left out of what is compared, since two tools do not write it alike.
            let keep: BTreeSet<&str> = shop::names(made)
                .into_iter()
                .filter(|n| *n != "uniqueNameHash")
                .collect();
            let beyond: Vec<&str> = shop::names(shipped)
                .into_iter()
                .filter(|n| *n != "uniqueNameHash" && !keep.contains(n))
                .collect();
            assert_eq!(beyond, unmodelled, "{ped}: {section}");

            // Every entry of the reference has the elements of the plan's, in their order.
            let (made, shipped) = (shop::only(made, &keep), shop::only(shipped, &keep));
            assert_eq!(
                shop::shapes(&made),
                shop::shapes(&shipped),
                "{ped}: {section}"
            );
            // Every value the plan writes for an element that is no index is one the reference writes, in either ped's file:
            // the female file lists no wrist anchor, which the male file does.
            let both = shop::only(&both, &keep);
            for name in keep.iter().filter(|n| !n.ends_with("Index")) {
                let (ours, theirs) = (shop::values(&made, name), shop::values(&both, name));
                assert!(
                    ours.is_subset(&theirs),
                    "{ped}: {section}: {name} {ours:?} against {theirs:?}"
                );
            }
            // Every entry of the reference is the plan's: an entry that points at no garment is kept as stated.
            let common = shop::common(&made, &shipped);
            assert_eq!(
                common,
                shipped.len(),
                "{ped}: {section}: {common} of the {} entries of the reference are the plan's",
                shipped.len()
            );
        }
    }
}

/// The names of a shop's entries, as the `uniqueNameHash` of each states them.
fn unique_names(shop: &shop::Shop) -> BTreeSet<String> {
    shop.components
        .iter()
        .chain(&shop.props)
        .flatten()
        .filter(|(name, _)| name == "uniqueNameHash")
        .map(|(_, value)| value.clone())
        .collect()
}

/// When the reference was built by Durty Cloth Tool its entries are named `CLO_DCT_…`, and the pack's
/// option then writes the names the plan lacks; a reference built by anything else says nothing about
/// the option, and the test says so and holds nothing against it.
#[test]
fn the_durty_option_writes_the_names_a_durty_built_reference_has() {
    let Some(root) = reference() else {
        return;
    };
    let mut imported = import(&root, &Options::default()).expect("the reference imports");
    let theirs: BTreeSet<String> = Ped::ALL
        .into_iter()
        .flat_map(|ped| {
            let path = root.join(layout::shop_path(ped, &imported.wardrobe.collection(ped)));
            let text = std::fs::read_to_string(path).expect("the reference ships it");
            unique_names(&shop::read(&text))
        })
        .collect();
    let durty = !theirs.is_empty() && theirs.iter().all(|n| n.starts_with("CLO_DCT_"));
    eprintln!(
        "the reference names {} entries; built by Durty Cloth Tool: {durty}",
        theirs.len()
    );
    if !durty {
        return;
    }
    imported.wardrobe.shop.durty_hashes = true;
    let facts = inspect(&root, &imported.wardrobe, Depth::Stat);
    let planned = plan(&imported.wardrobe, &facts, "wardrobe", &root);
    let ours: BTreeSet<String> = Ped::ALL
        .into_iter()
        .flat_map(|ped| unique_names(&shop::read(&planned.documents.shops[&ped])))
        .collect();
    let found = theirs.intersection(&ours).count();
    assert!(
        found * 100 >= theirs.len() * SHARE_OF_THE_REFERENCE,
        "{found} of the {} names of the reference are the plan's",
        theirs.len()
    );
}

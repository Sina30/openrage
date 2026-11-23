//! A real server's clothing resource, with shop metadata of its own: every entry it states is
//! resolved to a garment's texture or kept and listed, the ids are written back as the file wrote
//! them, and a second import of the output gives the same ids.
//!
//! The reference is a resource somebody built and a server runs: it is in no repository and not in
//! the corpus, so `OPENRAGE_REFERENCE_PACK` names its folder, the one that holds `stream/` and
//! `data/`. Without it this suite skips, which is not a pass. ⚠ The pack is only read: it is imported
//! in place and planned at `Depth::Stat`, nothing is written in it (the first test lists every file
//! before and after to say so), and the second import reads a folder of empty stand-ins for its
//! models and textures made in a scratch folder, so none of its 4 GB is copied.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use or_project::{Container, Module, Origin, Output};
use or_wardrobe::import::{import, Imported, Options};
use or_wardrobe::shop::{self, Node};
use or_wardrobe::{allocate, inspect, plan, plans_given, Depth, Ped, Planned, Slot, Wardrobe};

const WHAT: &str = "a_server_pack_with_its_own_shop_metadata_is_imported_and_written_back";

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

/// Every file under `dir` with its size and the time it was last written.
fn listing(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, Option<SystemTime>)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            listing(&path, out);
        } else if let Ok(meta) = entry.metadata() {
            out.insert(path, (meta.len(), meta.modified().ok()));
        }
    }
}

/// The shop apparel files of the pack: those whose root element is the shop's.
fn shop_files(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let data = root.join("data");
    for entry in std::fs::read_dir(&data).unwrap().flatten() {
        let path = entry.path();
        let is_xml = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("meta") || e.eq_ignore_ascii_case("xml"));
        if !is_xml {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if shop::looks_like_shop(&bytes) {
            let shown = format!("data/{}", entry.file_name().to_string_lossy());
            files.push((shown, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    files.sort();
    files
}

/// An element and everything in it as one string, its attributes in a fixed order.
fn canonical(node: &Node) -> String {
    let mut attrs = node.attrs.clone();
    attrs.sort();
    let attrs: String = attrs.iter().map(|(k, v)| format!(" {k}=\"{v}\"")).collect();
    let inside: String = node.children.iter().map(canonical).collect();
    format!(
        "<{}{attrs}>{}{inside}</{}>",
        node.name,
        node.text.trim(),
        node.name
    )
}

/// The entries of `text`, each as the sorted list of its elements, in either section.
fn entries(text: &str) -> Vec<String> {
    let root = shop::parse_tree(text.strip_prefix('\u{feff}').unwrap_or(text)).expect("it reads");
    let mut out = Vec::new();
    for section in ["pedComponents", "pedProps"] {
        let Some(section_node) = root.children.iter().find(|c| c.name == section) else {
            continue;
        };
        for item in section_node.children.iter().filter(|c| c.name == "Item") {
            let mut parts: Vec<String> = item.children.iter().map(canonical).collect();
            parts.sort();
            out.push(format!("{section}:{}", parts.concat()));
        }
    }
    out
}

fn imported(root: &Path) -> Imported {
    import(root, &Options::default()).expect("the reference imports")
}

fn planned(root: &Path, w: &Wardrobe) -> Planned {
    plan(w, &inspect(root, w, Depth::Stat), "wardrobe", root)
}

/// The id of each texture the import resolved, by what it points at.
fn resolved_ids(w: &Wardrobe) -> BTreeMap<(Ped, Slot, u32, usize), String> {
    let mut ids = BTreeMap::new();
    for g in &w.garments {
        for (t, v) in g.variations.iter().enumerate() {
            if let (Some(ped), Some(slot), Some(position), Some(entry)) =
                (g.ped, g.slot, g.position, &v.shop)
            {
                ids.insert((ped, slot, position, t), entry.id.clone());
            }
        }
    }
    ids
}

#[test]
fn every_entry_of_the_pack_is_resolved_or_kept_and_nothing_in_the_pack_is_written() {
    let Some(root) = reference() else {
        return;
    };
    let mut before = BTreeMap::new();
    listing(&root, &mut before);

    let first = imported(&root);

    let mut after = BTreeMap::new();
    listing(&root, &mut after);
    assert_eq!(after, before, "an import writes nothing in the pack");

    let files = shop_files(&root);
    assert!(
        !files.is_empty(),
        "the pack holds shop apparel metadata of its own"
    );
    let shop_report = &first.report.shop;
    assert_eq!(
        shop_report.files.len(),
        files.len(),
        "{:?}",
        shop_report.files
    );
    // The entries the files state, counted by a reader of the test's own.
    let stated: usize = files
        .iter()
        .map(|(_, text)| common::shop::read(text))
        .map(|s| s.components.len() + s.props.len())
        .sum();
    assert_eq!(shop_report.entries, stated);
    // None is dropped: each is the id of a texture or kept as it was stated.
    assert_eq!(
        shop_report.resolved + shop_report.unplaced.len(),
        shop_report.entries,
        "{:?}",
        &shop_report.unplaced[..shop_report.unplaced.len().min(5)]
    );
    assert_eq!(
        first.wardrobe.shop.unplaced.len(),
        shop_report.unplaced.len()
    );
    assert_eq!(resolved_ids(&first.wardrobe).len(), shop_report.resolved);

    // The ids are kept as written, whether or not 32 bits hold them.
    let all_ids: Vec<String> = files
        .iter()
        .flat_map(|(_, text)| entries(text))
        .filter_map(|e| {
            let at = e.find("<uniqueNameHash>")? + "<uniqueNameHash>".len();
            Some(e[at..e.find("</uniqueNameHash>")?].to_string())
        })
        .collect();
    let beyond_32_bits = all_ids
        .iter()
        .filter(|id| {
            id.chars().all(|c| c.is_ascii_hexdigit()) && u32::from_str_radix(id, 16).is_err()
        })
        .count();
    let cloth_missing = first.report.cloth_missing.len();
    eprintln!(
        "{} shop file(s), {} entries: {} resolved, {} kept for want of a garment, {} garment(s) with a texture no entry names, {} id(s) claimed twice, {} file(s) carried, {} not read; {} id(s) beyond 32 bits; {} cloth file(s) linked, {} garment(s) missing their cloth",
        shop_report.files.len(),
        shop_report.entries,
        shop_report.resolved,
        shop_report.unplaced.len(),
        shop_report.unentried.len(),
        shop_report.clashes.len(),
        shop_report.carried.len(),
        shop_report.unread.len(),
        beyond_32_bits,
        first.report.cloth,
        cloth_missing
    );
    assert!(
        shop_report.unread.is_empty(),
        "{:?}",
        &shop_report.unread[..shop_report.unread.len().min(5)]
    );
}

#[test]
fn the_plan_writes_every_entry_the_pack_states_back_with_its_id_unchanged() {
    let Some(root) = reference() else {
        return;
    };
    let first = imported(&root);
    let made = planned(&root, &first.wardrobe);
    let files = shop_files(&root);

    let mut lost = Vec::new();
    let (mut stated, mut written) = (0usize, 0usize);
    for ped in Ped::ALL {
        let ours = entries(&made.documents.shops[&ped]);
        written += ours.len();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in &ours {
            *counts.entry(entry).or_default() += 1;
        }
        // The file of this ped is the one whose `pedName` names it.
        let theirs: Vec<String> = files
            .iter()
            .filter(|(_, text)| shop::parse(text, "x").is_ok_and(|f| f.ped == Some(ped)))
            .flat_map(|(_, text)| entries(text))
            .collect();
        stated += theirs.len();
        for entry in theirs {
            match counts.get_mut(entry.as_str()).filter(|n| **n > 0) {
                Some(n) => *n -= 1,
                None => lost.push(format!("{ped}: {entry}")),
            }
        }
    }
    eprintln!(
        "{stated} entries stated, {written} written ({} named by the build for textures the shop file names nothing for), {} lost",
        written.saturating_sub(stated),
        lost.len()
    );
    assert!(
        lost.is_empty(),
        "{} of {stated} entries are not written back as stated; the first: {:?}",
        lost.len(),
        &lost[..lost.len().min(3)]
    );
    assert!(
        made.contribution
            .manifest
            .iter()
            .any(|m| m.key.ends_with("data_file")),
        "the manifest registers the shop files"
    );
}

#[test]
fn a_second_import_of_the_output_gives_the_same_ids() {
    let Some(root) = reference() else {
        return;
    };
    let first = imported(&root);
    let made = planned(&root, &first.wardrobe);
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::fivem_resource(
            "fivem",
            &first.wardrobe.pack,
            &["wardrobe"],
        )],
    };
    let scratch = tempfile::tempdir().unwrap();
    let built = plans_given(
        &container,
        "fivem",
        scratch.path(),
        &[],
        vec![made.contribution.clone()],
    )
    .expect("the output is planned")
    .remove(0);
    let out = scratch.path().join("out");
    let mut stand_ins = 0usize;
    for file in &built.files {
        let to = out.join(&file.path);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        match &file.origin {
            // A model or a texture is read by its name alone here: an empty file stands for it.
            Origin::Copy { .. } | Origin::Retarget { .. } => {
                std::fs::write(&to, b"").unwrap();
                stand_ins += 1;
            }
            Origin::Generated(bytes) => std::fs::write(&to, bytes).unwrap(),
            Origin::Document(text) => std::fs::write(&to, text).unwrap(),
            Origin::Cook { .. } => panic!("a clothing pack cooks no source: {}", file.path),
        }
    }
    // The server's own file travels as it is, so the stand-in for it holds its bytes.
    for carried in &first.wardrobe.shop.carried {
        let name = carried.file.rsplit('/').next().unwrap();
        std::fs::write(
            out.join("data").join(name),
            std::fs::read(root.join(&carried.file)).unwrap(),
        )
        .unwrap();
    }
    let second = imported(&out);
    eprintln!(
        "{stand_ins} stand-in files; the second import resolved {} of {} entries",
        second.report.shop.resolved, second.report.shop.entries
    );

    let (before, after) = (
        resolved_ids(&first.wardrobe),
        resolved_ids(&second.wardrobe),
    );
    let changed: Vec<_> = before
        .iter()
        .filter(|(place, id)| after.get(place) != Some(id))
        .map(|(place, id)| (*place, id.clone(), after.get(place).cloned()))
        .collect();
    assert!(
        changed.is_empty(),
        "{} of {} ids changed; the first: {:?}",
        changed.len(),
        before.len(),
        &changed[..changed.len().min(3)]
    );
    // What the second import reads more is what the build named, for the textures the shop file named nothing for.
    let named_by_the_build = shop::written(&first.wardrobe, &allocate(&first.wardrobe))
        .into_iter()
        .filter(|w| w.garment.is_some())
        .count()
        - before.len();
    assert_eq!(after.len() - before.len(), named_by_the_build);
    let kept = |w: &Wardrobe| {
        let mut kept: Vec<_> = w
            .shop
            .unplaced
            .iter()
            .map(|s| {
                (
                    s.ped,
                    s.prop,
                    s.kind.clone(),
                    s.drawable,
                    s.texture,
                    s.entry.id.clone(),
                    s.entry.cost.clone(),
                    s.entry.fields.clone(),
                )
            })
            .collect();
        kept.sort();
        kept
    };
    assert_eq!(kept(&second.wardrobe), kept(&first.wardrobe));
    assert_eq!(second.wardrobe.shop.carried, first.wardrobe.shop.carried);
}

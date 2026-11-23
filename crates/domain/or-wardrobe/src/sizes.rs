//! What a pack costs and what it needs: the files of a build ranked by garment and by slot, and the files and references the garments name.
//!
//! The sizes are the plan's: a file copied is the bytes it has, a source cooked is the payload the plan counts it as until a build
//! has cooked it, and the sizes a build wrote can be given in their place ([`or_project::Cooked`]). The weights of the garments and of
//! what no garment owns add up to the contribution's files, so they total what the plan writes for the pack.

use std::collections::BTreeMap;
use std::path::Path;

use or_project::reference::Reference;
use or_project::sizes::{ranked, Weight};
use or_project::{Cooked, Origin, PlannedFile};

use crate::facts::{Fact, Facts};
use crate::follow::{follow, Led};
use crate::model::Wardrobe;
use crate::names::{Ped, Slot};
use crate::plan::Planned;
use crate::rules::name_of;

/// What the files written for one slot of one ped come to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotWeight {
    pub ped: Ped,
    pub slot: Slot,
    pub files: u64,
    pub bytes: u64,
}

/// A pack's files ranked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sizes {
    /// The garments that own files, the heaviest first.
    pub garments: Vec<Weight>,
    /// The same files by the slot of a ped they are written for, the heaviest first.
    pub slots: Vec<SlotWeight>,
    /// The files no garment owns: the metadata and the documents the pack writes.
    pub rest: Weight,
    pub files: u64,
    pub bytes: u64,
}

fn bytes_of(file: &PlannedFile, cooked: &[Cooked]) -> u64 {
    match &file.origin {
        Origin::Cook { .. } => (cooked.iter())
            .find(|c| c.path == file.path)
            .map_or_else(|| file.bytes(), |c| c.bytes),
        _ => file.bytes(),
    }
}

/// The files `planned` writes for `wardrobe`, weighed by garment and by slot.
pub fn sizes(wardrobe: &Wardrobe, planned: &Planned, cooked: &[Cooked]) -> Sizes {
    let owner: BTreeMap<&str, usize> = (planned.owners.iter())
        .map(|(path, garment)| (path.as_str(), *garment))
        .collect();
    let mut by_garment: BTreeMap<usize, (u64, u64)> = BTreeMap::new();
    let mut by_slot: BTreeMap<(Ped, Slot), (u64, u64)> = BTreeMap::new();
    let mut sizes = Sizes {
        rest: Weight {
            name: "metadata and documents".to_string(),
            ..Weight::default()
        },
        ..Sizes::default()
    };
    for file in &planned.contribution.files {
        let bytes = bytes_of(file, cooked);
        sizes.files += 1;
        sizes.bytes += bytes;
        let Some(&garment) = owner.get(file.path.as_str()) else {
            sizes.rest.files += 1;
            sizes.rest.bytes += bytes;
            continue;
        };
        let row = by_garment.entry(garment).or_default();
        row.0 += 1;
        row.1 += bytes;
        let g = &wardrobe.garments[garment];
        if let (Some(ped), Some(slot)) = (g.ped, g.slot) {
            let row = by_slot.entry((ped, slot)).or_default();
            row.0 += 1;
            row.1 += bytes;
        }
    }
    sizes.garments = (by_garment.into_iter())
        .map(|(at, (files, bytes))| Weight {
            name: name_of(&wardrobe.garments[at]),
            files,
            bytes,
        })
        .collect();
    sizes.slots = (by_slot.into_iter())
        .map(|((ped, slot), (files, bytes))| SlotWeight {
            ped,
            slot,
            files,
            bytes,
        })
        .collect();
    ranked(&mut sizes.garments);
    sizes
        .slots
        .sort_by(|a, b| (b.bytes.cmp(&a.bytes)).then((a.ped, a.slot).cmp(&(b.ped, b.slot))));
    sizes
}

/// Where a thing a garment names is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leads {
    /// A file or a source folder of the pack's own folder.
    Inside,
    /// A reference to a file or source of this project or another, said in words.
    Reference(String),
    /// A reference that is only the id of a game file.
    GameId(String),
    /// Not there, or a reference that leads nowhere.
    Missing,
}

/// One file or source the pack needs, and the garments that name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// As the garments name it.
    pub path: String,
    pub leads: Leads,
    /// The garments that name it, by the name a list gives them.
    pub used_by: Vec<String>,
    /// What it weighs as the scan saw it.
    pub bytes: u64,
}

/// What each garment names, once for each file and with the garments that name it: the files and sources of the pack, and what it needs from outside, the heaviest first.
pub fn dependencies(wardrobe: &Wardrobe, folder: &Path, facts: &Facts) -> Vec<Dependency> {
    let mut users: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for g in &wardrobe.garments {
        let named = (g.model.iter().chain(&g.alternates).chain(g.cloth.iter()))
            .chain(g.variations.iter().filter_map(|v| v.texture.as_ref()));
        for path in named {
            let who = users.entry(path.as_str()).or_default();
            let name = name_of(g);
            if !who.contains(&name) {
                who.push(name);
            }
        }
    }
    let mut found: Vec<Dependency> = users
        .into_iter()
        .map(|(path, used_by)| {
            let leads = match follow(folder, path) {
                Led::Own => match facts.get(path) {
                    Some(Fact::Missing) | None => Leads::Missing,
                    Some(_) => Leads::Inside,
                },
                Led::To { .. } => Leads::Reference(describe(
                    &folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                )),
                Led::Id(id) => Leads::GameId(id),
                Led::Nowhere { .. } => Leads::Missing,
            };
            let bytes = match facts.get(path) {
                Some(Fact::File { bytes, .. }) => *bytes,
                _ => 0,
            };
            Dependency {
                path: path.to_string(),
                leads,
                used_by,
                bytes,
            }
        })
        .collect();
    found.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    found
}

/// What a reference file says it leads to, in words.
fn describe(file: &Path) -> String {
    match Reference::read(file) {
        Ok(Reference::Here { path, .. }) => format!("{path} of this project"),
        Ok(Reference::Other { project, path, .. }) => format!("{path} of the project at {project}"),
        Ok(Reference::ById { id }) => format!("the game file {id}"),
        Err(why) => why,
    }
}

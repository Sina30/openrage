//! Credits: who made the sources a pack uses and on what terms, read from each source's `asset.json` and listed in a file of a build.
//!
//! A source states its credit itself ([`or_source::asset::Credit`]), so it travels with the folder into another project, a package
//! or a repository. The file a build writes lists each source once, with the garments that use it, and lists apart the sources
//! that state no credit, so that a creator sees what is missing before it ships. A compiled file has no `asset.json` and no
//! credit to list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use or_source::asset::{set_credit, Credit, Source};

use crate::edit::Error;
use crate::facts::{is_source, stays_inside};
use crate::follow::{follow, Led};
use crate::model::{Garment, Wardrobe};
use crate::rules::name_of;

/// The file a build writes the credits to, at the root of the resource.
pub const FILE: &str = "credits.md";

/// A source a pack uses, with what it states of who made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The asset's own name.
    pub name: String,
    /// The word for the kind of source, such as `textures`.
    pub kind: String,
    pub id: String,
    /// The garments that name it, by the name a list gives them.
    pub used_by: Vec<String>,
    pub credit: Option<Credit>,
}

/// The source folders garment `g` names, each with whether it is the pack's own folder (a reference may lead into another project).
fn sources_of(folder: &Path, g: &Garment) -> Vec<(PathBuf, bool)> {
    let named = (g.model.iter().chain(&g.alternates))
        .chain(g.variations.iter().filter_map(|v| v.texture.as_ref()));
    named
        .filter_map(|rel| match follow(folder, rel) {
            Led::Own if stays_inside(rel) && is_source(rel) => Some((
                folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)),
                true,
            )),
            Led::To { at, source: true } => Some((at, false)),
            _ => None,
        })
        .collect()
}

/// Every source the pack names, once each, in the order of kind, name and id.
pub fn entries(wardrobe: &Wardrobe, folder: &Path) -> Vec<Entry> {
    let mut by_dir: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for g in &wardrobe.garments {
        for (dir, _) in sources_of(folder, g) {
            let users = by_dir.entry(dir).or_default();
            let name = name_of(g);
            if !users.contains(&name) {
                users.push(name);
            }
        }
    }
    let mut found: Vec<Entry> = by_dir
        .into_iter()
        .filter_map(|(dir, used_by)| {
            let manifest = Source::open(&dir).ok()?.manifest;
            Some(Entry {
                name: manifest.name,
                kind: manifest.kind,
                id: manifest.id,
                used_by,
                credit: manifest.credit.filter(|c| !c.is_empty()),
            })
        })
        .collect();
    found.sort_by(|a, b| (&a.kind, &a.name, &a.id).cmp(&(&b.kind, &b.name, &b.id)));
    found
}

fn cell(text: &str) -> String {
    match text.is_empty() {
        true => String::new(),
        false => text.replace('|', "\\|").replace('\n', " "),
    }
}

/// The text of the credits file: a table of the sources that state a credit and a list of those that do not.
pub fn markdown(pack: &str, entries: &[Entry]) -> String {
    let mut text = format!("# Credits for {pack}\n\n");
    let credited: Vec<&Entry> = entries.iter().filter(|e| e.credit.is_some()).collect();
    if !credited.is_empty() {
        text.push_str(
            "| Asset | Kind | Used by | Author | Licence | Link |\n|---|---|---|---|---|---|\n",
        );
        for e in credited {
            let c = e.credit.clone().unwrap_or_default();
            text.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                cell(&e.name),
                cell(&e.kind),
                cell(&e.used_by.join(", ")),
                cell(&c.author),
                cell(&c.licence),
                cell(&c.url)
            ));
        }
        text.push('\n');
    }
    let bare: Vec<&Entry> = entries.iter().filter(|e| e.credit.is_none()).collect();
    if !bare.is_empty() {
        text.push_str("## Without a credit\n\nThese sources state no author and no licence.\n\n");
        for e in bare {
            text.push_str(&format!(
                "- {} ({}), used by {}\n",
                e.name,
                e.kind,
                e.used_by.join(", ")
            ));
        }
    }
    if entries.is_empty() {
        text.push_str("The pack names no source folder, so there is no credit to list.\n");
    }
    text
}

/// How many of `entries` state a credit.
pub fn credited(entries: &[Entry]) -> usize {
    entries.iter().filter(|e| e.credit.is_some()).count()
}

/// What stating a credit for a garment did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stated {
    /// The sources that now state it, by name.
    pub set: Vec<String>,
    /// The sources it left alone because a reference leads to them, which their own project states the credit of.
    pub left: Vec<String>,
}

/// State `credit` in the `asset.json` of every source of the pack's own folder that garment `id` names; an empty credit takes the statement away.
pub fn state_for(
    folder: &Path,
    wardrobe: &Wardrobe,
    id: &str,
    credit: &Credit,
) -> Result<Stated, Error> {
    let garment = wardrobe
        .garment(id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let mut done = Stated::default();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (dir, own) in sources_of(folder, garment) {
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir.clone());
        let name = Source::open(&dir)
            .map(|s| s.manifest.name)
            .map_err(|p| Error::Refused(p.to_string()))?;
        match own {
            true => {
                set_credit(&dir, credit.clone()).map_err(|p| Error::Refused(p.to_string()))?;
                done.set.push(name);
            }
            false => done.left.push(name),
        }
    }
    Ok(done)
}

/// The credits file of a build that asks for one, as a document of the resource; a pack cannot carry one, and a note says it was left out.
///
/// ⚠ A file for people is not a file the game is told about, so a singleplayer DLC has no type to register it with.
pub(crate) fn contribute(
    wardrobe: &Wardrobe,
    folder: &Path,
    build: &crate::plan::Build,
    contribution: &mut or_project::Contribution,
) {
    if !build.options.credits {
        return;
    }
    if build.is_pack() {
        contribution.notes.push(
            "the credits file is for people and a pack registers only files the game reads, so this build leaves it out; a FiveM resource carries it".to_string(),
        );
        return;
    }
    let found = entries(wardrobe, folder);
    let bare = found.len() - credited(&found);
    if bare > 0 {
        contribution.notes.push(format!(
            "{bare} of the {} sources the pack uses state no author and no licence; the credits file lists them apart",
            found.len()
        ));
    }
    contribution.files.push(or_project::PlannedFile {
        path: FILE.to_string(),
        origin: or_project::Origin::Document(markdown(&wardrobe.pack, &found)),
    });
}

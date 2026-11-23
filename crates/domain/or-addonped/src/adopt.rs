//! Adopting a folder that holds addon peds: what of it this engine knows, and what it could not place.
//!
//! The folder's model is read where it stands. An `addonped.json` already there is the model;
//! without one, each `.yft` that the folder's `peds.meta` defines a ped under is one: the files
//! named for it beside it, and the files in the folder of its name, are its own. The model that comes
//! out is a file the adoption asks to add. The data file is carried as the model's, and the manifest
//! is not, since a build writes its own.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use or_project::adopt::{Recognised, Recogniser, Unplaced, Write};
use or_project::content::Content;
use or_project::{Contribution, Module, ModuleKind};

use crate::layout::resource_path;
use crate::metadata;
use crate::model::{is_identifier, AddonPeds, Ped, PedFile, FILE};
use crate::plan::{contribution, module_folder};

/// The id of the addon ped content an adoption makes.
const ID: &str = "addon-peds";

/// The name of the data file a resource states its peds in.
const META: &str = "peds.meta";

/// The kind of content that is addon peds.
pub struct AddonPed;

impl Content for AddonPed {
    fn contribution(&self, project_folder: &Path, module: &Module) -> Contribution {
        contribution(project_folder, module)
    }

    fn start(&self, project_folder: &Path, module: &Module, _name: &str) -> Result<(), String> {
        start_module(project_folder, module)
    }
}

impl Recogniser for AddonPed {
    fn kind(&self) -> ModuleKind {
        ModuleKind::AddonPed
    }

    fn holds(&self, folder: &Path) -> bool {
        folder.join(FILE).is_file()
    }

    fn recognise(&self, folder: &Path, files: &[PathBuf]) -> Result<Option<Recognised>, String> {
        // Every file of the folder by the name a plan would give it.
        let held: HashMap<String, &PathBuf> = files
            .iter()
            .filter_map(|file| Some((key(folder, file)?, file)))
            .collect();
        let mut unplaced = Vec::new();
        let mut writes = Vec::new();
        let model = if self.holds(folder) {
            AddonPeds::read(folder).map_err(|e| e.to_string())?
        } else {
            let rels: Vec<String> = files
                .iter()
                .filter_map(|file| relative(folder, file))
                .collect();
            let model = infer(folder, &rels, &mut unplaced);
            if model.peds.is_empty() {
                return Ok(None);
            }
            writes.push(Write {
                path: folder.join(FILE),
                bytes: model.to_json().into_bytes(),
            });
            model
        };

        let mut claimed: Vec<PathBuf> = Vec::new();
        let mut take = |name: &str| {
            if let Some(file) = held.get(&name.to_lowercase()) {
                claimed.push((*file).clone());
            }
        };
        take(FILE);
        for ped in &model.peds {
            for file in &ped.files {
                take(&file.path);
            }
        }
        if let Some(meta) = &model.meta {
            take(meta);
        }
        if let Some(manifest) = held.get("fxmanifest.lua") {
            claimed.push((*manifest).clone());
            unplaced.push(Unplaced {
                path: "fxmanifest.lua".to_string(),
                why: "a build writes its own fxmanifest.lua from the content, so a line here that the content does not state (a script, a file list) is not carried: compare it by hand".to_string(),
            });
        }

        let count: usize = model.peds.iter().map(|p| p.files.len()).sum();
        let names: Vec<&str> = model.peds.iter().map(|p| p.name.as_str()).collect();
        let summary = format!(
            "{} ped(s), {count} file(s): {}",
            model.peds.len(),
            names.join(", ")
        );
        Ok(Some(Recognised {
            module: Module::addon_ped(ID, "."),
            summary,
            claimed,
            unplaced,
            writes,
        }))
    }
}

/// The peds the files of a folder make: each `.yft` that the folder's `peds.meta` defines a ped under, with the files named for it and the folder of its name.
///
/// ⚠ A model is a ped only when the data file names it: a vehicle's or a weapon's is a `.yft` too, and
/// taking it for a ped would move the folder's own manifest out of the Files content.
///
/// `rels` are the files by the path they have inside the folder, in the case they are spelled in; the
/// game does not tell two cases of a name apart, so a ped is found whatever case its files are in.
fn infer(folder: &Path, rels: &[String], unplaced: &mut Vec<Unplaced>) -> AddonPeds {
    let mut model = AddonPeds::new();
    let mut names: Vec<&String> = rels.iter().collect();
    names.sort();
    // The data file: the shallowest `peds.meta` of the folder.
    model.meta = names
        .iter()
        .filter(|n| n.to_lowercase().rsplit('/').next() == Some(META))
        .min_by_key(|n| n.matches('/').count())
        .map(|n| n.to_string());
    let defined = metadata::entries(folder, &model)
        .and_then(Result::ok)
        .unwrap_or_default();
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    for yft in names.iter().filter(|n| n.to_lowercase().ends_with(".yft")) {
        let (dir, leaf) = yft
            .rsplit_once('/')
            .map_or(("", yft.as_str()), |(d, l)| (d, l));
        let stem = leaf[..leaf.len() - ".yft".len()].to_lowercase();
        if !is_identifier(&stem) {
            unplaced.push(Unplaced {
                path: yft.to_string(),
                why: format!(
                    "`{leaf}` is not named like a ped (letters, digits and underscores, from a letter), so it stays a loose file"
                ),
            });
            continue;
        }
        if !defined.contains(&stem) {
            continue;
        }
        let (dir, folder) = (
            dir.to_lowercase(),
            format!(
                "{}{stem}/",
                if dir.is_empty() {
                    String::new()
                } else {
                    format!("{}/", dir.to_lowercase())
                }
            ),
        );
        let beside = |name: &str| {
            let lower = name.to_lowercase();
            let (d, l) = lower
                .rsplit_once('/')
                .map_or(("", lower.as_str()), |(d, l)| (d, l));
            d == dir && l.split_once('.').map_or(l, |(s, _)| s) == stem
        };
        let mut ped = Ped::new(&stem);
        for name in &names {
            let inside = name.to_lowercase().starts_with(&folder);
            if !(beside(name) || inside) || taken.contains(name.as_str()) {
                continue;
            }
            taken.insert(name.as_str());
            let mut file = PedFile {
                path: name.to_string(),
                at: None,
            };
            // The place the file has in a resource already, when it is not the one the layout would give it.
            let default = resource_path(&stem, &file);
            if name.to_lowercase().starts_with("stream/") && !default.eq_ignore_ascii_case(name) {
                file.at = Some(name.to_string());
            }
            ped.files.push(file);
        }
        model.peds.push(ped);
    }
    model
}

/// `stream/A.yft` for a file inside `folder`, in the case it is spelled in.
fn relative(folder: &Path, file: &Path) -> Option<String> {
    let rest = file.strip_prefix(folder).ok()?;
    Some(rest.to_string_lossy().replace('\\', "/"))
}

/// `stream/a.yft` for a file inside `folder`, lower case: a plan's name for it, compared without regard to case.
fn key(folder: &Path, file: &Path) -> Option<String> {
    let rest = file.strip_prefix(folder).ok()?;
    Some(rest.to_string_lossy().replace('\\', "/").to_lowercase())
}

/// Make the folder of the addon ped module `module`: the folder, and an empty `addonped.json` in it unless it has one.
pub fn start_module(project_folder: &Path, module: &Module) -> Result<(), String> {
    let folder = module_folder(project_folder, module).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    if folder.join(FILE).exists() {
        return Ok(());
    }
    AddonPeds::new().write(&folder).map_err(|e| e.to_string())
}

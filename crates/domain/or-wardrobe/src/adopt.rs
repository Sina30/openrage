//! Adopting a folder that holds a clothing pack: what of it this engine knows, and what it could not place.
//!
//! The folder's pack is read where it stands. A `wardrobe.json` already there is the pack; without
//! one the files are imported, and the pack that comes out is a file the adoption asks to add. The
//! files the build writes again, or ships from the pack, are not Files content: carrying them as
//! loose files too would write the metadata of one collection twice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use or_project::adopt::{Recognised, Recogniser, Unplaced, Write};
use or_project::content::Content;
use or_project::{Contribution, Module, ModuleKind, Origin, Output};

use crate::facts::{inspect, Depth};
use crate::import::{self, Options};
use crate::model::{Wardrobe, FILE};
use crate::names;
use crate::plan::{module_folder, plan};

/// The id of the clothing content an adoption makes.
const ID: &str = "clothing";

/// The recogniser of clothing: a folder with a `wardrobe.json`, or one whose files the resolver reads.
pub struct Clothing;

impl Recogniser for Clothing {
    fn kind(&self) -> ModuleKind {
        ModuleKind::Clothing
    }

    fn holds(&self, folder: &Path) -> bool {
        folder.join(FILE).is_file()
    }

    fn recognise(&self, folder: &Path, files: &[PathBuf]) -> Result<Option<Recognised>, String> {
        let existing = self.holds(folder);
        let mut left_out = Vec::new();
        let mut stated = HashSet::new();
        let mut writes = Vec::new();
        let (wardrobe, summary) = if existing {
            let wardrobe = Wardrobe::read(folder).map_err(|e| e.to_string())?;
            let summary = format!("{} garment(s) in the pack there", wardrobe.garments.len());
            (wardrobe, summary)
        } else {
            let imported = match import::import(folder, &Options::default()) {
                Ok(imported) => imported,
                Err(import::Error::Nothing(_)) => return Ok(None),
                Err(e) => return Err(e.to_string()),
            };
            let pack = &imported.wardrobe.pack;
            if !names::is_identifier(pack) {
                return Err(format!(
                    "the pack is called {pack:?}, which is not a lower case name: `openrage-cli wardrobe import --pack <name>` takes one"
                ));
            }
            let report = &imported.report;
            // ⚠ Said only when there is one: "0 places held" is a figure nobody can decode.
            let held = match report.reserved {
                0 => String::new(),
                n => format!(", {n} place(s) held"),
            };
            let summary = format!(
                "{} model(s) and {} texture(s) of the pack `{pack}`{held}",
                report.models, report.textures
            );
            left_out = report.left_out.clone();
            stated = report.metadata.iter().map(|p| p.to_lowercase()).collect();
            writes.push(Write {
                path: folder.join(FILE),
                bytes: imported.wardrobe.to_json().into_bytes(),
            });
            (imported.wardrobe, summary)
        };

        // Every file of the folder by the name a plan would give it.
        let held: HashMap<String, &PathBuf> = files
            .iter()
            .filter_map(|file| Some((key(folder, file)?, file)))
            .collect();
        let mut claimed: Vec<PathBuf> = Vec::new();
        let mut unplaced: Vec<Unplaced> = Vec::new();

        claim(&held, &mut claimed, FILE);
        let planned = plan(
            &wardrobe,
            &inspect(folder, &wardrobe, Depth::Stat),
            ID,
            folder,
        );
        for planned_file in &planned.contribution.files {
            match &planned_file.origin {
                Origin::Copy { from, .. } => {
                    if let Some(name) = key(folder, from) {
                        claim(&held, &mut claimed, &name);
                    }
                }
                Origin::Document(_) | Origin::Generated(_) => {
                    if claim(&held, &mut claimed, &planned_file.path).is_some() {
                        unplaced.push(Unplaced {
                            path: planned_file.path.clone(),
                            why: "the build writes it again from the pack, so this copy is not carried"
                                .into(),
                        });
                    }
                }
                Origin::Retarget { .. } | Origin::Cook { .. } => {}
            }
        }
        if let Some(file) = claim(&held, &mut claimed, "fxmanifest.lua") {
            unplaced.push(Unplaced {
                path: relative(folder, &file),
                why: "a build writes its own fxmanifest.lua from the content, so a line here that the content does not state (a script, a file list, a data file) is not carried: compare it by hand".into(),
            });
        }

        if !existing {
            // What the import read is the pack; what it left out stays a loose file and is said.
            let left: HashSet<String> = left_out.iter().map(|l| l.path.to_lowercase()).collect();
            for file in files {
                let Some(name) = key(folder, file) else {
                    continue;
                };
                let kept_by_the_pack =
                    (name.ends_with(".ydd") || name.ends_with(".ytd")) && !left.contains(&name);
                if kept_by_the_pack || stated.contains(&name) {
                    claimed.push(file.clone());
                }
            }
        }
        let claimed_names: HashSet<String> = claimed
            .iter()
            .filter_map(|file| key(folder, file))
            .collect();
        for l in left_out {
            let why = if claimed_names.contains(&l.path.to_lowercase()) {
                format!(
                    "{}: the build writes the metadata from the pack, so this file is not carried",
                    l.why
                )
            } else {
                format!("{}: it is in the Files content and ships as it is", l.why)
            };
            unplaced.push(Unplaced { path: l.path, why });
        }

        Ok(Some(Recognised {
            module: Module::clothing(ID, "."),
            summary,
            claimed,
            unplaced,
            writes,
        }))
    }
}

/// Take the file named `name` out of the Files content, and give it back.
fn claim(
    held: &HashMap<String, &PathBuf>,
    claimed: &mut Vec<PathBuf>,
    name: &str,
) -> Option<PathBuf> {
    let file = (*held.get(&name.to_lowercase())?).clone();
    claimed.push(file.clone());
    Some(file)
}

/// `stream/a.ydd` for a file inside `folder`, lower case: a plan's name for it, compared without regard to case.
fn key(folder: &Path, file: &Path) -> Option<String> {
    Some(relative_inside(folder, file)?.to_lowercase())
}

fn relative(folder: &Path, file: &Path) -> String {
    relative_inside(folder, file).unwrap_or_else(|| file.display().to_string())
}

fn relative_inside(folder: &Path, file: &Path) -> Option<String> {
    let rest = file.strip_prefix(folder).ok()?;
    Some(rest.to_string_lossy().replace('\\', "/"))
}

/// Make the folder of the clothing content `module`: the folder, and an empty pack called `pack` in it unless it has a pack.
pub fn start_module(project_folder: &Path, module: &Module, pack: &str) -> Result<(), String> {
    if !names::is_identifier(pack) {
        return Err(format!(
            "{pack:?} is not a name for a pack: it is lower case letters, digits and underscores, from a letter"
        ));
    }
    let folder = module_folder(project_folder, module).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    if folder.join(FILE).exists() {
        return Ok(());
    }
    Wardrobe::new(pack)
        .write(&folder)
        .map_err(|e| e.to_string())
}

impl Content for Clothing {
    fn contribution(&self, project_folder: &Path, module: &Module) -> Contribution {
        crate::plan::contribution(project_folder, module)
    }

    fn contribution_for(
        &self,
        project_folder: &Path,
        module: &Module,
        output: &Output,
    ) -> Contribution {
        crate::plan::contribution_for(project_folder, module, output)
    }

    fn start(&self, project_folder: &Path, module: &Module, name: &str) -> Result<(), String> {
        start_module(project_folder, module, name)
    }
}

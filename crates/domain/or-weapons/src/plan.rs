//! The planner: what a module of weapons hands to the plan of a build.
//!
//! [`plan`] is a pure function of the model and what the disk said of its sources. [`contribution`] reads both for a module of a
//! project and hands the result over as the [`Contribution`] the plan merges, so weapons are one more kind of content going through
//! the one path: the sources cook, and a data source ships at [`crate::layout::DATA_FOLDER`] with the line that registers it.

use std::path::{Path, PathBuf};

use or_project::{
    Claim, Container, Contribution, Figure, Item, ManifestEntry, Module, ModuleKind, Origin,
    PlannedFile,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::facts::{inspect, Depth, Facts, SourceFact};
use crate::layout::{data_path, stream_path};
use crate::model::{self, Weapon, Weapons};
use crate::rules::{check, Finding};

/// The kind of name the game resolves once that a weapon's is.
pub const WEAPON_NAME: &str = "weapon name";

/// A module planned.
#[derive(Debug, Clone)]
pub struct Planned {
    pub contribution: Contribution,
    pub findings: Vec<Finding>,
}

/// The sentence a finding that stops a build becomes in a plan's problems.
pub fn problem(finding: &Finding) -> String {
    format!("{}: {}", finding.code, finding.message)
}

/// Where the source `source` lands inside the resource, with the type it is registered under when it is a data file.
pub fn place_of(source: &SourceFact) -> (String, Option<&'static str>) {
    if source.is_metadata() {
        let registered = match &source.defined {
            Some(Ok(defined)) => defined.data_file_type(),
            _ => None,
        };
        (data_path(&source.file_name()), registered)
    } else {
        (stream_path(&source.file_name()), None)
    }
}

/// What `model` hands to the plan of module `module`, whose folder is `folder`, given what the disk said of its sources.
pub fn plan(model: &Weapons, facts: &Facts, module: &str, folder: &Path) -> Planned {
    let findings = check(model, facts);
    let mut contribution = Contribution::of(module);
    contribution.problems = findings
        .iter()
        .filter(|f| f.blocks())
        .map(problem)
        .collect();

    let mut shipped: Vec<&str> = Vec::new();
    let mut sources = 0u64;
    for weapon in &model.weapons {
        for path in &weapon.sources {
            let Some(source) = facts.source(path) else {
                continue;
            };
            // A source several weapons name ships once: a file of metadata defines many weapons.
            if shipped.contains(&path.as_str()) {
                continue;
            }
            shipped.push(path);
            sources += 1;
            let (at, registered) = place_of(source);
            contribution.files.push(PlannedFile {
                path: at.clone(),
                origin: Origin::Cook {
                    source: folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                    kind: source.kind.clone(),
                    name: source.name.clone(),
                    bytes: source.bytes,
                    game: source.game.clone(),
                },
            });
            match (source.is_metadata(), registered) {
                (true, Some(file_type)) => contribution.manifest.push(ManifestEntry {
                    key: "data_file".to_string(),
                    values: vec![file_type.to_string(), at],
                }),
                (true, None) => contribution.notes.push(format!(
                    "`{path}` is metadata of a kind this content has no type to register, so the game is not told to read it"
                )),
                (false, _) => {}
            }
        }
        if !weapon.name.trim().is_empty() {
            contribution.claims.push(Claim {
                kind: WEAPON_NAME.to_string(),
                name: weapon.name.trim().to_string(),
            });
        }
        contribution.items.push(item(weapon, facts));
    }
    contribution.figures = vec![
        Figure {
            name: "weapons".to_string(),
            amount: model.weapons.len() as u64,
        },
        Figure {
            name: "weapon_sources".to_string(),
            amount: sources,
        },
    ];
    Planned {
        contribution,
        findings,
    }
}

/// The item of a weapon: its id, the name the game sees and a digest of everything it ships.
fn item(weapon: &Weapon, facts: &Facts) -> Item {
    let sources: Vec<_> = weapon
        .sources
        .iter()
        .map(|path| match facts.source(path) {
            Some(source) => {
                let (at, _) = place_of(source);
                json!({ "at": at, "content": source.digest.clone().unwrap_or_default() })
            }
            None => json!({ "at": path, "content": "" }),
        })
        .collect();
    let text = serde_json::to_string(&json!({ "name": weapon.name.trim(), "sources": sources }))
        .expect("a JSON value serialises");
    Item {
        id: weapon.id.clone(),
        name: weapon.name.trim().to_string(),
        digest: digest_of(Sha256::digest(text.as_bytes()).as_slice()),
    }
}

fn digest_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Model(#[from] model::Error),
    #[error("the folder of content `{module}` is {why}")]
    Folder { module: String, why: String },
    #[error("{0}")]
    Linked(String),
}

/// The folder a weapons module's content is in: inside `project_folder`, or where its link leads.
pub fn module_folder(project_folder: &Path, module: &Module) -> Result<PathBuf, Error> {
    use or_project::content::folder::{module_folder, FolderError};
    module_folder(project_folder, module, "a weapons module").map_err(|e| match e {
        FolderError::Folder { module, why } => Error::Folder { module, why },
        FolderError::Linked(why) => Error::Linked(why),
    })
}

/// The folder a weapons module is edited in: [`module_folder`], except that a link is refused.
///
/// ⚠ The project that holds a linked folder is the only one that writes it.
pub fn module_folder_to_edit(
    project: &Path,
    container: &Container,
    module: &Module,
) -> Result<PathBuf, Error> {
    let folder = module_folder(&or_project::project_folder(project), module)?;
    or_project::links::refuse_write(container, project, &folder.join(model::FILE))
        .map_err(Error::Linked)?;
    Ok(folder)
}

/// A module read from its folder in the project in `project_folder`, and planned.
pub fn planned(
    project_folder: &Path,
    module: &Module,
    depth: Depth,
) -> Result<(Weapons, Facts, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let model = Weapons::read(&folder)?;
    let facts = inspect(&folder, &model, depth);
    let planned = plan(&model, &facts, &module.id, &folder);
    Ok((model, facts, planned))
}

/// What module `module` hands over; a module whose folder or `weapons.json` cannot be read hands over the reason.
pub fn contribution(project_folder: &Path, module: &Module) -> Contribution {
    match planned(project_folder, module, Depth::Open) {
        Ok((.., planned)) => planned.contribution,
        Err(e) => {
            let mut contribution = Contribution::of(&module.id);
            contribution.problems.push(e.to_string());
            contribution
        }
    }
}

/// What every weapons module of `container` hands over: all of them, because the names the game resolves once are one namespace for the whole project.
pub fn contributions(container: &Container, project_folder: &Path) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Weapons)
        .map(|m| contribution(project_folder, m))
        .collect()
}

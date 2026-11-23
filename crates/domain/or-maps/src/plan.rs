//! The planner: what a module of map sets hands to the plan of a build.
//!
//! [`plan`] is a pure function of the model and what the disk said of its sources. [`contribution`] reads both for a module of a
//! project and hands the result over as the [`Contribution`] the plan merges, so maps are one more kind of content going through the
//! one path: each source cooks to `stream/<name>.<extension>`, and a type file is registered by the line that tells the game to read it.

use std::path::{Path, PathBuf};

use or_project::{
    Claim, Container, Contribution, Figure, Item, ManifestEntry, Module, ModuleKind, Origin,
    PlannedFile,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::facts::{inspect, Depth, Facts, Role, SourceFact};
use crate::layout::{stream_path, TYPE_FILE};
use crate::model::{self, MapSet, Maps};
use crate::rules::{check, Finding, Known};

/// The kind of name the game resolves once that an archetype's is.
pub const ARCHETYPE_NAME: &str = "archetype name";
/// The kind of name the game resolves once that a map file's is.
pub const MAP_FILE: &str = "map file";

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

/// Where the source `source` lands inside the resource.
pub fn place_of(source: &SourceFact) -> String {
    stream_path(&source.file_name())
}

/// What `model` hands to the plan of module `module`, whose folder is `folder`, given what the disk said of its sources.
pub fn plan(model: &Maps, facts: &Facts, module: &str, folder: &Path, known: Known<'_>) -> Planned {
    let findings = check(model, facts, known);
    let mut contribution = Contribution::of(module);
    contribution.problems = findings
        .iter()
        .filter(|f| f.blocks())
        .map(problem)
        .collect();

    let mut shipped: Vec<&str> = Vec::new();
    let mut archetypes_claimed: Vec<u32> = Vec::new();
    let (mut entities, mut archetypes, mut interiors) = (0u64, 0u64, 0u64);
    let mut maps = 0u64;
    for set in &model.maps {
        for path in &set.sources {
            let Some(source) = facts.source(path) else {
                continue;
            };
            // A source several sets name ships once.
            if shipped.contains(&path.as_str()) {
                continue;
            }
            shipped.push(path);
            let at = place_of(source);
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
            match source.role() {
                Some(Role::Types) => {
                    contribution.manifest.push(ManifestEntry {
                        key: "data_file".to_string(),
                        values: vec![TYPE_FILE.to_string(), at],
                    });
                    if let Some(types) = source.types() {
                        for archetype in &types.archetypes {
                            archetypes += 1;
                            interiors += u64::from(archetype.interior.is_some());
                            if !archetypes_claimed.contains(&archetype.name.hash) {
                                archetypes_claimed.push(archetype.name.hash);
                                contribution.claims.push(Claim {
                                    kind: ARCHETYPE_NAME.to_string(),
                                    name: archetype.name.shown(),
                                });
                            }
                        }
                    }
                }
                Some(Role::Map) => {
                    maps += 1;
                    entities += source.map().map_or(0, |m| m.entities.len() as u64);
                    contribution.claims.push(Claim {
                        kind: MAP_FILE.to_string(),
                        name: source.name.clone(),
                    });
                }
                _ => {}
            }
        }
        contribution.items.push(item(set, facts));
    }
    contribution.figures = vec![
        Figure {
            name: "map_sets".to_string(),
            amount: model.maps.len() as u64,
        },
        Figure {
            name: "map_files".to_string(),
            amount: maps,
        },
        Figure {
            name: "entities".to_string(),
            amount: entities,
        },
        Figure {
            name: "archetypes".to_string(),
            amount: archetypes,
        },
        Figure {
            name: "interiors".to_string(),
            amount: interiors,
        },
        Figure {
            name: "map_sources".to_string(),
            amount: shipped.len() as u64,
        },
    ];
    Planned {
        contribution,
        findings,
    }
}

/// The item of a set: its id, its name and a digest of everything it ships.
fn item(set: &MapSet, facts: &Facts) -> Item {
    let sources: Vec<_> = set
        .sources
        .iter()
        .map(|path| match facts.source(path) {
            Some(source) => {
                json!({ "at": place_of(source), "content": source.digest.clone().unwrap_or_default() })
            }
            None => json!({ "at": path, "content": "" }),
        })
        .collect();
    let text = serde_json::to_string(&json!({ "name": set.name.trim(), "sources": sources }))
        .expect("a JSON value serialises");
    Item {
        id: set.id.clone(),
        name: set.name.trim().to_string(),
        digest: Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
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

/// The folder a maps module's content is in: inside `project_folder`, or where its link leads.
pub fn module_folder(project_folder: &Path, module: &Module) -> Result<PathBuf, Error> {
    use or_project::content::folder::{module_folder, FolderError};
    module_folder(project_folder, module, "a maps module").map_err(|e| match e {
        FolderError::Folder { module, why } => Error::Folder { module, why },
        FolderError::Linked(why) => Error::Linked(why),
    })
}

/// The folder a maps module is edited in: [`module_folder`], except that a link is refused.
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
    known: Known<'_>,
) -> Result<(Maps, Facts, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let model = Maps::read(&folder)?;
    let facts = inspect(&folder, &model, depth);
    let planned = plan(&model, &facts, &module.id, &folder, known);
    Ok((model, facts, planned))
}

/// What module `module` hands over; a module whose folder or `maps.json` cannot be read hands over the reason.
pub fn contribution(project_folder: &Path, module: &Module) -> Contribution {
    match planned(project_folder, module, Depth::Open, None) {
        Ok((.., planned)) => planned.contribution,
        Err(e) => {
            let mut contribution = Contribution::of(&module.id);
            contribution.problems.push(e.to_string());
            contribution
        }
    }
}

/// What every maps module of `container` hands over.
pub fn contributions(container: &Container, project_folder: &Path) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Maps)
        .map(|m| contribution(project_folder, m))
        .collect()
}

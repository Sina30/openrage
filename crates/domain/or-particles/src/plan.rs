//! The planner: what a module of particle assets hands to the plan of a build.
//!
//! [`plan`] is a pure function of the model and what the disk said of its sources. [`contribution`] reads both for a module of a
//! project and hands the result over as the [`Contribution`] the plan merges, so particles are one more kind of content going through
//! the one path: each source cooks to `stream/<name>.ypt`.

use std::path::{Path, PathBuf};

use or_project::{Container, Contribution, Figure, Item, Module, ModuleKind, Origin, PlannedFile};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::facts::{inspect, Depth, Facts, SourceFact};
use crate::layout::stream_path;
use crate::model::{self, Asset, Particles};
use crate::rules::{check, Finding};

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
pub fn plan(model: &Particles, facts: &Facts, module: &str, folder: &Path) -> Planned {
    let findings = check(model, facts);
    let mut contribution = Contribution::of(module);
    contribution.problems = findings
        .iter()
        .filter(|f| f.blocks())
        .map(problem)
        .collect();

    let mut shipped: Vec<&str> = Vec::new();
    let mut effects = 0u64;
    for asset in &model.assets {
        let path = asset.source.as_str();
        let Some(source) = facts.source(path) else {
            continue;
        };
        // A source several assets name ships once.
        if shipped.contains(&path) {
            continue;
        }
        shipped.push(path);
        effects += source
            .inside
            .as_ref()
            .and_then(|i| i.as_ref().ok())
            .map_or(0, |i| i.effects.len() as u64);
        contribution.files.push(PlannedFile {
            path: place_of(source),
            origin: Origin::Cook {
                source: folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                kind: source.kind.clone(),
                name: source.name.clone(),
                bytes: source.bytes,
                game: source.game.clone(),
            },
        });
        contribution.items.push(item(asset, source));
    }
    contribution.figures = vec![
        Figure {
            name: "particle_files".to_string(),
            amount: shipped.len() as u64,
        },
        Figure {
            name: "effects".to_string(),
            amount: effects,
        },
    ];
    Planned {
        contribution,
        findings,
    }
}

/// The item of an asset: its id, the name of the file the game sees and a digest of everything it ships.
fn item(asset: &Asset, source: &SourceFact) -> Item {
    let text = serde_json::to_string(&json!({
        "at": place_of(source),
        "content": source.digest.clone().unwrap_or_default(),
    }))
    .expect("a JSON value serialises");
    Item {
        id: asset.id.clone(),
        name: source.file_name(),
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

/// The folder a particles module's content is in: inside `project_folder`, or where its link leads.
pub fn module_folder(project_folder: &Path, module: &Module) -> Result<PathBuf, Error> {
    use or_project::content::folder::{module_folder, FolderError};
    module_folder(project_folder, module, "a particles module").map_err(|e| match e {
        FolderError::Folder { module, why } => Error::Folder { module, why },
        FolderError::Linked(why) => Error::Linked(why),
    })
}

/// The folder a particles module is edited in: [`module_folder`], except that a link is refused.
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
) -> Result<(Particles, Facts, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let model = Particles::read(&folder)?;
    let facts = inspect(&folder, &model, depth);
    let planned = plan(&model, &facts, &module.id, &folder);
    Ok((model, facts, planned))
}

/// What module `module` hands over; a module whose folder or `effects.json` cannot be read hands over the reason.
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

/// What every particles module of `container` hands over.
pub fn contributions(container: &Container, project_folder: &Path) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Particles)
        .map(|m| contribution(project_folder, m))
        .collect()
}

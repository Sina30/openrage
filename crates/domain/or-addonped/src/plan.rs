//! The planner: what a module of addon peds hands to the plan of a build.
//!
//! [`plan`] is a pure function of the model and what the disk said of its files. [`contribution`]
//! reads both for a module of a project and hands the result over as the [`Contribution`] the plan
//! merges, so addon peds are one more kind of content going through the one path.

use std::path::{Path, PathBuf};

use or_project::{
    Claim, Container, Contribution, Figure, Item, ManifestEntry, Module, ModuleKind, Origin,
    PlannedFile,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::facts::{inspect, Depth, Fact, Facts};
use crate::layout::{meta_path, resource_path};
use crate::metadata::{self, Entries};
use crate::model::{self, AddonPeds, Ped};
use crate::rules::{check, Finding};

/// The kind of name the game resolves once that a ped's is.
pub const PED_MODEL: &str = "ped model";

/// The type the game is told the data file is.
pub const DATA_FILE_TYPE: &str = "PED_METADATA_FILE";

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

/// What `model` hands to the plan of module `module`, whose folder is `folder`, given what the disk said of its files.
pub fn plan(
    model: &AddonPeds,
    facts: &Facts,
    entries: Option<&Entries>,
    module: &str,
    folder: &Path,
) -> Planned {
    let findings = check(model, facts, entries);
    let mut contribution = Contribution::of(module);
    contribution.problems = findings
        .iter()
        .filter(|f| f.blocks())
        .map(problem)
        .collect();

    let mut ped_files = 0u64;
    for ped in &model.peds {
        for file in &ped.files {
            ped_files += 1;
            if let Some(Fact::File { bytes, .. }) = facts.get(&file.path) {
                contribution.files.push(PlannedFile {
                    path: resource_path(&ped.name, file),
                    origin: Origin::Copy {
                        from: folder.join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR)),
                        bytes: *bytes,
                    },
                });
            }
        }
        contribution.claims.push(Claim {
            kind: PED_MODEL.to_string(),
            name: ped.name.clone(),
        });
        contribution.items.push(item(ped, facts));
    }
    if let Some(meta) = &model.meta {
        if let Some(Fact::File { bytes, .. }) = facts.get(meta) {
            let at = meta_path(meta);
            contribution.files.push(PlannedFile {
                path: at.clone(),
                origin: Origin::Copy {
                    from: folder.join(meta.replace('/', std::path::MAIN_SEPARATOR_STR)),
                    bytes: *bytes,
                },
            });
            contribution.manifest.push(ManifestEntry {
                key: "data_file".to_string(),
                values: vec![DATA_FILE_TYPE.to_string(), at],
            });
        }
    }
    contribution.figures = vec![
        Figure {
            name: "addon_peds".to_string(),
            amount: model.peds.len() as u64,
        },
        Figure {
            name: "addon_ped_files".to_string(),
            amount: ped_files,
        },
    ];
    Planned {
        contribution,
        findings,
    }
}

/// The item of a ped: its id, the name the game sees and a digest of everything it ships.
fn item(ped: &Ped, facts: &Facts) -> Item {
    let files: Vec<_> = ped
        .files
        .iter()
        .map(|f| {
            let content = match facts.get(&f.path) {
                Some(Fact::File {
                    digest: Some(d), ..
                }) => d.clone(),
                _ => String::new(),
            };
            json!({ "at": resource_path(&ped.name, f), "content": content })
        })
        .collect();
    let text = serde_json::to_string(&json!({ "name": ped.name, "files": files }))
        .expect("a JSON value serialises");
    Item {
        id: ped.id.clone(),
        name: ped.name.clone(),
        digest: crate::facts::digest(Sha256::digest(text.as_bytes()).as_slice()),
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

/// The folder an addon ped module's content is in: inside `project_folder`, or where its link leads.
pub fn module_folder(project_folder: &Path, module: &Module) -> Result<PathBuf, Error> {
    use or_project::content::folder::{module_folder, FolderError};
    module_folder(project_folder, module, "an addon ped module").map_err(|e| match e {
        FolderError::Folder { module, why } => Error::Folder { module, why },
        FolderError::Linked(why) => Error::Linked(why),
    })
}

/// The folder an addon ped module is edited in: [`module_folder`], except that a link is refused.
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
) -> Result<(AddonPeds, Planned), Error> {
    let folder = module_folder(project_folder, module)?;
    let model = AddonPeds::read(&folder)?;
    let facts = inspect(&folder, &model, depth);
    let entries = metadata::entries(&folder, &model);
    let planned = plan(&model, &facts, entries.as_ref(), &module.id, &folder);
    Ok((model, planned))
}

/// What module `module` hands over; a module whose folder or `addonped.json` cannot be read hands over the reason.
pub fn contribution(project_folder: &Path, module: &Module) -> Contribution {
    match planned(project_folder, module, Depth::Digest) {
        Ok((_, planned)) => planned.contribution,
        Err(e) => {
            let mut contribution = Contribution::of(&module.id);
            contribution.problems.push(e.to_string());
            contribution
        }
    }
}

/// What every addon ped module of `container` hands over: all of them, because the names the game resolves once are one namespace for the whole project.
pub fn contributions(container: &Container, project_folder: &Path) -> Vec<Contribution> {
    container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::AddonPed)
        .map(|m| contribution(project_folder, m))
        .collect()
}

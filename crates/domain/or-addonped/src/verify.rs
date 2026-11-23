//! The verifier: what a built resource has to hold for the peds that were planned, read back.
//!
//! A build already says that every file it wrote is the plan's and reloads through a reader. This says
//! what only a module of peds knows: that each ped's model streams under the ped's name, that the data
//! file is there and defines every ped, and that the manifest registers it.
//!
//! ⚠ It checks the files and not the game. Whether GTA V loads a ped is a thing nobody working on
//! this repository can run.

use std::path::Path;

use or_io::meta::MetaDocument;

use crate::layout::{meta_path, resource_path};
use crate::metadata::names_in;
use crate::model::AddonPeds;
use crate::plan::DATA_FILE_TYPE;

pub use or_project::content::check::{Check, Verification};

/// Read `resource`, the folder a build of `model` made, against `model`.
pub fn verify(model: &AddonPeds, resource: &Path) -> Verification {
    let mut found = Verification::default();
    for ped in &model.peds {
        let name = format!("model of {}", ped.name);
        let wanted: Vec<String> = ped
            .files
            .iter()
            .filter(|f| f.path.to_ascii_lowercase().ends_with(".yft"))
            .map(|f| resource_path(&ped.name, f))
            .collect();
        let mut problems = Vec::new();
        for place in &wanted {
            if !resource.join(place).is_file() {
                problems.push(format!("`{place}` is not in the resource"));
            }
        }
        if wanted.is_empty() {
            problems.push("the ped lists no .yft".to_string());
        }
        let held = format!("{} streams as `{}`", ped.name, wanted.join("`, `"));
        found.add(name, problems, held);
    }
    if let Some(meta) = &model.meta {
        data_file(&mut found, model, meta, resource);
    }
    found
}

fn data_file(found: &mut Verification, model: &AddonPeds, meta: &str, resource: &Path) {
    let at = meta_path(meta);
    let name = "data file".to_string();
    let bytes = match std::fs::read(resource.join(&at)) {
        Ok(bytes) => bytes,
        Err(e) => {
            return found.add(
                name,
                vec![format!("`{at}` cannot be read: {e}")],
                String::new(),
            )
        }
    };
    let mut problems = Vec::new();
    if let Err(e) = MetaDocument::parse(or_io::meta::default_library(), bytes.clone()) {
        problems.push(format!("`{at}` does not reload: {e}"));
    }
    let names = names_in(&String::from_utf8_lossy(&bytes));
    for ped in &model.peds {
        if !names.contains(&ped.name.to_lowercase()) {
            problems.push(format!("`{at}` has no entry named `{}`", ped.name));
        }
    }
    let manifest = std::fs::read_to_string(resource.join("fxmanifest.lua")).unwrap_or_default();
    let registered = format!("data_file '{DATA_FILE_TYPE}' '{at}'");
    if !manifest.contains(&registered) {
        problems.push(format!("the manifest does not state `{registered}`"));
    }
    found.add(
        name,
        problems,
        format!(
            "`{at}` defines {} ped(s) and the manifest registers it as {DATA_FILE_TYPE}",
            model.peds.len()
        ),
    );
}

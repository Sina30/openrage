//! The verifier: what a built resource has to hold for the weapons that were planned, read back.
//!
//! A build already says that every file it wrote is the plan's and reloads through a reader. This says what only a module of weapons
//! knows: that each source reached the place the plan gave it, that each data file reloads as the root it was registered as and
//! defines the weapons of the model, and that the manifest registers it.
//!
//! ⚠ It checks the files and not the game. Whether GTA V loads a weapon is a thing nobody working on this repository can run.

use std::path::Path;

use crate::facts::Facts;
use crate::metadata;
use crate::model::Weapons;
use crate::plan::place_of;

pub use or_project::content::check::{Check, Verification};

/// Read `resource`, the folder a build of `model` made, against `model` whose sources `facts` looked at.
pub fn verify(model: &Weapons, facts: &Facts, resource: &Path) -> Verification {
    let mut found = Verification::default();
    let manifest = std::fs::read_to_string(resource.join("fxmanifest.lua")).unwrap_or_default();
    let mut seen: Vec<&str> = Vec::new();
    for weapon in &model.weapons {
        let mut problems = Vec::new();
        let mut shipped = Vec::new();
        for path in &weapon.sources {
            let Some(source) = facts.source(path) else {
                continue;
            };
            let (at, registered) = place_of(source);
            // A source of an unchanged original is left out by a build that is told to, and a data file defined in it is the game's.
            if !resource.join(&at).is_file() {
                if !source.game.is_empty() {
                    continue;
                }
                problems.push(format!("`{at}` is not in the resource"));
                continue;
            }
            shipped.push(at.clone());
            if !source.is_metadata() || seen.contains(&path.as_str()) {
                continue;
            }
            seen.push(path);
            problems.extend(data_file(resource, &at, registered, &manifest));
        }
        let name = format!("weapon {}", weapon.name.trim());
        let held = format!(
            "{} is shipped as `{}`",
            weapon.name.trim(),
            shipped.join("`, `")
        );
        found.add(name, problems, held);
    }
    // Every weapon of the model is defined by a file of the resource.
    let mut defined: Vec<String> = Vec::new();
    for weapon in &model.weapons {
        for path in &weapon.sources {
            if let Some(source) = facts.source(path).filter(|s| s.is_metadata()) {
                let (at, _) = place_of(source);
                if let Ok(bytes) = std::fs::read(resource.join(&at)) {
                    if let Ok(read) = metadata::read(&bytes) {
                        defined.extend(
                            read.weapons
                                .into_iter()
                                .map(|w| w.name.to_ascii_lowercase()),
                        );
                    }
                }
            }
        }
    }
    let lacking: Vec<String> = model
        .weapons
        .iter()
        .filter(|w| {
            let has_metadata = w.sources.iter().any(|p| {
                facts
                    .source(p)
                    .is_some_and(|s| s.is_metadata() && s.game.is_empty())
            });
            has_metadata && !defined.contains(&w.name.trim().to_ascii_lowercase())
        })
        .map(|w| {
            format!(
                "`{}` is defined by no data file of the resource",
                w.name.trim()
            )
        })
        .collect();
    found.add(
        "weapons defined",
        lacking,
        format!(
            "{} weapon(s) are defined by the data files of the resource",
            model.weapons.len()
        ),
    );
    found
}

/// What is wrong with the data file at `at` of `resource`: it does not reload as the root it is registered as, or the manifest does not register it.
fn data_file(resource: &Path, at: &str, registered: Option<&str>, manifest: &str) -> Vec<String> {
    let bytes = match std::fs::read(resource.join(at)) {
        Ok(bytes) => bytes,
        Err(e) => return vec![format!("`{at}` cannot be read: {e}")],
    };
    let mut problems = Vec::new();
    match or_io::meta::typed::weapon::reload(&bytes) {
        Some(Ok(_)) => {}
        Some(Err(why)) => problems.push(format!("`{at}` does not reload: {why}")),
        None => {
            // A document of a root this content has no reader for is shipped as it is.
        }
    }
    if let Some(file_type) = registered {
        let line = format!("data_file '{file_type}' '{at}'");
        if !manifest.contains(&line) {
            problems.push(format!("the manifest does not state `{line}`"));
        }
    }
    problems
}

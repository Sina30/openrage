//! The verifier: what a built resource has to hold for the map sets that were planned, read back.
//!
//! A build already says that every file it wrote is the plan's and reloads through a reader. This says what only a module of maps
//! knows: that each source reached the place the plan gave it, that each map file and type file reloads as the root it is and holds
//! what its source states, and that the manifest registers each type file.
//!
//! ⚠ It checks the files and not the game. Whether GTA V streams a map is a thing nobody working on this repository can run.

use std::path::Path;

use or_io::meta::typed::{CMapData, CMapTypes};

use crate::facts::{Facts, Role, SourceFact};
use crate::layout::TYPE_FILE;
use crate::model::Maps;
use crate::plan::place_of;

pub use or_project::content::check::{Check, Verification};

/// Read `resource`, the folder a build of `model` made, against `model` whose sources `facts` looked at.
pub fn verify(model: &Maps, facts: &Facts, resource: &Path) -> Verification {
    let mut found = Verification::default();
    let manifest = std::fs::read_to_string(resource.join("fxmanifest.lua")).unwrap_or_default();
    let mut seen: Vec<&str> = Vec::new();
    for set in &model.maps {
        let mut problems = Vec::new();
        let mut shipped = Vec::new();
        for path in &set.sources {
            let Some(source) = facts.source(path) else {
                continue;
            };
            let at = place_of(source);
            // A source of an unchanged original is left out by a build that is told to.
            if !resource.join(&at).is_file() {
                if !source.game.is_empty() {
                    continue;
                }
                problems.push(format!("`{at}` is not in the resource"));
                continue;
            }
            shipped.push(at.clone());
            if seen.contains(&path.as_str()) {
                continue;
            }
            seen.push(path);
            problems.extend(file(resource, &at, source, &manifest));
        }
        found.add(
            format!("map {}", set.name.trim()),
            problems,
            format!(
                "{} is shipped as `{}`",
                set.name.trim(),
                shipped.join("`, `")
            ),
        );
    }
    found
}

/// What is wrong with the file at `at` of `resource`: it does not reload as the root it is, holds other than its source states, or is a type file the manifest does not register.
fn file(resource: &Path, at: &str, source: &SourceFact, manifest: &str) -> Vec<String> {
    let bytes = match std::fs::read(resource.join(at)) {
        Ok(bytes) => bytes,
        Err(e) => return vec![format!("`{at}` cannot be read: {e}")],
    };
    let mut problems = Vec::new();
    // A model and a texture dictionary are reloaded by the build itself, through the readers of their formats.
    if !matches!(source.role(), Some(Role::Map | Role::Types)) {
        return problems;
    }
    match reload(source, &bytes) {
        Ok(held) => {
            let stated = match (source.map(), source.types()) {
                (Some(map), _) => Some(map.entities.len()),
                (_, Some(types)) => Some(types.archetypes.len()),
                _ => None,
            };
            if stated.is_some_and(|stated| stated != held) {
                problems.push(format!(
                    "`{at}` reloads with {held} entries and its source states {}",
                    stated.unwrap_or_default()
                ));
            }
        }
        Err(why) => problems.push(format!("`{at}` does not reload: {why}")),
    }
    if source.role() == Some(Role::Types) {
        let line = format!("data_file '{TYPE_FILE}' '{at}'");
        if !manifest.contains(&line) {
            problems.push(format!("the manifest does not state `{line}`"));
        }
    }
    problems
}

/// Open `bytes` as the typed root the source's file is, and say how many entities or archetypes it holds.
fn reload(source: &SourceFact, bytes: &[u8]) -> Result<usize, String> {
    use or_io::decl::File;
    match source.role() {
        Some(Role::Map) => CMapData::from_bytes(bytes)
            .map(|map| map.entities.len())
            .map_err(|e| e.to_string()),
        _ => CMapTypes::from_bytes(bytes)
            .map(|types| types.archetypes.len())
            .map_err(|e| e.to_string()),
    }
}

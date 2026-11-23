//! The verifier: what a built resource has to hold for the particle assets that were planned, read back.
//!
//! A build already says that every file it wrote is the plan's and reloads through a reader. This says what only a module of particles
//! knows: that each source reached the place the plan gave it and reloads as a particle list that holds the effects its source states.
//!
//! ⚠ It checks the files and not the game. Whether GTA V plays an effect is a thing nobody working on this repository can run.

use std::path::Path;

use crate::facts::Facts;
use crate::model::Particles;
use crate::plan::place_of;

pub use or_project::content::check::{Check, Verification};

/// Read `resource`, the folder a build of `model` made, against `model` whose sources `facts` looked at.
pub fn verify(model: &Particles, facts: &Facts, resource: &Path) -> Verification {
    let mut found = Verification::default();
    let mut seen: Vec<&str> = Vec::new();
    for asset in &model.assets {
        let path = asset.source.as_str();
        let Some(source) = facts.source(path) else {
            continue;
        };
        if seen.contains(&path) {
            continue;
        }
        seen.push(path);
        let at = place_of(source);
        let mut problems = Vec::new();
        let file = resource.join(&at);
        if !file.is_file() {
            // A source of an unchanged original is left out by a build that is told to.
            if !source.game.is_empty() {
                continue;
            }
            problems.push(format!("`{at}` is not in the resource"));
        } else {
            match std::fs::read(&file) {
                Err(e) => problems.push(format!("`{at}` cannot be read: {e}")),
                Ok(bytes) => match or_io::gta5::formats::format("ypt") {
                    Some(format) => {
                        if let Err(why) = format.open_declared(&bytes) {
                            problems.push(format!("`{at}` does not reload: {why}"));
                        }
                    }
                    None => problems.push(format!("`{at}` has no reader to try")),
                },
            }
        }
        let held = format!("`{at}` reloads as a particle list");
        found.add(
            format!("particle file {}", source.file_name()),
            problems,
            held,
        );
    }
    found
}

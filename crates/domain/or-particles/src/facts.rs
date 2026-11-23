//! What the disk says of the sources a module names, gathered once so that every rule and the plan are pure functions of it.
//!
//! An asset names a source folder of its module. For each one this looks at the folder as a source does (`or_source::cook::describe`:
//! what it cooks to, how big its inputs are, whether it is the game's own file), and from `Depth::Open` on it reads the rules the
//! source states, so that a rule can say which effects it holds and what they name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub use or_project::content::facts::{stays_inside, Depth};
use serde_json::Value;

use crate::model::Particles;

/// What is known of one source an asset names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    /// The name leaves the module's folder, or is no name a host can create.
    Escapes,
    Missing,
    /// There is something there and it is not a source this build can cook, with the reason.
    Unreadable(String),
    Source(Box<SourceFact>),
}

/// A source folder as a build sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFact {
    /// The word that chooses the compiler: `particles` for a source of this content.
    pub kind: String,
    /// The asset's own name, which names the cooked file.
    pub name: String,
    /// The game extension of the cooked file.
    pub extension: String,
    /// The size of every file the cook reads.
    pub bytes: u64,
    /// The SHA-256 of the game files the source was taken from, which a build that leaves out an unchanged original compares with.
    pub game: Vec<String>,
    /// A digest of every file in the folder, from [`Depth::Digest`] on.
    pub digest: Option<String>,
    /// What the rules of the source state, or why they cannot be read; from [`Depth::Open`] on.
    pub inside: Option<Result<Inside, String>>,
}

impl SourceFact {
    /// The file the source cooks to, such as `fx_smoke.ypt`.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.name, self.extension)
    }

    pub fn is_particles(&self) -> bool {
        self.kind == or_source::particles::KIND
    }
}

/// What the document of rules of a source states, counted and cross-read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inside {
    /// The names of its effects, in the order of the document.
    pub effects: Vec<String>,
    pub emitter_rules: usize,
    pub particle_rules: usize,
    /// An event or a spawner naming a rule the list does not hold, in a sentence each.
    pub unresolved: Vec<String>,
    pub textures: usize,
    /// A particle rule drawing a texture of its own list that the source does not hold, in a sentence each.
    pub textures_missing: Vec<String>,
    /// Whether its particles draw models, kept whole beside the rules.
    pub models: bool,
}

/// The sources of a module by the path it names them with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    sources: BTreeMap<String, Fact>,
    /// The source folders of the module's own folder that no asset names.
    pub unused: Vec<String>,
}

impl Facts {
    pub fn get(&self, path: &str) -> Option<&Fact> {
        self.sources.get(path)
    }

    pub fn insert(&mut self, path: &str, fact: Fact) {
        self.sources.insert(path.to_string(), fact);
    }

    /// The source `path` names, none when it is missing or not one.
    pub fn source(&self, path: &str) -> Option<&SourceFact> {
        match self.get(path) {
            Some(Fact::Source(source)) => Some(source),
            _ => None,
        }
    }
}

/// Every source a module names, once each.
pub fn named_paths(model: &Particles) -> impl Iterator<Item = &str> {
    let mut seen = BTreeSet::new();
    model
        .assets
        .iter()
        .map(|a| a.source.as_str())
        .filter(move |path| seen.insert(*path))
}

/// The sources `model` names, looked at in `folder` as deeply as `depth` says.
pub fn inspect(folder: &Path, model: &Particles, depth: Depth) -> Facts {
    let mut facts = Facts::default();
    for path in named_paths(model) {
        facts.insert(path, look(folder, path, depth));
    }
    facts.unused = unused(folder, model);
    facts
}

/// The source folders of `folder` that `model` does not name, in order.
fn unused(folder: &Path, model: &Particles) -> Vec<String> {
    let named: BTreeSet<&str> = named_paths(model).collect();
    let mut unused: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.to_ascii_lowercase().ends_with(".src") && !named.contains(name.as_str())
        })
        .collect();
    unused.sort();
    unused
}

fn look(folder: &Path, path: &str, depth: Depth) -> Fact {
    if !stays_inside(path) {
        return Fact::Escapes;
    }
    let dir = folder.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.exists() {
        return Fact::Missing;
    }
    if !dir.is_dir() {
        return Fact::Unreadable(
            "is a file, and an asset names the source folders of its module".to_string(),
        );
    }
    let described = match or_source::cook::describe(&dir) {
        Ok(described) => described,
        Err(why) => return Fact::Unreadable(why),
    };
    if described.kind != or_source::particles::KIND {
        return Fact::Unreadable(format!(
            "is a {} source, and a particles module holds particles sources",
            described.kind
        ));
    }
    let digest = (depth >= Depth::Digest)
        .then(|| or_source::game_import::tree_digest(&dir).ok())
        .flatten();
    let inside = (depth >= Depth::Open).then(|| read_inside(&dir));
    Fact::Source(Box::new(SourceFact {
        kind: described.kind,
        name: described.name,
        extension: described.extension,
        bytes: described.bytes,
        game: described.game,
        digest,
        inside,
    }))
}

/// The members of the object `v` at `key` that are objects, in order.
fn objects<'v>(v: &'v Value, key: &str) -> impl Iterator<Item = &'v Value> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.is_object())
}

fn text<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// What the document of rules in `dir` states.
pub fn read_inside(dir: &Path) -> Result<Inside, String> {
    let bytes = std::fs::read(dir.join(or_source::particles::RULES))
        .map_err(|e| format!("its rules cannot be read: {e}"))?;
    let rules: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("its rules are not a document: {e}"))?;
    let names = |key: &str| -> BTreeSet<String> {
        objects(&rules, key)
            .map(|rule| text(rule, "name").to_ascii_lowercase())
            .collect()
    };
    let (emitters, particles) = (names("emitter_rules"), names("particle_rules"));
    let mut inside = Inside {
        effects: objects(&rules, "effect_rules")
            .map(|rule| text(rule, "name").to_string())
            .collect(),
        emitter_rules: emitters.len(),
        particle_rules: particles.len(),
        models: dir.join(or_source::particles::MODELS).is_file(),
        ..Inside::default()
    };
    for effect in objects(&rules, "effect_rules") {
        let who = text(effect, "name");
        for event in effect
            .get("timeline")
            .map(|t| objects(t, "events"))
            .into_iter()
            .flatten()
        {
            for (key, held, what) in [
                ("emitter_rule_name", &emitters, "emitter rule"),
                ("particle_rule_name", &particles, "particle rule"),
            ] {
                let named = text(event, key);
                if !named.is_empty() && !held.contains(&named.to_ascii_lowercase()) {
                    inside.unresolved.push(format!(
                        "the effect `{who}` fires the {what} `{named}`, which its list does not hold"
                    ));
                }
            }
        }
    }
    let held: BTreeSet<String> = std::fs::read_dir(dir.join(or_source::textures::DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
        })
        .collect();
    inside.textures = held.len();
    for rule in objects(&rules, "particle_rules") {
        let who = text(rule, "name");
        let vars = rule
            .get("shader_inst")
            .map(|inst| inst.get("inst_vars").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .flatten();
        for var in vars {
            let draws = text(var, "texture_name");
            let texture_var = var.pointer("/base/type").and_then(Value::as_u64) == Some(6);
            // A flag that is false is not stated.
            let outside = var.get("external_reference").and_then(Value::as_bool) == Some(true);
            if texture_var
                && !outside
                && !draws.is_empty()
                && !held.contains(&draws.to_ascii_lowercase())
            {
                inside.textures_missing.push(format!(
                    "the particle rule `{who}` draws the texture `{draws}`, which its list does not hold"
                ));
            }
        }
    }
    Ok(inside)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Asset;

    #[test]
    fn the_sources_a_model_names_are_looked_at_to_the_depth_asked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("nothing.particles.src")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a file").unwrap();
        let mut model = Particles::new();
        for path in [
            "missing.particles.src",
            "../out.particles.src",
            "a.txt",
            "nothing.particles.src",
        ] {
            model.assets.push(Asset::new(path));
        }
        let facts = inspect(dir.path(), &model, Depth::Stat);
        assert_eq!(facts.get("missing.particles.src"), Some(&Fact::Missing));
        assert_eq!(facts.get("../out.particles.src"), Some(&Fact::Escapes));
        assert!(matches!(facts.get("a.txt"), Some(Fact::Unreadable(_))));
        // A folder that is no source says why a build could not cook it.
        assert!(matches!(
            facts.get("nothing.particles.src"),
            Some(Fact::Unreadable(_))
        ));
    }
}

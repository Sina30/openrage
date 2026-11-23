//! What the disk says of the sources a module names, gathered once so that every rule and the plan are pure functions of it.
//!
//! A weapon names source folders of its module. For each one this looks at the folder as a source does (`or_source::cook::describe`:
//! what it cooks to, how big its inputs are, whether it is the game's own file), and for a metadata source it reads the XML through the
//! weapon family, so that a rule can say what the weapon is defined as.

use std::collections::BTreeMap;
use std::path::Path;

pub use or_project::content::facts::{stays_inside, Depth};

use crate::metadata::{self, Defined};
use crate::model::Weapons;

/// What is known of one source a weapon names.
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
    /// The word that chooses the compiler: `drawable`, `drawables`, `fragment`, `textures` or `metadata`.
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
    /// What a metadata source defines, or why it cannot be read; from [`Depth::Open`] on.
    pub defined: Option<Result<Defined, String>>,
}

impl SourceFact {
    /// The file the source cooks to, such as `w_pi_pistol.ydr`.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.name, self.extension)
    }

    /// Whether the source is a model: a drawable, a set of them or a fragment.
    pub fn is_model(&self) -> bool {
        matches!(self.kind.as_str(), "drawable" | "drawables" | "fragment")
    }

    pub fn is_textures(&self) -> bool {
        self.kind == "textures"
    }

    pub fn is_metadata(&self) -> bool {
        self.kind == "metadata"
    }
}

/// The sources of a module by the path it names them with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    sources: BTreeMap<String, Fact>,
    /// The source folders of the module's own folder that no weapon names.
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

/// Every source a module names: the sources of its weapons, once each.
pub fn named_paths(model: &Weapons) -> impl Iterator<Item = &str> {
    let mut seen = std::collections::BTreeSet::new();
    model
        .weapons
        .iter()
        .flat_map(|w| w.sources.iter().map(String::as_str))
        .filter(move |path| seen.insert(*path))
}

/// The sources `model` names, looked at in `folder` as deeply as `depth` says.
pub fn inspect(folder: &Path, model: &Weapons, depth: Depth) -> Facts {
    let mut facts = Facts::default();
    for path in named_paths(model) {
        facts.insert(path, look(folder, path, depth));
    }
    facts.unused = unused(folder, model);
    facts
}

/// The source folders of `folder` that `model` does not name, in order.
fn unused(folder: &Path, model: &Weapons) -> Vec<String> {
    let named: std::collections::BTreeSet<&str> = named_paths(model).collect();
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
            "is a file, and a weapon names the source folders of its module".to_string(),
        );
    }
    let described = match or_source::cook::describe(&dir) {
        Ok(described) => described,
        Err(why) => return Fact::Unreadable(why),
    };
    let digest = (depth >= Depth::Digest)
        .then(|| or_source::game_import::tree_digest(&dir).ok())
        .flatten();
    let defined =
        (depth >= Depth::Open && described.kind == or_source::metadata::KIND).then(|| {
            std::fs::read(dir.join(or_source::metadata::XML))
                .map_err(|e| format!("cannot be read: {e}"))
                .and_then(|bytes| metadata::read(&bytes))
        });
    Fact::Source(Box::new(SourceFact {
        kind: described.kind,
        name: described.name,
        extension: described.extension,
        bytes: described.bytes,
        game: described.game,
        digest,
        defined,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Weapon;

    #[test]
    fn the_sources_a_model_names_are_looked_at_to_the_depth_asked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("nothing.metadata.src")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a file").unwrap();
        let mut model = Weapons::new();
        let mut weapon = Weapon::new("WEAPON_X");
        for path in [
            "missing.drawable.src",
            "../out.drawable.src",
            "a.txt",
            "nothing.metadata.src",
        ] {
            weapon.sources.push(path.into());
        }
        model.weapons.push(weapon);

        let facts = inspect(dir.path(), &model, Depth::Stat);
        assert_eq!(facts.get("missing.drawable.src"), Some(&Fact::Missing));
        assert_eq!(facts.get("../out.drawable.src"), Some(&Fact::Escapes));
        assert!(matches!(facts.get("a.txt"), Some(Fact::Unreadable(_))));
        // A folder that is no source says why a build could not cook it.
        assert!(matches!(
            facts.get("nothing.metadata.src"),
            Some(Fact::Unreadable(_))
        ));
    }
}

//! The model of a pack of maps: each map set a person streams into the game, with the sources it is made of.
//!
//! It is the file `maps.json` of a maps module, canonical like every source of OpenRage: keys sorted, two-space indent, LF. A map set
//! names its sources by the folders of its module (`harbour.metadata.src`, `dock_crane.drawable.src`); what each is for comes from the
//! source (a map file, a type file, a model, a texture dictionary), and what the set places, the entities, the archetypes and the
//! interiors, comes from the map files, so none of that is written twice.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file a maps module keeps its model in.
pub const FILE: &str = "maps.json";
pub const FORMAT: &str = "openrage-maps";
pub const SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not a maps file: {1}")]
    Json(String, serde_json::Error),
    #[error("{0} is not a maps file: its format is `{1}`, not `{FORMAT}`")]
    Format(String, String),
    #[error(
        "{0} was written by a newer OpenRage (schema {1}) than this one reads (schema {SCHEMA})"
    )]
    Newer(String, u32),
    #[error("{0} is not a maps file: the map id {1:?} is not one plain file name")]
    Id(String, String),
}

/// The map sets of one module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Maps {
    pub format: String,
    pub schema: u32,
    pub maps: Vec<MapSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
    /// Names a person typed (an archetype, an asset), so that the hash a file stores of each shows as the name: a map file keeps the number only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// One map set: the map files, type files, models and textures a person edits and ships together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapSet {
    /// Stable for as long as the set exists and never reused: a rename or an edit keeps it.
    pub id: String,
    /// A name for people; the game never sees it.
    pub name: String,
    /// The sources it is made of, as folders of the module with `/`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// A line of the model that says why a finding stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waiver {
    pub code: String,
    /// The id of the set it sets a finding aside for; every set when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    pub reason: String,
}

impl Default for Maps {
    fn default() -> Self {
        Self::new()
    }
}

impl Maps {
    pub fn new() -> Self {
        Self {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            maps: Vec::new(),
            waivers: Vec::new(),
            names: Vec::new(),
        }
    }

    /// The text of a name a person typed, where one hashes to `hash`.
    pub fn name_of(&self, hash: u32) -> Option<String> {
        self.names
            .iter()
            .find(|name| or_io::string_hash(name.as_bytes()) == hash)
            .cloned()
    }

    /// Remember the names in `typed` that are not remembered yet; whether any was new.
    pub fn remember(&mut self, typed: &[&str]) -> bool {
        let mut new = false;
        for name in typed.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
            if !self.names.iter().any(|held| held == name) {
                self.names.push(name.to_string());
                new = true;
            }
        }
        new
    }

    pub fn map(&self, id: &str) -> Option<&MapSet> {
        self.maps.iter().find(|m| m.id == id)
    }

    pub fn map_mut(&mut self, id: &str) -> Option<&mut MapSet> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("a maps model is JSON");
        let mut text = serde_json::to_string_pretty(&value).expect("a JSON value serialises");
        text.push('\n');
        text
    }

    pub fn from_json(bytes: &[u8], shown: &str) -> Result<Self, Error> {
        let model: Self =
            serde_json::from_slice(bytes).map_err(|e| Error::Json(shown.to_string(), e))?;
        if model.format != FORMAT {
            return Err(Error::Format(shown.to_string(), model.format));
        }
        if model.schema > SCHEMA {
            return Err(Error::Newer(shown.to_string(), model.schema));
        }
        // ⚠ An id is a plain file name: it is how a person's edit names a set, and what the diff of two builds records.
        if let Some(bad) = model
            .maps
            .iter()
            .find(|m| !or_project::paths::is_component(&m.id))
        {
            return Err(Error::Id(shown.to_string(), bad.id.clone()));
        }
        Ok(model)
    }

    pub fn read(folder: &Path) -> Result<Self, Error> {
        let path = folder.join(FILE);
        let shown = path.display().to_string();
        let bytes = std::fs::read(&path).map_err(|e| Error::Io(shown.clone(), e))?;
        Self::from_json(&bytes, &shown)
    }

    /// Write the model beside its sources: to a temporary file first, so a failure leaves the one that was there.
    pub fn write(&self, folder: &Path) -> Result<(), Error> {
        let path = folder.join(FILE);
        let shown = path.display().to_string();
        or_project::content::folder::write_atomic(&path, &self.to_json())
            .map_err(|e| Error::Io(shown, e))
    }
}

impl MapSet {
    /// A set called `name` with a fresh id and no sources.
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            name: name.to_string(),
            sources: Vec::new(),
        }
    }
}

pub use or_project::content::folder::new_id;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_written_is_read_back_and_a_newer_or_foreign_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut model = Maps::new();
        let mut set = MapSet::new("harbour");
        set.sources.push("harbour.metadata.src".into());
        model.maps.push(set);
        model.write(dir.path()).unwrap();
        assert_eq!(Maps::read(dir.path()).unwrap(), model);
        assert!(model.to_json().ends_with("}\n"));

        let newer = model.to_json().replace("\"schema\": 1", "\"schema\": 2");
        assert!(matches!(
            Maps::from_json(newer.as_bytes(), "x"),
            Err(Error::Newer(..))
        ));
        let foreign = model.to_json().replace("openrage-maps", "openrage-weapons");
        assert!(matches!(
            Maps::from_json(foreign.as_bytes(), "x"),
            Err(Error::Format(..))
        ));
    }

    #[test]
    fn an_id_that_is_no_plain_file_name_is_refused_when_the_file_is_read() {
        let mut model = Maps::new();
        let mut set = MapSet::new("x");
        set.id = "../up".into();
        model.maps.push(set);
        assert!(matches!(
            Maps::from_json(model.to_json().as_bytes(), "x"),
            Err(Error::Id(..))
        ));
    }
}

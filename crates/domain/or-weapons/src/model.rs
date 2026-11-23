//! The model of a pack of weapons: each weapon a person streams into the game, with the sources it is made of.
//!
//! It is the file `weapons.json` of a weapons module, canonical like every source of OpenRage: keys sorted, two-space indent, LF.
//! A weapon names its sources by the folders of its module (`w_pi_pistol.drawable.src`, `weapons.metadata.src`); what each is for
//! comes from the source (a model, a texture dictionary, a metadata file), and what the weapon is made of, its components and the bone
//! each attaches to, comes from the metadata that defines it, so none of that is written twice.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file a weapons module keeps its model in.
pub const FILE: &str = "weapons.json";
pub const FORMAT: &str = "openrage-weapons";
pub const SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not a weapons file: {1}")]
    Json(String, serde_json::Error),
    #[error("{0} is not a weapons file: its format is `{1}`, not `{FORMAT}`")]
    Format(String, String),
    #[error(
        "{0} was written by a newer OpenRage (schema {1}) than this one reads (schema {SCHEMA})"
    )]
    Newer(String, u32),
    #[error("{0} is not a weapons file: the weapon id {1:?} is not one plain file name")]
    Id(String, String),
}

/// The weapons of one module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weapons {
    pub format: String,
    pub schema: u32,
    pub weapons: Vec<Weapon>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
}

/// One weapon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    /// Stable for as long as the weapon exists and never reused: a rename or an edit keeps it.
    pub id: String,
    /// A name for people; the game never sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The name the game resolves the weapon by, such as `WEAPON_PISTOL`: the `Name` its metadata defines it under.
    pub name: String,
    /// The sources it is made of, as folders of the module with `/`: its models, its textures and the metadata that defines it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// A line of the model that says why a finding stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waiver {
    pub code: String,
    /// The id of the weapon it sets a finding aside for; every weapon when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    pub reason: String,
}

impl Default for Weapons {
    fn default() -> Self {
        Self::new()
    }
}

impl Weapons {
    pub fn new() -> Self {
        Self {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            weapons: Vec::new(),
            waivers: Vec::new(),
        }
    }

    pub fn weapon(&self, id: &str) -> Option<&Weapon> {
        self.weapons.iter().find(|w| w.id == id)
    }

    pub fn weapon_mut(&mut self, id: &str) -> Option<&mut Weapon> {
        self.weapons.iter_mut().find(|w| w.id == id)
    }

    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("a weapons model is JSON");
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
        // ⚠ An id is a plain file name: it is how a person's edit names a weapon, and what the diff of two builds records.
        if let Some(bad) = model
            .weapons
            .iter()
            .find(|w| !or_project::paths::is_component(&w.id))
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

impl Weapon {
    /// A weapon called `name` with a fresh id and no sources.
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            label: None,
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
        let mut model = Weapons::new();
        let mut weapon = Weapon::new("WEAPON_TESTPISTOL");
        weapon.sources.push("weapons.metadata.src".into());
        model.weapons.push(weapon);
        model.write(dir.path()).unwrap();
        assert_eq!(Weapons::read(dir.path()).unwrap(), model);
        assert!(model.to_json().ends_with("}\n"));

        let newer = model.to_json().replace("\"schema\": 1", "\"schema\": 2");
        assert!(matches!(
            Weapons::from_json(newer.as_bytes(), "x"),
            Err(Error::Newer(..))
        ));
        let foreign = model
            .to_json()
            .replace("openrage-weapons", "openrage-wardrobe");
        assert!(matches!(
            Weapons::from_json(foreign.as_bytes(), "x"),
            Err(Error::Format(..))
        ));
    }

    #[test]
    fn an_id_that_is_no_plain_file_name_is_refused_when_the_file_is_read() {
        let mut model = Weapons::new();
        let mut weapon = Weapon::new("WEAPON_X");
        weapon.id = "../up".into();
        model.weapons.push(weapon);
        assert!(matches!(
            Weapons::from_json(model.to_json().as_bytes(), "x"),
            Err(Error::Id(..))
        ));
    }
}

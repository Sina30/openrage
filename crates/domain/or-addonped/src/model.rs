//! The model of addon peds: each ped a person streams into the game, with the files it is made of.
//!
//! It is the file `addonped.json` of an addon ped module, canonical like every source of OpenRage:
//! keys sorted, two-space indent, LF. A value left out is the default, so a ped that states only
//! its name and its files is a complete one.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file an addon ped module keeps its model in.
pub const FILE: &str = "addonped.json";
pub const FORMAT: &str = "openrage-addonped";
pub const SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not an addon ped file: {1}")]
    Json(String, serde_json::Error),
    #[error("{0} is not an addon ped file: its format is `{1}`, not `{FORMAT}`")]
    Format(String, String),
    #[error(
        "{0} was written by a newer OpenRage (schema {1}) than this one reads (schema {SCHEMA})"
    )]
    Newer(String, u32),
    #[error("{0} is not an addon ped file: the ped id {1:?} is not one plain file name")]
    Id(String, String),
}

/// The addon peds of one module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddonPeds {
    pub format: String,
    pub schema: u32,
    /// The data file that defines these peds to the game (`peds.meta`), a path inside the module's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<String>,
    pub peds: Vec<Ped>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
}

/// One addon ped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ped {
    /// Stable for as long as the ped exists and never reused: a rename or an edit keeps it.
    pub id: String,
    /// A name for people; the game never sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The name the game resolves the ped by: its model streams under it, and the data file states it.
    pub name: String,
    /// Every file the ped is made of: its model, textures, components, clips and expressions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PedFile>,
}

/// One file of a ped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedFile {
    /// The file, as a path inside the module's folder with `/`.
    pub path: String,
    /// Where it lands inside the resource, when it is not where [`crate::layout::resource_path`] puts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// A line of the model that says why a finding stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waiver {
    pub code: String,
    /// The id of the ped it sets a finding aside for; every ped when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ped: Option<String>,
    pub reason: String,
}

impl Default for AddonPeds {
    fn default() -> Self {
        Self::new()
    }
}

impl AddonPeds {
    pub fn new() -> Self {
        Self {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            meta: None,
            peds: Vec::new(),
            waivers: Vec::new(),
        }
    }

    pub fn ped(&self, id: &str) -> Option<&Ped> {
        self.peds.iter().find(|p| p.id == id)
    }

    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("an addon ped model is JSON");
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
        // ⚠ An id is a plain file name: it is how a person's edit names a ped, and what the diff records.
        if let Some(bad) = model.peds.iter().find(|p| !is_plain(&p.id)) {
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

    /// Write the model beside its files: to a temporary file first, so a failure leaves the one that was there.
    pub fn write(&self, folder: &Path) -> Result<(), Error> {
        let path = folder.join(FILE);
        let shown = path.display().to_string();
        or_project::content::folder::write_atomic(&path, &self.to_json())
            .map_err(|e| Error::Io(shown, e))
    }
}

impl Ped {
    /// A ped called `name` with a fresh id and no files.
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            label: None,
            name: name.to_string(),
            files: Vec::new(),
        }
    }
}

/// Whether `text` is one name of a file: not empty, no separator, no way up.
fn is_plain(text: &str) -> bool {
    or_project::paths::is_component(text)
}

pub use or_project::content::folder::{is_identifier, new_id};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_written_is_read_back_and_a_newer_or_foreign_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut model = AddonPeds::new();
        model.meta = Some("peds.meta".into());
        let mut ped = Ped::new("my_ped");
        ped.files.push(PedFile {
            path: "my_ped.yft".into(),
            at: None,
        });
        model.peds.push(ped);
        model.write(dir.path()).unwrap();
        assert_eq!(AddonPeds::read(dir.path()).unwrap(), model);
        assert!(model.to_json().ends_with("}\n"));

        let newer = model.to_json().replace("\"schema\": 1", "\"schema\": 2");
        assert!(matches!(
            AddonPeds::from_json(newer.as_bytes(), "x"),
            Err(Error::Newer(..))
        ));
        let foreign = model
            .to_json()
            .replace("openrage-addonped", "openrage-wardrobe");
        assert!(matches!(
            AddonPeds::from_json(foreign.as_bytes(), "x"),
            Err(Error::Format(..))
        ));
    }

    #[test]
    fn an_id_that_is_no_plain_file_name_is_refused_when_the_file_is_read() {
        let mut model = AddonPeds::new();
        let mut ped = Ped::new("my_ped");
        ped.id = "../up".into();
        model.peds.push(ped);
        assert!(matches!(
            AddonPeds::from_json(model.to_json().as_bytes(), "x"),
            Err(Error::Id(..))
        ));
    }

    #[test]
    fn a_name_is_an_identifier_when_it_is_lower_case_letters_digits_and_underscores_from_a_letter()
    {
        for good in ["a", "my_ped", "a_m_y_business_03", "p2"] {
            assert!(is_identifier(good), "{good}");
        }
        for bad in [
            "",
            "My_ped",
            "2ped",
            "_ped",
            "my ped",
            "my-ped",
            "pe\u{e9}d",
        ] {
            assert!(!is_identifier(bad), "{bad}");
        }
    }
}

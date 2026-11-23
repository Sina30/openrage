//! Presets: a metadata record's chosen fields, saved under the configuration and applied to a selection.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use or_io::meta::{MetaInstance, Value};
use serde::{Deserialize, Serialize};

use super::meta_edit::{value_at, MetaEdit};
use super::{AppState, Change, DocPayload, DocumentId};

// ⚠ A shared preset outlives the build that wrote it: a change to `Preset`'s
// shape bumps this and migrates the older versions in `read_preset`.
/// The preset file version this build writes and the newest it reads.
pub const PRESET_VERSION: u32 = 1;

/// One step from a metadata document's root towards a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaStep {
    Field(String),
    /// The field of that name after `n` others of it, which a document read without a schema repeats.
    Nth(String, usize),
    Index(usize),
}

/// A record type's chosen fields and their values, one JSON file under `<config>/openrage/presets/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub version: u32,
    pub name: String,
    pub type_name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Deserialize)]
struct PresetHeader {
    version: u32,
    #[serde(default)]
    name: String,
}

/// Read one preset file, refusing a version newer than [`PRESET_VERSION`] by its name.
fn read_preset(path: &Path) -> Result<Preset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let header: PresetHeader =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if header.version > PRESET_VERSION {
        return Err(format!(
            "the preset `{}` ({}) is version {}, and this build reads presets up to version \
             {PRESET_VERSION}; update OpenRage to use it",
            header.name,
            path.display(),
            header.version
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The presets in one directory, and why any file there was not offered.
#[derive(Debug, Clone, Default)]
pub struct PresetLibrary {
    dir: Option<PathBuf>,
    loaded: bool,
    pub list: Vec<Preset>,
    pub refused: Vec<String>,
}

impl PresetLibrary {
    /// An empty library that saves into `dir`.
    pub fn at(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            loaded: true,
            ..Self::default()
        }
    }

    /// Every `*.json` preset in `dir`, in file-name order.
    pub fn load(dir: PathBuf) -> Self {
        let mut library = Self::at(dir.clone());
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
        paths.sort();
        for path in paths {
            match read_preset(&path) {
                Ok(preset) => library.list.push(preset),
                Err(why) => {
                    tracing::warn!("{why}");
                    library.refused.push(why);
                }
            }
        }
        library
    }

    /// Read the configuration's `presets` directory the first time it is asked for.
    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(dir) = super::prefs::config_root().map(|c| c.join("presets")) {
            *self = Self::load(dir);
        }
    }

    /// Write `preset` to its file, replacing a preset of the same name.
    pub fn save(&mut self, preset: Preset) -> Result<PathBuf, String> {
        let dir = self
            .dir
            .clone()
            .ok_or("there is no configuration directory to save a preset in")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let stem: String = preset
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("{stem}.json"));
        let text = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.list.retain(|p| p.name != preset.name);
        self.list.push(preset);
        Ok(path)
    }
}

/// The records selected together: indices into one array of one metadata document.
#[derive(Debug, Clone, Default)]
pub struct RecordSelection {
    document: Option<DocumentId>,
    array: Vec<MetaStep>,
    indices: BTreeSet<usize>,
}

impl RecordSelection {
    /// Add a record to the selection or take it out; a record of another array starts a new one.
    pub fn toggle(&mut self, doc: DocumentId, array: &[MetaStep], index: usize) {
        if self.document != Some(doc) || self.array != array {
            self.set(doc, array, [index]);
        } else if !self.indices.remove(&index) {
            self.indices.insert(index);
        }
    }

    pub fn set(
        &mut self,
        doc: DocumentId,
        array: &[MetaStep],
        indices: impl IntoIterator<Item = usize>,
    ) {
        self.document = Some(doc);
        self.array = array.to_vec();
        self.indices = indices.into_iter().collect();
    }

    /// The array of `doc` records are selected in, and their indices; `None` when none is selected there.
    pub fn of(&self, doc: DocumentId) -> Option<(&[MetaStep], &BTreeSet<usize>)> {
        let selected = self.document == Some(doc) && !self.indices.is_empty();
        selected.then_some((self.array.as_slice(), &self.indices))
    }

    /// The selected indices of `array` in `doc`, empty when the selection is elsewhere.
    pub fn in_array(&self, doc: DocumentId, array: &[MetaStep]) -> &BTreeSet<usize> {
        static NONE: BTreeSet<usize> = BTreeSet::new();
        if self.document == Some(doc) && self.array == array {
            &self.indices
        } else {
            &NONE
        }
    }
}

/// What applying a preset did: how many records took it, and each one that did not, with why.
#[derive(Debug, Clone, Default)]
pub struct PresetReport {
    pub applied: usize,
    pub refused: Vec<(usize, String)>,
}

/// The array `path` reaches from `inst`, through struct fields, pointers and elements.
fn array_at<'a>(inst: &'a mut MetaInstance, path: &[MetaStep]) -> Option<&'a mut Vec<Value>> {
    match value_at(inst, path)? {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

/// The record a value holds, inline or behind a pointer.
pub fn record_of(value: &Value) -> Option<&MetaInstance> {
    match value {
        Value::Struct(inner) | Value::Pointer(Some(inner)) => Some(inner),
        _ => None,
    }
}

fn record_of_mut(value: &mut Value) -> Option<&mut MetaInstance> {
    match value {
        Value::Struct(inner) | Value::Pointer(Some(inner)) => Some(inner),
        _ => None,
    }
}

impl AppState {
    fn meta_array(&mut self, doc: DocumentId, array: &[MetaStep]) -> Option<&mut Vec<Value>> {
        match &mut self.document_mut(doc)?.payload {
            DocPayload::Meta(meta) => array_at(&mut meta.parsed.as_mut()?.instance, array),
            _ => None,
        }
    }

    /// Save `fields` of the record at `index` of `array` as the preset `name`.
    pub fn save_preset(
        &mut self,
        doc: DocumentId,
        array: &[MetaStep],
        index: usize,
        name: &str,
        fields: &[String],
    ) -> Result<PathBuf, String> {
        let record = self
            .meta_array(doc, array)
            .and_then(|items| items.get(index))
            .and_then(record_of)
            .ok_or_else(|| format!("there is no record {index} to save a preset from"))?;
        let preset = Preset {
            version: PRESET_VERSION,
            name: name.to_string(),
            type_name: record.type_name.clone(),
            fields: fields
                .iter()
                .filter_map(|f| Some((f.clone(), record.get(f)?.clone())))
                .collect(),
        };
        self.presets.save(preset)
    }

    /// Apply preset `preset` to every selected record of `doc` as one undoable edit.
    pub fn apply_preset(&mut self, doc: DocumentId, preset: usize) -> Result<PresetReport, String> {
        let preset = self
            .presets
            .list
            .get(preset)
            .cloned()
            .ok_or("that preset is no longer loaded")?;
        let selection = self.records_selected.clone();
        let indices = selection.in_array(doc, &selection.array);
        if indices.is_empty() {
            return Err("no record is selected".into());
        }
        let items = self
            .meta_array(doc, &selection.array)
            .ok_or("the selected records are no longer in the document")?;

        let mut report = PresetReport::default();
        let mut edits = Vec::new();
        for &index in indices {
            let Some(value) = items.get_mut(index) else {
                report
                    .refused
                    .push((index, format!("there is no record {index} any more")));
                continue;
            };
            let why = match record_of(value) {
                Some(record) if record.type_name == preset.type_name => None,
                Some(record) => Some(format!(
                    "record {index} is a {}, not a {}",
                    record.type_name, preset.type_name
                )),
                None => Some(format!("record {index} is not a structure")),
            };
            if let Some(why) = why {
                report.refused.push((index, why));
                continue;
            }
            let before = value.clone();
            if let Some(record) = record_of_mut(value) {
                for (name, field) in &preset.fields {
                    record.set(name.clone(), field.clone());
                }
            }
            report.applied += 1;
            let mut path = selection.array.clone();
            path.push(MetaStep::Index(index));
            edits.push(MetaEdit::Set {
                path,
                other: before,
            });
        }
        if edits.is_empty() {
            return Ok(report);
        }
        if let Some(DocPayload::Meta(meta)) = self.document_mut(doc).map(|d| &mut d.payload) {
            meta.after_visual_edit_in_place();
        }
        let label = format!("Apply preset {}", preset.name);
        self.record(doc, label, Change::Meta(edits));
        Ok(report)
    }
}

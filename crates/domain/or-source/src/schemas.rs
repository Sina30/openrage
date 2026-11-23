//! The JSON Schemas of the files a source folder holds, generated from the types that read and
//! write them, so an editor can check and complete a file a person edits by hand.
//!
//! The schemas are kept in `schemas/` beside this crate. A test holds each to what the types
//! generate (`OPENRAGE_WRITE_SCHEMAS=1` rewrites them) and another holds every key a document
//! writes to a property of its schema, so a field cannot be added without one.

use std::path::Path;

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde_json::Value;

use crate::asset::AssetManifest;
use crate::{json::canonical, metadata, models, textures};

/// The folder of the schemas, beside this crate's manifest.
pub const DIR: &str = "schemas";
/// Set to rewrite the schema files from the types instead of comparing them.
pub const WRITE_ENV: &str = "OPENRAGE_WRITE_SCHEMAS";

/// A schema, the file it is kept in and the paths an editor binds it to.
pub struct Schema {
    /// The file under [`DIR`].
    pub file: &'static str,
    /// Globs of the files it describes, for an editor's `json.schemas` setting.
    pub binds: &'static [&'static str],
    pub schema: Value,
}

/// The schema `T` generates, as JSON Schema 2020-12.
pub fn generate<T: JsonSchema>() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    json_of(&generator.root_schema_for::<T>())
}

fn json_of(schema: &schemars::Schema) -> Value {
    // A schema is a JSON object already.
    serde_json::to_value(schema).unwrap_or(Value::Null)
}

/// `asset.json`, whose `compile` block belongs to the kinds that state one, so it is described here and carried by the manifest.
fn asset() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let model = json_of(&generator.subschema_for::<models::Compile>());
    let metadata = json_of(&generator.subschema_for::<metadata::Compile>());
    let mut root = json_of(&generator.root_schema_for::<AssetManifest>());
    root["properties"]["compile"] = serde_json::json!({
        "description": "What a kind that compiles from a base keeps in the manifest: a model the mode and the hash of the base its glb was made from, a metadata source the extension of the file and the hash of the base its XML was exported from",
        "anyOf": [model, metadata],
    });
    root
}

/// The schemas of the files every source folder holds, in the order `schemas/` lists them.
pub fn sources() -> Vec<Schema> {
    vec![
        Schema {
            file: "asset.schema.json",
            binds: &["**/*.src/asset.json"],
            schema: asset(),
        },
        Schema {
            file: "textures.schema.json",
            binds: &["**/*.textures.src/textures.json"],
            schema: generate::<textures::Rules>(),
        },
        Schema {
            file: "drawable.schema.json",
            binds: &[
                "**/*.drawable.src/drawable.json",
                "**/*.drawables.src/drawables/*/drawable.json",
                "**/*.fragment.src/drawable/drawable.json",
            ],
            schema: generate::<models::Rules>(),
        },
        Schema {
            file: "dictionary.schema.json",
            binds: &["**/*.drawables.src/dictionary.json"],
            schema: generate::<models::Dictionary>(),
        },
    ]
}

impl Schema {
    /// The canonical text the file holds.
    pub fn text(&self) -> String {
        canonical(&self.schema).unwrap_or_default()
    }

    /// Holds the file in `dir` to the schema, or writes it where [`WRITE_ENV`] is set.
    pub fn keep(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(self.file);
        let wanted = self.text();
        if std::env::var_os(WRITE_ENV).is_some_and(|v| !v.is_empty()) {
            return std::fs::write(&path, wanted).map_err(|e| format!("{}: {e}", path.display()));
        }
        // A checkout may hold the file with CRLF line ends.
        let held = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}; set {WRITE_ENV}=1 to write it", path.display()))?
            .replace("\r\n", "\n");
        if held == wanted {
            Ok(())
        } else {
            Err(format!(
                "{} is not what the types generate; set {WRITE_ENV}=1 and run the test to write it",
                path.display()
            ))
        }
    }
}

/// Every key of `document` that `schema` has no property for, as dotted paths; free-form places (a `Value` field, extra keys a manifest carries) take any.
pub fn undescribed(schema: &Value, document: &Value) -> Vec<String> {
    let mut missing = Vec::new();
    walk(schema, schema, document, "", &mut missing);
    missing
}

fn walk(root: &Value, node: &Value, value: &Value, at: &str, missing: &mut Vec<String>) {
    let node = match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => root
            .pointer(reference.trim_start_matches('#'))
            .unwrap_or(&Value::Null),
        None => node,
    };
    for combined in ["allOf", "anyOf", "oneOf"] {
        let Some(branches) = node.get(combined).and_then(Value::as_array) else {
            continue;
        };
        // One branch must take the document whole; report the one that comes nearest, among those of its kind.
        let tries: Vec<Vec<String>> = branches
            .iter()
            .filter(|branch| is_of_kind(root, branch, value))
            .map(|branch| {
                let mut tried = Vec::new();
                walk(root, branch, value, at, &mut tried);
                tried
            })
            .collect();
        let report = if combined == "allOf" {
            tries.into_iter().flatten().collect()
        } else {
            tries.into_iter().min_by_key(Vec::len).unwrap_or_default()
        };
        missing.extend(report);
    }
    match value {
        Value::Object(entries) => {
            let properties = node.get("properties").and_then(Value::as_object);
            let extra = node.get("additionalProperties");
            for (key, entry) in entries {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                if let Some(property) = properties.and_then(|p| p.get(key)) {
                    walk(root, property, entry, &path, missing);
                } else if let Some(extra) = extra.filter(|e| e.is_object()) {
                    walk(root, extra, entry, &path, missing);
                } else if properties.is_some() && extra != Some(&Value::Bool(true)) {
                    missing.push(path);
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = node.get("items") {
                for item_value in items {
                    walk(root, item, item_value, at, missing);
                }
            }
        }
        _ => {}
    }
}

/// Whether `node` allows the kind of JSON `value` is, so the `null` branch of an optional field is not taken for an object.
fn is_of_kind(root: &Value, node: &Value, value: &Value) -> bool {
    let node = match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => root
            .pointer(reference.trim_start_matches('#'))
            .unwrap_or(&Value::Null),
        None => node,
    };
    let kinds: Vec<&str> = match node.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => return true,
    };
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    kinds.contains(&kind) || (kind == "integer" && kinds.contains(&"number"))
}

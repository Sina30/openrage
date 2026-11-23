//! Base-game data-file registry loader for `search --mounted`.
//!
//! `common.rpf` has no `content.xml`: the base game registers its data files
//! through `ContentsOfDataFileXml` documents (`startup`, `default`,
//! `contentpatch`, and each level file `levels.ymt` names), whose `dataFiles`
//! list is what `--fileTypes` filters on; `includedDataFiles` and
//! `includedXmlFiles` are followed. The title update's `update:/content` is
//! the mount builder's, not this loader's.

use std::collections::HashSet;

use or_io::meta::{MetaInstance, Value};
use or_title::dlc::ContentXml;
use or_title::Vfs;

pub struct DataFileRef {
    pub filename: String,
    pub file_type: String,
    pub disabled: bool,
}

pub struct BaseRegistry {
    pub files: Vec<DataFileRef>,
    pub warnings: Vec<String>,
    enabled: Vec<String>,
}

pub fn load_base_game(vfs: &Vfs) -> BaseRegistry {
    let mut out = BaseRegistry {
        files: Vec::new(),
        warnings: Vec::new(),
        enabled: Vec::new(),
    };
    let mut visited: HashSet<String> = HashSet::new();

    for path in [
        "platform:/data/startup",
        "platform:/data/default",
        "common:/data/contentpatch",
    ] {
        load_contents(vfs, path, &mut out, &mut visited);
    }

    for level_file in load_level_files(vfs, &mut out) {
        load_contents(vfs, &level_file, &mut out, &mut visited);
    }

    let enabled: HashSet<String> = out
        .enabled
        .iter()
        .map(|f| f.trim().to_ascii_lowercase())
        .collect();
    if !enabled.is_empty() {
        for f in &mut out.files {
            if f.disabled && enabled.contains(&f.filename.trim().to_ascii_lowercase()) {
                f.disabled = false;
            }
        }
    }

    out
}

fn load_level_files(vfs: &Vfs, out: &mut BaseRegistry) -> Vec<String> {
    let Some((bytes, _)) = resolve(vfs, "platform:/data/levels") else {
        return Vec::new();
    };
    let inst = match decode_meta(&bytes) {
        Some(i) => i,
        None => {
            out.warnings
                .push("platform:/data/levels: could not decode".into());
            return Vec::new();
        }
    };
    let mut files = Vec::new();
    if let Some(Value::Array(levels)) = inst.get("aLevelsData") {
        for lvl in levels {
            if let Value::Struct(s) = lvl {
                if let Some(Value::String(name)) = s.get("cFilename") {
                    if !name.is_empty() {
                        files.push(name.clone());
                    }
                }
            }
        }
    }
    files
}

fn load_contents(
    vfs: &Vfs,
    logical_path: &str,
    out: &mut BaseRegistry,
    visited: &mut HashSet<String>,
) {
    let Some((bytes, resolved)) = resolve(vfs, logical_path) else {
        return;
    };
    if !visited.insert(resolved.clone()) {
        return;
    }
    let (files, includes, files_to_enable) = match extract_contents(&bytes) {
        Some(v) => v,
        None => {
            out.warnings
                .push(format!("{resolved}: not a data-file registry"));
            return;
        }
    };
    out.files.extend(files);
    out.enabled.extend(files_to_enable);
    for inc in includes {
        load_contents(vfs, &inc, out, visited);
    }
}

fn extract_contents(bytes: &[u8]) -> Option<(Vec<DataFileRef>, Vec<String>, Vec<String>)> {
    if or_io::meta::detect_binary_format(bytes).is_some() {
        let inst = decode_meta(bytes)?;
        Some(extract_from_instance(&inst))
    } else {
        let content = ContentXml::parse(bytes).ok()?;
        let enabled = content
            .files_to_enable()
            .into_iter()
            .map(|i| i.value)
            .collect();
        let includes = content
            .included_data_files
            .into_iter()
            .map(|i| i.value)
            .collect();
        let files = content
            .data_files
            .into_iter()
            .map(|d| DataFileRef {
                filename: d.filename,
                file_type: d.file_type,
                disabled: d.disabled,
            })
            .collect();
        Some((files, includes, enabled))
    }
}

fn extract_from_instance(inst: &MetaInstance) -> (Vec<DataFileRef>, Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut includes = Vec::new();
    let mut files_to_enable = Vec::new();

    if let Some(Value::Array(items)) = inst.get("dataFiles") {
        for item in items {
            if let Value::Struct(s) = item {
                if let Some(df) = data_file_from_struct(s) {
                    files.push(df);
                }
            }
        }
    }
    if let Some(Value::Array(items)) = inst.get("includedDataFiles") {
        for item in items {
            if let Value::String(s) = item {
                includes.push(s.clone());
            }
        }
    }
    if let Some(Value::Array(items)) = inst.get("includedXmlFiles") {
        for item in items {
            if let Value::Struct(s) = item {
                if let Some(Value::Array(inner)) = s.get("dataFiles") {
                    for d in inner {
                        if let Value::Struct(ds) = d {
                            if let Some(df) = data_file_from_struct(ds) {
                                files.push(df);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(Value::Array(sets)) = inst.get("contentChangeSets") {
        for cs in sets {
            if let Value::Struct(s) = cs {
                collect_files_to_enable(s, &mut files_to_enable);
            }
        }
    }
    (files, includes, files_to_enable)
}

fn collect_files_to_enable(cs: &MetaInstance, out: &mut Vec<String>) {
    if let Some(Value::Array(items)) = cs.get("filesToEnable") {
        for it in items {
            if let Value::String(f) = it {
                out.push(f.clone());
            }
        }
    }
    if let Some(Value::Array(maps)) = cs.get("mapChangeSetData") {
        for m in maps {
            if let Value::Struct(ms) = m {
                collect_files_to_enable(ms, out);
            }
        }
    }
}

fn data_file_from_struct(s: &MetaInstance) -> Option<DataFileRef> {
    let filename = match s.get("filename") {
        Some(Value::String(f)) if !f.is_empty() => f.clone(),
        _ => return None,
    };
    let file_type = match s.get("fileType") {
        Some(Value::Enum { variant, .. }) => variant.clone(),
        Some(Value::String(v)) => v.clone(),
        _ => "RPF_FILE".to_string(),
    };
    let disabled = matches!(s.get("disabled"), Some(Value::Bool(true)));
    Some(DataFileRef {
        filename,
        file_type,
        disabled,
    })
}

fn decode_meta(bytes: &[u8]) -> Option<MetaInstance> {
    or_io::meta::binary::parse_meta_binary(crate::meta::schema(), bytes).ok()
}

fn resolve(vfs: &Vfs, path: &str) -> Option<(Vec<u8>, String)> {
    let norm = normalize_device(path);
    for candidate in extension_candidates(&norm) {
        if let Some(bytes) = vfs.open(&candidate) {
            return Some((bytes, candidate));
        }
    }
    None
}

fn normalize_device(path: &str) -> String {
    let expanded = expand_platform(path);
    if let Some((mount, rest)) = expanded.split_once(":/") {
        let m = mount.to_ascii_lowercase();
        let base = m.strip_suffix("crc").unwrap_or(&m);
        format!("{base}:/{rest}")
    } else {
        expanded
    }
}

pub fn expand_platform(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    let lower = path.to_ascii_lowercase();
    if let Some(pos) = lower.find("%platform%") {
        let end = pos + "%platform%".len();
        format!("{}x64{}", &path[..pos], &path[end..])
    } else {
        path.to_string()
    }
}

fn extension_candidates(path: &str) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".ymt")
        || lower.ends_with(".meta")
        || lower.ends_with(".xml")
        || lower.ends_with(".pso")
    {
        vec![path.to_string()]
    } else {
        vec![
            format!("{path}.ymt"),
            format!("{path}.meta"),
            format!("{path}.xml"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_crc_and_expands_platform() {
        assert_eq!(
            normalize_device("platformcrc:/data/default"),
            "platform:/data/default"
        );
        assert_eq!(
            normalize_device("commoncrc:/data/ai/weapons.meta"),
            "common:/data/ai/weapons.meta"
        );
        assert_eq!(
            normalize_device("update:/%PLATFORM%/patch/x.rpf"),
            "update:/x64/patch/x.rpf"
        );
    }

    #[test]
    fn extension_candidates_appends_when_bare() {
        assert_eq!(
            extension_candidates("platform:/data/default"),
            vec![
                "platform:/data/default.ymt",
                "platform:/data/default.meta",
                "platform:/data/default.xml",
            ]
        );
        assert_eq!(
            extension_candidates("common:/data/levels/gta5/gta5.meta"),
            vec!["common:/data/levels/gta5/gta5.meta"]
        );
    }
}

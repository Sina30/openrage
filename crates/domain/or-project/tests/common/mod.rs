//! What the suites about an output's target share: a project declared by hand, and small real resources.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::retarget::texture_dictionary_to_gen9;
use or_io::gta5::PgDictionary;
use or_project::{plans, Plan, Project};
use serde_json::{json, Value};

pub fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A project folder holding `files`, whose one Files content states `file_types` and which declares `output`.
pub fn declare(
    folder: &Path,
    files: &[(&str, Vec<u8>)],
    file_types: Value,
    output: Value,
) -> Project {
    for (rel, body) in files {
        write(&folder.join(rel), body);
    }
    let documents: Vec<Value> = files
        .iter()
        .map(|(rel, _)| json!({ "path": rel }))
        .collect();
    let mut files_module = json!({ "id": "files", "type": "files" });
    if !file_types.is_null() {
        files_module["file_types"] = file_types;
    }
    let project = json!({
        "schema": 4,
        "open": [],
        "documents": documents,
        "modules": [files_module],
        "outputs": [output],
    });
    let path = folder.join("pack.openrage-proj");
    write(&path, &serde_json::to_vec_pretty(&project).unwrap());
    Project::load(&path).expect("the project reads")
}

/// The plans of the project's one output.
pub fn planned(project: &Project) -> Vec<Plan> {
    let id = project.container.outputs[0].id.clone();
    plans(
        &project.container,
        &id,
        &project.folder(),
        &project.sources(),
    )
    .expect("the output is declared")
}

/// Every file under `dir` by its `/` path, with its bytes.
pub fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap();
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A Legacy texture dictionary of one block-compressed texture.
///
/// With `layers`, the texture is an array, which Enhanced cannot describe: retargeting it is refused.
pub fn legacy_dictionary(name: &str, layers: u8) -> PgDictionary<GrcTexture> {
    let format = GrcTextureFormat::DXT1;
    let texture = GrcTexture {
        name: name.into(),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 1,
        layer_count: layers,
        mip_stride: u16::try_from(format.legacy_mip_stride(16)).unwrap(),
        format: format.to_raw_format(),
        backing_store: vec![0x5a; format.total_size(16, 16, 1) as usize],
        ..Default::default()
    };
    PgDictionary {
        codes: vec![or_io::string_hash(name.as_bytes())],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    }
}

pub fn legacy_ytd(name: &str) -> Vec<u8> {
    legacy_dictionary(name, 0).to_bytes().expect("it saves")
}

/// The same dictionary as Enhanced lays it out.
pub fn enhanced_ytd(name: &str) -> Vec<u8> {
    let (dictionary, _) =
        texture_dictionary_to_gen9(&legacy_dictionary(name, 0)).expect("retargets");
    dictionary.to_bytes().expect("it saves")
}

/// A Legacy dictionary of an array texture, which is a valid file and cannot be retargeted.
pub fn unretargetable_ytd(name: &str) -> Vec<u8> {
    legacy_dictionary(name, 5).to_bytes().expect("it saves")
}

/// Which target a resource's own RSC7 version says it is laid out for.
pub fn layout_of(ext: &str, bytes: &[u8]) -> or_io::Generation {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let version = Rsc7::peek(bytes).expect("an RSC7 file").version();
    or_io::gta5::formats::format(ext)
        .expect("a declared format")
        .target_of_version(version)
        .expect("a version a target declares")
}

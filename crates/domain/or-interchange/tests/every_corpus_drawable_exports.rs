//! Every staged drawable exported, and the result held to what glTF requires.
//!
//! The Khronos validator's rules, restated in Rust so they run wherever the
//! corpus is. Seven of them have caught a real defect — the non-affine inverse
//! bind matrix, the non-unit normal and the index past the vertex run among
//! them. One file is resident at a time.

use std::path::Path;

use or_interchange::extract::Extracted;
use or_interchange::gltf::{self, Container};
use or_interchange::{ExtractOptions, LodSelection, SceneBuilder};
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{File, GtaDrawable, GtaFragType, PgDictionary, RmcDrawable};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::Value;

/// How many files of each extension the suite reads.
const CAP: usize = 80;

fn document(bytes: &[u8]) -> Value {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let text = std::str::from_utf8(&bytes[20..20 + length]).expect("the JSON chunk is UTF-8");
    serde_json::from_str(text.trim_end()).expect("the JSON chunk parses")
}

fn binary(bytes: &[u8]) -> &[u8] {
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let at = 20 + json_length;
    if at + 8 > bytes.len() {
        return &[];
    }
    &bytes[at + 8..]
}

fn floats(bin: &[u8], offset: usize, count: usize, components: usize) -> Vec<f32> {
    (0..count * components)
        .map(|i| {
            let at = offset + i * 4;
            f32::from_le_bytes([bin[at], bin[at + 1], bin[at + 2], bin[at + 3]])
        })
        .collect()
}

fn view_offset(doc: &Value, accessor: usize) -> usize {
    let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
    doc["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize
}

/// What glTF requires of one exported document, checked against its own
/// bytes. Returns how many primitives it held to those rules.
fn hold_to_the_spec(label: &str, bytes: &[u8]) -> usize {
    let doc = document(bytes);
    let bin = binary(bytes);
    let mut checked = 0usize;

    for (index, accessor) in doc["accessors"].as_array().unwrap().iter().enumerate() {
        let width = match accessor["componentType"].as_u64().unwrap() {
            5123 => 2,
            5125 | 5126 => 4,
            other => panic!("{label}: component type {other} is not one this writer emits"),
        };
        assert_eq!(
            view_offset(&doc, index) % width,
            0,
            "{label}: accessor {index} is not aligned to its component size"
        );
    }

    for (mesh_index, mesh) in doc["meshes"].as_array().unwrap().iter().enumerate() {
        for (slot, primitive) in mesh["primitives"].as_array().unwrap().iter().enumerate() {
            let where_ = format!("{label}: mesh {mesh_index} primitive {slot}");
            let attributes = primitive["attributes"].as_object().unwrap();
            checked += 1;

            let position = attributes["POSITION"].as_u64().unwrap() as usize;
            let vertices = doc["accessors"][position]["count"].as_u64().unwrap() as usize;
            assert!(vertices > 0, "{where_}: an empty primitive was written");

            // POSITION carries min and max, and both must be finite numbers.
            for bound in ["min", "max"] {
                let values = doc["accessors"][position][bound].as_array().unwrap();
                assert_eq!(values.len(), 3, "{where_}: {bound} is not a VEC3");
                for value in values {
                    assert!(
                        value.as_f64().unwrap().is_finite(),
                        "{where_}: {bound} is not finite"
                    );
                }
            }

            // Every attribute run is as long as POSITION's.
            for (name, accessor) in attributes {
                let at = accessor.as_u64().unwrap() as usize;
                assert_eq!(
                    doc["accessors"][at]["count"].as_u64().unwrap() as usize,
                    vertices,
                    "{where_}: {name} is a different length from POSITION"
                );
            }

            // NORMAL and TANGENT are unit vectors, which the validator
            // rejects a file for getting wrong.
            for name in ["NORMAL", "TANGENT"] {
                let Some(accessor) = attributes.get(name) else {
                    continue;
                };
                let at = accessor.as_u64().unwrap() as usize;
                let components = if name == "TANGENT" { 4 } else { 3 };
                let data = floats(bin, view_offset(&doc, at), vertices, components);
                for vertex in 0..vertices {
                    let base = vertex * components;
                    let length = (data[base] * data[base]
                        + data[base + 1] * data[base + 1]
                        + data[base + 2] * data[base + 2])
                        .sqrt();
                    assert!(
                        (length - 1.0).abs() < 1e-3,
                        "{where_}: {name} of vertex {vertex} has length {length}"
                    );
                    if components == 4 {
                        let sign = data[base + 3];
                        assert!(
                            sign == 1.0 || sign == -1.0,
                            "{where_}: a tangent's fourth component is {sign}, not a sign"
                        );
                    }
                }
            }

            // Indices land inside the vertex run.
            let indices = primitive["indices"].as_u64().unwrap() as usize;
            let count = doc["accessors"][indices]["count"].as_u64().unwrap() as usize;
            assert_eq!(count % 3, 0, "{where_}: not a whole number of triangles");
            let offset = view_offset(&doc, indices);
            let narrow = doc["accessors"][indices]["componentType"].as_u64().unwrap() == 5123;
            for i in 0..count {
                let value = if narrow {
                    let at = offset + i * 2;
                    u32::from(u16::from_le_bytes([bin[at], bin[at + 1]]))
                } else {
                    let at = offset + i * 4;
                    u32::from_le_bytes([bin[at], bin[at + 1], bin[at + 2], bin[at + 3]])
                };
                assert!(
                    (value as usize) < vertices,
                    "{where_}: index {i} is {value} of {vertices} vertices"
                );
            }

            // A material index names a material that is there.
            if let Some(material) = primitive["material"].as_u64() {
                assert!(
                    (material as usize) < doc["materials"].as_array().map_or(0, Vec::len),
                    "{where_}: material {material} is past the end"
                );
            }

            // JOINTS_0 indexes the skin that the node using this mesh names.
            if let Some(joints) = attributes.get("JOINTS_0") {
                assert!(
                    attributes.contains_key("WEIGHTS_0"),
                    "{where_}: joints without weights"
                );
                let at = joints.as_u64().unwrap() as usize;
                let offset = view_offset(&doc, at);
                let widest = doc["skins"]
                    .as_array()
                    .map(|skins| {
                        skins
                            .iter()
                            .map(|s| s["joints"].as_array().map_or(0, Vec::len))
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                assert!(widest > 0, "{where_}: joints with no skin in the document");
                for i in 0..vertices * 4 {
                    let a = offset + i * 2;
                    let value = u16::from_le_bytes([bin[a], bin[a + 1]]) as usize;
                    assert!(value < widest, "{where_}: joint {value} is past the skin");
                }
            }
        }
    }

    // An inverse bind matrix must stay affine.
    for (index, skin) in doc["skins"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let at = skin["inverseBindMatrices"].as_u64().unwrap() as usize;
        let count = doc["accessors"][at]["count"].as_u64().unwrap() as usize;
        assert_eq!(
            count,
            skin["joints"].as_array().unwrap().len(),
            "{label}: skin {index} has a matrix per joint or it has neither"
        );
        let data = floats(bin, view_offset(&doc, at), count, 16);
        for matrix in 0..count {
            let base = matrix * 16;
            assert_eq!(data[base + 3], 0.0, "{label}: skin {index} matrix {matrix}");
            assert_eq!(data[base + 7], 0.0, "{label}: skin {index} matrix {matrix}");
            assert_eq!(
                data[base + 11],
                0.0,
                "{label}: skin {index} matrix {matrix}"
            );
            assert_eq!(
                data[base + 15],
                1.0,
                "{label}: skin {index} matrix {matrix}"
            );
            for value in &data[base..base + 16] {
                assert!(value.is_finite(), "{label}: skin {index} matrix {matrix}");
            }
        }
    }

    // Every node index a scene or a node names is one the document has.
    let nodes = doc["nodes"].as_array().map_or(0, Vec::len);
    for scene in doc["scenes"].as_array().unwrap() {
        for root in scene["nodes"].as_array().unwrap() {
            assert!(
                (root.as_u64().unwrap() as usize) < nodes,
                "{label}: bad root"
            );
        }
    }
    for node in doc["nodes"].as_array().unwrap() {
        for child in node["children"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            assert!(
                (child.as_u64().unwrap() as usize) < nodes,
                "{label}: a node names a child that is not there"
            );
        }
    }

    checked
}

/// How many primitives were exported and checked; 0 when the drawables carry
/// no mesh.
fn export_and_check<T: Extracted>(label: &str, drawables: &[(&str, &RmcDrawable<T>)]) -> usize {
    let mut builder = SceneBuilder::new(label);
    let options = ExtractOptions {
        lods: LodSelection::All,
        // The textures are BCn and this suite is about geometry; decoding
        // them is minutes of work for nothing it asserts.
        textures: false,
    };
    for (name, drawable) in drawables {
        builder.push(name, *drawable, &options);
    }
    let scene = builder.finish();
    if scene.objects.iter().all(|o| o.meshes.is_empty()) {
        return 0;
    }
    let files = gltf::write(&scene, label, Container::Glb);
    hold_to_the_spec(label, &files.primary)
}

/// ⚠ A suite that read no file reports the same green as one that read every
/// file.
fn checked_something(what: &str, documents: usize, primitives: usize) {
    assert!(
        documents > 0 && primitives > 0,
        "{what} found fixtures and produced {documents} documents and \
         {primitives} primitives, so it asserted nothing"
    );
    eprintln!("{what}: {documents} documents, {primitives} primitives");
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mesh")
        .to_string()
}

/// Every corpus `.ydr` of `target`, exported and held to the spec.
fn every_ydr<T: Extracted>(what: &str, target: &str)
where
    GtaDrawable<T>: File,
{
    let Some(files) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "ydr")
        .target(target)
        .limit(CAP)
        .find_or_skip(what)
    else {
        return;
    };

    let mut documents = 0usize;
    let mut primitives = 0usize;

    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(drawable) = GtaDrawable::<T>::from_bytes(&bytes) else {
            continue;
        };
        let label = stem(&path);
        let found = export_and_check(&label, &[(label.as_str(), &drawable.base)]);
        if found > 0 {
            documents += 1;
            primitives += found;
        }
    }
    checked_something(what, documents, primitives);
}

/// Every corpus `.ydd` of `target`, exported and held to the spec.
fn every_ydd<T: Extracted>(what: &str, target: &str)
where
    PgDictionary<GtaDrawable<T>>: File,
{
    let Some(files) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "ydd")
        .target(target)
        .limit(CAP)
        .find_or_skip(what)
    else {
        return;
    };

    let mut documents = 0usize;
    let mut primitives = 0usize;

    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes) else {
            continue;
        };
        let label = stem(&path);
        let names: Vec<String> = (0..dictionary.entries.len())
            .map(|i| format!("entry{i}"))
            .collect();
        let drawables: Vec<(&str, &RmcDrawable<T>)> = dictionary
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| Some((names[i].as_str(), &entry.as_deref()?.base)))
            .collect();
        let found = export_and_check(&label, &drawables);
        if found > 0 {
            documents += 1;
            primitives += found;
        }
    }
    checked_something(what, documents, primitives);
}

/// Every corpus `.yft` of `target`, exported and held to the spec.
fn every_yft<T: Extracted>(what: &str, target: &str)
where
    GtaFragType<T>: File,
{
    let Some(files) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "yft")
        .target(target)
        .limit(CAP)
        .find_or_skip(what)
    else {
        return;
    };

    let mut documents = 0usize;
    let mut primitives = 0usize;

    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(fragment) = GtaFragType::<T>::from_bytes(&bytes) else {
            continue;
        };
        let label = stem(&path);
        let parts: Vec<&RmcDrawable<T>> = fragment
            .common_drawable
            .iter()
            .chain(fragment.extra_drawables.iter())
            .map(|d| &d.base)
            .collect();
        let names: Vec<String> = (0..parts.len()).map(|i| format!("part{i}")).collect();
        let drawables: Vec<(&str, &RmcDrawable<T>)> = parts
            .iter()
            .enumerate()
            .map(|(i, d)| (names[i].as_str(), *d))
            .collect();
        let found = export_and_check(&label, &drawables);
        if found > 0 {
            documents += 1;
            primitives += found;
        }
    }
    checked_something(what, documents, primitives);
}

#[test]
fn every_corpus_ydr_exports_a_document_gltf_accepts() {
    every_ydr::<Gen8>(
        "every_corpus_ydr_exports_a_document_gltf_accepts",
        TARGET_GEN8,
    );
}

#[test]
fn every_corpus_ydd_exports_a_document_gltf_accepts() {
    every_ydd::<Gen8>(
        "every_corpus_ydd_exports_a_document_gltf_accepts",
        TARGET_GEN8,
    );
}

#[test]
fn every_corpus_yft_exports_a_document_gltf_accepts() {
    every_yft::<Gen8>(
        "every_corpus_yft_exports_a_document_gltf_accepts",
        TARGET_GEN8,
    );
}

#[test]
fn every_enhanced_ydr_exports_a_document_gltf_accepts() {
    every_ydr::<Gen9>(
        "every_enhanced_ydr_exports_a_document_gltf_accepts",
        TARGET_GEN9,
    );
}

#[test]
fn every_enhanced_ydd_exports_a_document_gltf_accepts() {
    every_ydd::<Gen9>(
        "every_enhanced_ydd_exports_a_document_gltf_accepts",
        TARGET_GEN9,
    );
}

#[test]
fn every_enhanced_yft_exports_a_document_gltf_accepts() {
    every_yft::<Gen9>(
        "every_enhanced_yft_exports_a_document_gltf_accepts",
        TARGET_GEN9,
    );
}

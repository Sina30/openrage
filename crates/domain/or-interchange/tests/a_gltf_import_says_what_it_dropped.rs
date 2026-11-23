//! What a glTF file holds that a drawable has no place for is counted by the reader, not ignored without a word.

use std::path::Path;

use or_interchange::gltf::{self, Container};
use or_interchange::{Bone, Dropped, Image, Material, Mesh, Object, Primitive, Scene};
use serde_json::{json, Value};

fn png() -> Vec<u8> {
    let mut image = or_io::codec::texture::RgbaImage::new(4, 4);
    image.set_pixel(1, 1, [9, 8, 7, 255]);
    or_io::codec::texture::encode_png(&image).unwrap()
}

/// One skinned triangle with a material, a texture and a bone.
fn triangle() -> Scene {
    let primitive = Primitive {
        material: Some(0),
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        joints: vec![[0; 4]; 3],
        weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
        indices: vec![0, 1, 2],
        ..Primitive::default()
    };
    Scene {
        name: "triangle".to_string(),
        objects: vec![Object {
            name: "triangle".to_string(),
            meshes: vec![Mesh {
                name: "lod0_model0".to_string(),
                primitives: vec![primitive],
                ..Mesh::default()
            }],
            bones: vec![Bone {
                name: "root".to_string(),
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
                ..Bone::default()
            }],
        }],
        materials: vec![Material {
            name: "ped_default".to_string(),
            shader: "ped_default".to_string(),
            base_colour: Some(0),
            ..Material::default()
        }],
        images: vec![Image {
            name: "diffuse".to_string(),
            png: png(),
        }],
        ..Scene::default()
    }
}

/// `glb` with its JSON chunk passed through `edit`, and its binary chunk as it was.
fn patched(glb: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let word = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
    let json_length = word(12);
    let mut document: Value = serde_json::from_slice(&glb[20..20 + json_length]).unwrap();
    edit(&mut document);
    let mut text = serde_json::to_vec(&document).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let rest = &glb[20 + json_length..];
    let mut out = Vec::new();
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + text.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    out
}

fn count(scene: &Scene, what: &str) -> Option<usize> {
    scene
        .dropped
        .iter()
        .find(|d| d.what == what)
        .map(|d| d.count)
}

#[test]
fn a_file_with_only_geometry_drops_what_the_drawable_keeps_of_its_own() {
    let glb = gltf::write(&triangle(), "triangle", Container::Glb).primary;
    let scene = gltf::read::load(Path::new("triangle.glb"), &glb).unwrap();
    // The material and the texture are the exporter's own: the drawable keeps its materials and
    // its textures, so an edit takes neither.
    assert_eq!(
        scene.dropped,
        vec![Dropped::new("materials", 1), Dropped::new("textures", 1)]
    );
}

#[test]
fn a_file_from_a_modelling_tool_says_everything_an_edit_has_no_place_for() {
    let glb = gltf::write(&triangle(), "triangle", Container::Glb).primary;
    let glb = patched(&glb, |doc| {
        doc["animations"] =
            json!([{"channels": [], "samplers": []}, {"channels": [], "samplers": []}]);
        doc["cameras"] = json!([{"type": "perspective"}]);
        doc["extensions"] =
            json!({"KHR_lights_punctual": {"lights": [{"type": "point"}, {"type": "spot"}]}});
        let skin = doc["skins"][0].clone();
        doc["skins"].as_array_mut().unwrap().push(skin);
        let primitive = &mut doc["meshes"][0]["primitives"][0];
        primitive["targets"] = json!([{"POSITION": 0}]);
        let joints = primitive["attributes"]["JOINTS_0"].clone();
        let weights = primitive["attributes"]["WEIGHTS_0"].clone();
        primitive["attributes"]["JOINTS_1"] = joints;
        primitive["attributes"]["WEIGHTS_1"] = weights;
    });
    let scene = gltf::read::load(Path::new("tool.glb"), &glb).unwrap();

    assert_eq!(count(&scene, "animations"), Some(2));
    assert_eq!(count(&scene, "cameras"), Some(1));
    assert_eq!(count(&scene, "lights"), Some(2));
    assert_eq!(count(&scene, "skins after the first"), Some(1));
    assert_eq!(count(&scene, "primitives with morph targets"), Some(1));
    assert_eq!(
        count(&scene, "primitives with more than four influences"),
        Some(1)
    );
    assert_eq!(count(&scene, "materials"), Some(1));
    // What an edit does take is not in the report.
    assert_eq!(count(&scene, "meshes"), None);
}

//! A drawable built in Rust, exported, and read back out of the JSON.
//!
//! Built from scratch because an exporter fed only by the reader hides every
//! assumption the two share; the corpus-gated half is
//! `every_corpus_drawable_exports.rs`.

use or_interchange::gltf::{self, Container, BASIS_ROTATION};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout};
use or_io::decl::{Gen8, Hash32};
use or_io::gta5::drawable::gen8;
use or_io::gta5::{
    CrBoneData, CrSkeletonData, GrmGeometry, GrmModel, GrmShaderGroup, RmcDrawable, RmcLod, Vec4V,
};

use serde_json::Value;

/// An FVF declaring `channels`, each at its encoding.
fn fvf(channels: &[(GrcFvfChannel, GrcDataSize)]) -> gen8::GrcFvf {
    let mut fvf = gen8::GrcFvf::default();
    for &(channel, size) in channels {
        fvf.fvf |= 1 << channel as u32;
        fvf.channel_sizes |= (size as u64) << (channel as u64 * 4);
    }
    fvf.channel_count = channels.len() as u8;
    fvf.fvf_size = VertexLayout::of(fvf.fvf, fvf.channel_sizes).packed_stride() as u8;
    fvf
}

/// A unit triangle with a position, a normal and one UV set.
fn triangle(skinned: bool) -> GrmGeometry<Gen8> {
    let mut channels = vec![
        (GrcFvfChannel::Position, GrcDataSize::Float3),
        (GrcFvfChannel::Normal, GrcDataSize::Float3),
    ];
    if skinned {
        channels.push((GrcFvfChannel::Weight, GrcDataSize::Color));
        channels.push((GrcFvfChannel::Binding, GrcDataSize::UByte4));
    }
    channels.push((GrcFvfChannel::Tex0, GrcDataSize::Float2));

    let points = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let mut data = Vec::new();
    for (index, point) in points.iter().enumerate() {
        for axis in point {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        // The normal points along +Y, which is the right-hand-rule normal of
        // this winding.
        for axis in [0.0f32, 1.0, 0.0] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        if skinned {
            data.extend_from_slice(&[255u8, 0, 0, 0]);
            data.extend_from_slice(&[1u8, 0, 0, 0]);
        }
        for axis in [index as f32 * 0.5, 0.25f32] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
    }

    GrmGeometry {
        // The value shipped files carry.
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf(&channels))),
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: vec![0, 1, 2],
            ..Default::default()
        }),
        matrix_palette: if skinned { vec![0, 2] } else { Vec::new() },
        ..Default::default()
    }
}

fn bone(name: &str, id: u16, parent: i16, index: u16) -> CrBoneData {
    CrBoneData {
        name: name.into(),
        bone_id: id,
        parent_index: parent,
        index,
        rotation: Vec4V {
            w: 1.0,
            ..Default::default()
        },
        scale: Vec4V {
            x: 1.0,
            y: 1.0,
            z: 1.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn skeleton() -> CrSkeletonData {
    let mut child = bone("child", 7, 0, 1);
    child.translation = Vec4V {
        z: 2.0,
        w: 1.0,
        ..Default::default()
    };
    CrSkeletonData {
        parent_indices: vec![-1, 0, 1],
        bones: vec![bone("root", 0, -1, 0), child, bone("tip", 9, 1, 2)],
        ..Default::default()
    }
}

fn drawable(skinned: bool, with_skeleton: bool) -> RmcDrawable<Gen8> {
    let model = GrmModel {
        aabbs: vec![Default::default()],
        shader_indices: vec![0],
        geometries: vec![Box::new(triangle(skinned))],
        ..Default::default()
    };
    let mut out = RmcDrawable::<Gen8> {
        name: "subject".into(),
        shader_group: Some(Box::new(GrmShaderGroup {
            shaders: vec![Box::new(gen8::GrcInstanceData {
                basis_hash: Hash32::of("default"),
                ..Default::default()
            })],
            ..Default::default()
        })),
        ..Default::default()
    };
    out.lod_group.lods[0] = RmcLod {
        models: vec![Box::new(model)],
        ..Default::default()
    }
    .into();
    if with_skeleton {
        out.skeleton = Some(Box::new(skeleton()));
    }
    out
}

fn document(bytes: &[u8]) -> Value {
    // The JSON chunk of a `.glb`: past the 12-byte header and the 8-byte
    // chunk header, for the chunk's declared length.
    let length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let text = std::str::from_utf8(&bytes[20..20 + length]).expect("the JSON chunk is UTF-8");
    serde_json::from_str(text.trim_end()).expect("the JSON chunk parses")
}

fn export(drawable: &RmcDrawable<Gen8>) -> Value {
    let scene = drawable_scene(
        "subject",
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    let files = gltf::write(&scene, "subject", Container::Glb);
    document(&files.primary)
}

#[test]
fn a_triangle_survives_the_trip_out() {
    let doc = export(&drawable(false, false));

    assert_eq!(doc["asset"]["version"], "2.0");
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
    let primitive = &doc["meshes"][0]["primitives"][0];
    let attributes = primitive["attributes"].as_object().unwrap();
    assert!(attributes.contains_key("POSITION"));
    assert!(attributes.contains_key("NORMAL"));
    assert!(attributes.contains_key("TEXCOORD_0"));
    assert!(
        !attributes.contains_key("JOINTS_0"),
        "an unskinned geometry must not claim influences"
    );

    let positions = &doc["accessors"][attributes["POSITION"].as_u64().unwrap() as usize];
    assert_eq!(positions["count"], 3);
    assert_eq!(positions["type"], "VEC3");
    assert_eq!(positions["min"][0], 0.0);
    assert_eq!(positions["max"][0], 1.0);
    assert_eq!(positions["max"][2], 1.0);

    let indices = &doc["accessors"][primitive["indices"].as_u64().unwrap() as usize];
    assert_eq!(indices["count"], 3);
    assert_eq!(indices["type"], "SCALAR");
}

/// The two counts decide, not `primitive_type`.
#[test]
fn the_shipped_primitive_type_still_reads_as_a_triangle_list() {
    use or_interchange::extract::is_triangle_list;

    assert!(is_triangle_list(&triangle(false)));

    let mut short = triangle(false);
    short.index_buffer.indices.push(0);
    assert!(
        !is_triangle_list(&short),
        "four indices are not a whole number of triangles"
    );

    let mut silent = triangle(false);
    silent.primitive_type = 0;
    assert!(
        is_triangle_list(&silent),
        "with no primitive count, a multiple of three is the best the file says"
    );
}

/// ⚠ The basis is a rotation carried on one node, so it must take RAGE's up
/// axis to glTF's and leave the handedness alone. A determinant of −1 would
/// mirror the mesh and invert every triangle's facing at the same time, which
/// renders plausibly and is inside out.
#[test]
fn the_root_basis_takes_z_up_to_y_up_without_mirroring() {
    let q = glam::Quat::from_xyzw(
        BASIS_ROTATION[0],
        BASIS_ROTATION[1],
        BASIS_ROTATION[2],
        BASIS_ROTATION[3],
    );
    assert!(q.is_normalized());

    let up = q * glam::Vec3::new(0.0, 0.0, 1.0);
    assert!((up - glam::Vec3::Y).length() < 1e-6, "RAGE +Z is glTF +Y");

    let forward = q * glam::Vec3::new(0.0, 1.0, 0.0);
    assert!(
        (forward - glam::Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6,
        "RAGE +Y is glTF −Z"
    );

    let basis = glam::Mat3::from_quat(q);
    assert!((basis.determinant() - 1.0).abs() < 1e-6);
}

#[test]
fn a_skinned_geometry_gets_a_skin_and_its_bones() {
    let doc = export(&drawable(true, true));

    let attributes = doc["meshes"][0]["primitives"][0]["attributes"]
        .as_object()
        .unwrap();
    assert!(attributes.contains_key("JOINTS_0"));
    assert!(attributes.contains_key("WEIGHTS_0"));

    let joints = &doc["accessors"][attributes["JOINTS_0"].as_u64().unwrap() as usize];
    assert_eq!(joints["componentType"], 5123, "unsigned short");
    assert_eq!(joints["type"], "VEC4");

    let skins = doc["skins"].as_array().expect("one skin");
    assert_eq!(skins.len(), 1);
    assert_eq!(skins[0]["joints"].as_array().unwrap().len(), 3);
    assert!(skins[0]["inverseBindMatrices"].is_number());

    // The bone hierarchy reaches glTF as nodes, parent to child.
    let nodes = doc["nodes"].as_array().unwrap();
    let root = nodes
        .iter()
        .find(|n| n["name"] == "root")
        .expect("the root bone is a node");
    assert_eq!(root["children"].as_array().unwrap().len(), 1);
}

/// A ped component `.ydd` states palette indices into a skeleton in another
/// file, which glTF cannot say.
#[test]
fn skinning_without_a_skeleton_is_withheld_and_reported() {
    let subject = drawable(true, false);
    let scene = drawable_scene("subject", &subject, &ExtractOptions::default());

    assert_eq!(scene.report.skinned_without_a_skeleton, 1);
    assert!(
        scene.primitives().any(|p| p.is_skinned()),
        "the model keeps the influences whatever glTF can carry"
    );

    let doc = export(&subject);
    let attributes = doc["meshes"][0]["primitives"][0]["attributes"]
        .as_object()
        .unwrap();
    assert!(!attributes.contains_key("JOINTS_0"));
    assert!(!attributes.contains_key("WEIGHTS_0"));
    assert!(doc["skins"].is_null());
}

/// ⚠ An inverse bind matrix must be affine, and a 4x4 inversion does not
/// land there exactly: the validator rejected 17 of `head_010_r.ydd`'s 128
/// bones over a bottom-right element of 1.0000001.
#[test]
fn every_inverse_bind_matrix_is_still_affine() {
    let subject = drawable(true, true);
    let scene = drawable_scene("subject", &subject, &ExtractOptions::default());
    let files = gltf::write(&scene, "subject", Container::Glb);
    let doc = document(&files.primary);

    let accessor = doc["skins"][0]["inverseBindMatrices"].as_u64().unwrap() as usize;
    let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
    let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = doc["accessors"][accessor]["count"].as_u64().unwrap() as usize;

    let json_length = u32::from_le_bytes([
        files.primary[12],
        files.primary[13],
        files.primary[14],
        files.primary[15],
    ]) as usize;
    let bin = &files.primary[20 + json_length + 8..];

    for matrix in 0..count {
        let base = offset + matrix * 64;
        let at = |element: usize| {
            let start = base + element * 4;
            f32::from_le_bytes([bin[start], bin[start + 1], bin[start + 2], bin[start + 3]])
        };
        assert_eq!(at(3), 0.0, "matrix {matrix} element 3");
        assert_eq!(at(7), 0.0, "matrix {matrix} element 7");
        assert_eq!(at(11), 0.0, "matrix {matrix} element 11");
        assert_eq!(at(15), 1.0, "matrix {matrix} element 15");
    }
}

#[test]
fn the_glb_container_is_padded_the_way_the_format_says() {
    let scene = drawable_scene(
        "subject",
        &drawable(false, false),
        &ExtractOptions::default(),
    );
    let files = gltf::write(&scene, "subject", Container::Glb);
    let bytes = &files.primary;

    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
        bytes.len(),
        "the header states the whole file's length"
    );

    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(json_length % 4, 0);
    assert_eq!(&bytes[16..20], b"JSON");
    let chunk = &bytes[20..20 + json_length];
    let document_ends = chunk.iter().rposition(|b| *b == b'}').unwrap();
    assert!(
        chunk[document_ends + 1..].iter().all(|b| *b == b' '),
        "the JSON chunk pads with spaces, not zeroes"
    );

    let bin_at = 20 + json_length;
    let bin_length = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().unwrap()) as usize;
    assert_eq!(bin_length % 4, 0);
    assert_eq!(&bytes[bin_at + 4..bin_at + 8], b"BIN\0");
    assert_eq!(bin_at + 8 + bin_length, bytes.len());

    // ⚠ One document proves nothing about the pad: its JSON may already be a
    // multiple of four. Four names of consecutive length cover every residue,
    // so at least three of these actually carry padding.
    let mut padded = 0;
    for length in 1..=12 {
        let name = "n".repeat(length);
        let scene = drawable_scene(&name, &drawable(false, false), &ExtractOptions::default());
        let files = gltf::write(&scene, &name, Container::Glb);
        let json_length = u32::from_le_bytes(files.primary[12..16].try_into().unwrap()) as usize;
        assert_eq!(json_length % 4, 0);
        let chunk = &files.primary[20..20 + json_length];
        let ends = chunk.iter().rposition(|b| *b == b'}').unwrap();
        if ends + 1 < chunk.len() {
            padded += 1;
            assert!(chunk[ends + 1..].iter().all(|b| *b == b' '));
        }
    }
    assert!(
        padded > 0,
        "none of twelve document lengths needed padding, so nothing above \
         actually checked the pad byte"
    );
}

/// ⚠ Every accessor's offset must be a multiple of its component size, which
/// is what the four-byte view alignment is for. A PNG of odd length between
/// two float runs is how it breaks.
#[test]
fn every_accessor_starts_where_its_component_size_allows() {
    let scene = drawable_scene("subject", &drawable(true, true), &ExtractOptions::default());
    let files = gltf::write(&scene, "subject", Container::Glb);
    let doc = document(&files.primary);

    for accessor in doc["accessors"].as_array().unwrap() {
        let width = match accessor["componentType"].as_u64().unwrap() {
            5123 => 2,
            5125 | 5126 => 4,
            other => panic!("unexpected component type {other}"),
        };
        let view = accessor["bufferView"].as_u64().unwrap() as usize;
        let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0);
        assert_eq!(
            offset % width,
            0,
            "a view at {offset} cannot hold {width}-byte components"
        );
    }
}

/// The separate container names its sidecars and declares the buffer it wrote.
#[test]
fn the_text_container_points_at_the_files_beside_it() {
    let scene = drawable_scene(
        "subject",
        &drawable(false, false),
        &ExtractOptions::default(),
    );
    let files = gltf::write(&scene, "subject", Container::Separate);

    let doc: Value = serde_json::from_slice(&files.primary).expect("the .gltf is JSON");
    let uri = doc["buffers"][0]["uri"].as_str().expect("a buffer uri");
    assert_eq!(uri, "subject.bin");
    assert!(
        files.sidecars.iter().any(|(name, _)| name == uri),
        "the file it names has to be one of the ones it produced"
    );
    assert_eq!(
        doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize,
        files
            .sidecars
            .iter()
            .find(|(name, _)| name == uri)
            .unwrap()
            .1
            .len()
    );
}

/// ⚠ A material states no metallic and no roughness, and glTF's own defaults
/// for both are 1.0 — which renders every exported mesh as dark, rough
/// metal.
#[test]
fn a_material_states_the_pbr_factors_rather_than_taking_gltfs_defaults() {
    let doc = export(&drawable(false, false));
    let material = &doc["materials"][0];
    assert_eq!(material["pbrMetallicRoughness"]["metallicFactor"], 0.0);
    assert_eq!(material["pbrMetallicRoughness"]["roughnessFactor"], 1.0);
    assert_eq!(doc["meshes"][0]["primitives"][0]["material"], 0);
}

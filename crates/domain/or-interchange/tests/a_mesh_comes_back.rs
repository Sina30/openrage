//! A mesh out and back, and every way the way back refuses.
//!
//! Each refusal is reached here: an importer's failure mode is a file that
//! reads back identically in this repository and that the game reads off the
//! end of.

use or_interchange::gltf::{self, Container};
use or_interchange::scene::{LodSlot, Primitive, Scene};
use or_interchange::{drawable_scene, replace_geometry, ExtractOptions, ImportOptions};
use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout, VertexReader};
use or_io::decl::{Gen8, Hash32};
use or_io::gta5::drawable::gen8;
use or_io::gta5::{
    CrBoneData, CrSkeletonData, GrmGeometry, GrmModel, GrmShaderGroup, RmcDrawable, RmcLod,
    SpdAABB, Vec4V,
};

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

/// A geometry of `data` laid out as `channels` declare, drawing `indices`.
fn laid_out(
    channels: &[(GrcFvfChannel, GrcDataSize)],
    data: Vec<u8>,
    indices: Vec<u16>,
    matrix_palette: Vec<u16>,
) -> GrmGeometry<Gen8> {
    GrmGeometry {
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf(channels))),
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices,
            ..Default::default()
        }),
        matrix_palette,
        ..Default::default()
    }
}

/// A geometry declaring position, normal, one UV set and the two skin
/// channels — the shape most shipped geometries have.
fn geometry(vertices: usize, skinned: bool) -> GrmGeometry<Gen8> {
    let mut channels = vec![
        (GrcFvfChannel::Position, GrcDataSize::Float3),
        (GrcFvfChannel::Normal, GrcDataSize::Float3),
    ];
    if skinned {
        channels.push((GrcFvfChannel::Weight, GrcDataSize::Color));
        channels.push((GrcFvfChannel::Binding, GrcDataSize::UByte4));
    }
    channels.push((GrcFvfChannel::Tex0, GrcDataSize::Float2));

    let mut data = Vec::new();
    for index in 0..vertices {
        let along = index as f32;
        for axis in [along, 0.0, along * 0.5] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        for axis in [0.0f32, 1.0, 0.0] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        if skinned {
            data.extend_from_slice(&[255u8, 0, 0, 0]);
            data.extend_from_slice(&[(index % 2) as u8, 0, 0, 0]);
        }
        for axis in [along * 0.25, 0.5f32] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
    }

    let triangles = vertices / 3;
    let indices: Vec<u16> = (0..triangles * 3).map(|i| i as u16).collect();
    let palette = if skinned { vec![0, 1] } else { Vec::new() };
    laid_out(&channels, data, indices, palette)
}

fn model(geometries: Vec<GrmGeometry<Gen8>>) -> RmcLod<Gen8> {
    let count = geometries.len();
    RmcLod {
        models: vec![Box::new(GrmModel {
            aabbs: vec![SpdAABB::default(); or_io::codec::mesh::aabb_slots(count)],
            shader_indices: vec![0; count],
            geometries: geometries.into_iter().map(Box::new).collect(),
            ..Default::default()
        })],
        ..Default::default()
    }
}

fn drawable(geometries: Vec<GrmGeometry<Gen8>>, skinned: bool) -> RmcDrawable<Gen8> {
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
    out.lod_group.lods[0] = model(geometries).into();
    if skinned {
        out.skeleton = Some(Box::new(skeleton(2)));
    }
    out
}

fn skeleton(bones: usize) -> CrSkeletonData {
    CrSkeletonData {
        bones: (0..bones)
            .map(|i| CrBoneData {
                name: format!("bone{i}"),
                bone_id: i as u16,
                index: i as u16,
                parent_index: if i == 0 { -1 } else { 0 },
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
            })
            .collect(),
        parent_indices: (0..bones).map(|i| if i == 0 { -1 } else { 0 }).collect(),
        ..Default::default()
    }
}

/// Export and read straight back, which is the path a user takes.
fn through_gltf(drawable: &RmcDrawable<Gen8>) -> Scene {
    let exported = drawable_scene(
        "subject",
        drawable,
        &ExtractOptions {
            textures: false,
            ..ExtractOptions::default()
        },
    );
    let files = gltf::write(&exported, "subject", Container::Glb);
    gltf::read::load(std::path::Path::new("subject.glb"), &files.primary)
        .expect("what the writer wrote, the reader reads")
}

/// Every geometry of every level, in walk order.
fn geometries(drawable: &RmcDrawable<Gen8>) -> Vec<&GrmGeometry<Gen8>> {
    drawable
        .lod_group
        .lods
        .iter()
        .flat_map(|lod| lod.models.iter())
        .flat_map(|model| model.geometries.iter().map(|g| &**g))
        .collect()
}

fn reader(geometry: &GrmGeometry<Gen8>) -> VertexReader<'_> {
    VertexReader::new(&*geometry.vertex_buffer).unwrap()
}

fn positions(drawable: &RmcDrawable<Gen8>) -> Vec<[f32; 3]> {
    geometries(drawable)
        .into_iter()
        .flat_map(|g| reader(g).positions().collect::<Vec<_>>())
        .collect()
}

/// The skeleton bone each vertex's first weighted lane names, resolved
/// through whatever palette the geometry happens to carry.
fn first_bones(drawable: &RmcDrawable<Gen8>) -> Vec<u16> {
    let mut out = Vec::new();
    for geometry in geometries(drawable) {
        let reader = reader(geometry);
        for vertex in 0..reader.count() {
            let slot =
                or_interchange::extract::channel_lanes(&reader, GrcFvfChannel::Binding, vertex)[0];
            out.push(
                geometry
                    .matrix_palette
                    .get(usize::from(slot))
                    .copied()
                    .unwrap_or(u16::MAX),
            );
        }
    }
    out
}

#[test]
fn a_mesh_out_and_back_is_the_same_mesh() {
    let before = drawable(vec![geometry(9, false), geometry(6, false)], false);
    let scene = through_gltf(&before);
    let (after, report) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("the round trip");

    assert_eq!(report.geometries_replaced, 2);
    assert_eq!(report.vertices_before, 15);
    assert_eq!(report.vertices_after, 15);
    assert!(report.channels_filled.is_empty(), "nothing was invented");

    // ⚠ Position is `Float3` on both sides, so this is exact rather than
    // within a tolerance. A tolerance here would hide a basis that was
    // applied twice.
    assert_eq!(positions(&after), positions(&before));

    for (old, new) in geometries(&before).into_iter().zip(geometries(&after)) {
        assert_eq!(reader(new).count(), reader(old).count());
        assert_eq!(reader(new).stride(), reader(old).stride());
        assert_eq!(new.primitive_count(), old.primitive_count());
        assert_eq!(
            new.index_buffer.indices, old.index_buffer.indices,
            "winding needs no flip, so the indices come back in order"
        );
    }
}

/// An import rebuilds the palette from the influences it was given, so the
/// resolved bone is compared rather than the palette.
#[test]
fn a_skinned_mesh_keeps_the_bone_each_vertex_names() {
    let before = drawable(vec![geometry(9, true)], true);
    let scene = through_gltf(&before);
    let (after, report) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("the round trip");

    assert_eq!(report.palettes_rebuilt, 1);
    assert_eq!(first_bones(&after), first_bones(&before));
}

/// Geometry and model boxes are derivable and the drawable's is authored.
#[test]
fn an_import_reboxes_the_model_and_leaves_the_drawable_box_alone() {
    let mut before = drawable(vec![geometry(9, false)], false);
    let corner = |at: f32| Vec4V {
        x: at,
        y: at,
        z: at,
        ..Default::default()
    };
    let authored = SpdAABB {
        min: corner(-500.0),
        max: corner(500.0),
        ..Default::default()
    };
    before.lod_group.bounds_box = authored.clone();

    let scene = through_gltf(&before);
    let (after, report) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("the round trip");

    assert!(report.drawable_box_left_alone);
    assert_eq!(report.models_reboxed, 1);
    assert_eq!(
        after.lod_group.bounds_box, authored,
        "the drawable's own box is authored and must survive an import untouched"
    );

    let model = &after.lod_group.lods[0].models[0];
    let computed = reader(&model.geometries[0]).aabb().unwrap();
    let stated = &model.aabbs[0];
    assert_eq!(
        (
            [stated.min.x, stated.min.y, stated.min.z],
            [stated.max.x, stated.max.y, stated.max.z]
        ),
        (computed.min, computed.max),
        "the model's box came from its own vertices"
    );
    assert_eq!(model.aabbs.len(), 1, "one geometry ships one box");
    assert_ne!(model.aabbs[0], authored);
}

#[test]
fn a_channel_the_geometry_declares_and_the_mesh_omits_is_refused() {
    let before = drawable(vec![geometry(9, false)], false);
    let mut scene = through_gltf(&before);
    // Drop the normals the target's FVF declares.
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                primitive.normals.clear();
            }
        }
    }

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a missing declared channel must refuse")
        .to_string();
    assert!(error.contains("normal channel"), "{error}");
    // ⚠ The app shows this text in its import question, which asks in words and never by a flag.
    assert!(
        error.contains("accept a stated default") && !error.contains("--"),
        "the way out is named in words: {error}"
    );

    // ⚠ And with the option it is accepted *and named*, so a filled channel
    // is never silent.
    let (_, report) = replace_geometry(
        &before,
        &scene,
        &ImportOptions {
            fill_missing: true,
            ..ImportOptions::default()
        },
    )
    .expect("the option accepts it");
    assert_eq!(report.channels_filled.len(), 1);
    assert!(report.channels_filled[0].contains("normal"));
}

#[test]
fn a_mesh_with_the_wrong_number_of_geometries_is_refused() {
    let before = drawable(vec![geometry(9, false), geometry(6, false)], false);
    let mut scene = through_gltf(&before);
    scene.objects[0].meshes[0].primitives.pop();

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a geometry count mismatch must refuse")
        .to_string();
    assert!(error.contains("2 geometries"), "{error}");
    assert!(error.contains("authoring rather than import"), "{error}");
}

/// ⚠ `vertices_count` is a `u16` and the index payload is 16 bit beside it,
/// so this is the ceiling an import meets and an export does not: the
/// largest vertex run in the staged corpus is 5,954. A mesh with triangles
/// is cut to fit (`pairing_by_name.rs`); one with none has nothing to cut.
#[test]
fn a_mesh_past_the_sixteen_bit_ceiling_with_no_triangle_is_still_refused() {
    let before = drawable(vec![geometry(9, false)], false);
    let mut scene = through_gltf(&before);
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    primitive.positions = vec![[0.0; 3]; 70_000];
    primitive.normals = vec![[0.0, 0.0, 1.0]; 70_000];
    primitive.uvs[0] = vec![[0.0; 2]; 70_000];
    primitive.indices = Vec::new();

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("70,000 vertices and no triangle must refuse")
        .to_string();
    assert!(error.contains("70000"), "{error}");
    assert!(error.contains("u16"), "{error}");
}

/// ⚠ A file whose meshes were reordered after export cannot be paired, and
/// pairing it positionally would put one geometry's vertices under another's
/// material.
#[test]
fn a_reordered_mesh_is_refused_rather_than_paired_positionally() {
    let before = drawable(vec![geometry(9, false), geometry(6, false)], false);
    let mut scene = through_gltf(&before);
    scene.objects[0].meshes[0].primitives.swap(0, 1);

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a reordered mesh must refuse")
        .to_string();
    assert!(error.contains("order changed"), "{error}");
}

/// ⚠ A file that never stated where it came from is paired positionally,
/// which is the third-party case. `None` and `Some((0, 0))` are different
/// answers and this is why.
#[test]
fn a_mesh_that_never_said_where_it_came_from_is_paired_positionally() {
    let before = drawable(vec![geometry(9, false), geometry(6, false)], false);
    let mut scene = through_gltf(&before);
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                primitive.stated_address = None;
            }
        }
    }
    assert!(replace_geometry(&before, &scene, &ImportOptions::default()).is_ok());
}

/// ⚠ Weights are eight-bit fractions and the engine blends by the stored
/// bytes, so four lanes rounded on their own sum to 254 or 256 and scale
/// every bone at once.
#[test]
fn quantised_weights_sum_to_the_whole() {
    let before = drawable(vec![geometry(9, true)], true);
    let mut scene = through_gltf(&before);
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    let vertices = primitive.positions.len();
    // A third each: 85 is 255/3, but 0.333... floors to 84 on every lane.
    primitive.weights = vec![[1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 0.0]; vertices];
    primitive.joints = vec![[0, 1, 0, 0]; vertices];

    let (after, _) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("a weighted import");

    let reader = reader(&after.lod_group.lods[0].models[0].geometries[0]);
    for vertex in 0..reader.count() {
        let lanes = or_interchange::extract::channel_lanes(&reader, GrcFvfChannel::Weight, vertex);
        let sum: u32 = lanes.iter().map(|l| u32::from(*l)).sum();
        assert_eq!(sum, 255, "vertex {vertex} weights {lanes:?}");
    }
}

/// ⚠ A mirroring node transform reverses which side a winding describes, so
/// the indices must reverse with it. glTF's own winding rule is the one the
/// export measured as already RAGE's, and a negative determinant is the one
/// case where that stops holding.
#[test]
fn a_mirrored_transform_reverses_the_winding() {
    let before = drawable(vec![geometry(9, false)], false);
    let exported = drawable_scene(
        "subject",
        &before,
        &ExtractOptions {
            textures: false,
            ..ExtractOptions::default()
        },
    );
    let files = gltf::write(&exported, "subject", Container::Glb);

    // Mirror the scene root, which every mesh hangs under.
    let mut document: serde_json::Value = {
        let length = u32::from_le_bytes(files.primary[12..16].try_into().unwrap()) as usize;
        let text = std::str::from_utf8(&files.primary[20..20 + length]).unwrap();
        serde_json::from_str(text.trim_end()).unwrap()
    };
    document["nodes"][0]["scale"] = serde_json::json!([-1.0, 1.0, 1.0]);
    let json = serde_json::to_vec(&document).unwrap();
    let mirrored = glb(&json, &files.primary);

    let scene = gltf::read::load(std::path::Path::new("m.glb"), &mirrored).expect("reads");
    let primitive = &scene.objects[0].meshes[0].primitives[0];

    let original: Vec<u32> = (0..primitive.indices.len() as u32).collect();
    assert_ne!(
        primitive.indices, original,
        "a mirrored transform must not leave the winding as it was"
    );
    assert_eq!(
        primitive.indices[0..3],
        [0, 2, 1],
        "the triangle's second and third corners swap"
    );
}

/// Rebuild a `.glb` around a replacement JSON chunk, keeping the original BIN.
fn glb(json: &[u8], original: &[u8]) -> Vec<u8> {
    let old_json = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
    let bin = &original[20 + old_json..];
    let json_pad = (4 - json.len() % 4) % 4;
    let mut out = Vec::new();
    out.extend_from_slice(&original[0..4]);
    out.extend_from_slice(&2u32.to_le_bytes());
    let total = 12 + 8 + json.len() + json_pad + bin.len();
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&((json.len() + json_pad) as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json);
    out.extend(std::iter::repeat_n(b' ', json_pad));
    out.extend_from_slice(bin);
    out
}

/// ⚠ An embedded data uri is refused by name rather than half-decoded.
#[test]
fn an_embedded_data_uri_is_refused_by_name() {
    let document = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": 4, "uri": "data:application/octet-stream;base64,AAAA" }],
    });
    let text = serde_json::to_vec(&document).unwrap();
    let error = gltf::read::load(std::path::Path::new("e.gltf"), &text)
        .expect_err("a data uri must refuse")
        .to_string();
    assert!(error.contains("data uri"), "{error}");
    assert!(error.contains(".glb"), "the way out is named: {error}");
}

/// ⚠ An accessor with no bufferView reads as zeroes by definition, which for
/// a position channel is a mesh collapsed to the origin. Refusing says so.
#[test]
fn an_accessor_with_no_buffer_view_is_refused() {
    let document = serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 0 }] }],
        "accessors": [{ "componentType": 5126, "count": 3, "type": "VEC3" }],
    });
    let text = serde_json::to_vec(&document).unwrap();
    let error = gltf::read::load(std::path::Path::new("z.gltf"), &text)
        .expect_err("a view-less accessor must refuse")
        .to_string();
    assert!(error.contains("read as zero"), "{error}");
}

/// The LOD a primitive says it came from survives the trip, so a multi-LOD
/// drawable pairs correctly rather than collapsing onto LOD 0.
#[test]
fn the_lod_a_mesh_came_from_survives_the_trip() {
    let mut before = drawable(vec![geometry(9, false)], false);
    before.lod_group.lods[2] = model(vec![geometry(6, false)]).into();

    let exported = drawable_scene(
        "subject",
        &before,
        &ExtractOptions {
            lods: or_interchange::LodSelection::All,
            textures: false,
        },
    );
    let files = gltf::write(&exported, "subject", Container::Glb);
    let scene = gltf::read::load(std::path::Path::new("l.glb"), &files.primary).unwrap();

    let lods: Vec<LodSlot> = scene
        .objects
        .iter()
        .flat_map(|o| o.meshes.iter())
        .map(|m| m.lod)
        .collect();
    assert!(lods.contains(&LodSlot::Level(0)));
    assert!(lods.contains(&LodSlot::Level(2)), "{lods:?}");

    let (_, report) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("both LODs pair");
    assert_eq!(report.geometries_replaced, 2);
    assert_eq!(report.models_reboxed, 2);
}

/// The reader snaps a near-identity transform to identity; a transform that
/// is genuinely something else must survive.
#[test]
fn a_transform_that_is_really_a_transform_is_not_snapped_away() {
    let before = drawable(vec![geometry(9, false)], false);
    let exported = drawable_scene(
        "subject",
        &before,
        &ExtractOptions {
            textures: false,
            ..ExtractOptions::default()
        },
    );
    let files = gltf::write(&exported, "subject", Container::Glb);

    let length = u32::from_le_bytes(files.primary[12..16].try_into().unwrap()) as usize;
    let mut document: serde_json::Value = serde_json::from_str(
        std::str::from_utf8(&files.primary[20..20 + length])
            .unwrap()
            .trim_end(),
    )
    .unwrap();
    // Ten metres along X, which is not a rounding of anything.
    document["nodes"][0]["translation"] = serde_json::json!([10.0, 0.0, 0.0]);
    let moved = glb(&serde_json::to_vec(&document).unwrap(), &files.primary);

    let scene = gltf::read::load(std::path::Path::new("t.glb"), &moved).unwrap();
    let first = scene.objects[0].meshes[0].primitives[0].positions[0];
    assert!(
        (first[0] - 10.0).abs() < 1e-5,
        "a real translation must survive: {first:?}"
    );
}

/// A drawable skinned against a skeleton it does not carry cannot get its
/// influences back through glTF; keeping the file's own is opt-in and reported.
#[test]
fn skinning_that_could_not_be_exported_is_kept_only_when_asked() {
    let mut before = drawable(vec![geometry(9, true)], true);
    // No skeleton, which is what makes the export withhold the attributes.
    before.skeleton = None;

    let scene = through_gltf(&before);
    assert!(
        scene.objects[0].meshes[0].primitives[0].joints.is_empty(),
        "the export withholds influences it cannot place"
    );

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("by default the missing channel refuses")
        .to_string();
    assert!(error.contains("weight channel"), "{error}");

    let (after, report) = replace_geometry(
        &before,
        &scene,
        &ImportOptions {
            keep_skinning: true,
            ..ImportOptions::default()
        },
    )
    .expect("keeping what the file had");
    assert_eq!(report.skinning_kept, 1);
    assert_eq!(
        first_bones(&after),
        first_bones(&before),
        "the influences already in the file came across unchanged"
    );
}

#[test]
fn kept_skinning_refuses_when_the_vertex_count_moved() {
    let mut before = drawable(vec![geometry(9, true)], true);
    before.skeleton = None;
    let mut scene = through_gltf(&before);

    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    primitive.positions.truncate(6);
    primitive.normals.truncate(6);
    primitive.uvs[0].truncate(6);
    primitive.indices.truncate(6);

    let error = replace_geometry(
        &before,
        &scene,
        &ImportOptions {
            keep_skinning: true,
            ..ImportOptions::default()
        },
    )
    .expect_err("a changed vertex count must refuse")
    .to_string();
    assert!(
        error.contains("6 vertices where the geometry had 9"),
        "{error}"
    );
    assert!(
        error.contains("re-author the skin"),
        "the way out is named: {error}"
    );
}

/// ⚠ `Binding` is an index payload and `Color` is a normalised channel, and
/// they are the same four bytes: a bone slot of 5 put through the normalised
/// encoder clamps to 1 and writes 255. `des_bridge_root.ydr` declares both
/// skin channels `Color`.
#[test]
fn a_binding_declared_as_a_colour_keeps_its_bone_slots() {
    let channels = [
        (GrcFvfChannel::Position, GrcDataSize::Float3),
        (GrcFvfChannel::Weight, GrcDataSize::Color),
        // The trap: an index payload spelled as a colour.
        (GrcFvfChannel::Binding, GrcDataSize::Color),
    ];

    let mut data = Vec::new();
    for index in 0..6usize {
        for axis in [index as f32, 0.0, 0.0] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        data.extend_from_slice(&[255u8, 0, 0, 0]);
        // Slot 5, which a normalised encoder cannot represent.
        data.extend_from_slice(&[5u8, 0, 0, 0]);
    }

    let subject = laid_out(&channels, data, vec![0, 1, 2, 3, 4, 5], (0..8).collect());
    let mut before = drawable(vec![subject], false);
    before.skeleton = Some(Box::new(skeleton(8)));

    let scene = through_gltf(&before);
    let (after, _) =
        replace_geometry(&before, &scene, &ImportOptions::default()).expect("the round trip");

    let geometry = &after.lod_group.lods[0].models[0].geometries[0];
    let reader = reader(geometry);
    for vertex in 0..reader.count() {
        let slot =
            or_interchange::extract::channel_lanes(&reader, GrcFvfChannel::Binding, vertex)[0];
        assert_eq!(
            geometry.matrix_palette.get(usize::from(slot)).copied(),
            Some(5),
            "vertex {vertex} landed on palette slot {slot}, whose bone is not 5"
        );
    }
}

/// A primitive the reader produced carries its own addressing, so the
/// positional pairing has something to cross-check against.
#[test]
fn a_read_primitive_carries_the_address_the_export_stated() {
    let before = drawable(vec![geometry(9, false), geometry(6, false)], false);
    let scene = through_gltf(&before);
    let stated: Vec<Option<(usize, usize)>> = scene
        .primitives()
        .map(|p: &Primitive| p.stated_address)
        .collect();
    assert_eq!(stated, vec![Some((0, 0)), Some((0, 1))]);
}

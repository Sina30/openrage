//! A file that reorders or renames the parts of a drawable is paired by what it calls them, and one that overflows a geometry is split.

use or_interchange::extract::effect_name;
use or_interchange::gltf::{self, Container};
use or_interchange::scene::Scene;
use or_interchange::{
    drawable_scene, replace_geometry, replace_geometry_named, ExtractOptions, ImportNames,
    ImportOptions,
};
use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout, VertexReader};
use or_io::decl::{Gen8, Hash32};
use or_io::gta5::drawable::gen8;
use or_io::gta5::{
    CrBoneData, CrSkeletonData, GrmGeometry, GrmModel, GrmShaderGroup, RmcDrawable, RmcLod,
    SpdAABB, Vec4V,
};

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

/// A geometry of `vertices` vertices along x, shifted by `shift` so two of them are not one geometry.
fn geometry(vertices: usize, skinned: bool, shift: f32) -> GrmGeometry<Gen8> {
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
        let along = index as f32 + shift;
        for axis in [along, 0.0, along * 0.5] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        for axis in [0.0f32, 1.0, 0.0] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        if skinned {
            data.extend_from_slice(&[255u8, 0, 0, 0]);
            data.extend_from_slice(&[0u8, 0, 0, 0]);
        }
        for axis in [along * 0.25, 0.5f32] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
    }
    let triangles = vertices / 3;
    let mut out = GrmGeometry::<Gen8> {
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf(&channels))),
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: (0..triangles * 3).map(|i| i as u16).collect(),
            ..Default::default()
        }),
        matrix_palette: if skinned { vec![0] } else { Vec::new() },
        ..Default::default()
    };
    out.take_counts_from_runs();
    out
}

/// The shaders a drawable's geometries draw with, by the name each is hashed from.
const SHADERS: [&str; 2] = ["default", "default_spec"];

/// Models of geometries, each geometry with the index of the shader it draws with.
fn drawable(models: Vec<Vec<(GrmGeometry<Gen8>, u16)>>) -> RmcDrawable<Gen8> {
    let mut out = RmcDrawable::<Gen8> {
        name: "subject".into(),
        shader_group: Some(Box::new(GrmShaderGroup {
            shaders: SHADERS
                .iter()
                .map(|name| {
                    Box::new(gen8::GrcInstanceData {
                        basis_hash: Hash32::of(name),
                        ..Default::default()
                    })
                })
                .collect(),
            ..Default::default()
        })),
        ..Default::default()
    };
    out.lod_group.lods[0] = RmcLod {
        models: models
            .into_iter()
            .map(|geometries| {
                let count = geometries.len();
                let (geometries, shaders): (Vec<_>, Vec<_>) = geometries.into_iter().unzip();
                Box::new(GrmModel {
                    aabbs: vec![SpdAABB::default(); or_io::codec::mesh::aabb_slots(count)],
                    shader_indices: shaders,
                    geometries: geometries.into_iter().map(Box::new).collect(),
                    ..Default::default()
                })
            })
            .collect(),
        ..Default::default()
    }
    .into();
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

/// The first position of each geometry of the first level, model by model.
fn firsts(drawable: &RmcDrawable<Gen8>) -> Vec<Vec<f32>> {
    drawable.lod_group.lods[0]
        .models
        .iter()
        .map(|model| {
            model
                .geometries
                .iter()
                .map(|g| {
                    VertexReader::new(&*g.vertex_buffer)
                        .unwrap()
                        .positions()
                        .next()
                        .unwrap()[2]
                })
                .collect()
        })
        .collect()
}

/// Lifts every vertex of the primitive `at` of the mesh `mesh` by `by`.
fn lift(scene: &mut Scene, mesh: usize, at: usize, by: f32) {
    for position in &mut scene.objects[0].meshes[mesh].primitives[at].positions {
        position[2] += by;
    }
}

fn two_models() -> RmcDrawable<Gen8> {
    drawable(vec![
        vec![(geometry(6, false, 0.0), 0)],
        vec![(geometry(9, false, 40.0), 0)],
    ])
}

#[test]
fn meshes_a_file_reorders_are_paired_to_their_models_by_name() {
    let before = two_models();
    let mut scene = through_gltf(&before);
    lift(&mut scene, 0, 0, 100.0);
    lift(&mut scene, 1, 0, 200.0);
    scene.objects[0].meshes.reverse();

    let (after, _) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("two meshes named for their models pair whatever order they arrive in");
    let (was, is) = (firsts(&before), firsts(&after));
    assert_eq!(is[0][0], was[0][0] + 100.0, "model0 took model1's mesh");
    assert_eq!(is[1][0], was[1][0] + 200.0, "model1 took model0's mesh");
}

#[test]
fn a_mesh_named_for_no_model_takes_the_model_nothing_else_named() {
    let before = two_models();
    let mut scene = through_gltf(&before);
    lift(&mut scene, 1, 0, 200.0);
    scene.objects[0].meshes[0].name = "Torso".into();
    scene.objects[0].meshes.reverse();

    let (after, _) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("the one mesh that names a model is paired, and the other takes what is left");
    let (was, is) = (firsts(&before), firsts(&after));
    assert_eq!(is[1][0], was[1][0] + 200.0);
    assert_eq!(is[0][0], was[0][0]);
}

#[test]
fn names_the_caller_states_decide_over_the_ones_an_export_writes() {
    let before = two_models();
    let mut scene = through_gltf(&before);
    lift(&mut scene, 0, 0, 100.0);
    scene.objects[0].meshes[0].name = "Torso".into();
    scene.objects[0].meshes[1].name = "Hair".into();
    scene.objects[0].meshes.reverse();

    let names = ImportNames {
        models: vec![
            (or_interchange::LodSlot::Level(0), 0, "Torso".into()),
            (or_interchange::LodSlot::Level(0), 1, "Hair".into()),
        ],
        materials: Vec::new(),
    };
    let (after, _) =
        replace_geometry_named(&before, &scene, &ImportOptions::default(), &names).expect("paired");
    let (was, is) = (firsts(&before), firsts(&after));
    assert_eq!(is[0][0], was[0][0] + 100.0, "Torso is model0");
    assert_eq!(is[1][0], was[1][0]);
}

#[test]
fn primitives_are_paired_to_geometries_by_the_name_of_their_material() {
    let before = drawable(vec![vec![
        (geometry(6, false, 0.0), 0),
        (geometry(9, false, 40.0), 1),
    ]]);
    let mut scene = through_gltf(&before);
    lift(&mut scene, 0, 0, 100.0);
    lift(&mut scene, 0, 1, 200.0);
    scene.objects[0].meshes[0].primitives.reverse();

    let (after, report) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("two primitives with two materials pair by material whatever order they arrive in");
    let (was, is) = (firsts(&before), firsts(&after));
    assert_eq!(
        is[0][0],
        was[0][0] + 100.0,
        "geometry 0 draws with the first shader"
    );
    assert_eq!(is[0][1], was[0][1] + 200.0);
    assert_eq!(report.geometries_replaced, 2);
}

#[test]
fn a_material_a_tool_renamed_with_a_copy_suffix_still_pairs() {
    let before = drawable(vec![vec![
        (geometry(6, false, 0.0), 0),
        (geometry(9, false, 40.0), 1),
    ]]);
    let mut scene = through_gltf(&before);
    lift(&mut scene, 0, 0, 100.0);
    for primitive in &mut scene.objects[0].meshes[0].primitives {
        let name = primitive.material_name.clone().unwrap();
        primitive.material_name = Some(format!("{name}.001"));
    }
    scene.objects[0].meshes[0].primitives.reverse();

    let (after, _) = replace_geometry(&before, &scene, &ImportOptions::default()).expect("paired");
    assert_eq!(firsts(&after)[0][0], firsts(&before)[0][0] + 100.0);
}

#[test]
fn the_name_of_a_material_is_the_shader_it_draws_with() {
    let before = drawable(vec![vec![(geometry(6, false, 0.0), 1)]]);
    let scene = through_gltf(&before);
    let hash = before.shader_group.as_ref().unwrap().shaders[1]
        .basis_hash
        .get();
    assert_eq!(
        scene.objects[0].meshes[0].primitives[0]
            .material_name
            .as_deref(),
        Some(effect_name(hash).as_str())
    );
}

#[test]
fn two_primitives_of_one_material_keep_the_order_their_export_stated() {
    let before = drawable(vec![vec![
        (geometry(6, false, 0.0), 0),
        (geometry(9, false, 40.0), 0),
    ]]);
    let mut scene = through_gltf(&before);
    scene.objects[0].meshes[0].primitives.swap(0, 1);

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a name two geometries share cannot say which is which")
        .to_string();
    assert!(error.contains("order changed"), "{error}");
}

#[test]
fn a_file_naming_nothing_is_paired_by_position_as_it_was() {
    let before = two_models();
    let mut scene = through_gltf(&before);
    lift(&mut scene, 1, 0, 200.0);
    for (at, mesh) in scene.objects[0].meshes.iter_mut().enumerate() {
        mesh.name = format!("Object.{at}");
        for primitive in &mut mesh.primitives {
            primitive.material_name = None;
            primitive.stated_address = None;
        }
    }
    let (after, _) = replace_geometry(&before, &scene, &ImportOptions::default()).expect("paired");
    assert_eq!(firsts(&after)[1][0], firsts(&before)[1][0] + 200.0);
}

#[test]
fn a_model_holding_a_different_number_of_geometries_than_its_mesh_is_refused_by_name() {
    let before = drawable(vec![
        vec![(geometry(6, false, 0.0), 0), (geometry(9, false, 40.0), 1)],
        vec![(geometry(6, false, 80.0), 0)],
    ]);
    let mut scene = through_gltf(&before);
    scene.objects[0].meshes[0].primitives.pop();

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a model's mesh must bring the model's geometries")
        .to_string();
    assert!(error.contains("2 geometries"), "{error}");
    assert!(error.contains("model0"), "{error}");
    assert!(error.contains("authoring rather than import"), "{error}");
}

/// An export writes no mesh for a model with no geometry, so no mesh is owed for it.
#[test]
fn a_model_with_no_geometry_beside_full_ones_is_not_owed_a_mesh() {
    let before = drawable(vec![vec![(geometry(6, false, 0.0), 0)], vec![]]);
    let mut scene = through_gltf(&before);
    assert_eq!(
        scene.objects[0].meshes.len(),
        1,
        "the empty model has no mesh"
    );
    lift(&mut scene, 0, 0, 100.0);

    let (after, report) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("an unedited export of a drawable with an empty model comes back");
    assert_eq!(report.geometries_replaced, 1);
    assert_eq!(firsts(&after)[0][0], firsts(&before)[0][0] + 100.0);
}

#[test]
fn a_mesh_named_for_no_model_never_takes_a_model_with_no_geometry() {
    let before = drawable(vec![
        vec![(geometry(6, false, 0.0), 0)],
        vec![],
        vec![(geometry(9, false, 40.0), 0)],
    ]);
    let mut scene = through_gltf(&before);
    lift(&mut scene, 1, 0, 200.0);
    scene.objects[0].meshes[0].name = "Torso".into();

    let (after, _) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("model2 names its mesh and the one left over takes model0, not the empty model1");
    let (was, is) = (firsts(&before), firsts(&after));
    assert_eq!(is[2][0], was[2][0] + 200.0);
    assert_eq!(is[0][0], was[0][0]);
}

/// A modelling tool that separates an object by material keeps its name, with a copy suffix.
#[test]
fn meshes_a_tool_split_from_one_model_keep_pairing_with_it() {
    let before = drawable(vec![vec![
        (geometry(6, false, 0.0), 0),
        (geometry(9, false, 40.0), 1),
    ]]);
    for stated in [true, false] {
        let mut scene = through_gltf(&before);
        lift(&mut scene, 0, 0, 100.0);
        lift(&mut scene, 0, 1, 200.0);
        let mesh = &mut scene.objects[0].meshes[0];
        let mut second = mesh.clone();
        second.name = format!("{}.001", mesh.name);
        second.primitives = vec![mesh.primitives.pop().unwrap()];
        scene.objects[0].meshes.push(second);
        if !stated {
            for mesh in &mut scene.objects[0].meshes {
                for primitive in &mut mesh.primitives {
                    primitive.stated_address = None;
                    primitive.material_name = None;
                }
            }
        }

        let (after, report) = replace_geometry(&before, &scene, &ImportOptions::default())
            .expect("the pieces of one model's name are one model's geometries");
        let (was, is) = (firsts(&before), firsts(&after));
        assert_eq!(report.geometries_replaced, 2, "stated addresses: {stated}");
        assert_eq!(is[0][0], was[0][0] + 100.0, "stated addresses: {stated}");
        assert_eq!(is[0][1], was[0][1] + 200.0, "stated addresses: {stated}");
    }
}

/// Joining the pieces of one name must not let a copy of a model slip in as geometry.
#[test]
fn a_copy_of_a_model_is_still_refused_as_authoring() {
    let before = drawable(vec![vec![(geometry(6, false, 0.0), 0)]]);
    let mut scene = through_gltf(&before);
    let mut copy = scene.objects[0].meshes[0].clone();
    copy.name = format!("{}.001", copy.name);
    scene.objects[0].meshes.push(copy);

    let error = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect_err("a second copy of the model is a geometry the drawable does not have")
        .to_string();
    assert!(error.contains("authoring rather than import"), "{error}");
}

/// ⚠ `vertices_count` is a `u16` with the index payload 16 bit beside it; a mesh past it is cut at the
/// ceiling into geometries that hold the same shader and declaration, and the report says so.
#[test]
fn a_mesh_past_the_sixteen_bit_ceiling_is_split_into_geometries_and_reported() {
    let before = drawable(vec![vec![
        (geometry(9, false, 0.0), 0),
        (geometry(9, false, 40.0), 1),
    ]]);
    let mut scene = through_gltf(&before);
    let wide = 70_002usize;
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    primitive.positions = (0..wide).map(|i| [i as f32, 0.0, 0.0]).collect();
    primitive.normals = vec![[0.0, 0.0, 1.0]; wide];
    primitive.uvs[0] = vec![[0.0; 2]; wide];
    primitive.indices = (0..wide as u32).collect();

    let (after, report) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("a mesh past the ceiling is split and not refused");
    let model = &after.lod_group.lods[0].models[0];
    assert!(
        model.geometries.len() > 2,
        "{} geometries",
        model.geometries.len()
    );
    assert_eq!(model.shader_indices.len(), model.geometries.len());
    let held: usize = model.geometries[..model.geometries.len() - 1]
        .iter()
        .map(|g| VertexReader::new(&*g.vertex_buffer).unwrap().count())
        .sum();
    assert!(model.geometries.iter().all(|g| {
        let count = VertexReader::new(&*g.vertex_buffer).unwrap().count();
        count > 0 && count < 65_536
    }));
    assert_eq!(held, wide, "every vertex of a triangle lands in some piece");
    // The geometry the mesh did not overflow keeps its place and its shader, after the ones that split.
    assert_eq!(*model.shader_indices.last().unwrap(), 1);
    assert!(model.shader_indices[..model.shader_indices.len() - 1]
        .iter()
        .all(|s| *s == 0));
    assert_eq!(report.geometries_split, 1);
    let said = report.lines().join("\n");
    assert!(
        said.contains("lod0/model0/geometry0") && said.contains("split"),
        "{said}"
    );
    assert_eq!(
        model.aabbs.len(),
        model.geometries.len() + 1,
        "a box each and the union"
    );
}

/// ⚠ A `Binding` lane addresses the palette with one byte, so a geometry reaches 256 bones.
#[test]
fn a_mesh_weighted_past_the_palette_ceiling_is_split_by_bones_and_reported() {
    let mut before = drawable(vec![vec![(geometry(9, true, 0.0), 0)]]);
    before.skeleton = Some(Box::new(skeleton(600)));
    let mut scene = through_gltf(&before);
    let primitive = &mut scene.objects[0].meshes[0].primitives[0];
    let wide = 900usize;
    primitive.positions = vec![[0.0; 3]; wide];
    primitive.normals = vec![[0.0, 0.0, 1.0]; wide];
    primitive.uvs[0] = vec![[0.0; 2]; wide];
    // Every triangle takes three fresh bones of its own, so 300 triangles want 900 of 600.
    primitive.joints = (0..wide).map(|i| [(i % 600) as u16, 0, 0, 0]).collect();
    primitive.weights = vec![[1.0, 0.0, 0.0, 0.0]; wide];
    primitive.indices = (0..wide as u32).collect();

    let (after, report) = replace_geometry(&before, &scene, &ImportOptions::default())
        .expect("a mesh weighted to more bones than a palette holds is split and not refused");
    let model = &after.lod_group.lods[0].models[0];
    assert!(
        model.geometries.len() >= 3,
        "{} geometries",
        model.geometries.len()
    );
    assert!(model
        .geometries
        .iter()
        .all(|g| g.matrix_palette.len() <= 256));
    assert_eq!(report.geometries_split, 1);
    assert!(
        report.lines().join("\n").contains("bones"),
        "{:?}",
        report.lines()
    );
}

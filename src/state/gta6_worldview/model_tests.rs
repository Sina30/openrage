use or_io::gta6::headerless::{
    Geometry, HeaderlessError, Model, Palette, Parameter, ParameterValue, Shader, Vertices,
};

use super::*;

/// A triangle drawn with shader `shader`; its positions refused where `read` is false.
fn geometry(shader: u16, read: bool) -> Geometry {
    Geometry {
        shader,
        vertex_count: 3,
        channels: Vec::new(),
        triangles: vec![[0, 1, 2]],
        positions: match read {
            true => Ok(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
            false => Err(HeaderlessError::Positions {
                why: "the decoded vertices do not reach the geometry's box",
            }),
        },
        vertices: Vertices::default(),
    }
}

fn level(geometries: Vec<Geometry>) -> Option<Vec<Model>> {
    Some(vec![Model {
        geometries: geometries.into_iter().map(Ok).collect(),
        palette: Ok(Palette::Own),
        matrix_index: 0,
    }])
}

fn drawable(
    shaders: Vec<Shader>,
    levels: [Option<Vec<Model>>; 4],
    distances: [f32; 4],
) -> Drawable {
    Drawable {
        name: "box.#dr".to_string(),
        box_min: [0.0; 3],
        box_max: [1.0, 1.0, 0.0],
        shaders,
        levels,
        lod_distances: distances,
        bound: None,
        embedded: None,
        extra_skeletons: Vec::new(),
    }
}

/// A shader whose texture records state `records`, each with its key, then `constants`.
fn shader(records: &[(u32, Option<u64>)], constants: &[(u32, f32)]) -> Shader {
    let textures = (records.iter().zip(0u8..)).map(|((hash, _), slot)| Parameter {
        hash: *hash,
        value: ParameterValue::Texture { slot },
    });
    let constants = constants.iter().map(|(hash, value)| Parameter {
        hash: *hash,
        value: ParameterValue::Constant {
            buffer: 0,
            offset: 0,
            words: vec![value.to_bits()],
        },
    });
    Shader {
        name_hash: 1,
        texture_keys: records.iter().map(|(_, key)| *key).collect(),
        parameters: textures.chain(constants).collect(),
    }
}

fn plain() -> Shader {
    shader(&[], &[])
}

const FAR: f32 = 9998.0;

#[test]
fn a_model_holds_every_level_its_body_does_each_drawn_to_its_distance_and_the_last_for_ever() {
    let levels = [
        level(vec![geometry(0, true), geometry(0, true)]),
        level(vec![geometry(0, true)]),
        level(vec![geometry(0, true)]),
        None,
    ];
    let model = model_of(
        &drawable(vec![plain()], levels, [25.0, 80.0, FAR, FAR]),
        &[],
        &|_| None,
    );
    let read: Vec<(f32, usize)> = (model.lods.iter())
        .map(|lod| (lod.until, lod.meshes.len()))
        .collect();
    assert_eq!(read, [(25.0, 2), (80.0, 1), (f32::INFINITY, 1)]);
    assert_eq!(model.lods[0].meshes[0].indices, [0, 1, 2]);
    // Four triangles of three vertices and three indices each.
    let vertex = size_of::<MeshVertex>() as u64;
    assert_eq!(model.bytes(), 4 * (3 * vertex + 3 * 4));

    // One level alone is drawn for ever, whatever distance its body states for it.
    let one = [level(vec![geometry(0, true)]), None, None, None];
    let model = model_of(
        &drawable(vec![plain()], one, [40.0, FAR, FAR, FAR]),
        &[],
        &|_| None,
    );
    assert_eq!(model.lods.len(), 1);
    assert_eq!(model.lods[0].until, f32::INFINITY);
}

#[test]
fn a_level_none_of_whose_geometries_read_is_left_out_and_a_model_with_none_keeps_one_empty_level() {
    let levels = [
        level(vec![geometry(0, true)]),
        level(vec![geometry(0, false)]),
        level(vec![geometry(0, true)]),
        None,
    ];
    let model = model_of(
        &drawable(vec![plain()], levels, [25.0, 80.0, FAR, FAR]),
        &[],
        &|_| None,
    );
    let until: Vec<f32> = model.lods.iter().map(|lod| lod.until).collect();
    assert_eq!(until, [25.0, f32::INFINITY]);

    let none = [level(vec![geometry(0, false)]), None, None, None];
    let model = model_of(&drawable(vec![plain()], none, [FAR; 4]), &[], &|_| None);
    assert_eq!(model.lods.len(), 1);
    assert!(model.lods[0].meshes.is_empty());
    assert_eq!(model.lods[0].until, f32::INFINITY);
    assert_eq!(model.bytes(), 0);
}

#[test]
fn a_mesh_takes_the_maps_and_the_bucket_of_its_own_shader_past_a_geometry_that_was_refused() {
    let shaders = vec![
        shader(&[(COLOUR[0], Some(0xA))], &[]),
        shader(&[(COLOUR[0], Some(0xB))], &[]),
        shader(&[(COLOUR[0], Some(0xC))], &[]),
    ];
    let levels = [
        level(vec![
            geometry(0, true),
            geometry(1, false),
            geometry(2, true),
        ]),
        None,
        None,
        None,
    ];
    let model = model_of(&drawable(shaders, levels, [FAR; 4]), &[0, 2, 3], &|_| None);
    let drawn: Vec<(Option<u64>, Bucket)> = (model.lods[0].meshes.iter())
        .map(|mesh| (mesh.maps.colour, mesh.bucket))
        .collect();
    assert_eq!(
        drawn,
        [(Some(0xA), Bucket::Opaque), (Some(0xC), Bucket::Cutout)]
    );
}

#[test]
fn each_map_comes_from_its_own_record_and_a_layered_material_gives_its_first_layer() {
    let (colour, normal, specular, other) = (0xC0105_u64, 0xB0B_u64, 0x5EC_u64, 0x07E_u64);
    // Every key in a pack, `colour` and `other` as sRGB.
    let packed = |key: u64| Some(key == colour || key == other);
    let unknown = 0x1234_5678;
    let plain = shader(
        &[
            (unknown, Some(other)),
            (SPECULAR, Some(specular)),
            (NORMAL[0], Some(normal)),
            (COLOUR[0], Some(colour)),
        ],
        &[],
    );
    let all = Maps {
        colour: Some(colour),
        normal: Some(normal),
        specular: Some(specular),
    };
    assert_eq!(maps_of(&plain, &packed), all);
    for layers in [1, 2] {
        let layered = shader(
            &[
                (unknown, Some(other)),
                (NORMAL[layers], Some(normal)),
                (COLOUR[layers], Some(colour)),
            ],
            &[],
        );
        let first = Maps {
            specular: None,
            ..all
        };
        assert_eq!(maps_of(&layered, &packed), first, "{layers}");
    }
    // A colour record whose slot is unbound gives way to the first key a pack states as sRGB.
    let unbound = shader(&[(COLOUR[0], None), (unknown, Some(other))], &[]);
    assert_eq!(maps_of(&unbound, &packed).colour, Some(other));
}

#[test]
fn a_material_with_a_normal_map_alone_gets_no_colour_and_no_normal_map_stands_in_for_one() {
    let (normal, specular, data) = (0xB0B_u64, 0x5EC_u64, 0xDA7A_u64);
    let bumped = shader(
        &[(NORMAL[0], Some(normal)), (SPECULAR, Some(specular))],
        &[],
    );
    // Its keys in a pack that states them linear: nothing is a colour.
    let maps = maps_of(&bumped, &|_| Some(false));
    assert_eq!((maps.colour, maps.normal), (None, Some(normal)));
    // With no pack indexed nothing is known of a key: the maps its records name are still no colour.
    assert_eq!(maps_of(&bumped, &|_| None).colour, None);
    // A key no record names and no pack is known to hold is the one asked for as the colour.
    let more = shader(&[(NORMAL[0], Some(normal)), (0x1234_5678, Some(data))], &[]);
    assert_eq!(maps_of(&more, &|_| None).colour, Some(data));
    assert_eq!(maps_of(&more, &|_| Some(false)).colour, None);
}

#[test]
fn the_bucket_is_the_draw_bucket_byte_under_its_batch_bit_and_a_water_mask_draws_nothing() {
    let of = |byte: u8| Bucket::of(byte, false);
    assert_eq!(of(0), Some(Bucket::Opaque));
    assert_eq!(of(1), Some(Bucket::Blend));
    assert_eq!(of(2), Some(Bucket::Decal));
    assert_eq!(of(3), Some(Bucket::Cutout));
    assert_eq!(of(6), Some(Bucket::Blend));
    assert_eq!((of(4), of(5)), (None, None));
    for byte in 0..8 {
        assert_eq!(of(0x80 | byte), of(byte), "{byte}");
    }

    let glow = |strength: f32| shader(&[], &[(GLOW, strength)]);
    let levels = || {
        let geometries = (0..5).map(|shader| geometry(shader, true)).collect();
        [level(geometries), None, None, None]
    };
    let shaders = vec![glow(5.0), glow(0.0), glow(5.0), glow(5.0), plain()];
    let model = model_of(
        &drawable(shaders, levels(), [FAR; 4]),
        &[0x80, 0, 3, 1, 4],
        &|_| None,
    );
    let buckets: Vec<Bucket> = model.lods[0].meshes.iter().map(|m| m.bucket).collect();
    // A glow makes an opaque material emissive and leaves a cutout or blended one as it is; the mask is gone.
    assert_eq!(
        buckets,
        [
            Bucket::Emissive,
            Bucket::Opaque,
            Bucket::Cutout,
            Bucket::Blend
        ]
    );
    // Shaders past the bytes read are opaque.
    let model = model_of(
        &drawable(vec![plain(); 5], levels(), [FAR; 4]),
        &[],
        &|_| None,
    );
    assert!(model.lods[0]
        .meshes
        .iter()
        .all(|m| m.bucket == Bucket::Opaque));
    assert_eq!(model.lods[0].meshes.len(), 5);
}

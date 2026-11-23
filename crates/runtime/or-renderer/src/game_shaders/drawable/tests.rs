use or_interchange::{Image as SceneImage, LodSelection, Object, TextureBinding};

use super::*;

/// A texture of two pixels square in `colour`, by `name`.
fn pixels_by_name(name: &str, colour: [u8; 4]) -> Textures {
    [(
        name.to_string(),
        Image {
            width: 2,
            height: 2,
            rgba: colour.repeat(4),
            srgb: false,
        },
    )]
    .into_iter()
    .collect()
}

fn png_of(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba(pixel))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("the picture encodes");
    out.into_inner()
}

fn primitive() -> Primitive {
    Primitive {
        material: Some(0),
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        // The drawable skips `Tex0`, so its first texture coordinate is `Tex1` and the next `Tex3`.
        uvs: vec![vec![[0.25, 0.5]; 3], vec![[0.75, 0.125]; 3]],
        uv_channels: vec![7, 9],
        colours: vec![vec![[1.0, 0.0, 0.0, 1.0]; 3], vec![[0.0, 1.0, 0.0, 1.0]; 3]],
        colour_channels: vec![4, 5],
        indices: vec![0, 1, 2],
        ..Primitive::default()
    }
}

fn scene() -> Scene {
    let mut mesh = or_interchange::Mesh::default();
    mesh.primitives.push(primitive());
    let object = Object {
        meshes: vec![mesh],
        ..Object::default()
    };
    Scene {
        objects: vec![object],
        materials: vec![SceneMaterial {
            shader: "normal_spec.sps".into(),
            vectors: vec![("specularFalloffMult".into(), vec![[3.0, 0.0, 0.0, 0.0]])],
            textures: vec![
                TextureBinding {
                    parameter: "DiffuseSampler".into(),
                    texture: "wall".into(),
                    image: Some(0),
                },
                TextureBinding {
                    parameter: "BumpSampler".into(),
                    texture: "not in a dictionary".into(),
                    image: None,
                },
            ],
            ..SceneMaterial::default()
        }],
        images: vec![SceneImage {
            name: "wall".into(),
            png: png_of(2, 3, [10, 20, 30, 255]),
        }],
        ..Scene::default()
    }
}

#[test]
fn a_texture_channel_feeds_the_semantic_of_its_own_number() {
    let surface = surface_of(&primitive());
    assert_eq!(surface.uvs.keys().copied().collect::<Vec<_>>(), [1, 3]);
    assert_eq!(surface.uvs[&1][0], [0.25, 0.5]);
    assert_eq!(surface.uvs[&3][0], [0.75, 0.125]);
}

#[test]
fn a_colour_channel_feeds_the_semantic_of_its_own_number() {
    let surface = surface_of(&primitive());
    assert_eq!(surface.colours[&0][0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(surface.colours[&1][0], [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn a_primitive_with_joints_is_skinned_and_one_without_is_not() {
    let mut skinned = primitive();
    skinned.joints = vec![[0; 4]; 3];
    assert!(surface_of(&skinned).skinned);
    assert!(!surface_of(&primitive()).skinned);
}

#[test]
fn the_effect_is_the_shaders_stem() {
    for (shader, effect) in [
        ("normal_spec.sps", "normal_spec"),
        ("normal_spec", "normal_spec"),
        ("shaders/x/default.sps", "default"),
        ("win\\default.sps", "default"),
    ] {
        assert_eq!(effect_of(shader), effect, "{shader}");
    }
}

#[test]
fn a_material_carries_its_effect_its_values_and_the_textures_it_has_pixels_for() {
    let surfaces = surfaces_of(&scene(), &Textures::new());
    assert_eq!(surfaces.len(), 1);
    let material = &surfaces[0].1;
    assert_eq!(material.effect, "normal_spec");
    assert_eq!(material.vectors[0].0, "specularFalloffMult");
    assert_eq!(
        material.textures.keys().collect::<Vec<_>>(),
        ["DiffuseSampler"],
        "a binding no dictionary holds has no pixels to draw"
    );
    let wall = &material.textures["DiffuseSampler"];
    assert_eq!((wall.width, wall.height), (2, 3));
    assert_eq!(&wall.rgba[..4], &[10, 20, 30, 255]);
    assert_eq!(wall.rgba.len(), 2 * 3 * 4);
}

#[test]
fn a_primitive_with_no_material_draws_with_none() {
    let mut scene = scene();
    scene.objects[0].meshes[0].primitives[0].material = None;
    assert_eq!(surfaces_of(&scene, &Textures::new())[0].1.effect, "");
}

#[test]
fn a_picture_that_is_not_a_png_is_left_out_of_the_material() {
    let mut scene = scene();
    scene.images[0].png = vec![1, 2, 3];
    assert!(surfaces_of(&scene, &Textures::new())[0]
        .1
        .textures
        .is_empty());
}

#[test]
fn a_texture_the_drawable_lacks_is_taken_from_beside_it_by_the_name_the_material_gives() {
    let beside = pixels_by_name("wall", [3, 2, 1, 255]);
    let mut scene = scene();
    scene.materials[0].textures[0].image = None;
    scene.materials[0].textures[0].texture = "WaLl".into();
    let material = &surfaces_of(&scene, &beside)[0].1;
    assert_eq!(
        &material.textures["DiffuseSampler"].rgba[..4],
        &[3, 2, 1, 255]
    );
}

#[test]
fn a_texture_in_the_drawable_wins_over_the_one_beside_it() {
    let beside = pixels_by_name("wall", [3, 2, 1, 255]);
    let material = &surfaces_of(&scene(), &beside)[0].1;
    assert_eq!(
        &material.textures["DiffuseSampler"].rgba[..4],
        &[10, 20, 30, 255]
    );
}

#[test]
fn a_materials_draw_bucket_decides_how_its_coverage_is_resolved() {
    use or_interchange::AlphaMode;
    for (mode, coverage) in [
        (AlphaMode::Opaque, Coverage::Opaque),
        (AlphaMode::Mask, Coverage::Cutout),
        (AlphaMode::Blend, Coverage::Blended),
    ] {
        let mut scene = scene();
        scene.materials[0].alpha_mode = mode;
        assert_eq!(
            surfaces_of(&scene, &Textures::new())[0].1.coverage,
            coverage,
            "{mode:?}"
        );
    }
}

#[test]
fn a_vehicles_colour_marker_is_the_colour_of_its_slot_and_no_other_value_is_touched() {
    let rows = |row: [f32; 4]| vec![row];
    // The interior marker, the first body marker and a real colour that only looks like one
    assert_eq!(
        vehicle_colours("matDiffuseColor", &rows([2.0, 5.0, 5.0, 0.0])),
        rows([0.12, 0.12, 0.13, 0.0])
    );
    assert_eq!(
        vehicle_colours("matDiffuseColor", &rows([2.0, 1.0, 1.0, 0.0])),
        rows([1.0, 1.0, 1.0, 0.0])
    );
    for real in [
        [2.0, 5.0, 4.0, 0.0],
        [2.0, 8.0, 8.0, 0.0],
        [1.0, 5.0, 5.0, 0.0],
        [2.0, 0.5, 0.5, 0.0],
    ] {
        assert_eq!(vehicle_colours("matDiffuseColor", &rows(real)), rows(real));
    }
    // The same numbers under another name are not a marker
    assert_eq!(
        vehicle_colours("dirtColor", &rows([2.0, 5.0, 5.0, 0.0])),
        rows([2.0, 5.0, 5.0, 0.0])
    );
}

fn block(min: [f32; 3], max: [f32; 3]) -> (Surface, Material) {
    let surface = Surface {
        positions: vec![min, max],
        ..Surface::default()
    };
    (surface, Material::default())
}

#[test]
fn a_ground_under_a_model_lies_a_little_below_its_lowest_point_and_is_three_times_as_wide() {
    let mut surfaces = vec![block([2.0, 4.0, 1.0], [6.0, 6.0, 3.0])];
    lay_ground_under(&mut surfaces);
    assert_eq!(surfaces.len(), 2);
    let ground = &surfaces[1].0;
    let (lo, hi) = bounds(&surfaces[1..]).expect("the ground has corners");
    // The footprint is four by two: the ground is as wide as three of the longer side, round its centre
    assert_eq!((lo.x, hi.x), (4.0 - 6.0, 4.0 + 6.0));
    assert_eq!((lo.y, hi.y), (5.0 - 6.0, 5.0 + 6.0));
    // Below the lowest point by a thousandth of the longest side, and flat, facing up
    assert_eq!((lo.z, hi.z), (1.0 - 0.002 * 4.0, 1.0 - 0.002 * 4.0));
    assert!(ground.normals.iter().all(|n| *n == [0.0, 0.0, 1.0]));
    assert_eq!(ground.indices.len(), 6);
}

#[test]
fn nothing_is_laid_under_a_scene_with_no_vertex() {
    let mut none: Vec<(Surface, Material)> = Vec::new();
    lay_ground_under(&mut none);
    assert!(none.is_empty());
}

#[test]
fn a_wheel_bone_says_its_side_and_whether_it_is_a_front_one() {
    for (bone, want) in [
        ("wheel_lf", Some((Side::Left, true))),
        ("wheel_rf", Some((Side::Right, true))),
        ("wheel_lr", Some((Side::Left, false))),
        ("wheel_rr", Some((Side::Right, false))),
        ("wheel_lm1", Some((Side::Left, false))),
        ("wheel_f", Some((Side::Middle, true))),
        ("wheel_r", Some((Side::Middle, false))),
        ("chassis", None),
        ("wheelie", None),
    ] {
        assert_eq!(wheel_place(bone), want, "{bone}");
    }
}

#[test]
fn a_bone_is_placed_by_its_own_transform_under_its_parents() {
    let bone =
        |parent: Option<usize>, translation: [f32; 3], rotation: [f32; 4]| or_interchange::Bone {
            parent,
            translation,
            rotation,
            scale: [1.0; 3],
            ..or_interchange::Bone::default()
        };
    // The parent is turned a quarter about the vertical and stands at x = 1: a child two along y of it is two along minus x of the world's origin
    let quarter = std::f32::consts::FRAC_PI_4.sin_cos();
    let bones = [
        bone(None, [1.0, 0.0, 0.0], [0.0, 0.0, quarter.0, quarter.1]),
        bone(Some(0), [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]),
    ];
    let at = bone_matrix(&bones, 1).transform_point3(Vec3::ZERO);
    assert!((at - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5, "{at:?}");
    // A skeleton whose bone is its own parent ends
    let looped = [bone(Some(0), [1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])];
    assert!(bone_matrix(&looped, 0)
        .transform_point3(Vec3::ZERO)
        .is_finite());
}

#[test]
fn every_wheel_bone_of_a_corpus_vehicle_gets_a_wheel_and_the_far_side_gets_it_turned() {
    use or_io::decl::{File, Gen8};
    use or_io::gta5::GtaFragType;
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let test = "every_wheel_bone_of_a_corpus_vehicle_gets_a_wheel";
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "yft").target(TARGET_GEN8).find();
    if files.is_empty() {
        or_test_support::skip(test, &root, "yft");
        return;
    }
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: false,
    };
    let (mut checked, mut turned) = (0, 0);
    for path in &files {
        let bytes = std::fs::read(path).expect("a corpus file reads");
        let fragment = GtaFragType::<Gen8>::from_bytes(&bytes).expect("a corpus fragment opens");
        let placed = fragment_wheels(&fragment, &options);
        if placed.is_empty() {
            continue;
        }
        let skeleton = fragment
            .common_drawable
            .as_deref()
            .and_then(|d| d.base.skeleton.as_deref())
            .expect("a fragment with wheels has a skeleton");
        let bones = bones_of(skeleton);
        let wheels: Vec<usize> = (0..bones.len())
            .filter(|i| wheel_place(&bones[*i].name).is_some())
            .collect();
        let name = path.display().to_string();
        assert_eq!(
            placed.len(),
            wheels.len(),
            "{name}: a wheel for each wheel bone"
        );
        for ((_, at), bone) in placed.iter().zip(&wheels) {
            let stands = bone_matrix(&bones, *bone).transform_point3(Vec3::ZERO);
            let wheel = at.transform_point3(Vec3::ZERO);
            assert!(
                (wheel - stands).length() < 1e-4,
                "{name}: {}",
                bones[*bone].name
            );
            // Turned half about the vertical or not, a wheel is never mirrored
            assert!(at.determinant() > 0.0, "{name}: {}", bones[*bone].name);
            // A wheel is turned only on the right of a vehicle, whose wheel models are the left's
            let own = bone_matrix(&bones, *bone).transform_vector3(Vec3::X).x;
            if at.transform_vector3(Vec3::X).x * own < 0.0 {
                assert_eq!(
                    wheel_place(&bones[*bone].name).map(|p| p.0),
                    Some(Side::Right),
                    "{name}"
                );
                turned += 1;
            }
        }
        checked += 1;
    }
    if checked == 0 {
        or_test_support::skip(test, &root, "yft with wheels");
    }
    assert!(
        turned > 0 || checked == 0,
        "no wheel of {checked} vehicles stood turned on the right"
    );
}

#[test]
fn water_is_lit_by_its_own_pass_over_the_frame_whatever_its_draw_bucket() {
    let mut scene = scene();
    scene.materials[0].shader = "water_shallow.sps".into();
    let water = &surfaces_of(&scene, &Textures::new())[0].1;
    assert_eq!(water.effect, "water_shallow");
    assert_eq!(water.coverage, Coverage::Blended);
}

#[test]
fn a_vehicles_colour_marker_is_found_under_its_hash_when_no_database_names_the_parameter() {
    let hashed = format!("0x{:08X}", or_io::hash::string_hash_str("matDiffuseColor"));
    assert_eq!(
        vehicle_colours(&hashed, &[[2.0, 5.0, 5.0, 0.0]]),
        vec![[0.12, 0.12, 0.13, 0.0]]
    );
    let other = format!("0x{:08X}", or_io::hash::string_hash_str("dirtColor"));
    assert_eq!(
        vehicle_colours(&other, &[[2.0, 5.0, 5.0, 0.0]]),
        vec![[2.0, 5.0, 5.0, 0.0]]
    );
}

#[test]
fn a_body_paint_takes_the_metallic_setting_the_game_gives_it_and_nothing_else_does() {
    let hashed = |name: &str| format!("0x{:08X}", or_io::hash::string_hash_str(name));
    let material = |marker: [f32; 4]| -> Vec<(String, Vec<[f32; 4]>)> {
        vec![
            (hashed("matDiffuseColor"), vec![marker]),
            (hashed("specularIntensityMult"), vec![[20.0, 0.0, 0.0, 0.0]]),
            (hashed("specularFalloffMult"), vec![[200.0, 0.0, 0.0, 0.0]]),
            (hashed("specularFresnel"), vec![[0.97, 0.0, 0.0, 0.0]]),
        ]
    };
    let value = |vectors: &[(String, Vec<[f32; 4]>)], name: &str| {
        vectors.iter().find(|(n, _)| *n == hashed(name)).unwrap().1[0][0]
    };
    let painted = vehicle_vectors(&material([2.0, 1.0, 1.0, 0.0]));
    assert_eq!(value(&painted, "specularIntensityMult"), 0.8);
    assert_eq!(value(&painted, "specularFalloffMult"), 510.0);
    assert_eq!(value(&painted, "specularFresnel"), 0.96);
    // The interior, the wheels and a real colour keep what their file says
    for marker in [
        [2.0, 5.0, 5.0, 0.0],
        [2.0, 4.0, 4.0, 0.0],
        [0.5, 0.5, 0.5, 0.0],
    ] {
        let kept = vehicle_vectors(&material(marker));
        assert_eq!(value(&kept, "specularIntensityMult"), 20.0, "{marker:?}");
    }
}

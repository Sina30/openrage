use or_io::gta5::shaders::spirv_pack::Variable;

use super::*;

fn variable(name: &str, offset: u32, size: u32) -> Variable {
    Variable {
        name: name.into(),
        offset,
        size,
        used: true,
        scalar: "float".into(),
        rows: 1,
        columns: 4,
        elements: 0,
    }
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
fn the_matrices_are_the_cameras_and_each_register_holds_a_column() {
    let scene = EngineConstants {
        world: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        ..EngineConstants::default()
    };
    let values = scene.values();
    // A translation lives in the last column of a column-vector matrix, which is the last register
    assert_eq!(
        values["gWorld"],
        vec![
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            1.0, 2.0, 3.0, 1.0
        ]
    );
    assert_eq!(values["gWorldViewProj"], values["gWorld"]);
    assert_eq!(values["gWorldView"], values["gWorld"]);
    assert_eq!(values["gViewInverse"], columns(Mat4::IDENTITY));
}

#[test]
fn the_world_view_and_projection_compose_in_that_order() {
    let scene = EngineConstants {
        world: Mat4::from_scale(Vec3::splat(2.0)),
        view: Mat4::from_translation(Vec3::new(0.0, 0.0, -5.0)),
        projection: Mat4::IDENTITY,
        ..EngineConstants::default()
    };
    let values = scene.values();
    // Scaled first, then moved: a point at one lands at two, then at minus three
    let point = scene.view * scene.world * glam::Vec4::new(0.0, 0.0, 1.0, 1.0);
    assert_eq!(point.z, -3.0);
    assert_eq!(values["gWorldViewProj"], columns(scene.view * scene.world));
    assert_ne!(
        values["gWorldViewProj"],
        columns(scene.world * scene.view),
        "the order matters"
    );
}

#[test]
fn the_sun_and_ambient_are_written_as_colours_and_a_direction() {
    let scene = EngineConstants::default();
    let values = scene.values();
    let sun = scene.sun_direction;
    assert_eq!(
        values["gDirectionalLight"],
        vec![-sun.x, -sun.y, -sun.z, 1.0],
        "the way the light travels, and a w of one so that the sun keeps its colour"
    );
    assert!((sun.length() - 1.0).abs() < 1e-6, "a unit direction");
    assert_eq!(values["gDirectionalColour"].len(), 4);
    assert_eq!(values["gDirectionalColour"][3], 1.0);
    assert_eq!(
        values["gLightNaturalAmbient0"],
        values["gDirectionalAmbientColour"]
    );
    // Five registers of fog parameters, none of them zero
    assert_eq!(values["globalFogParams"].len(), 20);
    assert!(values["globalFogParams"].iter().all(|v| *v != 0.0));
    // Nothing faded out
    assert_eq!(values["globalFade"], vec![1.0; 4]);
    // A clean vehicle
    assert_eq!(values["dirtLevelMod"], vec![0.0, 1.0, 1.0, 1.0]);
    // No transparency anti-aliasing
    assert_eq!(values["gTransparencyAASamples"], vec![0.0]);
    // The UV animation is three numbers a row
    assert_eq!(values["globalAnimUV0"].len(), 3);
    assert_eq!(values["globalAnimUV1"].len(), 3);
}

#[test]
fn a_buffer_is_filled_with_what_the_scene_has_for_its_variables_and_zero_for_the_rest() {
    let layout = ConstantBuffer {
        name: "lighting_globals".into(),
        size: 64,
        variables: vec![
            variable("gDirectionalLight", 0, 16),
            variable("gDirectionalColour", 16, 16),
            variable("gLightNotInTheScene", 32, 16),
            // Too small for the four values the scene has
            variable("gDirectionalAmbientColour", 48, 12),
        ],
    };
    let scene = EngineConstants::default();
    let words = floats(&scene.fill(&layout));
    let sun = scene.sun_direction;
    assert_eq!(&words[..4], &[-sun.x, -sun.y, -sun.z, 1.0]);
    assert_eq!(words[4], scene.sun_colour.x);
    assert_eq!(
        &words[8..12],
        &[0.0; 4],
        "a variable the scene has no value for"
    );
    assert_eq!(
        &words[12..16],
        &[0.0; 4],
        "a variable too small for the value"
    );
    assert_eq!(words.len(), 16);
}

#[test]
fn the_view_inverse_puts_the_camera_where_it_is() {
    let scene = EngineConstants {
        view: Mat4::from_translation(Vec3::new(0.0, 0.0, -5.0)),
        ..EngineConstants::default()
    };
    // The camera sits at the last column of the inverse of the view
    let inverse = scene.values()["gViewInverse"].clone();
    assert_eq!(&inverse[12..16], &[0.0, 0.0, 5.0, 1.0]);
}

#[test]
fn a_skeleton_at_rest_has_the_identity_for_each_of_its_bones() {
    let bones = EngineConstants::default().values()["gBoneMtx"].clone();
    // 255 bones of three registers of four
    assert_eq!(bones.len(), 255 * 12);
    for bone in bones.as_chunks::<12>().0.iter() {
        assert_eq!(
            bone,
            &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        );
    }
}

#[test]
fn the_camera_matrices_a_pass_reads_without_the_world_are_projection_times_view() {
    let scene = EngineConstants {
        world: Mat4::from_scale(Vec3::splat(2.0)),
        view: Mat4::from_translation(Vec3::new(1.0, 2.0, -5.0)),
        projection: Mat4::perspective_rh(1.0, 1.5, 0.1, 50.0),
        ..EngineConstants::default()
    };
    let values = scene.values();
    assert_eq!(values["gViewProj"], columns(scene.projection * scene.view));
    assert_eq!(values["gCloudViewProj"], values["gViewProj"]);
    assert_ne!(
        values["gViewProj"], values["gWorldViewProj"],
        "no world in it"
    );
}

#[test]
fn the_camera_position_is_where_the_view_puts_the_eye() {
    let scene = EngineConstants {
        view: Mat4::look_at_rh(Vec3::new(4.0, -2.0, 7.0), Vec3::ZERO, Vec3::Z),
        ..EngineConstants::default()
    };
    let position = scene.values()["gCameraPos"].clone();
    for (got, want) in position.iter().zip([4.0, -2.0, 7.0, 1.0]) {
        assert!((got - want).abs() < 1e-5, "{position:?}");
    }
}

#[test]
fn a_wheel_drawn_alone_has_the_matrices_of_its_model() {
    let scene = EngineConstants {
        world: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        ..EngineConstants::default()
    };
    let values = scene.values();
    assert_eq!(values["matWheelWorld"], values["gWorld"]);
    assert_eq!(values["matWheelWorldViewProj"], values["gWorldViewProj"]);
}

#[test]
fn a_tyre_not_driven_on_is_not_deformed() {
    assert_eq!(
        EngineConstants::default().values()["tyreDeformSwitchOn"],
        vec![0.0]
    );
}

#[test]
fn a_cable_has_a_width_in_pixels_that_follows_the_lens() {
    let narrow = EngineConstants {
        projection: Mat4::perspective_rh(0.5, 1.0, 0.1, 100.0),
        ..EngineConstants::default()
    };
    let wide = EngineConstants {
        projection: Mat4::perspective_rh(1.5, 1.0, 0.1, 100.0),
        ..EngineConstants::default()
    };
    let (narrow, wide) = (
        narrow.values()["gCableParams"].clone(),
        wide.values()["gCableParams"].clone(),
    );
    assert!(narrow[0] > wide[0] && wide[0] > 0.0, "{narrow:?} {wide:?}");
    assert_eq!(&narrow[1..], &[1.0, 1.0, 1.0]);
}

#[test]
fn the_light_of_a_moment_takes_the_place_of_the_sun_and_the_ambient_light_and_the_fog() {
    use super::super::lighting::Lighting;
    use glam::Vec4;
    let lighting = Lighting {
        directional_light: Vec4::new(0.0, 0.0, -1.0, 0.5),
        directional_colour: Vec4::new(3.0, 2.0, 1.0, 0.0),
        natural_ambient: [Vec4::splat(0.1), Vec4::splat(0.2)],
        artificial_interior_ambient: [Vec4::splat(0.3); 2],
        artificial_exterior_ambient: [Vec4::splat(0.4); 2],
        directional_ambient: Vec4::splat(0.5),
        dynamic_bakes: Vec4::new(0.7, 0.8, 0.0, 0.0),
        fog_params: [Vec4::splat(7.0); 5],
        fog_colours: [Vec4::splat(0.6); 4],
        night_emissive: 0.0,
    };
    let scene = EngineConstants {
        lighting: Some(lighting),
        ..EngineConstants::default()
    };
    let values = scene.values();
    assert_eq!(values["gDirectionalLight"], vec![0.0, 0.0, -1.0, 0.5]);
    assert_eq!(values["gDirectionalColour"], vec![3.0, 2.0, 1.0, 0.0]);
    assert_eq!(values["gLightNaturalAmbient1"], vec![0.2; 4]);
    assert_eq!(values["gDynamicBakesAndWetness"], vec![0.7, 0.8, 0.0, 0.0]);
    assert_eq!(values["globalFogParams"], vec![7.0; 20]);
    assert_eq!(values["globalFogColorMoon"], vec![0.6; 4]);
}

#[test]
fn how_near_and_how_far_a_projection_sees_is_read_back_from_it_whichever_way_it_maps_depth() {
    let standard = Mat4::perspective_rh(1.0, 1.0, 0.1, 250.0);
    let (near, far) = near_far(&standard);
    assert!(
        (near - 0.1).abs() < 1e-4 && (far - 250.0).abs() < 0.05,
        "{near} {far}"
    );
    // The projection the viewport draws with: a nearer surface has the greater depth
    let (n, f) = (0.25_f32, 80.0_f32);
    let reversed = Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, n / (f - n), -1.0),
        glam::Vec4::new(0.0, 0.0, n * f / (f - n), 0.0),
    );
    let (near, far) = near_far(&reversed);
    assert!(
        (near - n).abs() < 1e-4 && (far - f).abs() < 0.01,
        "{near} {far}"
    );
}

#[test]
fn the_depth_map_is_read_by_the_size_of_the_picture_and_a_depth_turns_back_into_the_distance_it_was_for(
) {
    let engine = EngineConstants {
        screen: [200.0, 100.0],
        projection: Mat4::from_cols(
            glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, 0.5 / 49.5, -1.0),
            glam::Vec4::new(0.0, 0.0, 0.5 * 50.0 / 49.5, 0.0),
        ),
        ..EngineConstants::default()
    };
    let values = engine.values();
    assert_eq!(values["gInvScreenSize"][..2], [1.0 / 200.0, 1.0 / 100.0]);
    // What the depth buffer holds at a distance, and the distance the shader turns it back into: one over the line through (1, 1/near) and (0, 1/far)
    let [_, _, slope, offset] = <[f32; 4]>::try_from(values["NearFarPlane"].clone()).unwrap();
    let (a, b) = (engine.projection.z_axis.z, engine.projection.w_axis.z);
    for distance in [0.5_f32, 3.0, 20.0, 50.0] {
        let held = b / distance - a;
        let back = 1.0 / (held * slope + offset);
        assert!(
            (back - distance).abs() < distance * 1e-3,
            "{distance} came back as {back}"
        );
    }
}

#[test]
fn a_scene_with_no_moment_of_the_day_keeps_a_ped_lit_by_the_middle_of_the_bake_ramp() {
    let values = EngineConstants::default().values();
    assert_eq!(values["gDynamicBakesAndWetness"], vec![0.5, 1.0, 0.0, 0.0]);
}

#[test]
fn a_cut_out_surface_is_discarded_below_the_engines_alpha_references() {
    let values = EngineConstants::default().values();
    assert_eq!(values["gAlphaRefVec0"][0], 90.0 / 255.0);
    assert_eq!(values["gAlphaRefVec0"][1], 128.0 / 255.0);
    assert_eq!(values["gAlphaRefVec1"][3], 32.0 / 255.0);
}

#[test]
fn the_scalars_say_the_night_emissives_of_the_moment_and_that_the_camera_is_outdoors() {
    let with = |night_emissive: Option<f32>| {
        let engine = EngineConstants {
            lighting: night_emissive.map(|night_emissive| Lighting {
                night_emissive,
                ..Lighting::default()
            }),
            ..EngineConstants::default()
        };
        engine.values()["globalScalars2"].clone()
    };
    // The timer, the emissives, whether it is an interior, and the ped's rim light: no interior, and with no moment of the day the emissives are on
    assert_eq!(with(Some(0.25)), [1.0, 0.25, 0.0, 1.0]);
    assert_eq!(with(Some(0.0))[1], 0.0);
    assert_eq!(with(None), [1.0, 1.0, 0.0, 1.0]);
}

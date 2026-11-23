use super::*;

fn volume() -> FogVolume {
    FogVolume {
        position: Vec3::new(1.0, 2.0, 3.0),
        scale: Vec3::ONE,
        rotation: Vec3::ZERO,
        range: 4.0,
        density: 50.0,
        falloff: 2.0,
        tint: Vec3::new(0.5, 0.25, 1.0),
        hdr_mult: 2.0,
        light: FogLight::DayFog,
        ground_tint: false,
    }
}

fn sky() -> FogSky {
    FogSky {
        sun: Vec3::new(4.0, 2.0, 1.0),
        sun_height: 0.5,
        day_fog: 3.0,
        ground: Vec3::new(0.5, 1.0, 0.0),
    }
}

#[test]
fn a_volumes_colour_is_its_tint_by_its_brightness_and_by_what_lights_it() {
    let fog = volume();
    assert_eq!(fog.colour(&sky()), Vec3::new(3.0, 1.5, 6.0));
    let sun = FogVolume {
        light: FogLight::Sun,
        ..fog
    };
    assert_eq!(sun.colour(&sky()), Vec3::new(2.0, 0.5, 1.0));
    let none = FogVolume {
        light: FogLight::None,
        ground_tint: true,
        ..fog
    };
    assert_eq!(none.colour(&sky()), Vec3::new(0.5, 0.5, 0.0));
    // A sun below the horizon gives the volume no light
    let night = FogSky {
        sun_height: -1.0,
        ..sky()
    };
    assert_eq!(sun.colour(&night), Vec3::ZERO);
}

#[test]
fn the_transform_scales_and_then_turns_with_the_first_angle_about_x() {
    let turned = FogVolume {
        scale: Vec3::new(2.0, 1.0, 1.0),
        rotation: Vec3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2),
        ..volume()
    };
    let at = turned.transform() * Vec3::X;
    assert!((at - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-6, "{at:?}");
    // Turned about X first and about Z after: Y goes to Z, and Z's quarter turn leaves it there
    let both = FogVolume {
        rotation: Vec3::new(
            std::f32::consts::FRAC_PI_2,
            0.0,
            std::f32::consts::FRAC_PI_2,
        ),
        ..volume()
    };
    let at = both.transform() * Vec3::Y;
    assert!((at - Vec3::Z).length() < 1e-6, "{at:?}");
}

#[test]
fn the_registers_hold_the_rows_of_the_matrix_the_pass_takes_a_point_by() {
    let fog = FogVolume {
        scale: Vec3::new(3.0, 1.0, 2.0),
        rotation: Vec3::new(0.2, -0.4, 0.7),
        ..volume()
    };
    let values: BTreeMap<_, _> = fog.values(&sky()).into_iter().collect();
    // The pass dots a point with each register to find a coordinate of the point it moves to
    let moved = |name: &str, point: Vec3| {
        let registers = &values[name];
        Vec3::from_array([0, 1, 2].map(|j| {
            let row = &registers[4 * j..4 * j + 3];
            point.dot(Vec3::from_slice(row))
        }))
    };
    let point = Vec3::new(0.3, -0.8, 1.1);
    assert!((moved("fogVolumeTransform", point) - fog.transform() * point).length() < 1e-5);
    assert!(
        (moved("fogVolumeInvTransform", fog.transform() * point) - point).length() < 1e-5,
        "the inverse does not take a point back"
    );
}

#[test]
fn the_params_are_the_density_by_the_power_of_ten_the_falloff_the_range_and_one() {
    let values: BTreeMap<_, _> = volume().values(&sky()).into_iter().collect();
    let params = &values["fogVolumeParams"];
    assert!((params[0] - 50.0 * 10f32.powf(-2.5)).abs() < 1e-6);
    assert_eq!(params[1..], [2.0, 4.0, 1.0]);
}

#[test]
fn a_camera_is_inside_a_volume_a_good_way_past_its_range_and_the_pass_follows_the_technique() {
    let fog = volume();
    let at = |distance: f32| fog.position + Vec3::new(distance, 0.0, 0.0);
    assert!(fog.contains(at(4.0 * 3.0)));
    assert!(!fog.contains(at(4.0 * 3.1)));
    assert_eq!(fog.variant(at(5.0)), 1);
    assert_eq!(fog.variant(at(100.0)), 0);
    let ellipsoid = FogVolume {
        scale: Vec3::new(2.0, 1.0, 1.0),
        ..fog
    };
    assert_eq!(ellipsoid.variant(at(5.0)), 3);
    assert_eq!(ellipsoid.variant(at(100.0)), 2);
}

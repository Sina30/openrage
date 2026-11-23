use super::*;

/// The coordinate the lighting shaders look a direction up at: the two paraboloids side by side.
fn shader_coord(reflection: Vec3) -> glam::Vec2 {
    let scale = 1.0 + reflection.z.abs();
    if reflection.z > 0.0 {
        glam::Vec2::new(
            0.75 + 0.25 * reflection.x / scale,
            0.5 - 0.5 * reflection.y / scale,
        )
    } else {
        glam::Vec2::new(
            0.25 - 0.25 * reflection.x / scale,
            0.5 - 0.5 * reflection.y / scale,
        )
    }
}

fn keyframe(scatter: f32) -> Keyframe {
    let mut k = Keyframe::default();
    for (name, value) in [
        ("sky_azimuth_east_col_r", 1.0),
        ("sky_azimuth_east_col_inten", 1.0),
        ("sky_azimuth_west_col_b", 1.0),
        ("sky_azimuth_west_col_inten", 1.0),
        ("sky_zenith_col_g", 1.0),
        ("sky_zenith_col_inten", 1.0),
        ("sky_azimuth_transition_position", 0.5),
        ("sky_zenith_transition_position", 0.8),
        ("sky_zenith_blend_start", 0.0),
        ("sky_hdr", 2.0),
        ("sky_sun_col_r", 1.0),
        ("sky_sun_col_g", 1.0),
        ("sky_sun_col_b", 1.0),
        ("sky_sun_hdr", 10.0),
        ("sky_sun_miephase", 1.0),
        ("sky_sun_miescatter", scatter),
        ("sky_sun_mie_intensity_mult", 1.0),
    ] {
        k.set(name, value);
    }
    k
}

/// A sky with no glow round its sun.
fn sky(toward_sun: Vec3) -> Sky {
    Sky::new(&keyframe(0.0), toward_sun, Vec3::splat(0.1))
}

#[test]
fn a_texel_looks_where_the_shaders_look_it_up() {
    for direction in [
        Vec3::new(0.3, -0.5, 0.8),
        Vec3::new(-0.7, 0.1, 0.4),
        Vec3::new(0.2, 0.9, -0.3),
        Vec3::new(-0.5, -0.5, -0.7),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
    ] {
        let direction = direction.normalize();
        let at = shader_coord(direction);
        let back = paraboloid_direction(at.x, at.y).expect("inside a paraboloid");
        assert!(
            (back - direction).length() < 1e-4,
            "{direction} -> {at} -> {back}"
        );
    }
    // A corner of the map is outside both
    assert!(paraboloid_direction(0.0, 0.0).is_none());
}

#[test]
fn the_horizon_is_the_azimuth_colours_and_the_top_of_the_sky_is_the_zenith_colour() {
    // The sun is far from where the test looks
    let sky = sky(Vec3::new(0.0, 0.0, -1.0));
    let towards = |d: Vec3| sky.colour(d.normalize());
    let east = towards(Vec3::X);
    let west = towards(-Vec3::X);
    // The colour of the sky's east, squared, and times the light of the sky
    assert!(east.x > 1.5 && east.y < 0.1 && east.z < 0.1, "{east}");
    assert!(west.z > 1.5 && west.x < 0.1, "{west}");
    let top = towards(Vec3::Z);
    assert!(top.y > 1.5 && top.x < 0.1, "{top}");
}

#[test]
fn the_sky_is_brighter_towards_the_sun_than_away_from_it() {
    let sun = Vec3::new(0.6, 0.0, 0.8);
    let sky = Sky::new(&keyframe(1.0), sun, Vec3::splat(0.1));
    let at = sky.colour(sun);
    let away = sky.colour(-Vec3::new(sun.x, 0.0, -sun.z).normalize());
    assert!(at.max_element() > away.max_element() * 2.0, "{at} {away}");
}

#[test]
fn under_the_horizon_the_ground_takes_over() {
    let sky = sky(Vec3::Z);
    let down = sky.colour(-Vec3::Z);
    assert!((down - Vec3::splat(0.1)).length() < 1e-5, "{down}");
    let horizon = sky.colour(Vec3::X);
    assert!(horizon.x > 0.5, "{horizon}");
}

#[test]
fn a_map_has_the_levels_it_is_asked_for_each_the_average_of_the_one_above() {
    let map = ReflectionMap::of(&sky(Vec3::Z), 32, 4);
    assert_eq!((map.width, map.height), (32, 16));
    assert_eq!(map.levels.len(), 4);
    for (level, texels) in map.levels.iter().enumerate() {
        let (w, h) = map.size_of(level);
        assert_eq!(texels.len(), (w * h) as usize, "level {level}");
    }
    let mean = |texels: &[[f32; 4]]| texels.iter().map(|t| t[0]).sum::<f32>() / texels.len() as f32;
    assert!((mean(&map.levels[0]) - mean(&map.levels[1])).abs() < 1e-4);
    // Every texel is opaque
    assert!(map.levels[0].iter().all(|t| t[3] == 1.0));
    // A map cannot be smaller than a texel, however many levels are asked for
    let small = ReflectionMap::of(&sky(Vec3::Z), 4, 10);
    assert_eq!(small.levels.last().map(Vec::len), Some(1));
}

#[test]
fn the_clouds_drift_with_the_cycle_time_and_come_round_to_where_they_began() {
    let at = |time: f32| sky(Vec3::Z).with_time(time).constants(0.0);
    let (now, later) = (at(13.0), at(13.0 + 11.0));
    // The detail moves a layer a day over eleven; the large noise moves five tenths of a day
    assert!((now["speedConstants"][0] - later["speedConstants"][0]).abs() < 1e-4);
    assert_ne!(now["speedConstants"], at(14.0)["speedConstants"]);
    assert_ne!(now["noisePhase"], at(14.0)["noisePhase"]);
    let phase = |time: f32| at(time)["noisePhase"][0];
    assert!((phase(2.0) - (5.0_f32 * 2.0 * 0.24).rem_euclid(1.0)).abs() < 1e-6);
    // Every value is a place in a texture that repeats
    for time in [0.0, 13.0, 27.5, 400.0] {
        let c = at(time);
        assert!(c["speedConstants"]
            .iter()
            .chain(&c["noisePhase"])
            .all(|v| (0.0..1.0).contains(v)));
    }
}

#[test]
fn the_moon_phase_comes_round_every_cycle() {
    let light = |time: f32| sky(Vec3::Z).with_time(time).constants(0.0)["lunarCycle"].clone();
    assert_eq!(light(5.0), light(5.0 + 27.0));
    assert_ne!(light(5.0), light(6.0));
}

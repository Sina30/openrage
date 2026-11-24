use super::*;

fn light(kind: LightKind, x: f32, hours: u32) -> Light {
    Light {
        kind,
        position: [x, 0.0, 0.0],
        direction: [0.0, 0.0, -1.0],
        colour: [1.0, 0.5, 0.0],
        intensity: 10.0,
        radius: 5.0,
        falloff_exponent: 8.0,
        hours,
        flags: 0,
        cone_inner: 20.0,
        cone_outer: 40.0,
        length: 4.0,
    }
}

fn placed(kind: LightKind, x: f32, hours: u32) -> Lamp {
    Lamp {
        light: light(kind, x, hours),
        reach: None,
    }
}

#[test]
fn a_light_comes_on_and_goes_off_over_the_last_minute_of_an_hour() {
    let night = 0x00E0_007F; // 21:00 to 06:59
    assert_eq!(hour_strength(night, 3.5), 1.0);
    assert_eq!(hour_strength(night, 12.0), 0.0);
    assert_eq!(hour_strength(night, 6.5), 1.0, "on through its last hour");
    assert!((hour_strength(night, 6.0 + 59.5 / 60.0) - 0.5).abs() < 1e-3);
    assert_eq!(hour_strength(night, 7.0), 0.0);
    assert!((hour_strength(night, 20.0 + 59.25 / 60.0) - 0.25).abs() < 1e-3);
    assert_eq!(
        hour_strength(night, 20.5),
        0.0,
        "off until the last minute before"
    );
    assert_eq!(hour_strength(night, 24.0), 1.0, "midnight is hour 0");
    assert_eq!(hour_strength(ALL_HOURS, 23.999), 1.0);
}

#[test]
fn the_brightest_at_the_camera_are_kept_and_spots_go_to_the_spot_pass() {
    let lamps = [
        placed(LightKind::Point, 100.0, ALL_HOURS),
        placed(LightKind::Spot, 10.0, ALL_HOURS),
        placed(LightKind::Capsule, 20.0, ALL_HOURS),
        placed(LightKind::Point, 15.0, 1 << 3),
    ];
    let (points, spots) = frame_lights(&lamps, Vec3::ZERO, 12.0, 0.5, 2);
    assert_eq!(spots.len(), 1);
    let spot = spots[0];
    assert_eq!(
        (spot.position.x, spot.intensity, spot.radius),
        (10.0, 5.0, 5.0)
    );
    assert_eq!(
        (spot.outer_degrees, spot.falloff, spot.direction),
        (40.0, 8.0, Vec3::NEG_Z)
    );
    assert_eq!(points.len(), 1, "the far point loses to the two near ones");
    assert_eq!(
        (points[0].position.x, points[0].radius),
        (20.0, 7.0),
        "a capsule reaches past its ends"
    );
    let (all, _) = frame_lights(&lamps, Vec3::ZERO, 3.5, 1.0, 8);
    assert_eq!(all.len(), 3, "the hour-3 light is lit at 03:30");
    assert_eq!(all[0].position.x, 15.0, "the nearest point first");
    let (none, no_spots) = frame_lights(&lamps, Vec3::ZERO, 3.5, 1.0, 0);
    assert!(none.is_empty() && no_spots.is_empty());
}

#[test]
fn a_level_of_detail_light_stands_at_its_parents_distant_light_and_fades_out_at_its_reach() {
    let lod = MapLod {
        direction: vec![[0.0, 0.0, -1.0]; 2],
        falloff: vec![3.0, 12.0],
        falloff_exponent: vec![1.0; 2],
        cone_inner: vec![51, 0],
        cone_outer: vec![102, 255],
        ..MapLod::default()
    };
    let parent = MapLod {
        distant: vec![
            Distant {
                position: [100.0, 0.0, 0.0],
                rgb: [255, 0, 51],
                intensity: 80.0,
                light_type: 2,
            },
            Distant {
                position: [200.0, 0.0, 0.0],
                rgb: [255, 255, 255],
                intensity: 40.0,
                light_type: 4,
            },
        ],
        category: 1,
        ..MapLod::default()
    };
    let reach = LodReach {
        range: [500.0, 1500.0, 3500.0],
        fadeout: [100.0, 100.0, 150.0],
    };
    let lamps = lod_lamps(&lod, &parent, &reach);
    assert_eq!(lamps.len(), 2);
    let spot = lamps[0].light;
    assert_eq!(
        (spot.kind, spot.position, spot.radius),
        (LightKind::Spot, [100.0, 0.0, 0.0], 3.0)
    );
    assert!((spot.cone_inner - 36.0).abs() < 1e-4 && (spot.cone_outer - 72.0).abs() < 1e-4);
    assert!((Vec3::from(spot.colour) - Vec3::new(1.0, 0.0, 0.2)).length() < 1e-6);
    assert_eq!(lamps[0].reach, Some([1500.0, 100.0]), "the medium class");
    let capsule = lamps[1].light;
    assert_eq!((capsule.kind, capsule.length), (LightKind::Capsule, 140.0));
    // A medium light 1,450 m off is half faded; 1,500 m off, gone; and lit at noon.
    let (_, at_half) = frame_lights(&lamps[..1], Vec3::new(1550.0, 0.0, 0.0), 12.0, 1.0, 8);
    assert!((at_half[0].intensity - 40.0).abs() < 1e-3, "{at_half:?}");
    let (_, gone) = frame_lights(&lamps[..1], Vec3::new(1600.0, 0.0, 0.0), 12.0, 1.0, 8);
    assert!(gone.is_empty());

    let short = MapLod {
        falloff: vec![3.0],
        ..lod.clone()
    };
    assert!(
        lod_lamps(&short, &parent, &reach).is_empty(),
        "lists that do not pair"
    );
    assert!(
        lod_lamps(&lod, &parent, &LodReach::default()).is_empty(),
        "no settings, no reach"
    );
    let odd = MapLod {
        category: 3,
        ..parent.clone()
    };
    assert!(lod_lamps(&lod, &odd, &reach).is_empty());
}

#[test]
fn a_level_of_detail_light_with_a_placed_light_in_its_place_gives_way_to_it() {
    let lod = |x: f32| Lamp {
        light: light(LightKind::Point, x, ALL_HOURS),
        reach: Some([500.0, 100.0]),
    };
    let near = [placed(LightKind::Point, 10.015, 1)];
    let kept = without_placed(vec![lod(10.0), lod(10.05), lod(-0.001)], &near);
    let xs: Vec<f32> = kept.iter().map(|l| l.light.position[0]).collect();
    assert_eq!(xs, [10.05, -0.001]);
}

#[test]
fn the_reach_of_each_size_class_is_read_from_the_visual_settings() {
    let settings = VisualSettings::parse(
        "lodlight.small.range 10\nlodlight.medium.range 20\nlodlight.large.range 30\n\
         lodlight.small.fadeout 1\nlodlight.medium.fadeout 2\nlodlight.large.fadeout 3\n",
    );
    let reach = LodReach::of(&settings);
    assert_eq!(reach.range, [10.0, 20.0, 30.0]);
    assert_eq!(reach.fadeout, [1.0, 2.0, 3.0]);
    assert_eq!(
        LodReach::of(&VisualSettings::parse("")),
        LodReach::default()
    );
}

use super::*;
use lights_lod::{Distant, MapLod};

fn light(kind: LightKind, x: f32, hours: u32) -> Light {
    Light {
        kind,
        position: [x, 0.0, 0.0],
        direction: [0.0, 0.0, -1.0],
        colour: [1.0; 3],
        intensity: 10.0,
        radius: 5.0,
        falloff_exponent: 1.0,
        hours,
        flags: 0,
        cone_inner: 20.0,
        cone_outer: 40.0,
        length: 4.0,
    }
}

fn placed(lights: Vec<Light>) -> MapLights {
    MapLights {
        placed: lights,
        ..MapLights::default()
    }
}

fn lod_map(parent: u32, falloff: &[f32]) -> MapLights {
    MapLights {
        parent,
        placed: Vec::new(),
        lod: MapLod {
            direction: vec![[0.0, 0.0, -1.0]; falloff.len()],
            falloff: falloff.to_vec(),
            falloff_exponent: vec![1.0; falloff.len()],
            cone_inner: vec![51; falloff.len()],
            cone_outer: vec![102; falloff.len()],
            ..MapLod::default()
        },
    }
}

/// A map of distant lights of `light_type` at `xs`, of size class `category`.
fn distant_map(xs: &[f32], light_type: u8, category: u16) -> MapLights {
    let light = |x: &f32| Distant {
        position: [*x, 0.0, 0.0],
        rgb: [255, 0, 51],
        intensity: 80.0,
        light_type,
    };
    MapLights {
        lod: MapLod {
            distant: xs.iter().map(light).collect(),
            category,
            ..MapLod::default()
        },
        ..MapLights::default()
    }
}

fn xs(lights: &[PointLight]) -> Vec<f32> {
    lights.iter().map(|l| l.position.x).collect()
}

#[test]
fn a_light_comes_on_and_goes_out_over_the_last_minute_of_an_hour() {
    // Lit from 21:00 to 06:59.
    let night = 0x00E0_007F;
    assert_eq!(hour_strength(night, 12.0), 0.0);
    assert_eq!(hour_strength(night, 20.5), 0.0);
    assert!((hour_strength(night, 20.0 + 59.5 / 60.0) - 0.5).abs() < 1e-3);
    assert_eq!(hour_strength(night, 21.0), 1.0);
    assert_eq!(hour_strength(night, 23.99), 1.0, "midnight is lit on both sides");
    assert_eq!(hour_strength(night, 6.5), 1.0);
    assert!((hour_strength(night, 6.0 + 59.5 / 60.0) - 0.5).abs() < 1e-3);
    assert_eq!(hour_strength(night, 7.0), 0.0);
    assert_eq!(hour_strength(ALL_HOURS, 13.99), 1.0);
}

#[test]
fn a_placed_light_is_drawn_in_its_hours_and_the_brightest_nearest_come_first() {
    let night_only = 1 << 23 | 1 << 0;
    let map = placed(vec![
        light(LightKind::Point, 50.0, ALL_HOURS),
        light(LightKind::Point, 10.0, night_only),
        light(LightKind::Point, 20.0, ALL_HOURS),
    ]);
    let loaded = HashMap::from([(1, map)]);
    let at = |hour: f32, most: usize| collect(&loaded, Vec3::ZERO, hour, 0.5, &LodRanges::default(), most);
    let (noon, spots) = at(12.5, 8);
    assert!(spots.is_empty());
    assert_eq!(xs(&noon), [20.0, 50.0]);
    assert_eq!((noon[0].intensity, noon[0].radius, noon[0].falloff), (5.0, 5.0, 1.0));
    assert_eq!(xs(&at(23.5, 8).0), [10.0, 20.0, 50.0]);
    assert_eq!(xs(&at(23.5, 2).0), [10.0, 20.0], "the budget keeps the brightest nearest");
}

#[test]
fn a_spot_keeps_its_cone_and_a_capsule_reaches_past_its_ends() {
    let map = placed(vec![
        light(LightKind::Spot, 1.0, ALL_HOURS),
        light(LightKind::Capsule, 2.0, ALL_HOURS),
    ]);
    let loaded = HashMap::from([(1, map)]);
    let (points, spots) = collect(&loaded, Vec3::ZERO, 12.0, 1.0, &LodRanges::default(), 8);
    assert_eq!(spots.len(), 1);
    assert_eq!((spots[0].direction, spots[0].outer_degrees), (Vec3::NEG_Z, 40.0));
    assert_eq!(xs(&points), [2.0]);
    assert_eq!(points[0].radius, 5.0 + 2.0);
}

#[test]
fn a_level_of_detail_light_stands_at_its_parents_distant_light_at_every_hour() {
    let loaded = HashMap::from([
        (7, lod_map(8, &[3.0, 12.0])),
        (8, distant_map(&[100.0, 200.0], 1, 0)),
    ]);
    for hour in [3.0, 12.0, 23.0] {
        let (lit, _) = collect(&loaded, Vec3::ZERO, hour, 0.1, &LodRanges::default(), 8);
        assert_eq!(xs(&lit), [100.0, 200.0], "at {hour}");
        assert_eq!((lit[0].radius, lit[0].falloff), (3.0, 1.0));
        assert!((lit[0].colour - Vec3::new(1.0, 0.0, 0.2)).length() < 1e-6);
        assert!((lit[0].intensity - 80.0 * 0.1).abs() < 1e-5);
    }
    let spots = HashMap::from([(7, lod_map(8, &[3.0])), (8, distant_map(&[1.0], 2, 0))]);
    let (_, cones) = collect(&spots, Vec3::ZERO, 0.0, 1.0, &LodRanges::default(), 8);
    assert!((cones[0].outer_degrees - 72.0).abs() < 1e-4, "{}", cones[0].outer_degrees);
}

#[test]
fn a_level_of_detail_light_whose_source_is_placed_is_not_drawn_twice() {
    let loaded = HashMap::from([
        (7, lod_map(8, &[3.0, 3.0])),
        (8, distant_map(&[100.0, 200.0], 1, 0)),
        // The source of the first, a centimetre off, out at this hour.
        (9, placed(vec![light(LightKind::Point, 100.01, 0)])),
    ]);
    let (lit, _) = collect(&loaded, Vec3::ZERO, 12.0, 1.0, &LodRanges::default(), 8);
    assert_eq!(xs(&lit), [200.0]);
}

#[test]
fn a_level_of_detail_light_fades_out_over_the_end_of_its_size_class_range() {
    let ranges = LodRanges {
        range: [500.0, 1500.0, 3500.0],
        fadeout: [100.0; 3],
    };
    assert_eq!(ranges.strength(0, 300.0), 1.0);
    assert!((ranges.strength(0, 450.0) - 0.5).abs() < 1e-6);
    assert_eq!(ranges.strength(0, 600.0), 0.0);
    assert_eq!(ranges.strength(2, 3000.0), 1.0);
    assert_eq!(ranges.strength(9, 3000.0), 1.0, "a class past large is large");
    let loaded = HashMap::from([
        (7, lod_map(8, &[3.0, 3.0])),
        (8, distant_map(&[100.0, 550.0], 1, 0)),
    ]);
    let (lit, _) = collect(&loaded, Vec3::ZERO, 12.0, 1.0, &ranges, 8);
    assert_eq!(xs(&lit), [100.0]);
    let settings = VisualSettings::parse("lodlight.medium.range 1500\nlodlight.medium.fadeout 100\n");
    let read = LodRanges::of(&settings);
    assert_eq!((read.range[1], read.fadeout[1]), (1500.0, 100.0));
    assert_eq!(read.range[0], f32::INFINITY, "a class the settings do not state is not limited");
}

#[test]
fn lists_that_do_not_pair_light_for_light_draw_nothing() {
    let none = |loaded: &HashMap<u32, MapLights>| {
        let (p, s) = collect(loaded, Vec3::ZERO, 23.0, 1.0, &LodRanges::default(), 8);
        p.is_empty() && s.is_empty()
    };
    assert!(none(&HashMap::from([(7, lod_map(8, &[3.0]))])));
    assert!(none(&HashMap::from([
        (7, lod_map(8, &[3.0])),
        (8, distant_map(&[1.0, 2.0], 1, 0))
    ])));
}

#[test]
fn the_lights_of_a_body_that_is_no_map_are_none() {
    let mut model = lights_lod::tests::model(&[], &[]);
    model.data_blocks[0].data_type = 1;
    assert_eq!(lights_of(&model, &WorldIndex::default()), None);
}

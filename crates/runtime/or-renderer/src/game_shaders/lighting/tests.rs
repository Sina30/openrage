use super::*;

fn table() -> TimeTable {
    TimeTable {
        sun_roll: 122f32.to_radians(),
        moon_roll: -122f32.to_radians(),
        moon_wobble_frequency: 2.0,
        moon_wobble_amplitude: 0.2,
        moon_wobble_offset: 0.375,
        ..TimeTable::default()
    }
}

fn keyframe() -> Keyframe {
    let mut k = Keyframe::default();
    for (name, value) in [
        ("light_dir_col_r", 1.0),
        ("light_dir_col_g", 0.5),
        ("light_dir_col_b", 0.25),
        ("light_dir_mult", 64.0),
        ("light_natural_amb_down_col_r", 0.1),
        ("light_natural_amb_down_col_g", 0.2),
        ("light_natural_amb_down_col_b", 0.4),
        ("light_natural_amb_down_intensity", 5.0),
        ("light_natural_amb_up_col_r", 0.5),
        ("light_natural_amb_up_col_g", 0.5),
        ("light_natural_amb_up_col_b", 0.5),
        ("light_natural_amb_up_intensity", 2.0),
        ("light_natural_amb_up_intensity_mult", 3.0),
        ("light_amb_down_wrap", 0.25),
        ("light_directional_amb_col_r", 1.0),
        ("light_directional_amb_col_g", 1.0),
        ("light_directional_amb_col_b", 1.0),
        ("light_directional_amb_intensity", 2.0),
        ("light_directional_amb_intensity_mult", 1.5),
        ("light_ambient_bake_ramp", 0.75),
        ("light_directional_amb_bounce_enabled", 1.0),
        ("reflection_tweak_directional", 0.9),
        ("fog_start", 100.0),
        ("far_clip", 1000.0),
        ("fog_density", 10.0),
        ("fog_falloff", 0.0),
        ("fog_hdr", 2.0),
        ("fog_col_r", 0.5),
        ("fog_col_g", 0.25),
        ("fog_col_b", 0.125),
    ] {
        k.set(name, value);
    }
    k
}

fn view() -> View {
    View {
        camera_height: 0.0,
        projection: Mat4::perspective_rh(60f32.to_radians(), 1.0, 0.1, 1000.0),
        near: 0.1,
    }
}

fn at(hours: f32, k: &Keyframe, table: &TimeTable) -> Lighting {
    let moment = Moment {
        keyframe: k,
        table,
        hours,
        cycle_time: 13.0,
        moon_dim: 0.3,
        wetness: 0.0,
        night_emissive: 0.0,
        stated: None,
    };
    Lighting::at(&moment, &view())
}

#[test]
fn the_sun_at_noon_lights_from_above_and_the_light_travels_down_away_from_it() {
    let (k, t) = (keyframe(), table());
    let light = at(12.0, &k, &t);
    let travel = light.directional_light.truncate();
    assert!((travel.length() - 1.0).abs() < 1e-5);
    // The source is never lower than the clamp, so the light never travels flatter than it
    assert!(travel.z <= -LOWEST_SOURCE + 1e-5, "{travel}");
    assert_eq!(light.directional_light.w, 0.9);
}

#[test]
fn the_directional_colour_is_its_colour_times_the_multiplier_while_the_sun_shows() {
    let (k, t) = (keyframe(), table());
    let light = at(12.0, &k, &t);
    assert_eq!(light.directional_colour, Vec4::new(64.0, 32.0, 16.0, 0.0));
    // A multiplier of nothing, or a black colour, gives no light at all
    let mut dark = keyframe();
    dark.set("light_dir_mult", 0.0);
    assert_eq!(at(12.0, &dark, &t).directional_colour, Vec4::ZERO);
}

#[test]
fn the_sun_is_gone_in_the_night_and_a_moon_far_from_full_gives_less_than_a_full_one() {
    let (k, t) = (keyframe(), table());
    let at_night = |cycle_time: f32| {
        let moment = Moment {
            keyframe: &k,
            table: &t,
            hours: 23.5,
            cycle_time,
            moon_dim: 0.3,
            wetness: 0.0,
            night_emissive: 0.0,
            stated: None,
        };
        Lighting::at(&moment, &view()).directional_colour.x
    };
    let (full, new) = (at_night(13.0), at_night(0.0));
    assert!(full <= 64.0 && new <= full, "{new} {full}");
    // Past the half hour after the sunset hour the sun gives nothing, so noon's light is the most there is
    assert!(full <= at(12.0, &k, &t).directional_colour.x);
}

#[test]
fn the_ambient_lights_are_their_colours_times_their_intensities_with_their_extras_in_w() {
    let (k, t) = (keyframe(), table());
    let light = at(12.0, &k, &t);
    let [down, up] = light.natural_ambient;
    assert!((down.truncate() - Vec3::new(0.5, 1.0, 2.0)).length() < 1e-5);
    assert_eq!(down.w, 0.25);
    assert!((up.truncate() - Vec3::splat(3.0)).length() < 1e-5);
    assert!((up.w - 0.8).abs() < 1e-6);
    // 1.0 * 2.0 * 1.5 of the sun's strength, with the bake ramp in w
    assert_eq!(light.directional_ambient, Vec4::new(3.0, 3.0, 3.0, 0.75));
}

#[test]
fn the_ground_bounce_is_the_direction_of_the_sun_mirrored_in_the_ground_where_it_is_enabled() {
    let (mut k, t) = (keyframe(), table());
    let mirrored = at(12.0, &k, &t);
    let bounce = Vec3::new(
        mirrored.artificial_interior_ambient[1].w,
        mirrored.artificial_exterior_ambient[0].w,
        mirrored.artificial_exterior_ambient[1].w,
    );
    let source = -mirrored.directional_light.truncate();
    assert!((bounce - Vec3::new(source.x, source.y, -source.z)).length() < 1e-5);
    k.set("light_directional_amb_bounce_enabled", 0.0);
    let plain = at(12.0, &k, &t);
    assert!((plain.artificial_exterior_ambient[1].w - source.z).abs() < 1e-5);
}

#[test]
fn the_fog_states_its_distances_its_density_at_the_viewer_and_its_colours() {
    let (k, t) = (keyframe(), table());
    let light = at(12.0, &k, &t);
    let [near, far, haze, sun, moon] = light.fog_params;
    assert_eq!((near.x, near.y), (100.0, 1000.0));
    assert!(
        near.z > 0.0 && near.z < 1.0,
        "the depth the fog starts at: {}",
        near.z
    );
    // The camera is on the ground of the fog, so the density there is the whole of it, negated
    assert!((far.w + 10.0 / 10000.0).abs() < 1e-9);
    assert_eq!(haze.y, 1.0);
    assert!((sun.truncate().length() - 1.0).abs() < 1e-5);
    assert!((moon.truncate().length() - 1.0).abs() < 1e-5);
    // The sun's colour times the fog's strength, where the sun gives all of the light
    assert!(light.fog_colours[0].truncate().x > 0.0);
}

#[test]
fn every_variable_it_writes_is_a_whole_number_of_registers_under_the_name_the_buffer_has() {
    let (k, t) = (keyframe(), table());
    let values = at(12.0, &k, &t).values();
    let names: Vec<_> = values.iter().map(|(name, _)| *name).collect();
    for name in [
        "gDirectionalLight",
        "gDirectionalColour",
        "gLightNaturalAmbient0",
        "gLightNaturalAmbient1",
        "gLightArtificialIntAmbient0",
        "gLightArtificialExtAmbient1",
        "gDirectionalAmbientColour",
        "gDynamicBakesAndWetness",
        "globalFogParams",
        "globalFogColor",
        "globalFogColorE",
        "globalFogColorN",
        "globalFogColorMoon",
    ] {
        assert!(names.contains(&name), "{name}");
    }
    for (name, v) in values {
        assert_eq!(
            v.len(),
            if name == "globalFogParams" { 20 } else { 4 },
            "{name}"
        );
        assert!(v.iter().all(|x| x.is_finite()), "{name} {v:?}");
    }
}

#[test]
fn a_day_works_its_lights_out_from_what_it_holds_and_the_view() {
    let day = Day {
        keyframe: keyframe(),
        table: table(),
        hours: 12.0,
        cycle_time: 13.0,
        moon_dim: 0.3,
        wetness: 0.0,
        night_emissive: 0.0,
        tone: Default::default(),
        stated: None,
        sky_view: None,
        post: None,
    };
    assert_eq!(day.lighting(&view()), at(12.0, &keyframe(), &table()));
    // The fog depends on how high the camera is
    let high = View {
        camera_height: 5000.0,
        ..view()
    };
    let mut falloff = day.clone();
    falloff.keyframe.set("fog_falloff", 1000.0);
    assert_ne!(
        falloff.lighting(&view()).fog_params[1],
        falloff.lighting(&high).fog_params[1]
    );
}

#[test]
fn a_surface_that_bakes_its_own_shade_keeps_the_ambient_ramp_the_tweak_sets() {
    let table = table();
    let mut k = keyframe();
    // No tweak is the middle of the ramp
    assert_eq!(
        at(12.0, &k, &table).dynamic_bakes,
        Vec4::new(0.5, 1.0, 0.0, 0.0)
    );
    k.set("light_dynamic_bake_tweak", 2.0);
    let most = at(12.0, &k, &table).dynamic_bakes;
    k.set("light_dynamic_bake_tweak", -2.0);
    let least = at(12.0, &k, &table).dynamic_bakes;
    assert!(
        (most - Vec4::new(0.7, 1.0, 0.0, 0.0)).abs().max_element() < 1e-6,
        "{most}"
    );
    assert!(
        (least - Vec4::new(0.3, 1.0, 0.0, 0.0)).abs().max_element() < 1e-6,
        "{least}"
    );
}

#[test]
fn a_wet_day_writes_how_wet_the_world_is_where_the_pass_reads_it_and_a_dry_one_none() {
    let day = |wetness: f32| Day {
        keyframe: keyframe(),
        table: table(),
        hours: 12.0,
        cycle_time: 13.0,
        moon_dim: 0.3,
        wetness,
        night_emissive: 0.0,
        tone: Default::default(),
        stated: None,
        sky_view: None,
        post: None,
    };
    let values = |wetness: f32| {
        let lighting = day(wetness).lighting(&view());
        let (_, bakes) = (lighting.values().into_iter())
            .find(|(name, _)| *name == "gDynamicBakesAndWetness")
            .expect("the lighting writes it");
        bakes
    };
    // The bake range stays where it was, in the first two numbers; the third is the wetness the game holds between nought and one
    let (dry, wet) = (values(0.0), values(1.0));
    assert_eq!((dry[2], wet[2]), (0.0, 1.0));
    assert_eq!(dry[..2], wet[..2]);
    assert_eq!(values(2.5)[2], 1.0, "more than wet is held to wet");
    assert_eq!(values(-1.0)[2], 0.0);
}

#[test]
fn the_night_emissives_are_off_by_day_fade_in_the_hour_before_they_start_and_out_the_hour_before_they_end(
) {
    // The install's hours, from twenty to seven
    let balance = |hours: f32| night_emissive(hours, 20, 7);
    assert_eq!(balance(12.0), 0.0);
    assert_eq!(balance(7.0), 0.0);
    assert_eq!(balance(19.0), 0.0, "the fade starts as the hour does");
    assert!((balance(19.5) - 0.5).abs() < 1e-6, "{}", balance(19.5));
    assert_eq!(balance(20.0), 1.0);
    assert_eq!(balance(23.75), 1.0);
    assert_eq!(balance(0.0), 1.0, "past midnight they are still on");
    assert_eq!(balance(5.0), 1.0);
    assert!((balance(6.25) - 0.75).abs() < 1e-6, "{}", balance(6.25));
}

#[test]
fn a_moment_carries_its_night_emissives_to_the_lights_the_engine_writes() {
    let table = table();
    let k = keyframe();
    let night = |night_emissive: f32| {
        let moment = Moment {
            keyframe: &k,
            table: &table,
            hours: 12.0,
            cycle_time: 13.0,
            moon_dim: 0.3,
            wetness: 0.0,
            night_emissive,
            stated: None,
        };
        Lighting::at(&moment, &view()).night_emissive
    };
    assert_eq!((night(0.0), night(1.0)), (0.0, 1.0));
}

#[test]
fn a_day_that_states_its_light_whole_is_lit_from_its_own_bodies_and_faded_by_nothing_more() {
    // At midnight the frame's own hours would light by a moon faded by its height and phase.
    let towards = Vec3::new(0.3, -0.2, 0.9).normalize();
    let moon = Vec3::new(0.6, 0.0, -0.05);
    let day = Day {
        keyframe: keyframe(),
        table: table(),
        hours: 0.0,
        cycle_time: 0.0,
        moon_dim: 0.1,
        stated: Some(Stated {
            sun: -Vec3::Z,
            moon,
            towards_light: towards,
            sun_fade: 0.0,
            moon_fade: 1.0,
            exposure: 1.0,
        }),
        ..Day::default()
    };
    let lighting = day.lighting(&view());
    assert!((-lighting.directional_light.truncate() - towards).length() < 1e-5);
    let whole = Vec3::new(1.0, 0.5, 0.25) * 64.0;
    assert!((lighting.directional_colour.truncate() - whole).length() < 1e-4);
    let (_, drawn_moon) = lighting.sun_and_moon();
    assert!((drawn_moon - moon.normalize()).length() < 1e-5);
    let unstated = Day {
        stated: None,
        ..day
    };
    let faded = unstated.lighting(&view()).directional_colour.truncate();
    assert!(faded.length() < whole.length() * 0.99, "{faded}");
}

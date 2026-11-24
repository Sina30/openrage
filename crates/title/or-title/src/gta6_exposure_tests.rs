use super::*;

fn keyframe(pairs: &[(&str, f32)]) -> Keyframe {
    let mut k = Keyframe::default();
    for (name, value) in pairs {
        k.set(name, *value);
    }
    k
}

/// Settings of this test's own, not an install's: a curve that falls a stop for each e-fold of light.
fn settings() -> ExposureSettings {
    ExposureSettings {
        curve: Curve {
            scale: -1.0,
            horizontal_offset: 0.0,
            vertical_offset: 6.0,
            bending: 0.0,
        },
        blend: Curve {
            scale: -0.5,
            vertical_offset: 2.0,
            ..Curve::default()
        },
        min_step: 0.2,
        max_step: 10.0,
        step_mult: 1.0,
    }
}

/// A day of this test's own: a sun of 100,000 lux, a moon of a third of a lux, a sky light at three tenths.
fn day() -> Keyframe {
    keyframe(&[
        ("sky_scattering_sun_illuminance", 100_000.0),
        ("sky_scattering_moon_illuminance", 0.3),
        ("light_dir_mult", 1.0),
        ("ambient_sky_intensity", 0.3),
        ("postfx_exposure_min", -11.5),
        ("postfx_exposure_max", 20.0),
    ])
}

/// The way towards a body `degrees` from the zenith, in the plane of east and up.
fn at_zenith(degrees: f32) -> [f32; 3] {
    let angle = degrees.to_radians();
    [angle.sin(), 0.0, angle.cos()]
}

#[test]
fn the_curve_is_a_natural_logarithm_with_a_bending_ten_stops_down() {
    let curve = Curve {
        scale: -1.0,
        horizontal_offset: 0.0,
        vertical_offset: 7.0,
        bending: 0.5,
    };
    assert_eq!(curve.stops(1.0), 0.5 + 7.0 - 10.0);
    let e2 = 2f32.exp();
    assert!((curve.stops(e2) - (-2.0 + 0.5 * e2 - 3.0)).abs() < 1e-5);
    let offset = Curve {
        horizontal_offset: 1.0,
        ..curve
    };
    assert!((offset.stops(e2 - 1.0) - (-2.0 + 0.5 * (e2 - 1.0) - 3.0)).abs() < 1e-5);
}

#[test]
fn the_settings_are_read_by_the_curves_names_and_the_steps_keep_their_defaults() {
    let get = |name: &str, default: f32| match name {
        "Exposure.curve.scale" => -2.0,
        "Exposure.curve.blend.verticalOffset" => 4.0,
        _ => default,
    };
    let read = ExposureSettings::of(&get);
    assert_eq!(read.curve.scale, -2.0);
    assert_eq!(read.blend.vertical_offset, 4.0);
    assert_eq!(read.curve.vertical_offset, 0.0);
    assert_eq!(
        (read.min_step, read.max_step, read.step_mult),
        (0.15, 15.0, 9.0)
    );
}

#[test]
fn the_target_is_held_before_and_after_the_exposure_is_added_and_blends_the_two_curves() {
    let settings = settings();
    let k = keyframe(&[
        ("postfx_exposure_min", -4.0),
        ("postfx_exposure_max", 4.0),
        ("postfx_exposure", 3.0),
    ]);
    // The curve gives -6 at e^2: held to -4, then 3 up.
    let l = 2f32.exp();
    assert!((settings.curve.stops(l) + 6.0).abs() < 1e-5);
    assert!((target_stops(&settings, &k, l) + 1.0).abs() < 1e-5);
    // 3 is within the range, and 3 up from it is held to the most.
    let dark = (-7.0f32).exp();
    assert!((settings.curve.stops(dark) - 3.0).abs() < 1e-4);
    assert_eq!(target_stops(&settings, &k, dark), 4.0);

    let blended = keyframe(&[("postfx_exposure_blend_curve_weight", 0.25)]);
    let (a, b) = (settings.curve.stops(l), settings.blend.stops(l));
    let want = a + (b - a) * 0.25;
    assert!((target_stops(&settings, &blended, l) - want).abs() < 1e-5);
    let unheld = target_stops(&settings, &Keyframe::default(), 1e9);
    assert!(unheld < -20.0, "{unheld}");
}

#[test]
fn the_key_value_mode_takes_the_key_over_the_luminance_and_its_compensation() {
    let settings = settings();
    let mut k = keyframe(&[
        ("postfx_exposure_use_key_value_mode", 1.0),
        ("postfx_exposure_key_value", 0.18),
    ]);
    assert!((target_stops(&settings, &k, 0.72) + 2.0).abs() < 1e-5);
    // A curve whose seven points lie evenly on y = x / 2 is that line.
    for i in 0..7 {
        let x = -12.0 + 2.0 * i as f32;
        k.set(&format!("postfx_exposure_compensation_ev_cp{i}_x"), x);
        k.set(&format!("postfx_exposure_compensation_ev_cp{i}_y"), x / 2.0);
    }
    assert!((compensation(&k, -3.0) + 1.5).abs() < 1e-4);
    assert!((compensation(&k, -9.0) + 4.5).abs() < 1e-4);
    assert!(compensation(&k, 5.0).abs() < 1e-4, "held to the last x");
    assert!((target_stops(&settings, &k, 0.72) - (-2.0 - 1.0)).abs() < 1e-4);
    assert_eq!(compensation(&Keyframe::default(), -3.0), 0.0);
}

#[test]
fn adaptation_moves_at_a_held_rate_and_never_past_its_target() {
    let settings = settings();
    let k = keyframe(&[("postfx_exposure_adaption_step_mult", 0.5)]);
    let step = adapt(&settings, &k, 0.0, 4.0, 0.1);
    assert!((step - 0.2).abs() < 1e-5, "{step}");
    // Slower than the least step: the least step, held at the target.
    assert_eq!(adapt(&settings, &k, 3.99, 4.0, 1.0), 4.0);
    assert!((adapt(&settings, &k, 3.0, 4.0, 0.1) - 3.05).abs() < 1e-5);
    // Faster than the most: the most.
    let fast = keyframe(&[("postfx_exposure_adaption_step_mult", 1.0)]);
    assert_eq!(adapt(&settings, &fast, 0.0, -50.0, 1.0), -10.0);
    assert_eq!(
        adapt(&settings, &k, 1.0, 4.0, 0.0),
        4.0,
        "no time, no history"
    );
}

#[test]
fn the_sun_lights_until_its_threshold_and_then_the_moon_by_its_own() {
    let k = keyframe(&[("light_dir_sun_mult", 0.5), ("light_dir_moon_mult", 0.8)]);
    let moon = at_zenith(40.0);
    let low_sun = bodies(&k, at_zenith(95.0), moon);
    assert!(low_sun.sun_leads);
    assert_eq!((low_sun.sun_fade, low_sun.fade), (1.0, 0.5));
    assert!(
        (low_sun.towards[2] - LOWEST_LIGHT).abs() < 0.02,
        "{:?}",
        low_sun.towards
    );
    let under = bodies(&k, at_zenith(100.5), moon);
    assert!(!under.sun_leads);
    assert_eq!(under.sun_fade, 0.0);
    assert!((under.moon_fade - 0.5).abs() < 1e-3, "{}", under.moon_fade);
    assert!((under.fade - 0.4).abs() < 1e-3);
    assert_eq!(under.body, moon);
    // A stated threshold and a crossfade.
    let early = keyframe(&[("sun_fade_threshold", 80.0)]);
    assert_eq!(bodies(&early, at_zenith(85.0), moon).sun_fade, 0.0);
    let set = |c: f32| {
        bodies(
            &keyframe(&[("sun_moon_crossfade", c)]),
            at_zenith(10.0),
            moon,
        )
    };
    assert_eq!((set(0.3).sun_fade, set(0.3).moon_fade), (0.3, 0.0));
    assert_eq!((set(-0.4).sun_fade, set(-0.4).moon_fade), (0.0, 0.4));
}

#[test]
fn the_moon_goes_round_the_compass_once_a_day_well_above_the_horizon() {
    let (roll, wobble) = (362f32.to_radians(), 0.8);
    for step in 0..24 {
        let moon = moon_towards(step as f32 / 24.0, roll, wobble);
        let height = moon[2].asin().to_degrees();
        assert!((36.0..41.0).contains(&height), "{step}: {height}");
    }
    assert!(
        moon_towards(0.0, roll, wobble)[1] > 0.9 * 0.6,
        "north at midnight"
    );
    assert!(moon_towards(0.25, roll, wobble)[0] < -0.7, "west at six");
    assert!(
        moon_towards(1.25, roll, wobble)[0] < -0.7,
        "whatever the day"
    );
}

#[test]
fn noon_is_exposed_darker_than_dusk_and_dusk_than_midnight() {
    let (k, settings) = (day(), settings());
    let moon = at_zenith(45.0);
    let (noon, noon_stops) = light(&k, at_zenith(15.0), moon, &settings);
    let (dusk, dusk_stops) = light(&k, at_zenith(89.0), moon, &settings);
    let (night, night_stops) = light(&k, at_zenith(150.0), moon, &settings);
    assert_eq!(noon_stops, -11.5, "noon is held at the day's least");
    assert!(noon_stops < dusk_stops - 3.0, "{noon_stops} {dusk_stops}");
    assert!(dusk_stops < night_stops - 3.0, "{dusk_stops} {night_stops}");
    assert_eq!(noon.exposure, noon_stops.exp2());
    assert!(night.illuminance < 1.0 && dusk.illuminance > 1000.0);
    // Through the low air the dusk sun reaches the ground at a small part of the noon sun.
    assert!(dusk.body_transmittance[1] < 0.2 * noon.body_transmittance[1]);
    // The frame's sun at noon is a few units, and the moon's light a small part of it.
    let frame = |light: &FrameLight| {
        or_io::gta6::timecycle::frame_keyframe(&k, light)
            .get("light_dir_mult")
            .unwrap_or(0.0)
    };
    assert!((5.0..15.0).contains(&frame(&noon)), "{}", frame(&noon));
    assert!(frame(&night) < 0.1 * frame(&noon), "{}", frame(&night));
}

#[test]
fn without_the_ten_stops_noon_would_not_be_held_at_the_least() {
    let k = day();
    let mut settings = settings();
    let (_, held) = light(&k, at_zenith(15.0), at_zenith(45.0), &settings);
    settings.curve.vertical_offset += CURVE_OFFSET;
    let (_, unheld) = light(&k, at_zenith(15.0), at_zenith(45.0), &settings);
    assert_eq!(held, -11.5);
    assert!(unheld > -11.5 + 5.0, "{unheld}");
}

#[test]
fn the_dome_is_lit_by_the_body_that_leads_and_the_disc_by_the_sun_alone() {
    let (k, settings) = (day(), settings());
    let (noon, _) = light(&k, at_zenith(15.0), at_zenith(45.0), &settings);
    assert!(noon.dome.iter().all(|c| c.iter().all(|v| *v > 0.0)));
    let (night, _) = light(&k, at_zenith(150.0), at_zenith(45.0), &settings);
    assert_eq!(
        night.sun_transmittance, [0.0; 3],
        "the sun is under the ground"
    );
    assert!(night.dome[0][2] < 1e-3 * noon.dome[0][2]);
    assert!(night.dome[0][2] > 0.0, "the moon lights the sky");
}

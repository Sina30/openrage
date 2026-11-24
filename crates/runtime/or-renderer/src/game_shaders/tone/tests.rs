use glam::Vec4;

use super::*;

/// The numbers of the install's visual settings.
fn install() -> ToneSettings {
    ToneSettings::from_visual(&|name, default| match name {
        "Tonemapping.bright.filmic.exposure" => -0.5,
        "Tonemapping.dark.filmic.exposure" => 3.0,
        "Tonemapping.dark.filmic.E" => 0.0,
        "Exposure.curve.scale" => -106.2,
        "Exposure.curve.power" => 0.007,
        "Exposure.curve.offset" => 104.7,
        _ => default,
    })
}

fn lit_by(sun: f32, ambient: f32) -> Lighting {
    Lighting {
        directional_colour: Vec4::new(sun, sun, sun, 0.0),
        natural_ambient: [Vec4::splat(ambient); 2],
        ..Lighting::default()
    }
}

#[test]
fn a_setting_the_install_does_not_state_is_the_games_own() {
    let settings = install();
    assert_eq!(settings.bright.a, 0.22);
    assert_eq!(settings.bright.w, 4.0);
    assert_eq!(settings.dark.e, 0.0);
    assert_eq!(settings.bright.e, 0.01);
    assert_eq!(
        (settings.bright_exposure, settings.dark_exposure),
        (-0.5, 3.0)
    );
}

#[test]
fn the_exposure_follows_the_curve_of_the_luminance_and_stays_within_what_the_moment_allows() {
    let settings = install();
    // One unit of luminance wants a stop and a half less light
    assert!((settings.exposure(1.0, 0.0, -3.5, 5.0) + 1.5).abs() < 1e-4);
    // Darker frames want more light, and brighter ones less, as far as the moment lets them
    assert!(settings.exposure(0.01, 0.0, -3.5, 5.0) > settings.exposure(1.0, 0.0, -3.5, 5.0));
    assert_eq!(settings.exposure(5000.0, 0.0, -3.5, 5.0), -3.5);
    assert_eq!(settings.exposure(1e-6, 0.0, -3.5, 1.0), 1.0);
}

#[test]
fn the_tweak_of_a_moment_is_added_before_the_exposure_is_held_between_its_limits() {
    let settings = install();
    let curve = |luminance: f32| {
        settings.curve_scale * luminance.powf(settings.curve_power) + settings.curve_offset
    };
    // A frame that wants far less light than the moment allows is still held there when a tweak asks for a little more
    assert!(curve(5000.0) + 2.0 < -3.5);
    assert_eq!(settings.exposure(5000.0, 2.0, -3.5, 5.0), -3.5);
    // Inside the limits the tweak is the stops it states
    assert!((settings.exposure(1.0, 0.25, -3.5, 5.0) + 1.25).abs() < 1e-4);
}

#[test]
fn a_brighter_moment_is_exposed_for_less_light() {
    let settings = install();
    let noon = Tone::new(&settings, None, &lit_by(54.0, 4.0));
    let dusk = Tone::new(&settings, None, &lit_by(0.5, 0.2));
    assert!(noon.stops < dusk.stops, "{} {}", noon.stops, dusk.stops);
    let [factor, stops] = noon.adaptation();
    assert!((factor - noon.stops.exp2()).abs() < 1e-6 && stops == noon.stops);
    // A frame of nothing is the luminance of nothing
    assert_eq!(expected_luminance(&lit_by(0.0, 0.0)), 0.0);
}

#[test]
fn the_two_curves_are_blended_by_where_the_exposure_sits_between_theirs() {
    let tone = Tone::new(&install(), None, &lit_by(1.0, 1.0));
    let values = tone.values();
    let range = 3.0 - -0.5;
    assert_eq!(values["TonemapParams"], vec![1.0 / range, 0.5 / range]);
    assert_eq!(values["BrightTonemapParams0"], vec![0.22, 0.3, 0.10, 0.2]);
    assert_eq!(values["DarkTonemapParams1"], vec![0.0, 0.3, 4.0, 0.0]);
    // Both curves at one exposure are not divided by nothing
    let same = Tone {
        dark_exposure: -0.5,
        ..tone
    };
    assert_eq!(same.values()["TonemapParams"], vec![1.0, 0.5]);
}

#[test]
fn a_moment_that_states_its_curve_takes_it_in_place_of_the_installs() {
    let mut moment = Keyframe::default();
    for (name, value) in [
        ("postfx_tonemap_filmic_override_dark", 1.0),
        ("postfx_tonemap_filmic_exposure_dark", 2.0),
        ("postfx_tonemap_filmic_a", 0.5),
        ("postfx_tonemap_filmic_w", 8.0),
        ("postfx_correct_col_r", 0.9),
        ("postfx_desaturation", 0.5),
        ("postfx_exposure", 0.25),
    ] {
        moment.set(name, value);
    }
    let light = lit_by(1.0, 1.0);
    let tone = Tone::new(&install(), Some(&moment), &light);
    // The dark curve alone stated stands for both
    assert_eq!((tone.dark.a, tone.dark.w), (0.5, 8.0));
    assert_eq!(tone.bright, tone.dark);
    assert_eq!((tone.dark_exposure, tone.bright_exposure), (2.0, 2.0));
    assert_eq!(tone.correct[0], 0.9);
    assert_eq!(tone.values()["Desaturate"], vec![0.5]);
    // The tweak of the moment is added to the stops the curve gives
    let plain = Tone::new(&install(), Some(&Keyframe::default()), &light);
    assert!((tone.stops - plain.stops - 0.25).abs() < 1e-5);
    // A moment that states nothing leaves the install's curves
    assert_eq!(plain.dark, install().dark);
}

#[test]
fn the_bright_pass_starts_where_the_moment_says_and_is_whole_a_width_above_it() {
    let narrow = Bloom {
        threshold: 1.0,
        width: 0.0,
        intensity: 0.4,
    };
    assert_eq!(narrow.params(), [1.0, 0.4, 1.0, 0.0]);
    // Half the width either side of the threshold, and one over the span
    let wide = Bloom {
        threshold: 1.0,
        width: 0.5,
        intensity: 0.4,
    };
    assert_eq!(wide.params(), [0.75, 0.4, 2.0, 0.0]);
    // Below nothing a threshold does not go
    let low = Bloom {
        threshold: 0.1,
        width: 0.5,
        intensity: 1.0,
    };
    assert_eq!(low.params()[0], 0.0);
    assert!((low.amount() - 0.25).abs() < 1e-6);
}

#[test]
fn the_white_point_is_the_curves_by_how_far_the_exposure_is_from_the_bright_one_to_the_dark() {
    let settings = ToneSettings {
        bright: Filmic {
            w: 2.0,
            ..install().bright
        },
        dark: Filmic {
            w: 8.0,
            ..install().dark
        },
        bright_exposure: -2.0,
        dark_exposure: 2.0,
        ..install()
    };
    let at = |stops: f32| Tone {
        stops,
        ..Tone::new(&settings, None, &lit_by(1.0, 1.0))
    };
    assert!((at(-2.0).one_over_white_point() - 0.5).abs() < 1e-6);
    assert!((at(2.0).one_over_white_point() - 0.125).abs() < 1e-6);
    // Halfway the white point is halfway
    assert!((at(0.0).one_over_white_point() - 0.2).abs() < 1e-6);
    // Outside the two it is held at the nearer
    assert!((at(9.0).one_over_white_point() - 0.125).abs() < 1e-6);
}

#[test]
fn a_moment_states_its_bloom_and_the_install_the_blends_of_its_blurs() {
    let mut moment = Keyframe::default();
    for (name, value) in [
        ("postfx_bright_pass_thresh", 1.2),
        ("postfx_bright_pass_thresh_width", 0.6),
        ("postfx_intensity_bloom", 0.2),
    ] {
        moment.set(name, value);
    }
    let tone = Tone::new(&install(), Some(&moment), &lit_by(1.0, 1.0));
    assert_eq!(
        tone.bloom,
        Bloom {
            threshold: 1.2,
            width: 0.6,
            intensity: 0.2
        }
    );
    // A moment that states none puts none back
    let none = Tone::new(&install(), Some(&Keyframe::default()), &lit_by(1.0, 1.0));
    assert_eq!(none.bloom.intensity, 0.0);
    // The blends are what the visual settings say, and the game's own where they say nothing
    let settings = ToneSettings::from_visual(&|name, default| match name {
        "bloom.blurblendmult.med" => 2.0,
        _ => default,
    });
    assert_eq!(settings.bloom_blend, [0.5, 2.0, 1.0]);
}

#[test]
fn curves_blended_by_the_stops_of_the_light_are_written_as_that_blend_whatever_the_frames_stops() {
    let tone = Tone {
        bright_exposure: -5.0,
        dark_exposure: -2.5,
        stops: 0.0,
        ..Tone::new(&install(), None, &lit_by(1.0, 1.0))
    };
    // The frame's own stops, held at nought, would give the dark curve at noon.
    assert_eq!(tone.values()["TonemapParams"], vec![0.4, 2.0]);
    let noon = tone.blended_by(-12.0);
    assert_eq!(noon.values()["TonemapParams"], vec![0.0, 0.0]);
    assert_eq!(
        noon.one_over_white_point(),
        1.0 / install().bright.w.max(1e-4)
    );
    // A linear exposure of two to the minus twelve, taken as stops, would be the dark curve whole.
    assert_eq!(
        tone.blended_by((-12f32).exp2()).values()["TonemapParams"],
        vec![0.0, 1.0]
    );
    let dusk = tone.blended_by(-4.375);
    assert_eq!(dusk.values()["TonemapParams"], vec![0.0, 0.25]);
    let night = tone.blended_by(3.1);
    assert_eq!(night.values()["TonemapParams"], vec![0.0, 1.0]);
}

use super::*;

fn table() -> DayTable {
    let samples: String = (0..13)
        .map(|k| {
            let kind = if k == 1 {
                "EASE_QUARTIC_INOUT"
            } else {
                "EASE_LINEAR_IN"
            };
            format!(r#"<sample name="s{k}" hour="{k}" uw_tc_mod="u" interpolation_type="{kind}"/>"#)
        })
        .collect();
    DayTable::parse(&format!(
        r#"<time_data version="1.0" mode="dynamic">{samples}<region name="URBAN"/></time_data>"#
    ))
}

#[test]
fn the_time_table_states_its_samples_their_easing_and_its_regions() {
    let table = table();
    assert_eq!(table.samples.len(), 13);
    assert_eq!(
        (table.samples[3].name.as_str(), table.samples[3].hour),
        ("s3", 3.0)
    );
    assert_eq!(table.samples[1].easing, Easing::QuarticInOut);
    assert_eq!(table.samples[0].easing, Easing::Linear);
    assert_eq!(table.regions, ["URBAN"]);
}

#[test]
fn a_sample_is_found_by_the_suns_angle_and_eased_by_the_first_of_the_two() {
    let table = table();
    // The sun on the eastern horizon is the fifth sample whole, and its top the seventh.
    assert_eq!(table.locate(0.0), Some((4, 5, 0.0)));
    let (first, second, along) = table.locate(1.57).expect("thirteen samples");
    assert_eq!((first, second), (6, 7));
    assert!(along.abs() < 1e-6);
    // Half way from the second sample to the third, bent by the second sample's own easing.
    let (first, second, along) = table
        .locate((-0.45 + -0.22) / 2.0)
        .expect("thirteen samples");
    assert_eq!((first, second), (1, 2));
    assert!(
        (along - 0.5).abs() < 1e-4,
        "the quartic passes through its middle: {along}"
    );
    let (_, _, early) = table.locate(-0.45 + 0.23 * 0.25).expect("thirteen samples");
    assert!((early - 8.0 * 0.25f32.powi(4)).abs() < 1e-3, "{early}");
    // The round closes: past the last row the way runs on to the first, and an angle under the first is a turn on.
    assert_eq!(table.locate(3.12).map(|l| (l.0, l.1)), Some((8, 9)));
    assert_eq!(table.locate(-3.2).map(|l| (l.0, l.1)), Some((7, 8)));
    assert_eq!(DayTable::default().locate(0.0), None);
}

#[test]
fn the_sun_rises_in_the_east_tops_in_the_south_and_is_under_the_floor_at_night() {
    let noon = SOLAR_NOON * 24.0;
    let at = |hours: f32| sun_towards(sun_round(hours), LATITUDE);
    // The clock bends about the middle of sunrise and sunset, which is not noon, so the top is near noon only.
    let top = at(noon);
    assert!(top[0].abs() < 0.01 && top[1] < 0.0, "{top:?}");
    assert!((top[2] - LATITUDE.cos()).abs() < 1e-4, "{top:?}");
    assert!((sun_round(noon) - PI / 2.0).abs() < 0.01);
    assert!((sun_angle(top) - PI / 2.0).abs() < 0.01);
    // At the bend's ends the clock is the day's own, and the sun is on the horizon the search allows.
    let turn = |a: f32, b: f32| ((a - b + PI).rem_euclid(TAU) - PI).abs();
    for part in [SUNRISE, SUNSET] {
        let unbent = PI / 2.0 + (part - SOLAR_NOON) * TAU;
        assert!(turn(sun_round(part * 24.0), unbent) < 1e-4, "{part}");
    }
    let rise = at(SUNRISE * 24.0);
    assert!(rise[0] > 0.99 && rise[2].abs() < 0.05, "{rise:?}");
    let set = at(SUNSET * 24.0);
    assert!(set[0] < -0.99 && set[2].abs() < 0.05, "{set:?}");
    assert!(at(1.0)[2] < 0.0 && at(23.0)[2] < 0.0);
    // It lingers near the horizon: an hour after its rise it has turned less than a twelfth of a half turn.
    assert!(sun_round(SUNRISE * 24.0 + 1.0) - sun_round(SUNRISE * 24.0) < PI / 12.0);
}

#[test]
fn the_frames_hour_puts_its_sun_where_the_round_is() {
    assert_eq!(frame_hours(0.0), 6.0);
    assert_eq!(frame_hours(PI / 2.0), 13.0);
    assert_eq!(frame_hours(PI), 20.0);
    let midnight = frame_hours(-PI / 2.0);
    assert!(!(1e-3..23.999).contains(&midnight), "{midnight}");
    assert!((frame_hours(-PI / 4.0) - 3.0).abs() < 1e-4);
    assert!((frame_hours(-3.0 * PI / 4.0) - 22.0).abs() < 1e-4);
    assert!(sun_lights(6.0) && sun_lights(21.0) && !sun_lights(22.0) && !sun_lights(3.0));
    assert!((sun_roll(0.0) - PI / 2.0).abs() < 1e-6);
}

#[test]
fn the_values_before_a_cycle_and_the_modifiers_of_a_file_are_read() {
    let cycle = r#"<timecycle_keyframe_data version="2.0">
  <High_Level_Vars>
    <cloud_cover> 0.200000</cloud_cover>
    <Fog> 1.5</Fog>
  </High_Level_Vars>
  <cycle name="Sunny" regions="1"><region name="GLOBAL"><fog_start> 1 2</fog_start></region></cycle>
</timecycle_keyframe_data>"#;
    let vars = high_level_vars(cycle);
    assert_eq!(vars.len(), 2);
    assert_eq!((vars["cloud_cover"], vars["Fog"]), (0.2, 1.5));
    assert!(high_level_vars("<timecycle_keyframe_data/>").is_empty());

    let mods = modifiers(
        r#"<timecycle_modifier_data version="1.0">
  <modifier name="a" description="" userFlags="8">
    <light_dir_mult>1.5 1.000000</light_dir_mult>
    <broken>1.0</broken>
  </modifier>
  <modifier name="b" userFlags="0"></modifier>
</timecycle_modifier_data>"#,
    );
    assert_eq!(mods.len(), 2);
    assert_eq!((mods[0].name.as_str(), mods[0].user_flags), ("a", 8));
    assert_eq!(
        mods[0].vars.len(),
        1,
        "a variable with one number is left out"
    );
    assert_eq!(mods[0].vars["light_dir_mult"], [1.5, 1.0]);
    assert!(mods[1].vars.is_empty());
}

fn own(pairs: &[(&str, f32)]) -> Keyframe {
    let mut frame = Keyframe::default();
    for (name, value) in pairs {
        frame.set(name, *value);
    }
    frame
}

#[test]
fn the_sun_at_its_stated_default_is_the_frames_default_and_its_fade_dims_everything_it_lights() {
    let noon = frame_keyframe(&own(&[("light_dir_col_g", 0.5)]), true);
    assert_eq!(noon.get("light_dir_mult"), Some(FRAME_SUN));
    assert_eq!(noon.get("light_dir_col_g"), Some(0.5));
    assert_eq!(
        noon.get("light_natural_amb_down_intensity"),
        Some(FRAME_AMBIENT_DOWN)
    );
    assert_eq!(noon.get("fog_hdr"), Some(1.0));

    let dusk = frame_keyframe(
        &own(&[
            ("sky_scattering_sun_intensity_multiplier", 0.5),
            ("sky_scattering_sun_col_b", 0.4),
            ("light_dir_mult", 2.0),
            ("sky_scattering_sun_illuminance", 50_000.0),
        ]),
        true,
    );
    assert_eq!(dusk.get("light_dir_mult"), Some(FRAME_SUN * 0.5));
    assert_eq!(dusk.get("light_dir_col_b"), Some(0.4));
    assert_eq!(
        dusk.get("light_natural_amb_up_intensity"),
        Some(FRAME_AMBIENT_UP * 0.5)
    );
    assert_eq!(
        dusk.get("sky_zenith_col_inten"),
        Some(FRAME_SUN * SKY_SHARE * 0.5)
    );
}

#[test]
fn the_moon_lights_by_its_own_values_and_a_share_of_the_sun() {
    let night = frame_keyframe(
        &own(&[
            ("sky_scattering_sun_intensity_multiplier", 0.0),
            ("sky_scattering_moon_intensity_multiplier", 0.5),
            ("sky_scattering_moon_col_r", 0.25),
            ("light_dir_mult", 2.0),
        ]),
        false,
    );
    assert_eq!(night.get("light_dir_mult"), Some(FRAME_SUN * MOON_SHARE));
    assert_eq!(night.get("light_dir_col_r"), Some(0.25));
    assert_eq!(night.get("sky_sun_hdr"), Some(0.0), "the sun's disc is out");
}

#[test]
fn what_gta_vi_states_under_the_frames_name_passes_and_the_rest_is_left_to_the_frame() {
    let frame = frame_keyframe(
        &own(&[
            ("postfx_tonemap_filmic_w", 11.2),
            ("far_clip", 7000.0),
            ("postfx_exposure_min", -11.0),
            ("sky_moon_iten", 1000.0),
        ]),
        true,
    );
    assert_eq!(frame.get("postfx_tonemap_filmic_w"), Some(11.2));
    assert_eq!(frame.get("far_clip"), Some(7000.0));
    // Stated in another unit than the frame's: not passed.
    assert_eq!(frame.get("postfx_exposure_min"), None);
    assert_eq!(frame.get("sky_moon_iten"), None);
    assert_eq!(frame.get("fog_density"), None);

    let all: Vec<(&str, f32)> = SAME.iter().map(|name| (*name, 1.0)).collect();
    let whole = frame_keyframe(&own(&all), true);
    assert_eq!(whole.len(), filled().count());
    for name in filled() {
        assert!(whole.get(name).is_some(), "{name}");
    }
}

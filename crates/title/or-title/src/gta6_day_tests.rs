use super::*;

fn time() -> String {
    let samples: String = (0..13)
        .map(|k| format!(r#"<sample name="s{k}" hour="{k}" interpolation_type="EASE_LINEAR_IN"/>"#))
        .collect();
    format!(r#"<?xml version="1.0"?><time_data mode="dynamic">{samples}</time_data>"#)
}

/// A cycle whose sun fades with the sample: sample 6 is the top of the round, 4 the rise, 0 the middle of the night.
fn cycle(name: &str, mult: f32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<timecycle_keyframe_data version="2.000000">
  <High_Level_Vars><cloud_cover> 0.25</cloud_cover></High_Level_Vars>
  <cycle name="{name}" regions="2">
    <region name="GLOBAL">
      <light_dir_mult> {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult} {mult}</light_dir_mult>
      <sky_scattering_sun_intensity_multiplier> 0 0 0 0 1 1 1 1 1 1 0 0 0</sky_scattering_sun_intensity_multiplier>
      <far_clip> 100 100 100 100 100 100 300 100 100 100 100 100 100</far_clip>
    </region>
    <region name="URBAN"><light_dir_mult> 9 9 9 9 9 9 9 9 9 9 9 9 9</light_dir_mult></region>
  </cycle>
</timecycle_keyframe_data>"#
    )
}

const WEATHER: &str = r#"<CContentsOfWeatherXmlFile>
  <WeatherTypeDatas>
    <Item><Name>WET</Name><Rain value="1.000000" /><TimeCycleName>Rainy</TimeCycleName></Item>
  </WeatherTypeDatas>
  <WeatherTypes><Item><Name>AM_Wet</Name><DataName>WET</DataName></Item></WeatherTypes>
</CContentsOfWeatherXmlFile>"#;

const SETTINGS: &str = "# Visual Configuration Table\nmisc.MoonDimMult\t0.3\nTonemapping.dark.filmic.W 11.2\nsky.latitude.YPosMin\t-100.0\nsky.latitude.YPosMax\t100.0\nsky.latitude.rangeMin\t20.0\nsky.latitude.rangeMax\t30.0\nmisc.Moon.RollOffset 90\n";

fn tables() -> DayTables {
    let (time, dry, wet) = (time(), cycle("Sunny", 1.0), cycle("Rainy", 0.5));
    let mods = r#"<timecycle_modifier_data><modifier name="m" userFlags="0"><far_clip>1 1</far_clip></modifier></timecycle_modifier_data>"#;
    DayTables::of_texts([
        wet.as_str(),
        "<some_other_table/>",
        SETTINGS,
        time.as_str(),
        "plain text of another kind",
        WEATHER,
        dry.as_str(),
        mods,
    ])
}

#[test]
fn nameless_texts_are_told_apart_by_what_they_open_with_in_any_order() {
    let tables = tables();
    assert!(!tables.is_empty());
    assert_eq!(tables.table.samples.len(), 13);
    assert_eq!(tables.weathers(), ["Rainy", "Sunny"]);
    assert_eq!(tables.high_level["sunny"]["cloud_cover"], 0.25);
    assert_eq!(tables.weather.datas.len(), 1);
    assert_eq!(tables.modifiers.len(), 1);
    assert_eq!(tables.visual_settings.get_f32("misc.MoonDimMult", 1.0), 0.3);
    assert!(DayTables::of_texts(["<other/>", "words"]).is_empty());
}

#[test]
fn the_latitude_is_the_settings_range_at_the_cameras_place_and_the_maps_middle_without_one() {
    let tables = tables();
    assert!((tables.latitude(0.0) - 25f32.to_radians()).abs() < 1e-6);
    assert!((tables.latitude(-500.0) - 20f32.to_radians()).abs() < 1e-6);
    assert!((tables.latitude(100.0) - 30f32.to_radians()).abs() < 1e-6);
    assert_eq!(DayTables::default().latitude(0.0), LATITUDE);
}

#[test]
fn a_moment_blends_the_two_samples_the_sun_stands_between_and_gives_the_frame_its_hour() {
    let tables = tables();
    // Solar noon by the clock: the top of the round, the seventh sample whole.
    let noon = tables
        .moment(13.45, "Sunny", 0.0)
        .expect("a cycle of that name");
    assert!(
        (noon.frame_hours - 13.0).abs() < 1e-3,
        "{}",
        noon.frame_hours
    );
    assert!((noon.own.get("far_clip").expect("stated") - 300.0).abs() < 0.5);
    assert_eq!(noon.keyframe.get("far_clip"), noon.own.get("far_clip"));
    assert!((noon.keyframe.get("light_dir_mult").expect("worked out") - 14.75).abs() < 1e-3);
    assert!(noon.sun[2] > 0.9 && noon.sun[1] < 0.0, "{:?}", noon.sun);
    assert!(
        (noon.table.sun_roll - (std::f32::consts::FRAC_PI_2 + 25f32.to_radians())).abs() < 1e-6
    );
    assert!((noon.table.moon_roll - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert_eq!((noon.moon_dim, noon.wetness), (0.3, 0.0));

    // The middle of the night: the sun's share is nought, and the moon lights.
    let night = tables
        .moment(1.45, "Sunny", 0.0)
        .expect("a cycle of that name");
    assert!(night.sun[2] < -0.5, "{:?}", night.sun);
    assert!(
        !(1.0..23.0).contains(&night.frame_hours),
        "{}",
        night.frame_hours
    );
    let moon = night.keyframe.get("light_dir_mult").expect("worked out");
    assert!(moon > 0.0 && moon < 14.75 * 0.2, "{moon}");
}

#[test]
fn a_weather_is_a_type_of_the_set_or_a_cycle_by_name_and_rain_wets_the_world() {
    let tables = tables();
    for name in ["AM_Wet", "wet", "rainy"] {
        let moment = tables.moment(13.45, name, 0.0).expect(name);
        let strength = moment.keyframe.get("light_dir_mult").expect("worked out");
        assert!((strength - 14.75 * 0.5).abs() < 1e-3, "{name}: {strength}");
    }
    assert_eq!(
        tables.moment(13.45, "AM_Wet", 0.0).map(|m| m.wetness),
        Some(1.0)
    );
    assert_eq!(
        tables.moment(13.45, "rainy", 0.0).map(|m| m.wetness),
        Some(0.0)
    );
    assert!(tables.moment(13.45, "Snow", 0.0).is_none());
    assert!(DayTables::default().moment(13.45, "Sunny", 0.0).is_none());
}

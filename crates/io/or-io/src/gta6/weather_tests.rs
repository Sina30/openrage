use super::*;

const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<CContentsOfWeatherXmlFile>
  <VersionNumber value="1.000000" />
  <WeatherTimeCycleFileNames>
    <Item>COMMON:/DATA/TIMECYCLE/W_ONE.XML</Item>
	<Item>COMMON:/data/timecycle/W_TWO.XML</Item>
  </WeatherTimeCycleFileNames>
  <WeatherTypeDatas>
    <Item>
      <Name>ONE</Name>
      <Sun value="1.000000" />
      <WindMin value="0.500000" />
      <WindMax value="2.000000" />
      <WindAngle value="135.000000" />
      <Rain value="0.250000" />
      <Snow value="0.000000" />
      <Fog value="0.100000" />
      <CloudType value="0.010000" />
      <Sandstorm value="false" />
      <IsNice value="true" />
      <OceanWaveAmplitude value="3.560000" />
      <TimeCycleName>One</TimeCycleName>
      <Variations />
      <CloudMapA>map_a</CloudMapA>
      <CloudMapB>map_b</CloudMapB>
    </Item>
    <Item>
      <Name>TWO</Name>
      <TimeCycleName>Two</TimeCycleName>
    </Item>
  </WeatherTypeDatas>
  <WeatherTypes>
    <Item>
      <Name>AM_One</Name>
      <ShortName>One</ShortName>
      <DataName>ONE</DataName>
      <ModAtmosphere>W_MOD_atmos</ModAtmosphere>
      <ModWind>W_MOD_wind</ModWind>
      <SkipDebugCycling value="true" />
      <WeatherAppDataName>Clear</WeatherAppDataName>
    </Item>
  </WeatherTypes>
  <WeatherTypeIndirectionList>
    <Item>
      <From>ONE</From>
      <To>AM_One</To>
    </Item>
  </WeatherTypeIndirectionList>
</CContentsOfWeatherXmlFile>"#;

#[test]
fn a_weather_set_states_its_files_what_a_weather_is_made_of_and_the_named_weathers() {
    let set = WeatherSet::parse(SET).expect("a weather set");
    assert_eq!(set.cycle_files.len(), 2);
    assert_eq!(set.cycle_files[1], "COMMON:/data/timecycle/W_TWO.XML");
    assert_eq!(set.datas.len(), 2);
    let one = set.data("one").expect("named in the set");
    assert_eq!((one.name.as_str(), one.time_cycle.as_str()), ("ONE", "One"));
    assert_eq!((one.sun, one.rain, one.fog, one.cloud_type), (1.0, 0.25, 0.1, 0.01));
    assert_eq!((one.wind_min, one.wind_max, one.wind_angle), (0.5, 2.0, 135.0));
    assert_eq!(one.values["OceanWaveAmplitude"], 3.56);
    assert_eq!((one.values["Sandstorm"], one.values["IsNice"]), (0.0, 1.0));
    assert_eq!(one.cloud_maps, ["map_a".to_string(), "map_b".to_string()]);
    assert_eq!(set.datas[1].time_cycle, "Two");

    assert_eq!(set.types.len(), 1);
    let weather = &set.types[0];
    assert_eq!((weather.short_name.as_str(), weather.data.as_str()), ("One", "ONE"));
    assert_eq!(weather.modifiers, ["W_MOD_atmos", "W_MOD_wind"]);
    assert_eq!(weather.app, "Clear");
    // A name an indirection names is the type it stands for.
    assert_eq!(set.weather("one").map(|w| w.name.as_str()), Some("AM_One"));
    assert_eq!(set.weather("am_one").map(|w| w.name.as_str()), Some("AM_One"));
    assert!(set.weather("three").is_none());
}

#[test]
fn another_document_and_a_value_that_is_no_number_are_refused() {
    assert_eq!(
        WeatherSet::parse("<time_data/>"),
        Err(WeatherError::NotAWeatherSet("time_data".into()))
    );
    assert_eq!(WeatherSet::parse(""), Err(WeatherError::NotAWeatherSet(String::new())));
    let bad = "<CContentsOfWeatherXmlFile><WeatherTypeDatas><Item><Sun value=\"much\" /></Item></WeatherTypeDatas></CContentsOfWeatherXmlFile>";
    assert_eq!(
        WeatherSet::parse(bad),
        Err(WeatherError::Value {
            element: "Sun".into(),
            value: "much".into()
        })
    );
}

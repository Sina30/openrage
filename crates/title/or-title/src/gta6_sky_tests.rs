use super::*;

/// A keyframe whose air scatters blue about five times as much as red.
fn air() -> Atmosphere {
    let mut k = Keyframe::default();
    for (c, value) in ["r", "g", "b"].iter().zip([0.2, 0.45, 1.0]) {
        k.set(&format!("sky_new_ray_scattering_col_{c}"), value);
    }
    Atmosphere::of(&k)
}

#[test]
fn both_phase_functions_spread_all_the_light_they_scatter() {
    let steps = 200_000;
    let whole = |phase: &dyn Fn(f64) -> f64| {
        (0..steps)
            .map(|i| {
                let c = -1.0 + 2.0 * (f64::from(i) + 0.5) / f64::from(steps);
                phase(c) * 2.0 * PI * 2.0 / f64::from(steps)
            })
            .sum::<f64>()
    };
    assert!((whole(&rayleigh_phase) - 1.0).abs() < 1e-6);
    let mie = whole(&|c| mie_phase(c, 0.8));
    assert!((mie - 1.0).abs() < 1e-3, "{mie}");
    // Mie light goes on forwards far more than back.
    assert!(mie_phase(1.0, 0.8) > 100.0 * mie_phase(-1.0, 0.8));
}

#[test]
fn straight_up_the_air_takes_the_light_each_layer_holds_in_its_scale_height() {
    let air = air();
    let up = air.transmittance(1.0);
    let karman = air.top - air.ground;
    for (c, got) in up.iter().enumerate() {
        let layer = |beta: f64, height: f64| beta * height * (1.0 - (-karman / height).exp());
        // The ozone's tent is whole at its peak and thirty kilometres wide at its foot.
        let depth = layer(air.rayleigh[c], air.rayleigh_height)
            + layer(
                air.mie_scattering[c] + air.mie_absorption[c],
                air.mie_height,
            )
            + air.ozone[c] * 15.0;
        let expected = (-depth).exp() as f32;
        assert!(
            (got - expected).abs() < 0.01 * expected,
            "{c}: {got} and {expected}"
        );
    }
}

#[test]
fn a_low_sun_is_dimmer_and_redder_and_one_under_the_ground_sends_nothing() {
    let air = air();
    let (high, low) = (air.transmittance(1.0), air.transmittance(0.05));
    assert!((0..3).all(|c| low[c] < high[c]), "{low:?} {high:?}");
    assert!(low[0] / low[2] > high[0] / high[2], "{low:?} {high:?}");
    assert!(low[0] > low[2]);
    assert_eq!(air.transmittance(-0.5), [0.0; 3]);
}

#[test]
fn the_sky_is_blue_overhead_scales_with_its_light_and_is_dark_below_the_horizon() {
    let air = air();
    let sun = [0.0, -0.6, 0.8];
    let zenith = air.radiance([0.0, 0.0, 1.0], sun, 100_000.0);
    assert!(zenith[2] > zenith[1] && zenith[1] > zenith[0], "{zenith:?}");
    let doubled = air.radiance([0.0, 0.0, 1.0], sun, 200_000.0);
    assert!((0..3).all(|c| (doubled[c] - 2.0 * zenith[c]).abs() < 1e-3 * doubled[c]));
    assert_eq!(air.radiance([0.0, 0.0, -1.0], sun, 100_000.0), [0.0; 3]);
    assert_eq!(air.radiance([0.0, 0.0, 1.0], sun, 0.0), [0.0; 3]);
    // Towards the sun the forward scattering brightens the sky.
    let near_sun = air.radiance([0.0, -0.62, 0.78], sun, 100_000.0);
    assert!(near_sun[0] > zenith[0], "{near_sun:?} {zenith:?}");
}

#[test]
fn the_ground_takes_less_of_the_skys_light_as_the_sun_goes_down() {
    let air = air();
    let high = air.irradiance([0.0, 0.0, 1.0], 100_000.0);
    let low = air.irradiance([1.0, 0.0, 0.05], 100_000.0);
    assert!(
        (0..3).all(|c| high[c] > low[c] && low[c] > 0.0),
        "{high:?} {low:?}"
    );
    // A sky is a small part of a sun's light, and it is blue.
    assert!(high[2] < 0.5 * 100_000.0 && high[2] > high[0], "{high:?}");
}

#[test]
fn an_unstated_atmosphere_is_the_published_earth_one() {
    let air = Atmosphere::of(&Keyframe::default());
    assert_eq!((air.ground, air.top), (6360.0, 6460.0));
    assert_eq!(
        (air.rayleigh_height, air.mie_height),
        (8.0, f64::from(1.2f32))
    );
    let earth = |got: [f64; 3], want: [f64; 3]| (0..3).all(|c| (got[c] - want[c]).abs() < 1e-9);
    assert!(
        earth(air.rayleigh, [0.005802, 0.013558, 0.0331]),
        "{:?}",
        air.rayleigh
    );
    assert!(earth(air.mie_scattering, [0.003996; 3]));
    assert!(earth(air.mie_absorption, [0.000444; 3]));
    assert!(
        earth(air.ozone, [0.000650, 0.001881, 0.000085]),
        "{:?}",
        air.ozone
    );
    // The ozone's density is a tent between ten and forty kilometres.
    assert!(air.densities(10.0)[2] < 1e-6);
    assert!((air.densities(25.0)[2] - 1.0).abs() < 1e-6);
    assert_eq!(air.densities(41.0)[2], 0.0);
}

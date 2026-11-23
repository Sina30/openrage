use super::*;

fn spot(outer_degrees: f32) -> SpotLight {
    SpotLight {
        position: Vec3::new(1.0, 2.0, 3.0),
        direction: Vec3::NEG_Z,
        colour: Vec3::new(0.5, 0.25, 0.125),
        intensity: 7.0,
        radius: 4.0,
        falloff: 2.0,
        outer_degrees,
    }
}

#[test]
fn a_spot_is_read_by_the_semantics_the_game_gives_its_inputs() {
    let light = spot(60.0);
    assert_eq!(light_value("TEXCOORD", 0, &light), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(light_value("TEXCOORD", 1, &light), [0.0, 0.0, -1.0, 2.0]);
    let [cos, sin, scale, intensity] = light_value("TEXCOORD", 2, &light);
    assert!((cos - 0.5).abs() < 1e-6 && (sin - 0.75f32.sqrt()).abs() < 1e-6);
    assert_eq!(intensity, 7.0);
    // The light fades from the inner angle, a degree, to the outer
    let inner = 1f32.to_radians().cos();
    assert!((scale - 1.0 / (inner - 0.5)).abs() < 1e-4, "{scale}");
    assert_eq!(light_value("COLOR", 0, &light), [0.5, 0.25, 0.125, 1.0]);
    assert_eq!(light_value("NORMAL", 0, &light), [0.0; 4]);
}

#[test]
fn the_cone_stays_between_a_degree_and_eighty_nine_whatever_the_rule_states() {
    let [cos, ..] = spot(0.0).cone();
    assert!((cos - 1f32.to_radians().cos()).abs() < 1e-6, "{cos}");
    let [cos, _, scale] = spot(120.0).cone();
    assert!((cos - 89f32.to_radians().cos()).abs() < 1e-6, "{cos}");
    assert!(scale.is_finite() && scale > 0.0);
    // A cone of one degree has no room for an inner angle, and the scale stays finite
    let [_, _, scale] = spot(1.0).cone();
    assert!(scale.is_finite() && scale > 0.0, "{scale}");
}

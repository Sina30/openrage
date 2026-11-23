use super::*;

#[test]
fn a_light_is_read_by_the_semantics_the_game_gives_its_inputs() {
    let light = PointLight {
        position: Vec3::new(1.0, 2.0, 3.0),
        colour: Vec3::new(0.5, 0.25, 0.125),
        intensity: 7.0,
        radius: 4.0,
        falloff: 2.0,
    };
    assert_eq!(light_value("TEXCOORD", 0, &light), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(light_value("TEXCOORD", 1, &light), [7.0, 2.0, 0.0, 0.0]);
    assert_eq!(light_value("COLOR", 0, &light), [0.5, 0.25, 0.125, 1.0]);
    assert_eq!(light_value("NORMAL", 0, &light), [0.0; 4]);
}

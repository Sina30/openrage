use or_io::gta6::map::occluders::{BoxOccluder, OccludeModel};
use or_io::gta6::map::Aabb;
use or_renderer::FlyCamera;

use super::*;

/// The occluders of one map of these boxes, each from its lowest corner to its highest.
fn walls(boxes: &[(Vec3, Vec3)]) -> Arc<[Arc<MapOccluders>]> {
    let boxes = (boxes.iter())
        .map(|(min, max)| BoxOccluder {
            corners: box_corners(*min, *max).map(Vec3::to_array),
        })
        .collect();
    Arc::from([Arc::new(MapOccluders {
        boxes,
        models: Vec::new(),
    })])
}

/// Drawn for the default camera, which stands at the origin and looks along +Y with Z up.
fn drawn(occluders: &Arc<[Arc<MapOccluders>]>) -> Occlusion {
    let mut occlusion = Occlusion::default();
    let view_proj = FlyCamera::default().view_proj(1.0);
    occlusion.draw(occluders, view_proj, Vec3::ZERO);
    occlusion
}

/// A wall 20 m wide and high, 20 m ahead, a metre thick: its sides are at a half of the distance.
fn wall() -> (Vec3, Vec3) {
    (Vec3::new(-10.0, 20.0, -10.0), Vec3::new(10.0, 21.0, 10.0))
}

#[test]
fn a_sphere_wholly_behind_a_wall_is_hidden() {
    let occlusion = drawn(&walls(&[wall()]));
    assert!(occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 2.0));
    // Near the wall's side and still behind it.
    assert!(occlusion.hides(Vec3::new(20.0, 50.0, 0.0), 2.0));
    // Larger than the wall's shadow is never hidden.
    assert!(!occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 30.0));
}

#[test]
fn a_sphere_half_out_from_behind_a_wall_is_seen() {
    let occlusion = drawn(&walls(&[wall()]));
    // Its centre on the line from the eye past the wall's side, and on the line past its top.
    assert!(!occlusion.hides(Vec3::new(25.0, 50.0, 0.0), 2.0));
    assert!(!occlusion.hides(Vec3::new(-25.0, 50.0, 0.0), 2.0));
    assert!(!occlusion.hides(Vec3::new(0.0, 50.0, 25.0), 2.0));
    // Barely out: a tenth of its radius past the side.
    assert!(!occlusion.hides(Vec3::new(25.0 + 2.0 * 1.1, 50.0, 0.0), 2.0));
}

#[test]
fn a_sphere_nearer_than_the_wall_is_seen() {
    let occlusion = drawn(&walls(&[wall()]));
    assert!(!occlusion.hides(Vec3::new(0.0, 10.0, 0.0), 2.0));
    // Touching the wall's face.
    assert!(!occlusion.hides(Vec3::new(0.0, 18.5, 0.0), 1.5));
    // Through the wall.
    assert!(!occlusion.hides(Vec3::new(0.0, 21.0, 0.0), 2.0));
    // Holding the eye.
    assert!(!occlusion.hides(Vec3::new(0.0, 1.0, 0.0), 2.0));
}

#[test]
fn an_occluder_that_holds_the_camera_hides_nothing() {
    let around = (Vec3::splat(-5.0), Vec3::splat(5.0));
    let occlusion = drawn(&walls(&[around]));
    assert!(!occlusion.any);
    assert!(!occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 2.0));
    // A wall still hides behind it while the camera stands in another occluder.
    let occlusion = drawn(&walls(&[around, wall()]));
    assert!(occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 2.0));
    assert!(!occlusion.hides(Vec3::new(0.0, 4.0, 0.0), 0.5));
}

#[test]
fn a_wall_that_reaches_behind_the_camera_is_cut_at_the_near_plane() {
    // Beside the eye, from 10 m behind it to 30 m ahead.
    let beside = (Vec3::new(2.0, -10.0, -10.0), Vec3::new(3.0, 30.0, 10.0));
    let occlusion = drawn(&walls(&[beside]));
    // Every line from the eye to this sphere's box crosses the wall.
    assert!(occlusion.hides(Vec3::new(10.0, 20.0, 0.0), 2.0));
    // On the other side of the eye.
    assert!(!occlusion.hides(Vec3::new(-10.0, 20.0, 0.0), 2.0));
    // Past the wall's far end.
    assert!(!occlusion.hides(Vec3::new(10.0, 200.0, 0.0), 2.0));
}

#[test]
fn what_is_small_on_the_screen_is_not_drawn() {
    // A metre cube 500 m off covers well under BOX_AREA texels.
    let small = (Vec3::new(0.0, 500.0, 0.0), Vec3::new(1.0, 501.0, 1.0));
    let occlusion = drawn(&walls(&[small]));
    assert!(!occlusion.any);
    // With no occluder nothing is hidden.
    let occlusion = drawn(&Arc::from([]));
    assert!(!occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 0.1));
}

#[test]
fn an_occlude_model_hides_as_a_box_does() {
    let (min, max) = wall();
    let vertices: Vec<[f32; 3]> = box_corners(min, max).map(Vec3::to_array).to_vec();
    let model = OccludeModel {
        bounds: Aabb {
            min: min.to_array(),
            max: max.to_array(),
        },
        flags: 0,
        vertices,
        triangles: BOX_TRIANGLES.to_vec(),
    };
    let occluders: Arc<[Arc<MapOccluders>]> = Arc::from([Arc::new(MapOccluders {
        boxes: Vec::new(),
        models: vec![model],
    })]);
    let occlusion = drawn(&occluders);
    assert!(occlusion.hides(Vec3::new(0.0, 50.0, 0.0), 2.0));
    assert!(!occlusion.hides(Vec3::new(25.0, 50.0, 0.0), 2.0));
}

#[test]
fn the_buffer_is_drawn_again_only_when_the_view_or_the_occluders_change() {
    let occluders = walls(&[wall()]);
    let mut occlusion = drawn(&occluders);
    // Marked by hand: a draw for the same view and list keeps it.
    occlusion.any = false;
    let view_proj = FlyCamera::default().view_proj(1.0);
    occlusion.draw(&occluders, view_proj, Vec3::ZERO);
    assert!(!occlusion.any);
    occlusion.draw(&walls(&[wall()]), view_proj, Vec3::ZERO);
    assert!(occlusion.any);
}

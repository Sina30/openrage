use super::*;

/// Three rooms 4 m long in a row along x, 4 m wide and 3 high: a front door from the outside into
/// room 1 at x = 0, a door from room 1 into room 2 at x = 4 and one from room 2 into room 3 at
/// x = 8 near the north wall.
fn row() -> (Vec<RoomBox>, Vec<Opening>, Vec3) {
    let room = |x: f32, to: f32| RoomBox {
        min: Vec3::new(x, -2.0, 0.0),
        max: Vec3::new(to, 2.0, 3.0),
    };
    let door = |rooms, x: f32, y: [f32; 2]| Opening {
        rooms,
        corners: [
            Vec3::new(x, y[0], 0.0),
            Vec3::new(x, y[1], 0.0),
            Vec3::new(x, y[1], 2.0),
            Vec3::new(x, y[0], 2.0),
        ],
        flags: 0,
    };
    let rooms = vec![
        room(0.0, 12.0),
        room(0.0, 4.0),
        room(4.0, 8.0),
        room(8.0, 12.0),
    ];
    let portals = vec![
        door([1, 0], 0.0, [-0.5, 0.5]),
        door([1, 2], 4.0, [-0.5, 0.5]),
        door([2, 3], 8.0, [1.0, 1.8]),
    ];
    (rooms, portals, Vec3::new(6.0, 0.0, 1.5))
}

fn rooms_seen(eye: [f32; 3]) -> Vec<bool> {
    let (rooms, portals, middle) = row();
    seen(&rooms, &portals, middle, Vec3::from(eye)).rooms
}

#[test]
fn the_camera_stands_in_the_smallest_room_that_holds_it() {
    let (rooms, ..) = row();
    assert_eq!(room_of(&rooms, Vec3::new(2.0, 0.0, 1.0)), Some(1));
    assert_eq!(room_of(&rooms, Vec3::new(6.0, 0.0, 1.0)), Some(2));
    // Room 0 holds every room and is never the one the camera stands in.
    assert_eq!(room_of(&rooms, Vec3::new(-5.0, 0.0, 1.0)), None);
}

#[test]
fn from_a_room_the_camera_sees_the_rooms_its_doors_open_on() {
    // In the middle of room 1 the door at x = 8 shows through the one at x = 4.
    assert_eq!(rooms_seen([1.0, 0.0, 1.0]), [true, true, true, true]);
    // From the south side of room 1 the north door at x = 8 is out of the line of the door at 4.
    assert_eq!(rooms_seen([1.0, -1.5, 1.0]), [true, true, true, true]);
    assert_eq!(rooms_seen([1.0, 1.5, 1.0]), [true, true, true, false]);
    let (rooms, portals, middle) = row();
    let seen = seen(&rooms, &portals, middle, Vec3::new(1.0, 1.5, 1.0));
    assert_eq!(seen.portals, [true, true, false]);
}

#[test]
fn from_outside_the_camera_sees_in_through_the_shell_s_doors_alone() {
    // Straight in, the door at x = 8 is narrower than what the door at x = 4 lets through, and off its line.
    assert_eq!(rooms_seen([-5.0, 0.0, 1.0]), [true, true, true, false]);
    // From a little south the three doors line up, through a sliver of each.
    assert_eq!(rooms_seen([-5.0, -0.9, 1.0]), [true, true, true, true]);
    // Off to the side, the door at x = 4 is not in line with the front door.
    assert_eq!(rooms_seen([-5.0, 3.0, 1.0]), [true, true, false, false]);
    // From behind the building the front door opens away from the camera.
    assert_eq!(rooms_seen([20.0, 0.0, 1.0]), [true, false, false, false]);
}

#[test]
fn inside_the_sky_s_ambient_light_comes_in_through_the_openings_to_the_outside_alone() {
    let (rooms, portals, _) = row();
    let at = |room, p: [f32; 3]| ambient_of(&rooms, &portals, room, Vec3::from(p));
    // In the front door, the whole of the sky's.
    assert_eq!(at(1, [0.0, 0.0, 1.0]), [1.0, 0.0]);
    // A metre in, the room's daylight factor: a door of 2 m² in a room of 80 m² of surface.
    let factor = 2.0 * 90.0 / (100.0 * 80.0 * 0.75);
    let inside = at(1, [1.0, 0.0, 1.0]);
    assert!(
        (inside[0] - factor).abs() < 1e-6 && inside[1] == 0.0,
        "{inside:?} {factor}"
    );
    // Half way out along the door's width past its edge, more than the room's and less than the whole.
    let edge = at(1, [0.0, 0.5 * 1.125, 1.0]);
    assert!(
        edge[0] > factor && edge[0] < 1.0 && edge[1] == 0.0,
        "{edge:?}"
    );
    // Room 2 has no opening to the outside, even beside the door into room 1.
    assert_eq!(at(2, [4.0, 0.0, 1.0]), [0.0, 0.0]);
    // What stands in room 0 stands outdoors.
    assert_eq!(at(0, [1.0, 0.0, 1.0]), or_renderer::OUTDOORS);
    // A mirror on the front, or a link to another interior, lets in none of the sky's.
    for flags in [MIRROR, LINK] {
        let mut shut = portals.clone();
        shut[0].flags = flags;
        assert_eq!(
            ambient_of(&rooms, &shut, 1, Vec3::new(0.0, 0.0, 1.0)),
            [0.0, 0.0]
        );
    }
}

#[test]
fn a_mirror_is_seen_and_nothing_is_seen_through_it() {
    let (rooms, mut portals, middle) = row();
    portals[1].flags = MIRROR;
    let seen = seen(&rooms, &portals, middle, Vec3::new(1.0, 0.0, 1.0));
    assert_eq!(seen.rooms, [true, true, false, false]);
    assert_eq!(seen.portals, [true, true, false]);
}

#[test]
fn a_camera_in_a_doorway_sees_through_the_whole_of_it() {
    let (rooms, portals, middle) = row();
    // Five centimetres into room 2 the door's edges pass behind the camera.
    let seen = seen(&rooms, &portals, middle, Vec3::new(4.05, 0.0, 1.0));
    assert!(seen.rooms[1] && seen.rooms[0]);
}

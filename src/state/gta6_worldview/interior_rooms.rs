//! Which rooms and portals of an interior a camera sees, in the interior's own space: the room it
//! stands in and the rooms seen through its portals, each portal clipped to the opening the camera
//! looks through to reach it; from outside, the rooms seen through the portals to the outside.
//!
//! ⚠ Room 0 is the outside of every other room: a portal to it is a door or window of the shell.

use glam::Vec3;

/// How near a portal's plane the camera may stand and still look through the whole of it, in metres.
const DOORWAY: f32 = 0.1;
/// How many portals deep a camera looks.
const DEPTH: usize = 8;

/// A room's box, in the interior's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RoomBox {
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

impl RoomBox {
    fn holds(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    fn volume(&self) -> f32 {
        (self.max - self.min).max(Vec3::ZERO).element_product()
    }
}

/// A portal between two rooms, with its four corners in the interior's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Opening {
    pub(super) rooms: [u32; 2],
    pub(super) corners: [Vec3; 4],
}

/// What a camera sees of an interior: by room and by portal.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Seen {
    pub(super) rooms: Vec<bool>,
    pub(super) portals: Vec<bool>,
}

/// The room other than room 0 whose box holds `eye`, the smallest where boxes overlap.
pub(super) fn room_of(rooms: &[RoomBox], eye: Vec3) -> Option<usize> {
    (rooms.iter().enumerate().skip(1))
        .filter(|(_, room)| room.holds(eye))
        .min_by(|(_, a), (_, b)| a.volume().total_cmp(&b.volume()))
        .map(|(k, _)| k)
}

/// A plane through the camera: a point `p` is inside where `normal · (p − eye) ≥ 0`.
type Planes = Vec<Vec3>;

/// The part of `polygon` inside every one of `planes` through `eye`.
fn clip(mut polygon: Vec<Vec3>, eye: Vec3, planes: &[Vec3]) -> Vec<Vec3> {
    for normal in planes {
        if polygon.len() < 3 {
            break;
        }
        let side = |p: Vec3| normal.dot(p - eye);
        let mut out = Vec::with_capacity(polygon.len() + 1);
        for (k, &a) in polygon.iter().enumerate() {
            let b = polygon[(k + 1) % polygon.len()];
            let (sa, sb) = (side(a), side(b));
            if sa >= 0.0 {
                out.push(a);
            }
            if (sa >= 0.0) != (sb >= 0.0) {
                out.push(a + (b - a) * (sa / (sa - sb)));
            }
        }
        polygon = out;
    }
    polygon
}

/// The planes through `eye` and each edge of `polygon`, each facing its middle.
fn pyramid(eye: Vec3, polygon: &[Vec3]) -> Planes {
    let middle = polygon.iter().copied().sum::<Vec3>() / polygon.len() as f32;
    (0..polygon.len())
        .filter_map(|k| {
            let (a, b) = (polygon[k] - eye, polygon[(k + 1) % polygon.len()] - eye);
            let normal = a.cross(b).normalize_or_zero();
            match normal.dot(middle - eye) {
                d if d > 0.0 => Some(normal),
                d if d < 0.0 => Some(-normal),
                _ => None,
            }
        })
        .collect()
}

/// The plane of an opening, as a unit normal and a point on it; `None` for one with no area.
fn plane_of(corners: &[Vec3; 4]) -> Option<(Vec3, Vec3)> {
    let normal = (corners[2] - corners[0]).cross(corners[3] - corners[1]);
    let normal = normal.try_normalize()?;
    Some((normal, corners.iter().copied().sum::<Vec3>() / 4.0))
}

struct Walk<'a> {
    eye: Vec3,
    portals: &'a [Opening],
    /// The middle of the interior's box: the side of a shell's portal its rooms are on.
    middle: Vec3,
    seen: Seen,
    path: Vec<u32>,
}

impl Walk<'_> {
    fn enter(&mut self, room: u32, planes: &[Vec3]) {
        if let Some(slot) = self.seen.rooms.get_mut(room as usize) {
            *slot = true;
        }
        if self.path.len() >= DEPTH {
            return;
        }
        self.path.push(room);
        for (k, portal) in self.portals.iter().enumerate() {
            let other = match portal.rooms {
                [a, b] if a == room && b != room => b,
                [a, b] if b == room && a != room => a,
                _ => continue,
            };
            if self.path.contains(&other) {
                continue;
            }
            let Some((normal, point)) = plane_of(&portal.corners) else {
                continue;
            };
            let off = normal.dot(self.eye - point);
            // From outside, a shell's portal is looked into only from the side away from its rooms.
            if room == 0 && off * normal.dot(self.middle - point) > 0.0 {
                continue;
            }
            let next = match off.abs() < DOORWAY {
                // Standing in the opening, the camera sees through all of it what it saw before.
                true => planes.to_vec(),
                false => {
                    let opening = clip(portal.corners.to_vec(), self.eye, planes);
                    if opening.len() < 3 {
                        continue;
                    }
                    pyramid(self.eye, &opening)
                }
            };
            self.seen.portals[k] = true;
            self.enter(other, &next);
        }
        self.path.pop();
    }
}

/// What a camera at `eye` sees of an interior of `rooms` and `portals` whose box's middle is
/// `middle`: from the room it stands in, else from outside.
pub(super) fn seen(rooms: &[RoomBox], portals: &[Opening], middle: Vec3, eye: Vec3) -> Seen {
    let mut walk = Walk {
        eye,
        portals,
        middle,
        seen: Seen {
            rooms: vec![false; rooms.len()],
            portals: vec![false; portals.len()],
        },
        path: Vec::new(),
    };
    let start = room_of(rooms, eye).unwrap_or(0) as u32;
    walk.enter(start, &[]);
    walk.seen
}

#[cfg(test)]
#[path = "interior_rooms_tests.rs"]
mod tests;

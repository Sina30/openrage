//! The lights the frame draws: each light's strength at the hour and at the camera's distance, in
//! the frame's units, and the brightest of them at the camera as the frame's point, spot and
//! capsule lights, each as the nested volumes that follow GTA VI's law (`lights_law`).

use glam::Vec3;
use or_io::gta5::data::visual_settings::VisualSettings;
use or_io::gta6::map::light_ext::{Light, LightKind};
use or_renderer::game_shaders::distant_lights::SpriteSize;
use or_renderer::{CapsuleLight, DistantLight, PointLight, SpotLight};

use super::lights_law::{spot_cones, SHELLS, SHELL_EXPONENT};
use super::lights_lod::{Distant, MapLod};

/// The most lights the frame is handed: the placed lights a dense street holds within 150 m.
pub const MOST: usize = 2048;
/// Every hour of the day: a level-of-detail list states none.
const ALL_HOURS: u32 = 0x00FF_FFFF;
/// The falloff exponent every level-of-detail list states, which GTA VI's law does not read.
const LOD_FALLOFF: f32 = 1.0;

/// The volumes the frame draws the lights with, by the pass of each kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameLights {
    pub points: Vec<PointLight>,
    pub spots: Vec<SpotLight>,
    pub capsules: Vec<CapsuleLight>,
    /// How many lights they draw.
    pub lights: usize,
    /// Where each light drawn as volumes stands, and how much of it its reach leaves.
    pub kept: Vec<(Vec3, f32)>,
    /// The distant lights drawn as sprites, and how large a sprite is.
    pub sprites: Vec<DistantLight>,
    pub sprite_size: SpriteSize,
}
/// A level-of-detail light stands for a placed light this near it, in metres.
const SAME_PLACE: f32 = 0.02;

/// A light the frame may draw: a placed one, or one of a level-of-detail list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lamp {
    /// In the world.
    pub light: Light,
    /// For a level-of-detail light: how far from the camera it is drawn, and the last metres of that it fades out over.
    pub reach: Option<[f32; 2]>,
}

/// How far the level-of-detail lights of each size class (small, medium, large) are drawn, and the metres they fade out over, in metres.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LodReach {
    pub range: [f32; 3],
    pub fadeout: [f32; 3],
}

impl LodReach {
    /// As the install's visual settings state it; nought, which draws no such light, where they do not.
    pub fn of(settings: &VisualSettings) -> Self {
        let size = ["small", "medium", "large"];
        let get = |s: &str, what: &str| settings.get_f32(&format!("lodlight.{s}.{what}"), 0.0);
        Self {
            range: size.map(|s| get(s, "range")),
            fadeout: size.map(|s| get(s, "fadeout")),
        }
    }

    fn of_category(&self, category: u16) -> Option<[f32; 2]> {
        let c = usize::from(category);
        Some([*self.range.get(c)?, *self.fadeout.get(c)?])
    }
}

/// How far a light lit in the hours `hours` is on at `hour` (0 to 24): it comes on, and goes off, over the last minute of an hour.
pub fn hour_strength(hours: u32, hour: f32) -> f32 {
    let hour = hour.rem_euclid(24.0);
    let now = (hour as u32).min(23);
    let lit = |h: u32| hours >> h & 1 == 1;
    let ramp = (hour.fract() * 60.0 - 59.0).clamp(0.0, 1.0);
    match (lit(now), lit((now + 1) % 24)) {
        (true, true) => 1.0,
        (false, false) => 0.0,
        (true, false) => 1.0 - ramp,
        (false, true) => ramp,
    }
}

fn reach_strength(reach: Option<[f32; 2]>, distance: f32) -> f32 {
    match reach {
        None => 1.0,
        Some([range, fade]) => ((range - distance) / fade.max(1e-3)).clamp(0.0, 1.0),
    }
}

/// The level-of-detail lights of `lod`, standing at the distant lights of `parent`, the map its `parent` names; none where the two lists are not as long.
pub fn lod_lamps(lod: &MapLod, parent: &MapLod, reach: &LodReach) -> Vec<Lamp> {
    let distant: &[Distant] = &parent.distant;
    let Some(reach) = reach.of_category(parent.category) else {
        return Vec::new();
    };
    if distant.len() != lod.falloff.len() || reach[0] <= 0.0 {
        return Vec::new();
    }
    let cone = |byte: u8| f32::from(byte) * 180.0 / 255.0;
    (distant.iter().enumerate())
        .filter_map(|(i, d)| {
            let kind = LightKind::of(d.light_type)?;
            let outer = lod.cone_outer[i];
            Some(Lamp {
                light: Light {
                    kind,
                    position: d.position,
                    direction: lod.direction[i],
                    colour: d.rgb.map(|c| f32::from(c) / 255.0),
                    intensity: d.intensity,
                    radius: lod.falloff[i],
                    falloff_exponent: LOD_FALLOFF,
                    hours: ALL_HOURS,
                    flags: 0,
                    cone_inner: cone(lod.cone_inner[i]),
                    cone_outer: cone(outer),
                    length: match kind {
                        LightKind::Capsule => f32::from(outer) * 140.0 / 255.0,
                        _ => 0.0,
                    },
                },
                reach: Some(reach),
            })
        })
        .collect()
}

/// Places, each with a value, found again by any point within [`SAME_PLACE`] of one.
pub struct Places(std::collections::HashMap<[i64; 3], Vec<(Vec3, f32)>>);

impl Places {
    fn cell(p: Vec3) -> [i64; 3] {
        p.to_array().map(|x| (x / SAME_PLACE).floor() as i64)
    }

    pub fn of(places: impl IntoIterator<Item = (Vec3, f32)>) -> Self {
        let mut at = std::collections::HashMap::<_, Vec<_>>::new();
        for (p, value) in places {
            at.entry(Self::cell(p)).or_default().push((p, value));
        }
        Self(at)
    }

    /// How far from `p` the farthest place lies, and [`SAME_PLACE`] beyond; nought where there is none.
    pub fn farthest_from(&self, p: Vec3) -> f32 {
        (self.0.values().flatten())
            .map(|(q, _)| q.distance(p) + SAME_PLACE)
            .fold(0.0, f32::max)
    }

    /// The value of a place within [`SAME_PLACE`] of `p`.
    pub fn near(&self, p: Vec3) -> Option<f32> {
        let [x, y, z] = Self::cell(p);
        let cells = (-1..=1).flat_map(|dx| {
            (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [x + dx, y + dy, z + dz]))
        });
        cells
            .filter_map(|c| self.0.get(&c))
            .flatten()
            .find(|(q, _)| q.distance(p) <= SAME_PLACE)
            .map(|(_, value)| *value)
    }
}

/// Where the placed lights stand, to find the level-of-detail lights they stand in for.
#[derive(Debug, Default)]
pub struct Placed {
    at: std::collections::HashMap<[i64; 3], Vec<Vec3>>,
    /// The box of them, [`SAME_PLACE`] wider each way.
    low: Vec3,
    high: Vec3,
}

fn cell(p: Vec3) -> [i64; 3] {
    p.to_array().map(|x| (x / SAME_PLACE).floor() as i64)
}

impl Placed {
    pub fn of(placed: &[Lamp]) -> Self {
        let mut out = Self {
            low: Vec3::INFINITY,
            high: Vec3::NEG_INFINITY,
            ..Self::default()
        };
        for lamp in placed {
            let p = Vec3::from(lamp.light.position);
            out.at.entry(cell(p)).or_default().push(p);
            (out.low, out.high) = (out.low.min(p), out.high.max(p));
        }
        (out.low, out.high) = (out.low - SAME_PLACE, out.high + SAME_PLACE);
        out
    }

    /// Whether a placed light stands within [`SAME_PLACE`] of `p`.
    pub fn stands_at(&self, p: [f32; 3]) -> bool {
        let p = Vec3::from(p);
        // ⚠ The placed lights stand near the camera and the lists' reach for kilometres: most of a list is no placed light's.
        if p.cmplt(self.low).any() || p.cmpgt(self.high).any() {
            return false;
        }
        let [x, y, z] = cell(p);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dz| {
                    (self.at.get(&[x + dx, y + dy, z + dz]).into_iter().flatten())
                        .any(|q| q.distance(p) <= SAME_PLACE)
                })
            })
        })
    }
}

/// `lod` without the lights a lamp of `placed` stands within [`SAME_PLACE`] of: the placed one is drawn instead.
#[cfg(test)]
pub fn without_placed(lod: Vec<Lamp>, placed: &[Lamp]) -> Vec<Lamp> {
    let near = Placed::of(placed);
    (lod.into_iter())
        .filter(|l| !near.stands_at(l.light.position))
        .collect()
}

/// A map's level-of-detail lamps, with the box they stand in and how far they are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct LodList {
    pub lamps: Vec<Lamp>,
    low: Vec3,
    high: Vec3,
    range: f32,
}

impl LodList {
    pub fn of(lamps: Vec<Lamp>) -> Self {
        let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        let mut range = 0.0f32;
        for lamp in &lamps {
            let p = Vec3::from(lamp.light.position);
            (low, high) = (low.min(p), high.max(p));
            range = range.max(lamp.reach.map_or(f32::INFINITY, |[range, _]| range));
        }
        Self {
            lamps,
            low,
            high,
            range,
        }
    }

    /// Whether a lamp of it may be drawn from `eye`: none past the reach of every one of them is.
    pub fn reaches(&self, eye: Vec3) -> bool {
        if self.lamps.is_empty() {
            return false;
        }
        // A position that is not a number makes no box: such a list is never passed over
        if !self.low.cmple(self.high).all() {
            return true;
        }
        // A metre to spare, so that a lamp on the box's face is never judged by a distance rounded differently.
        eye.distance(eye.clamp(self.low, self.high)) < self.range + 1.0
    }
}

/// The lamps lit at `hour` and within reach of `eye`, their intensity times `scale` (the frame's units for a stated intensity), the `most` nearest the camera by the edge of their reach, nearest first, as the volumes of the frame's passes.
pub fn frame_lights<'a>(
    lamps: impl IntoIterator<Item = &'a Lamp>,
    eye: Vec3,
    hour: f32,
    scale: f32,
    most: usize,
) -> FrameLights {
    let mut lit: Vec<(f32, Light, f32)> = (lamps.into_iter())
        .filter_map(|lamp| {
            let l = lamp.light;
            let distance = Vec3::from(l.position).distance(eye);
            let reach = reach_strength(lamp.reach, distance);
            let intensity = l.intensity * scale * hour_strength(l.hours, hour) * reach;
            if !(intensity > 0.0 && l.radius > 0.0 && l.colour.iter().any(|c| *c > 0.0)) {
                return None;
            }
            // The nearest by the edge of their reach are kept: a light past them is a distant light's sprite.
            Some((
                -(distance - l.radius).max(0.0),
                Light { intensity, ..l },
                reach,
            ))
        })
        .collect();
    if lit.len() > most && most > 0 {
        lit.select_nth_unstable_by(most - 1, |a, b| b.0.total_cmp(&a.0));
    }
    lit.truncate(most);
    lit.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = FrameLights {
        lights: lit.len(),
        kept: lit
            .iter()
            .map(|(_, l, reach)| (Vec3::from(l.position), *reach))
            .collect(),
        ..FrameLights::default()
    };
    for (_, l, _) in lit {
        let (position, colour) = (Vec3::from(l.position), Vec3::from(l.colour));
        let direction = Vec3::from(l.direction).normalize_or_zero();
        // A spot or a capsule with no axis shines all round.
        let kind = match direction == Vec3::ZERO {
            true => LightKind::Point,
            false => l.kind,
        };
        let cones = spot_cones(l.cone_inner, l.cone_outer);
        // The law's strength is the stated intensity over d², so each volume's is over R².
        let per_area = l.intensity / (l.radius * l.radius);
        for (share, strength) in SHELLS {
            let (radius, intensity) = (l.radius * share.sqrt(), per_area * strength);
            match kind {
                LightKind::Spot => {
                    let cones = cones.iter().filter(|(_, part)| *part > 0.0);
                    out.spots.extend(cones.map(|&(outer, part)| SpotLight {
                        position,
                        direction,
                        colour,
                        intensity: intensity * part,
                        radius,
                        falloff: SHELL_EXPONENT,
                        outer_degrees: outer,
                        inner_degrees: l.cone_inner,
                    }))
                }
                LightKind::Capsule => out.capsules.push(CapsuleLight {
                    position,
                    direction,
                    length: l.length,
                    colour,
                    intensity,
                    radius,
                    falloff: SHELL_EXPONENT,
                }),
                // ponytail: an area light shines as a point from its middle; the frame has no pass for a quad.
                LightKind::Point | LightKind::Area => out.points.push(PointLight {
                    position,
                    colour,
                    intensity,
                    radius,
                    falloff: SHELL_EXPONENT,
                }),
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "lights_frame_tests.rs"]
mod tests;

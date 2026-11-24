//! The lights of the maps around the camera, read on workers: the lights an entity owns, the
//! lights its archetype holds, and the level-of-detail lights of the maps that list them, as the
//! point and spot lights the frame draws.
//!
//! ⚠ A level-of-detail light is a copy of a placed light: where a placed light stands within
//! 2 cm of it, the placed one is drawn while its map is loaded and the copy is not.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use glam::Vec3;
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta5::data::visual_settings::VisualSettings;
use or_io::gta6::map::light_ext::{self, Light, LightKind};
use or_io::gta6::map::lights::{self as effects, LightOwner, Pose};
use or_io::gta6::map::{self, MapBody};
use or_io::meta::binary::rbm::schema::RbmModel;
use or_renderer::{PointLight, SpotLight};

use super::index::{Enabled, WorldIndex, ALL_HOURS};
use super::lights_lod::{self, MapLod};
use super::pool::Pool;

/// The bytes a map body is read to.
const BODY_LIMIT: u64 = 256 << 20;
/// How far the camera moves before the wanted maps are worked out again, in metres.
const RELOOK: f32 = 8.0;
/// How far from the camera a map's entities may lie for their own lights to be read, in metres.
const PLACED_REACH: f32 = 300.0;
/// The bits of a map's content flags that say it lists level-of-detail lights, and distant ones.
const LOD_LIGHTS: u32 = 1 << 7;
const DISTANT_LIGHTS: u32 = 1 << 8;
const HIGH_DETAIL: u32 = 1;
/// How near a placed light stands to a level-of-detail light it is the source of, in metres.
const SOURCE: f32 = 0.02;
/// The frame's lights take no exponent: every light fades by the one window.
const FALLOFF: f32 = 1.0;

/// What was read of the lights around the camera, for a panel to say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightsReport {
    pub maps_read: usize,
    /// Lights an entity owns or its archetype holds.
    pub placed: usize,
    /// Lights of the maps' level-of-detail light lists.
    pub lod: usize,
    pub distant: usize,
    /// Reflection probes read, which nothing draws.
    pub probes: usize,
    /// Lights handed to the frame last.
    pub drawn: usize,
}

/// How far from the camera a level-of-detail light of each size class is drawn, and the last stretch it fades out over, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodRanges {
    pub range: [f32; 3],
    pub fadeout: [f32; 3],
}

impl Default for LodRanges {
    /// Every light at every distance, where no visual settings are read.
    fn default() -> Self {
        Self {
            range: [f32::INFINITY; 3],
            fadeout: [0.0; 3],
        }
    }
}

impl LodRanges {
    /// The ranges the visual settings state under `lodlight.small`, `.medium` and `.large`.
    pub fn of(settings: &VisualSettings) -> Self {
        let mut ranges = Self::default();
        for (k, class) in ["small", "medium", "large"].into_iter().enumerate() {
            ranges.range[k] = settings.get_f32(&format!("lodlight.{class}.range"), f32::INFINITY);
            ranges.fadeout[k] = settings.get_f32(&format!("lodlight.{class}.fadeout"), 0.0);
        }
        ranges
    }

    /// How much of a light of size class `category` is drawn `away` metres from the camera.
    fn strength(&self, category: u16, away: f32) -> f32 {
        let k = usize::from(category).min(2);
        let (range, fade) = (self.range[k], self.fadeout[k]);
        match fade > 0.0 {
            true => ((range - away) / fade).clamp(0.0, 1.0),
            false => f32::from(u8::from(away <= range)),
        }
    }
}

/// How much of a light whose hours are `hours` (bit `h` the hour `h`) is lit at `hour`, 0 to 24:
/// it comes on over the last minute of the hour before its first lit hour and goes out over the
/// last minute of its last.
fn hour_strength(hours: u32, hour: f32) -> f32 {
    let hour = hour.rem_euclid(24.0);
    let now = (hour as u32).min(23);
    let lit = |h: u32| f32::from(u8::from(hours >> (h % 24) & 1 == 1));
    let turning = ((hour - now as f32) * 60.0 - 59.0).clamp(0.0, 1.0);
    lit(now) + (lit(now + 1) - lit(now)) * turning
}

/// The angle a level-of-detail light's cone byte states, in degrees.
fn cone_degrees(byte: u8) -> f32 {
    f32::from(byte) * 180.0 / 255.0
}

/// The length a level-of-detail capsule's outer cone byte states, in metres.
fn capsule_length(byte: u8) -> f32 {
    f32::from(byte) * 140.0 / 255.0
}

/// The lights of one map.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct MapLights {
    /// The map its level-of-detail lights take their places from.
    parent: u32,
    /// In the world.
    placed: Vec<Light>,
    lod: MapLod,
}

/// The lights of the map body `model` holds, its archetypes' by `index`; `None` for any other body.
fn lights_of(model: &RbmModel, index: &WorldIndex) -> Option<MapLights> {
    let MapBody::Data(data) = map::read(model).ok()? else {
        return None;
    };
    let mut out = MapLights {
        parent: data.parent,
        ..MapLights::default()
    };
    match effects::read(model) {
        Ok(read) => {
            for (owner, effect) in &read.effects {
                let LightOwner::Entity(at) = owner else {
                    continue;
                };
                let Some(entity) = data.entities.get(*at) else {
                    continue;
                };
                let pose = Pose::of(entity);
                (out.placed).extend(light_ext::lights_of(effect).map(|l| l.placed(&pose)));
            }
        }
        Err(error) => {
            tracing::debug!(map = data.name, %error, "a map's own lights are not read")
        }
    }
    for entity in &data.entities {
        let rows = index.archetype_lights(entity.archetype_word);
        if rows.is_empty() {
            continue;
        }
        let pose = Pose::of(entity);
        (out.placed).extend(rows.iter().filter_map(|r| Some(r.light()?.placed(&pose))));
    }
    match lights_lod::read(model) {
        Ok(lod) => out.lod = lod,
        Err(error) => tracing::debug!(map = data.name, %error, "a map's light lists are not read"),
    }
    Some(out)
}

/// The cell of the grid a light's source is looked for in.
fn cell(p: Vec3) -> [i32; 3] {
    (p / SOURCE).floor().as_ivec3().to_array()
}

/// The level-of-detail lights of `map`, which stand at its parent's distant lights, with how much of each `ranges` draws from `eye`.
fn lod_lights(map: &MapLights, parent: &MapLights, eye: Vec3, ranges: &LodRanges) -> Vec<(Light, f32)> {
    let (lod, places) = (&map.lod, &parent.lod.distant);
    if places.len() != lod.falloff.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, place) in places.iter().enumerate() {
        let Some(kind) = LightKind::of(place.light_type) else {
            continue;
        };
        let strength = ranges.strength(parent.lod.category, Vec3::from(place.position).distance(eye));
        if strength <= 0.0 {
            continue;
        }
        let light = Light {
            kind,
            position: place.position,
            direction: lod.direction[i],
            colour: place.rgb.map(|c| f32::from(c) / 255.0),
            intensity: place.intensity,
            radius: lod.falloff[i],
            falloff_exponent: FALLOFF,
            // ⚠ The lists state no hours: a level-of-detail light is lit at every hour.
            hours: ALL_HOURS,
            flags: 0,
            cone_inner: cone_degrees(lod.cone_inner[i]),
            cone_outer: cone_degrees(lod.cone_outer[i]),
            length: capsule_length(lod.cone_outer[i]),
        };
        out.push((light, strength));
    }
    out
}

/// The lights of `loaded` at `hour`, each `scale` times as strong as it states, the `most` that light `eye`'s surroundings most, as the frame's point and spot lights.
fn collect(
    loaded: &HashMap<u32, MapLights>,
    eye: Vec3,
    hour: f32,
    scale: f32,
    ranges: &LodRanges,
    most: usize,
) -> (Vec<PointLight>, Vec<SpotLight>) {
    let mut sources: HashSet<[i32; 3]> = HashSet::new();
    let mut lights: Vec<(Light, f32)> = Vec::new();
    for map in loaded.values() {
        for light in &map.placed {
            sources.insert(cell(Vec3::from(light.position)));
            let lit = hour_strength(light.hours, hour);
            if lit > 0.0 {
                lights.push((*light, lit));
            }
        }
    }
    let has_source = |p: Vec3| {
        let [x, y, z] = cell(p);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| (-1..=1).any(|dz| sources.contains(&[x + dx, y + dy, z + dz])))
        })
    };
    for map in loaded.values() {
        let Some(parent) = loaded.get(&map.parent) else {
            continue;
        };
        let copies = lod_lights(map, parent, eye, ranges).into_iter();
        lights.extend(copies.filter(|(l, _)| !has_source(Vec3::from(l.position))));
    }
    // ponytail: a light ranks by how bright it is where it reaches nearest the camera; rank per tile if a street needs more than `most`.
    let rank = |(l, s): &(Light, f32)| {
        let away = (Vec3::from(l.position).distance(eye) - l.radius).max(1.0);
        l.intensity * s / (away * away)
    };
    lights.retain(|(l, s)| l.intensity > 0.0 && l.radius > 0.0 && *s > 0.0);
    lights.sort_by(|a, b| rank(b).total_cmp(&rank(a)));
    lights.truncate(most);
    let (mut points, mut spots) = (Vec::new(), Vec::new());
    for (l, s) in lights {
        let (position, colour) = (Vec3::from(l.position), Vec3::from(l.colour));
        let intensity = l.intensity * s * scale;
        let point = |radius: f32| PointLight {
            position,
            colour,
            intensity,
            radius,
            falloff: FALLOFF,
        };
        match l.kind {
            LightKind::Spot => spots.push(SpotLight {
                position,
                direction: Vec3::from(l.direction),
                colour,
                intensity,
                radius: l.radius,
                falloff: FALLOFF,
                outer_degrees: l.cone_outer,
            }),
            // ponytail: a capsule shines from its middle as far as its ends reach, and a quad as a point, until the frame has their passes.
            LightKind::Capsule => points.push(point(l.radius + l.length / 2.0)),
            LightKind::Point | LightKind::Area => points.push(point(l.radius)),
        }
    }
    (points, spots)
}

/// The lights of the maps around the camera, read on workers.
pub struct Lights {
    index: Arc<WorldIndex>,
    readers: Pool<u32, Option<MapLights>>,
    loaded: HashMap<u32, MapLights>,
    /// The maps the camera wants the lights of, by name, with their distance.
    wanted: HashMap<u32, f32>,
    looked_at: Option<Vec3>,
    drawn: usize,
    /// What of the world is switched on, as the streamer is told it.
    enabled: Enabled,
    ranges: LodRanges,
    /// What a stated intensity is multiplied by for the frame: the frame's exposure of a light.
    // ponytail: a stand-in until the day gives its exposure, which the viewer then sets here.
    pub intensity_scale: f32,
}

impl Lights {
    pub fn new(index: Arc<WorldIndex>) -> Self {
        let shared = Arc::clone(&index);
        let readers = Pool::new(
            2,
            HashMap::<u32, File>::new,
            move |files, name: u32| -> Option<MapLights> {
                let row = shared.maps.get(shared.map_named(name)?)?;
                let file = match files.entry(row.archive) {
                    std::collections::hash_map::Entry::Occupied(open) => open.into_mut(),
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        let path = shared.archives.get(row.archive as usize)?;
                        slot.insert(File::open(path).ok()?)
                    }
                };
                let (find, bytes) = read_rpf9_body(&*file, row.offset, BODY_LIMIT).ok()?;
                let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
                    resource: Some(resource),
                    ..
                }) = find.kind
                else {
                    return None;
                };
                let (virtual_section, physical) = resource.sections(&bytes);
                let model =
                    or_io::meta::binary::rbm::gta6::read_model(virtual_section, physical).ok()?;
                lights_of(&model, &shared)
            },
        );
        Self {
            index,
            readers,
            loaded: HashMap::new(),
            wanted: HashMap::new(),
            looked_at: None,
            drawn: 0,
            enabled: Enabled::default(),
            ranges: LodRanges::default(),
            intensity_scale: 0.1,
        }
    }

    /// Say what of the world is switched on: the maps it holds back give no light.
    pub fn set_enabled(&mut self, enabled: &Enabled) {
        if self.enabled != *enabled {
            self.enabled = enabled.clone();
            // The wanted maps are worked out again at the next look.
            self.looked_at = None;
        }
    }

    /// Draw the level-of-detail lights to the ranges the install's visual settings state.
    pub fn set_lod_ranges(&mut self, settings: &VisualSettings) {
        self.ranges = LodRanges::of(settings);
    }

    /// Ask for the lights of the maps within reach of `eye` and take in those read since; whether any came or went.
    pub fn look(&mut self, eye: Vec3) -> bool {
        let mut changed = false;
        for (name, read) in self.readers.take(|| true) {
            // A map that did not read holds no light, and is not asked for again while it is wanted.
            let read = read.unwrap_or_default();
            if self.wanted.contains_key(&name) {
                self.loaded.insert(name, read);
                changed = true;
            }
        }
        if changed || self.looked_at.is_none_or(|at| at.distance(eye) >= RELOOK) {
            self.looked_at = Some(eye);
            self.rewant(eye);
            let before = self.loaded.len();
            self.loaded.retain(|name, _| self.wanted.contains_key(name));
            changed |= self.loaded.len() != before;
            let asked = (self.wanted.iter())
                .filter(|(name, _)| !self.loaded.contains_key(name))
                .map(|(name, away)| (*away, *name))
                .collect();
            self.readers.want(asked);
        }
        changed
    }

    // ponytail: every map's box is tested each time the camera has moved, as the streamer does.
    fn rewant(&mut self, eye: Vec3) {
        self.wanted.clear();
        for row in (self.index.maps.iter()).filter(|m| self.enabled.allows(m)) {
            let boxes = row.boxes();
            let (min, max) = boxes.entities;
            let away = eye.distance(eye.clamp(min, max));
            let lists = row.content_flags & (LOD_LIGHTS | DISTANT_LIGHTS) != 0 && boxes.wants(eye);
            let near = row.content_flags & HIGH_DETAIL != 0 && away <= PLACED_REACH;
            if lists || near {
                self.wanted.insert(row.name, away);
            }
        }
        // The map a loaded list takes its places from is wanted with it.
        let parents: Vec<(u32, f32)> = (self.loaded.iter())
            .filter(|(name, map)| map.parent != 0 && self.wanted.contains_key(name))
            .filter(|(_, map)| self.index.map_named(map.parent).is_some())
            .map(|(name, map)| (map.parent, self.wanted[name]))
            .collect();
        for (parent, away) in parents {
            self.wanted.entry(parent).or_insert(away);
        }
    }

    /// The lights at `hour` (0 to 24) that light `eye`'s surroundings most, at most `most`, as the frame's point and spot lights.
    pub fn lights(&mut self, eye: Vec3, hour: f32, most: usize) -> (Vec<PointLight>, Vec<SpotLight>) {
        let scale = self.intensity_scale;
        let lights = collect(&self.loaded, eye, hour, scale, &self.ranges, most);
        self.drawn = lights.0.len() + lights.1.len();
        lights
    }

    /// The maps whose lights are wanted and not read yet.
    pub fn waiting(&self) -> usize {
        (self.wanted.keys())
            .filter(|name| !self.loaded.contains_key(name))
            .count()
    }

    pub fn report(&self) -> LightsReport {
        let sum = |of: fn(&MapLights) -> usize| -> usize { self.loaded.values().map(of).sum() };
        LightsReport {
            maps_read: self.loaded.len(),
            placed: sum(|m| m.placed.len()),
            lod: sum(|m| m.lod.falloff.len()),
            distant: sum(|m| m.lod.distant.len()),
            probes: sum(|m| m.lod.probes),
            drawn: self.drawn,
        }
    }
}

#[cfg(test)]
#[path = "lights_tests.rs"]
mod tests;

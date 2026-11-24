//! The lights of the maps around the camera, read on workers: the lights an entity owns, those its
//! archetype holds, and the level-of-detail lights of the maps that list them, as the point and
//! spot lights the frame draws.
//!
//! ⚠ A level-of-detail light is a copy of a placed light: where the placed one is read, it is drawn
//! and the copy is not.

use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;

use glam::Vec3;
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::map::light_ext;
use or_io::gta6::map::lights::{self as effects, LightOwner, Pose};
use or_io::gta6::map::{self, MapBody};
use or_io::meta::binary::rbm::schema::RbmModel;
use or_renderer::{PointLight, SpotLight};

use super::index::{Enabled, WorldIndex};
use super::lights_frame::{self, Lamp, LodReach};
use super::lights_lod::{self, MapLod};
use super::pool::Pool;

pub use super::lights_frame::MOST;

/// The bytes a map body is read to.
const BODY_LIMIT: u64 = 256 << 20;
/// How far the camera moves before the wanted maps are worked out again, in metres.
const RELOOK: f32 = 8.0;
/// How far from the camera a map's entities may lie for their own lights to be read, in metres.
const PLACED_REACH: f32 = 150.0;
/// The bits of a map's content flags that say it lists level-of-detail lights, and distant ones.
const LOD_LIGHTS: u32 = 1 << 7;
const DISTANT_LIGHTS: u32 = 1 << 8;
const HIGH_DETAIL: u32 = 1;

/// What was read of the lights around the camera, for a panel to say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightsReport {
    pub maps_read: usize,
    /// Lights an entity owns or its archetype holds, in the world.
    pub placed: usize,
    /// Lights of the maps' level-of-detail light lists.
    pub lod: usize,
    pub distant: usize,
    /// Reflection probes read, which nothing draws.
    pub probes: usize,
    /// Lights handed to the frame last.
    pub drawn: usize,
}

/// The lights of one map.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct MapLights {
    /// The map its level-of-detail lights take their places from.
    parent: u32,
    /// The lights its entities own and their archetypes hold, in the world.
    placed: Vec<Lamp>,
    lod: MapLod,
}

/// The lights of the map body `model` holds, its archetypes' lights found in `index`; `None` for any other body.
fn lights_of(model: &RbmModel, index: &WorldIndex) -> Option<MapLights> {
    let MapBody::Data(data) = map::read(model).ok()? else {
        return None;
    };
    let mut out = MapLights {
        parent: data.parent,
        ..MapLights::default()
    };
    let lamp = |light| Lamp { light, reach: None };
    for entity in &data.entities {
        let held = index.archetype_lights(entity.archetype_word);
        if held.is_empty() {
            continue;
        }
        let pose = Pose::of(entity);
        (out.placed).extend(
            held.iter()
                .filter_map(|row| Some(lamp(row.light()?.placed(&pose)))),
        );
    }
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
                (out.placed).extend(light_ext::lights_of(effect).map(|l| lamp(l.placed(&pose))));
            }
        }
        Err(error) => {
            tracing::debug!(map = data.name, %error, "a map's own lights are not read")
        }
    }
    match lights_lod::read(model) {
        Ok(lod) => out.lod = lod,
        Err(error) => tracing::debug!(map = data.name, %error, "a map's light lists are not read"),
    }
    Some(out)
}

/// Every lamp of `loaded`: the placed lights, and the level-of-detail lights of each map whose parent is loaded but for those a placed light stands in.
fn lamps_of(loaded: &HashMap<u32, MapLights>, reach: &LodReach) -> Vec<Lamp> {
    let placed: Vec<Lamp> = (loaded.values())
        .flat_map(|m| m.placed.iter().copied())
        .collect();
    let lod: Vec<Lamp> = (loaded.values())
        .filter_map(|m| {
            Some(lights_frame::lod_lamps(
                &m.lod,
                &loaded.get(&m.parent)?.lod,
                reach,
            ))
        })
        .flatten()
        .collect();
    let mut lamps = lights_frame::without_placed(lod, &placed);
    lamps.extend(placed);
    lamps
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
    reach: LodReach,
    /// Every lamp of the maps loaded, worked out again when they or the reach change.
    lamps: Vec<Lamp>,
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
            reach: LodReach::default(),
            lamps: Vec::new(),
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

    /// Say how far the level-of-detail lights of each size class are drawn.
    pub fn set_reach(&mut self, reach: LodReach) {
        if self.reach != reach {
            self.reach = reach;
            self.lamps = lamps_of(&self.loaded, &self.reach);
        }
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
        if changed {
            self.lamps = lamps_of(&self.loaded, &self.reach);
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

    /// The lights lit at `hour` (0 to 24) brightest at `eye`, at most `most`, their stated intensity times `scale`, as the frame's point and spot lights.
    pub fn lights(
        &mut self,
        eye: Vec3,
        hour: f32,
        scale: f32,
        most: usize,
    ) -> (Vec<PointLight>, Vec<SpotLight>) {
        let (points, spots) = lights_frame::frame_lights(&self.lamps, eye, hour, scale, most);
        self.drawn = points.len() + spots.len();
        (points, spots)
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

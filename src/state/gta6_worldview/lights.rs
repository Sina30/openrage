//! The lights of the maps around the camera, read on workers: the lights an entity owns, those its
//! archetype holds, and the level-of-detail lights of the maps that list them, as the lights the
//! frame draws.
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

use super::index::{Enabled, WorldIndex};
use super::interior::{self, RoomsSeen};
use super::lights_distant::{self, DistantReach};
use super::lights_frame::{self, FrameLights, Lamp, LodList, LodReach, Placed};
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
/// What [`super::index::MapRow::held`] says of a map that places nothing.
const HELD_EMPTY: u32 = 3;

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
    /// The lights of the contents of the interiors it places, each with its interior's place among
    /// its entities and its room.
    inside: Vec<(Lamp, [u32; 2])>,
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

/// Make `lists` the level-of-detail lamps of each map of `loaded` whose parent is loaded too, making again only those of a map or parent that came or went.
fn relist(
    loaded: &HashMap<u32, MapLights>,
    reach: &LodReach,
    lists: &mut HashMap<u32, Option<LodList>>,
) {
    lists.retain(|name, list| {
        (loaded.get(name)).is_some_and(|m| list.is_some() == loaded.contains_key(&m.parent))
    });
    for (name, m) in loaded {
        lists.entry(*name).or_insert_with(|| {
            let parent = &loaded.get(&m.parent)?.lod;
            Some(LodList::of(lights_frame::lod_lamps(&m.lod, parent, reach)))
        });
    }
}

/// The placed lights of `loaded`, a map after another.
fn placed_of(loaded: &HashMap<u32, MapLights>) -> Vec<Lamp> {
    (loaded.values())
        .flat_map(|m| m.placed.iter().copied())
        .collect()
}

/// The lights of the interiors' contents of `loaded`, each with its map, its interior's place and its room.
fn inside_of(loaded: &HashMap<u32, MapLights>) -> Vec<(Lamp, [u32; 3])> {
    (loaded.iter())
        .flat_map(|(name, m)| {
            (m.inside.iter()).map(|(lamp, [at, room])| (*lamp, [*name, *at, *room]))
        })
        .collect()
}

/// A word that changes with what `rooms` says is seen, whatever order it holds it in.
fn digest(rooms: &RoomsSeen) -> u32 {
    use std::hash::{Hash, Hasher};
    (rooms.iter()).fold(0u64, |sum, entry| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entry.hash(&mut hasher);
        sum.wrapping_add(hasher.finish())
    }) as u32
}

/// The lamps the frame may draw seen from `eye`: the level-of-detail lights of each map in `lists` within reach, but for those a placed light stands in, then the placed lights.
fn lamps_seen<'a>(
    loaded: &'a HashMap<u32, MapLights>,
    lists: &'a HashMap<u32, Option<LodList>>,
    (placed, near): (&'a [Lamp], &'a Placed),
    eye: Vec3,
) -> impl Iterator<Item = &'a Lamp> {
    let reached = (loaded.keys())
        .filter_map(|name| lists.get(name)?.as_ref())
        .filter(move |list| list.reaches(eye));
    (reached.flat_map(|list| &list.lamps))
        .filter(|lamp| !near.stands_at(lamp.light.position))
        .chain(placed)
}

/// Every lamp of `loaded` from anywhere, as [`lamps_seen`] gives them.
#[cfg(test)]
fn lamps_of(
    loaded: &HashMap<u32, MapLights>,
    reach: &LodReach,
    lists: &mut HashMap<u32, Option<LodList>>,
) -> Vec<Lamp> {
    relist(loaded, reach, lists);
    let placed = placed_of(loaded);
    let near = Placed::of(&placed);
    (loaded.keys())
        .filter_map(|name| lists.get(name)?.as_ref())
        .flat_map(|list| list.lamps.iter().copied())
        .filter(|lamp| !near.stands_at(lamp.light.position))
        .chain(placed.iter().copied())
        .collect()
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
    distant_reach: DistantReach,
    /// Each loaded map's level-of-detail lamps, None while its parent is not loaded.
    lists: HashMap<u32, Option<LodList>>,
    /// The placed lights of the maps loaded, and where they stand, the interiors' among them.
    placed: Vec<Lamp>,
    near: Placed,
    /// The lights of the interiors' contents of the maps loaded, each with its map, interior and room.
    inside: Vec<(Lamp, [u32; 3])>,
    /// The frame's lights as last handed out, with the camera, hour, scale, most and rooms seen they were for; none once the lamps change.
    framed: Option<([u32; 7], FrameLights)>,
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
                let mut read = lights_of(&model, &shared)?;
                // The lights of an interior's contents stand in the map that places it.
                let inside = super::interior::lights_placed(&shared, files, &model);
                read.inside = (inside.into_iter())
                    .map(|(light, at)| (Lamp { light, reach: None }, at))
                    .collect();
                Some(read)
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
            distant_reach: DistantReach::default(),
            lists: HashMap::new(),
            placed: Vec::new(),
            near: Placed::default(),
            inside: Vec::new(),
            framed: None,
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
            self.lists.clear();
            relist(&self.loaded, &self.reach, &mut self.lists);
            self.framed = None;
        }
    }

    /// Say how far the distant lights of each size class are drawn as sprites, and how large.
    pub fn set_distant_reach(&mut self, reach: DistantReach) {
        if self.distant_reach != reach {
            self.distant_reach = reach;
            self.framed = None;
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
            self.relamp();
        }
        changed
    }

    /// Make the lamps of the maps loaded again.
    fn relamp(&mut self) {
        relist(&self.loaded, &self.reach, &mut self.lists);
        self.placed = placed_of(&self.loaded);
        self.inside = inside_of(&self.loaded);
        // A level-of-detail copy of an interior's lamp stays out whether its room is seen or not.
        let all: Vec<Lamp> = (self.placed.iter().copied())
            .chain(self.inside.iter().map(|(lamp, _)| *lamp))
            .collect();
        self.near = Placed::of(&all);
        self.framed = None;
    }

    // ponytail: every map's box is tested each time the camera has moved, as the streamer does.
    fn rewant(&mut self, eye: Vec3) {
        self.wanted.clear();
        let modes = u32::from(self.enabled.modes);
        for row in self.index.maps.iter() {
            let lists = row.content_flags & (LOD_LIGHTS | DISTANT_LIGHTS) != 0;
            // A map of light lists places no entity, which the index counts as empty: its lights stream all the same.
            let empty_list = lists && row.held == HELD_EMPTY && row.modes & modes != 0;
            if !(self.enabled.allows(row) || empty_list) {
                continue;
            }
            let boxes = row.boxes();
            let (min, max) = boxes.entities;
            let away = eye.distance(eye.clamp(min, max));
            let lists = lists && boxes.wants(eye);
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

    /// The lights lit at `hour` (0 to 24) nearest `eye`, at most `most`, their stated intensity times `scale`, as the volumes the frame draws them with, and the distant lights as sprites; an interior's lamps those of the rooms of `rooms` the camera sees.
    pub(super) fn lights(
        &mut self,
        (eye, hour, scale, most): (Vec3, f32, f32, usize),
        rooms: &RoomsSeen,
    ) -> FrameLights {
        let key = [eye.x, eye.y, eye.z, hour, scale].map(f32::to_bits);
        let key = [
            key[0],
            key[1],
            key[2],
            key[3],
            key[4],
            most as u32,
            digest(rooms),
        ];
        // ⚠ Worked out over every lamp of the maps loaded, which are many: only when what they are for changed.
        if let Some((_, out)) = self.framed.as_ref().filter(|(was, _)| *was == key) {
            return out.clone();
        }
        let near = (self.placed.as_slice(), &self.near);
        let inside = (self.inside.iter())
            .filter(|(_, [map, at, room])| interior::lit(rooms, (*map, *at), *room))
            .map(|(lamp, _)| lamp);
        let lamps = lamps_seen(&self.loaded, &self.lists, near, eye).chain(inside);
        let mut out = lights_frame::frame_lights(lamps, eye, hour, scale, most);
        self.drawn = out.lights;
        let lists = (self.loaded.values()).map(|m| (m.lod.distant.as_slice(), m.lod.category));
        let kept = lights_frame::Places::of(out.kept.iter().copied());
        out.sprites = lights_distant::sprites(lists, eye, scale, &self.distant_reach, &kept);
        out.sprite_size = self.distant_reach.size;
        self.framed = Some((key, out.clone()));
        out
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
            placed: sum(|m| m.placed.len() + m.inside.len()),
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

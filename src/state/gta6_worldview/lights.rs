//! The lights of the maps around the camera, read on workers: the lights an entity places, and
//! the level-of-detail lights of the maps that list them, as the point lights the frame draws.
//!
//! ⚠ A light an archetype holds stands once at each entity that places it, and no index says
//! which body declares an archetype: those lights are not read here.

use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;

use glam::Vec3;
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::map::lights::{self as placed, LightOwner};
use or_io::gta6::map::{self, MapBody};
use or_io::meta::binary::rbm::schema::RbmModel;
use or_renderer::PointLight;

use super::index::WorldIndex;
use super::lights_lod::{self, MapLod};
use super::pool::Pool;

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
    /// Lights an entity places.
    pub placed: usize,
    /// Lights of the maps' level-of-detail light lists.
    pub lod: usize,
    pub distant: usize,
    /// Reflection probes read, which nothing draws.
    pub probes: usize,
    /// Lights handed to the frame last.
    pub drawn: usize,
}

/// A light an entity places, in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    position: Vec3,
    colour: Vec3,
    intensity: f32,
    radius: f32,
    falloff: f32,
    /// One bit an hour of the day.
    time_flags: u32,
}

/// The lights of one map.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct MapLights {
    /// The map its level-of-detail lights take their places from.
    parent: u32,
    placed: Vec<Placed>,
    lod: MapLod,
}

/// The lights of the map body `model` holds; `None` for any other body.
fn lights_of(model: &RbmModel) -> Option<MapLights> {
    let MapBody::Data(data) = map::read(model).ok()? else {
        return None;
    };
    let mut out = MapLights {
        parent: data.parent,
        ..MapLights::default()
    };
    match placed::read(model) {
        Ok(read) => {
            for (owner, effect) in &read.effects {
                let LightOwner::Entity(at) = owner else {
                    continue;
                };
                let Some(entity) = data.entities.get(*at) else {
                    continue;
                };
                // ponytail: a spot and a capsule shine all round here; their own passes once the frame takes them.
                out.placed.extend(effect.lights().map(|light| Placed {
                    position: Vec3::from(placed::place(effect, light, entity).position),
                    colour: Vec3::from(light.colour.map(f32::from)) / 255.0,
                    intensity: light.intensity,
                    radius: light.falloff,
                    falloff: light.falloff_exponent,
                    time_flags: light.time_flags,
                }));
            }
        }
        Err(error) => {
            tracing::debug!(map = data.name, %error, "a map's placed lights are not read")
        }
    }
    match lights_lod::read(model) {
        Ok(lod) => out.lod = lod,
        Err(error) => tracing::debug!(map = data.name, %error, "a map's light lists are not read"),
    }
    Some(out)
}

/// The lights of `loaded` lit at `hour`, the level-of-detail ones `night` strong, nearest `eye` first and at most `most`.
fn collect(
    loaded: &HashMap<u32, MapLights>,
    eye: Vec3,
    hour: f32,
    night: f32,
    scale: f32,
    most: usize,
) -> Vec<PointLight> {
    let bit = (hour.rem_euclid(24.0) as u32).min(23);
    let mut lights: Vec<PointLight> = Vec::new();
    for map in loaded.values() {
        let lit = map.placed.iter().filter(|p| p.time_flags >> bit & 1 == 1);
        lights.extend(lit.map(|p| PointLight {
            position: p.position,
            colour: p.colour,
            intensity: p.intensity * scale,
            radius: p.radius,
            falloff: p.falloff,
        }));
        // ⚠ The hours a level-of-detail light is lit in are not found in its lists: they are lit as the night emissives are.
        let places = loaded.get(&map.parent).map(|parent| &parent.lod.distant);
        let Some(places) = places.filter(|p| night > 0.0 && p.len() == map.lod.falloff.len())
        else {
            continue;
        };
        let reach = map.lod.falloff.iter().zip(&map.lod.falloff_exponent);
        lights.extend(
            places
                .iter()
                .zip(reach)
                .map(|(light, (radius, falloff))| PointLight {
                    position: Vec3::from(light.position),
                    colour: Vec3::from(light.rgb.map(f32::from)) / 255.0,
                    intensity: light.intensity * scale * night,
                    radius: *radius,
                    falloff: *falloff,
                }),
        );
    }
    lights.retain(|l| l.intensity > 0.0 && l.radius > 0.0);
    let away = |l: &PointLight| l.position.distance(eye) - l.radius;
    lights.sort_by(|a, b| away(a).total_cmp(&away(b)));
    lights.truncate(most);
    lights
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
    /// What a stated intensity is multiplied by for the frame.
    // ponytail: GTA VI states a light in a photometric unit and exposes its frame for it; the frame has no such exposure, so one scale stands for all until the game's is read.
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
                lights_of(&model)
            },
        );
        Self {
            index,
            readers,
            loaded: HashMap::new(),
            wanted: HashMap::new(),
            looked_at: None,
            drawn: 0,
            intensity_scale: 0.1,
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
        changed
    }

    // ponytail: every map's box is tested each time the camera has moved, as the streamer does.
    fn rewant(&mut self, eye: Vec3) {
        self.wanted.clear();
        for row in self.index.maps.iter().filter(|m| m.held == 0) {
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

    /// The lights lit at `hour` (0 to 24) nearest `eye`, at most `most`, as the frame's point lights; `night` (0 to 1) is how far the night emissives are on.
    pub fn point_lights(
        &mut self,
        eye: Vec3,
        hour: f32,
        night: f32,
        most: usize,
    ) -> Vec<PointLight> {
        let lights = collect(&self.loaded, eye, hour, night, self.intensity_scale, most);
        self.drawn = lights.len();
        lights
    }

    pub fn report(&self) -> LightsReport {
        let sum = |of: fn(&MapLights) -> usize| -> usize { self.loaded.values().map(of).sum() };
        LightsReport {
            maps_read: self.loaded.len(),
            placed: sum(|m| m.placed.len()),
            lod: sum(|m| m.lod.falloff.len()),
            distant: sum(|m| m.lod.distant.len()),
            probes: sum(|m| m.lod.probes.len()),
            drawn: self.drawn,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lights_lod::Distant;

    fn placed(x: f32, time_flags: u32) -> Placed {
        Placed {
            position: Vec3::new(x, 0.0, 0.0),
            colour: Vec3::ONE,
            intensity: 10.0,
            radius: 5.0,
            falloff: 1.0,
            time_flags,
        }
    }

    fn lod_map(parent: u32, falloff: &[f32]) -> MapLights {
        MapLights {
            parent,
            placed: Vec::new(),
            lod: MapLod {
                falloff: falloff.to_vec(),
                falloff_exponent: vec![2.0; falloff.len()],
                ..MapLod::default()
            },
        }
    }

    fn distant_map(xs: &[f32]) -> MapLights {
        let light = |x: &f32| Distant {
            position: [*x, 0.0, 0.0],
            rgb: [255, 0, 51],
            intensity: 80.0,
            light_type: 1,
        };
        MapLights {
            lod: MapLod {
                distant: xs.iter().map(light).collect(),
                ..MapLod::default()
            },
            ..MapLights::default()
        }
    }

    #[test]
    fn a_placed_light_is_drawn_in_the_hours_its_flags_set_and_the_nearest_come_first() {
        let night_only = 1 << 23 | 1 << 0;
        let map = MapLights {
            placed: vec![
                placed(50.0, u32::MAX >> 8),
                placed(10.0, night_only),
                placed(20.0, u32::MAX >> 8),
            ],
            ..MapLights::default()
        };
        let loaded = HashMap::from([(1, map)]);
        let noon = collect(&loaded, Vec3::ZERO, 12.5, 0.0, 0.5, 8);
        assert_eq!(
            noon.iter().map(|l| l.position.x).collect::<Vec<_>>(),
            [20.0, 50.0]
        );
        assert_eq!(
            (noon[0].intensity, noon[0].radius, noon[0].falloff),
            (5.0, 5.0, 1.0)
        );
        let late = collect(&loaded, Vec3::ZERO, 23.9, 0.0, 0.5, 8);
        assert_eq!(
            late.iter().map(|l| l.position.x).collect::<Vec<_>>(),
            [10.0, 20.0, 50.0]
        );
        assert_eq!(collect(&loaded, Vec3::ZERO, 23.9, 0.0, 0.5, 2).len(), 2);
    }

    #[test]
    fn a_level_of_detail_light_stands_where_its_parents_distant_light_does_and_is_lit_by_night() {
        let loaded = HashMap::from([
            (7, lod_map(8, &[3.0, 12.0])),
            (8, distant_map(&[100.0, 200.0])),
        ]);
        assert!(
            collect(&loaded, Vec3::ZERO, 12.0, 0.0, 1.0, 8).is_empty(),
            "out by day"
        );
        let lit = collect(&loaded, Vec3::ZERO, 23.0, 0.5, 0.1, 8);
        assert_eq!(lit.len(), 2);
        assert_eq!(
            (lit[0].position.x, lit[0].radius, lit[0].falloff),
            (100.0, 3.0, 2.0)
        );
        assert!((lit[0].colour - Vec3::new(1.0, 0.0, 0.2)).length() < 1e-6);
        assert!((lit[0].intensity - 80.0 * 0.1 * 0.5).abs() < 1e-5);
        assert_eq!(lit[1].radius, 12.0);
    }

    #[test]
    fn lists_that_do_not_pair_light_for_light_draw_nothing() {
        let alone = HashMap::from([(7, lod_map(8, &[3.0]))]);
        assert!(collect(&alone, Vec3::ZERO, 23.0, 1.0, 1.0, 8).is_empty());
        let uneven = HashMap::from([(7, lod_map(8, &[3.0])), (8, distant_map(&[1.0, 2.0]))]);
        assert!(collect(&uneven, Vec3::ZERO, 23.0, 1.0, 1.0, 8).is_empty());
    }

    #[test]
    fn the_lights_of_a_body_that_is_no_map_are_none() {
        let mut model = lights_lod::tests::model(&[], &[]);
        model.data_blocks[0].data_type = 1;
        assert_eq!(lights_of(&model), None);
    }
}

//! The maps streamed in around the camera and what of them draws: each wanted map's body is read
//! on a worker into the nodes of the level-of-detail forest, and a selection gives each entity and
//! each instance that draws with the model that draws it, where it stands and how opaque it is.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use glam::Vec3;
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_world::streaming::{Forest, NodeId, NodeState, View};

use super::index::{Enabled, ModelRef, WorldIndex};
use super::pool::Pool;

#[path = "stream_grass.rs"]
mod grass;
#[path = "stream_read.rs"]
mod read;

pub use read::Rows;
pub(super) use read::{read_map, ReadMap};
use read::{place, stretch, Drawn, Loaded};

/// The bytes a map body is read to.
const BODY_LIMIT: u64 = 256 << 20;
/// How far the camera moves before the wanted maps are worked out again, in metres.
const RESTREAM: f32 = 4.0;
/// The hour the world starts at.
const NOON: f32 = 12.0;

/// One thing a selection draws: its model, where it stands, and its sphere in the world.
#[derive(Clone, Copy)]
pub struct Instance {
    /// Which of the selection's models draws it.
    pub model: u32,
    pub rows: Rows,
    pub centre: Vec3,
    pub radius: f32,
    /// How opaque it is, above 0 and to 1.
    pub fade: f32,
}

/// A run of a selection's instances that stand together, and the sphere that holds them all: a
/// camera that does not see the sphere sees none of them.
#[derive(Clone, Copy)]
pub struct Chunk {
    pub centre: Vec3,
    pub radius: f32,
    pub from: u32,
    pub to: u32,
}

/// How the streaming stands, for the screen to say.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Streaming {
    pub maps_wanted: usize,
    pub maps_loaded: usize,
    /// Map bodies asked for and not read yet.
    pub maps_waiting: usize,
    pub nodes: usize,
    pub drawn: usize,
}

/// What draws for one place of the camera: every instance, in chunks, and the models they draw with.
#[derive(Default)]
pub struct Selected {
    /// Counted up with each selection, so that one already laid out is not laid out again.
    pub version: u64,
    pub models: Vec<ModelRef>,
    pub chunks: Vec<Chunk>,
    pub instances: Vec<Instance>,
    /// Each model waited for, with how far the nearest thing that waits for it is.
    pub wanted: Vec<(f32, ModelRef)>,
    pub streaming: Streaming,
    /// Whether a fade in time is under way: the next selection differs though nothing moves.
    pub fading: bool,
}

/// The models of a selection, each once, in the order they were first drawn with.
#[derive(Default)]
struct Models {
    list: Vec<ModelRef>,
    places: HashMap<ModelRef, u32>,
}

impl Models {
    fn ix(&mut self, model: ModelRef) -> u32 {
        *self.places.entry(model).or_insert_with(|| {
            self.list.push(model);
            self.list.len() as u32 - 1
        })
    }
}

/// The maps around the camera.
pub(super) struct Streamer {
    index: Arc<WorldIndex>,
    forest: Forest,
    loaded: HashMap<u32, Loaded>,
    /// The maps the camera wants, by name, with their distance; their parents among them.
    wanted: HashMap<u32, f32>,
    /// A map that did not read is not asked for again.
    failed: HashSet<u32>,
    streamed_at: Option<Vec3>,
    /// What of the game is switched on: a map it does not allow is not wanted.
    enabled: Enabled,
    /// The hour of the day, 0 to 23: a time archetype draws inside its hours alone.
    hour: u32,
    readers: Pool<u32, Option<ReadMap>>,
    pub(super) streaming: Streaming,
}

/// The sphere that holds the spheres of `instances`.
fn bound(instances: &[Instance]) -> (Vec3, f32) {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for i in instances {
        min = min.min(i.centre - Vec3::splat(i.radius));
        max = max.max(i.centre + Vec3::splat(i.radius));
    }
    ((min + max) / 2.0, (max - min).length() / 2.0)
}

/// End a chunk of the instances of `out` from `from` on, where there are any.
fn close(out: &mut Selected, from: usize) {
    if out.instances.len() > from {
        let (centre, radius) = bound(&out.instances[from..]);
        out.chunks.push(Chunk {
            centre,
            radius,
            from: from as u32,
            to: out.instances.len() as u32,
        });
    }
}

/// The hour of the day `hour` falls in, 0 to 23.
fn hour_of(hour: f32) -> u32 {
    (hour.rem_euclid(24.0) as u32).min(23)
}

impl Streamer {
    pub(super) fn new(index: Arc<WorldIndex>) -> Self {
        let shared = Arc::clone(&index);
        let readers = Pool::new(
            4,
            HashMap::<u32, File>::new,
            move |files, name: u32| -> Option<ReadMap> {
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
                let map = or_io::gta6::map::stream::open(virtual_section, physical).ok()?;
                Some(read_map(&shared, &map))
            },
        );
        Self {
            index,
            forest: Forest::new(),
            loaded: HashMap::new(),
            wanted: HashMap::new(),
            failed: HashSet::new(),
            streamed_at: None,
            enabled: Enabled::default(),
            hour: hour_of(NOON),
            readers,
            streaming: Streaming::default(),
        }
    }

    /// Switch on what `enabled` names in place of what was on; whether that is a change.
    pub(super) fn set_enabled(&mut self, enabled: Enabled) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        // The wanted maps are worked out again though the camera stands.
        self.streamed_at = None;
        true
    }

    /// Set the hour of the day, 0.0 to 24.0; whether a time archetype may come or go by it.
    pub(super) fn set_hour(&mut self, hour: f32) -> bool {
        let hour = hour_of(hour);
        let changed = self.hour != hour;
        self.hour = hour;
        changed
    }

    /// Work out the maps the camera of `view` wants, ask for those not loaded and let go of the
    /// rest; whether a map came or went.
    pub(super) fn stream(&mut self, view: &View) -> bool {
        let mut changed = false;
        for (name, read) in self.readers.take(|| true) {
            match read {
                Some(read) => changed |= self.insert(name, read),
                None => {
                    self.failed.insert(name);
                }
            }
        }
        let moved = self
            .streamed_at
            .is_none_or(|at| at.distance(view.eye) >= RESTREAM);
        if moved {
            self.streamed_at = Some(view.eye);
            self.rewant(view);
        }
        // What is loaded and no longer wanted goes.
        let gone: Vec<u32> = (self.loaded.keys().copied())
            .filter(|name| !self.wanted.contains_key(name))
            .collect();
        for name in gone {
            self.loaded.remove(&name);
            self.forest.remove(u64::from(name));
            changed = true;
        }
        let asked: Vec<(f32, u32)> = (self.wanted.iter())
            .filter(|(name, _)| !self.loaded.contains_key(name) && !self.failed.contains(name))
            .map(|(name, away)| (*away, *name))
            .collect();
        // A map whose child map is wanted and not loaded yet holds its nodes in place.
        let mut held: HashSet<u32> = HashSet::new();
        for (_, name) in &asked {
            if let Some(at) = self.index.map_named(*name) {
                held.extend(self.index.parents_of(&self.index.maps[at]));
            }
        }
        for name in self.loaded.keys() {
            (self.forest).set_complete(u64::from(*name), !held.contains(name));
        }
        self.streaming.maps_wanted = self.wanted.len();
        self.streaming.maps_loaded = self.loaded.len();
        self.streaming.maps_waiting = asked.len();
        self.streaming.nodes = self.forest.len().1;
        self.readers.want(asked);
        changed
    }

    // ponytail: every map's box is tested each time the camera has moved; a grid of the boxes if the walk shows.
    fn rewant(&mut self, view: &View) {
        self.wanted.clear();
        let eye = view.eye;
        let mut parents = Vec::new();
        for map in (self.index.maps.iter()).filter(|m| self.enabled.allows(m)) {
            let boxes = map.boxes();
            if !boxes.wanted(view, map.content_flags) {
                continue;
            }
            let (min, max) = boxes.entities;
            let away = eye.distance(eye.clamp(min, max));
            self.wanted.insert(map.name, away);
            parents.extend(self.index.parents_of(map).iter().map(|p| (*p, away)));
        }
        // A wanted map's parents are wanted with it, and theirs.
        while let Some((name, away)) = parents.pop() {
            let Some(at) = self.index.map_named(name) else {
                continue;
            };
            let map = &self.index.maps[at];
            // ⚠ A parent that is not switched on is no part of the world, but one that holds
            // nothing (`held` 3) is still a link to its own parents.
            let off = !self.enabled.allows(map) && map.held != 3;
            if off || self.wanted.contains_key(&name) {
                continue;
            }
            self.wanted.insert(name, away);
            parents.extend(self.index.parents_of(map).iter().map(|p| (*p, away)));
        }
    }

    /// Take the map `name` in where it is still wanted; whether it was.
    fn insert(&mut self, name: u32, read: ReadMap) -> bool {
        if !self.wanted.contains_key(&name) {
            return false;
        }
        self.forest.insert(u64::from(name), read.nodes);
        self.loaded.insert(name, read.loaded);
        true
    }

    /// What draws for the camera of `view`; `ready` says whether a model is on the device, and
    /// `None` that it never will be.
    pub(super) fn select(
        &mut self,
        view: &View,
        ready: &dyn Fn(ModelRef) -> Option<bool>,
    ) -> Selected {
        let (loaded, hour, eye) = (&self.loaded, self.hour, view.eye);
        let drawn_of = |id: NodeId| {
            let map = loaded.get(&(id.0 as u32))?;
            Some((map, map.drawn.get(id.1 as usize)?))
        };
        // A time archetype outside its hours has no model for the while: it draws nothing, is
        // not waited for and holds nothing back.
        let model_of = |id: NodeId| drawn_of(id).and_then(|(_, drawn)| drawn.model_at(hour));
        let state = |id: NodeId| match model_of(id).map(ready) {
            None | Some(None) => NodeState::Absent,
            Some(Some(true)) => NodeState::Ready,
            Some(Some(false)) => NodeState::Loading,
        };
        let mut selection = self.forest.select(view, &state);
        let mut out = Selected {
            fading: selection.fading,
            ..Selected::default()
        };
        let mut models = Models::default();
        let instance = |model: u32, rows: Rows, drawn: &Drawn, fade: f32| Instance {
            model,
            rows,
            centre: place(&rows, drawn.centre),
            radius: drawn.radius * stretch(&rows),
            fade,
        };
        // The entities of one map stand together: a chunk a map.
        selection.drawn.sort_unstable_by_key(|d| d.node);
        let (mut from, mut of_map) = (0, None);
        for draw in &selection.drawn {
            let id = draw.node;
            if of_map != Some(id.0) {
                close(&mut out, from);
                (from, of_map) = (out.instances.len(), Some(id.0));
            }
            let Some((map, drawn)) = drawn_of(id) else {
                continue;
            };
            let rows = map.entities.get(id.1 as usize);
            if let (Some(model), Some(rows)) = (drawn.model_at(hour), rows) {
                let model = models.ix(model);
                (out.instances).push(instance(model, *rows, drawn, draw.fade));
            }
        }
        close(&mut out, from);
        // And the instances of one list: a chunk a list.
        for draw in &selection.lists {
            let Some((map, drawn)) = drawn_of(draw.node) else {
                continue;
            };
            let list = (draw.node.1 as usize)
                .checked_sub(map.entities.len())
                .and_then(|at| map.lists.get(at));
            let (Some(model), Some(list)) = (drawn.model_at(hour), list) else {
                continue;
            };
            let model = models.ix(model);
            let skipped = &selection.skipped[draw.skipped.clone()];
            let from = out.instances.len();
            // An instance that fades by its own distance fades with its list too.
            list.each(eye, view.scales[list.level as usize], skipped, |rows, fade| {
                (out.instances).push(instance(model, rows, drawn, fade * draw.fade));
            });
            close(&mut out, from);
        }
        // Each model waited for once, by the nearest node that waits for it.
        let mut waited: HashMap<ModelRef, f32> = HashMap::new();
        for id in &selection.wanted {
            let (Some(model), Some(node)) = (model_of(*id), self.forest.node(*id)) else {
                continue;
            };
            let away = eye.distance(node.position);
            let nearest = waited.entry(model).or_insert(away);
            *nearest = nearest.min(away);
        }
        out.wanted = (waited.into_iter().map(|(model, away)| (away, model))).collect();
        out.models = models.list;
        self.streaming.drawn = out.instances.len();
        out.streaming = self.streaming;
        out
    }
}

/// How far the camera moves before what draws is selected again, in metres.
const RESELECT: f32 = 0.5;
/// How long the world's thread waits for a change before it looks for map bodies that arrived.
const TICK: std::time::Duration = std::time::Duration::from_millis(20);
/// The shortest time between two passes a speed is measured over, in seconds.
const SPEED_STEP: f32 = 0.001;

/// What the screen tells the world's thread.
#[derive(Default)]
struct Told {
    eye: Vec3,
    scale: f32,
    hour: f32,
    /// What of the game to switch on, where that was told since the thread last took it.
    enabled: Option<Enabled>,
    /// Models that came to or left the device since the last selection, with whether each is
    /// there now and `None` for one that will never be.
    models: Vec<(ModelRef, Option<bool>)>,
    /// Whether anything was told since the thread last took what was.
    changed: bool,
    stopped: bool,
}

struct Shared {
    told: Mutex<Told>,
    woken: Condvar,
    latest: Mutex<Arc<Selected>>,
}

/// The world as a thread of its own streams and selects it, so that a frame only reads what was
/// selected last and never waits for the selection.
pub(super) struct World {
    shared: Arc<Shared>,
}

impl World {
    pub(super) fn new(index: Arc<WorldIndex>, eye: Vec3, scale: f32) -> Self {
        let shared = Arc::new(Shared {
            told: Mutex::new(Told {
                eye,
                scale,
                hour: NOON,
                ..Told::default()
            }),
            woken: Condvar::new(),
            latest: Mutex::default(),
        });
        let theirs = Arc::clone(&shared);
        std::thread::spawn(move || run(index, &theirs));
        Self { shared }
    }

    /// Tell the thread what `tell` changes, and wake it where that is a change.
    fn tell_of(&self, tell: impl FnOnce(&mut Told) -> bool) {
        let mut told = self.shared.told.lock().unwrap_or_else(|p| p.into_inner());
        let changed = tell(&mut told);
        told.changed |= changed;
        drop(told);
        if changed {
            self.shared.woken.notify_one();
        }
    }

    /// Say where the camera is and the scale of its distances.
    pub(super) fn look(&self, eye: Vec3, scale: f32) {
        self.tell_of(|told| {
            let changed = (told.eye, told.scale) != (eye, scale);
            (told.eye, told.scale) = (eye, scale);
            changed
        });
    }

    /// Say the hour of the day, 0.0 to 24.0: a time archetype draws inside its hours alone.
    pub(super) fn set_hour(&self, hour: f32) {
        self.tell_of(|told| {
            let changed = hour_of(told.hour) != hour_of(hour);
            told.hour = hour;
            changed
        });
    }

    /// Say what of the game is switched on: a map it does not allow is not streamed in.
    pub(super) fn set_enabled(&self, enabled: Enabled) {
        self.tell_of(|told| {
            told.enabled = Some(enabled);
            true
        });
    }

    /// Say which models came to the device, will never come, or left it.
    pub(super) fn tell(&self, models: &mut Vec<(ModelRef, Option<bool>)>) {
        if models.is_empty() {
            return;
        }
        self.tell_of(|told| {
            told.models.append(models);
            true
        });
    }

    /// What was selected last.
    pub(super) fn latest(&self) -> Arc<Selected> {
        Arc::clone(&self.shared.latest.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        (self.shared.told.lock().unwrap_or_else(|p| p.into_inner())).stopped = true;
        self.shared.woken.notify_one();
    }
}

/// The world's thread: stream the maps around the camera, and select again whenever the camera
/// moved, a map came or went, a model did, the hour or what is switched on changed, or a fade
/// in time is under way.
fn run(index: Arc<WorldIndex>, shared: &Shared) {
    let mut streamer = Streamer::new(index);
    let mut ready: HashMap<ModelRef, Option<bool>> = HashMap::new();
    let mut selected_at: Option<(Vec3, f32, Instant)> = None;
    let mut passed_at: Option<(Vec3, Instant)> = None;
    let (mut velocity, mut fading, mut version) = (Vec3::ZERO, false, 0);
    loop {
        let (eye, scale, hour, enabled, models) = {
            let mut told = shared.told.lock().unwrap_or_else(|p| p.into_inner());
            // What was told while the last selection was made is taken at once.
            if !told.changed && !told.stopped {
                (told, _) =
                    (shared.woken.wait_timeout(told, TICK)).unwrap_or_else(|p| p.into_inner());
            }
            if told.stopped {
                return;
            }
            told.changed = false;
            let (enabled, models) = (told.enabled.take(), std::mem::take(&mut told.models));
            (told.eye, told.scale, told.hour, enabled, models)
        };
        let now = Instant::now();
        // The camera's speed is what it moved by since the pass before; two passes too close
        // together to measure keep the speed they had.
        match passed_at {
            Some((at, then)) => {
                let step = now.duration_since(then).as_secs_f32();
                if step >= SPEED_STEP {
                    velocity = (eye - at) / step;
                    passed_at = Some((eye, now));
                }
            }
            None => passed_at = Some((eye, now)),
        }
        let mut stale = !models.is_empty() || fading;
        for (model, state) in models {
            match state {
                // A model that left the device is waited for again where it is wanted.
                Some(false) => ready.remove(&model),
                state => ready.insert(model, state),
            };
        }
        stale |= enabled.is_some_and(|enabled| streamer.set_enabled(enabled));
        stale |= streamer.set_hour(hour);
        let mut view = View::at(eye, scale);
        view.velocity = velocity;
        stale |= streamer.stream(&view);
        stale |= selected_at.is_none_or(|(at, by, _)| at.distance(eye) >= RESELECT || by != scale);
        if stale {
            view.dt = selected_at.map_or(0.0, |(.., then)| now.duration_since(then).as_secs_f32());
            let state = |model: ModelRef| ready.get(&model).copied().unwrap_or(Some(false));
            let mut selected = streamer.select(&view, &state);
            version += 1;
            selected.version = version;
            fading = selected.fading;
            selected_at = Some((eye, scale, now));
            *shared.latest.lock().unwrap_or_else(|p| p.into_inner()) = Arc::new(selected);
        }
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;

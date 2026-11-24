//! The GTA VI world viewer: the index of the install it flies through, built on a worker and kept,
//! the maps streamed in around its camera, and the models read for what they draw.
//!
//! ⚠ What is on the device is the render bridge's: a frame asks here what draws and which models
//! arrived, and says which are on the device.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use glam::Vec3;
use or_job::{Cancel, Progress};
use or_renderer::FlyCamera;

use crate::state::jobs::{JobKind, Outcome, RunningJob, Then};

pub(crate) mod index;
mod interior;
pub(crate) mod lights;
mod lights_frame;
pub(crate) mod lights_index;
mod lights_lod;
mod model;
mod occluders;
mod pool;
pub(crate) mod sky;
pub(crate) mod stream;
mod texture;
pub(crate) mod water;

pub use model::{Bucket, CpuMaterial, CpuModel, Family};
#[cfg(test)]
pub use model::{Constants, Emissive, Layer};
pub use texture::CpuTexture;

use index::{Enabled, ModelRef, WorldIndex};
use pool::Pool;
use stream::{Selected, Streaming, World};
use texture::{OpenPack, Places};

/// How high over the middle of the maps the camera starts, in metres.
const START_HEIGHT: f32 = 150.0;
/// The hour of the day the viewer starts at.
const NOON: f32 = 12.0;

/// How a build of the index ended.
enum Built {
    Index(Box<WorldIndex>, Option<Places>),
    Stopped,
    Failed(String),
}

struct Build {
    cancel: Cancel,
    progress: Progress,
    ended: Arc<Mutex<Option<Built>>>,
}

impl Drop for Build {
    fn drop(&mut self) {
        self.cancel.set();
    }
}

/// What the world viewer holds.
pub struct Viewer {
    /// The install it shows.
    root: Option<PathBuf>,
    pub index: Option<Arc<WorldIndex>>,
    world: Option<World>,
    /// The models on the device or known never to come, which are not asked for again.
    have: HashSet<ModelRef>,
    /// The selection whose waited-for models the readers were last given.
    wanted_of: u64,
    models: Option<Pool<ModelRef, Option<CpuModel>>>,
    /// A texture is asked for by its key and the screen size it is needed at.
    textures: Option<Pool<(u64, u32), Option<CpuTexture>>>,
    /// The bytes of the models and textures the workers read that were not taken yet.
    pub held: Arc<AtomicU64>,
    /// The hour and what is enabled, as the world's thread was last told them.
    sent: Option<(f32, Enabled)>,
    build: Option<Build>,
    /// How the last build ended, for the job list to say once.
    ended: Option<(Outcome, String)>,
    /// Why there is no index, where a build failed.
    pub failed: Option<String>,
    pub camera: FlyCamera,
    /// The scale every level-of-detail distance is multiplied by.
    pub lod_scale: f32,
    /// The hour of the day, from 0 to 24: what is placed for some hours alone draws inside them.
    pub hour: f32,
    /// Which of the game's modes and script-managed groups stream.
    pub enabled: Enabled,
    /// Whether the person chose the script-managed groups, which the hour then no longer sets.
    pub groups_chosen: bool,
    /// The lights of the maps around the camera, once an index is held.
    pub lights: Option<lights::Lights>,
    /// How many models wait to be read.
    pub models_waiting: usize,
    /// How many textures wait to be read.
    pub textures_waiting: usize,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            root: None,
            index: None,
            world: None,
            have: HashSet::new(),
            wanted_of: 0,
            models: None,
            textures: None,
            held: Arc::default(),
            sent: None,
            build: None,
            ended: None,
            failed: None,
            camera: FlyCamera {
                position: Vec3::new(0.0, 0.0, START_HEIGHT),
                yaw: 0.0,
                pitch: -0.35,
                speed: 30.0,
                fov_y: 60f32.to_radians(),
                near: 0.5,
                far: 20_000.0,
            },
            lod_scale: 1.0,
            hour: NOON,
            enabled: Enabled::default(),
            groups_chosen: false,
            lights: None,
            models_waiting: 0,
            textures_waiting: 0,
        }
    }
}

/// The viewer, locked: the screen draws from it and the render bridge asks it what draws.
// ponytail: one viewer for the whole app, of the install it was last opened on.
pub fn viewer() -> MutexGuard<'static, Viewer> {
    static VIEWER: LazyLock<Mutex<Viewer>> = LazyLock::new(Mutex::default);
    VIEWER.lock().unwrap_or_else(|p| p.into_inner())
}

impl Viewer {
    /// Show the install at `root`: the index kept of it where one stands, else one built on a worker.
    pub fn open(&mut self, root: &Path) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        // What is enabled is a choice among one install's groups: another install starts at rest.
        *self = Self {
            root: Some(root.to_path_buf()),
            camera: self.camera.clone(),
            lod_scale: self.lod_scale,
            hour: self.hour,
            ..Self::default()
        };
        self.start(false);
    }

    /// Build the index again on a worker, in place of the one held and of the one kept.
    pub fn rebuild(&mut self) {
        self.start(true);
    }

    /// Take the index kept of the install on a worker, unless `afresh`, else build one there.
    fn start(&mut self, afresh: bool) {
        let Some(root) = self.root.clone() else {
            return;
        };
        (self.index, self.world, self.models, self.failed) = (None, None, None, None);
        self.have.clear();
        (self.textures, self.lights) = (None, None);
        // What the workers held goes with them, and a new world's thread is told everything again.
        (self.held, self.sent) = (Arc::default(), None);
        // ⚠ Where the packs' index is kept is found here, on this thread.
        let places = crate::state::gta6_texture_index::pack_places(&root);
        let build = Build {
            cancel: Cancel::new(),
            progress: Progress::new(),
            ended: Arc::default(),
        };
        let (cancel, progress, ended) = (
            build.cancel.clone(),
            build.progress.clone(),
            Arc::clone(&build.ended),
        );
        std::thread::spawn(move || {
            let kept =
                crate::state::prefs::config_root().map(|config| index::kept_at(&config, &root));
            let held = (kept.as_ref().filter(|_| !afresh)).and_then(|file| WorldIndex::kept(file));
            let places = |cancel: &Cancel| {
                let (packs, keys, formats) = places(cancel, &Progress::new())?;
                Some(Places {
                    packs,
                    keys,
                    formats,
                })
            };
            let built = match held {
                Some(index) => Built::Index(Box::new(index), places(&cancel)),
                None => {
                    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        index::build(&root, &cancel, &progress)
                    }));
                    match built {
                        Ok(Ok(Some(index))) => {
                            if let Some(Err(error)) = kept.map(|file| index.keep(&file)) {
                                tracing::warn!(%error, "the world index was not kept");
                            }
                            Built::Index(Box::new(index), places(&cancel))
                        }
                        Ok(Ok(None)) => Built::Stopped,
                        Ok(Err(why)) => Built::Failed(why),
                        Err(_) => Built::Failed(tr!("It stopped on an internal error").to_string()),
                    }
                }
            };
            *ended.lock().unwrap_or_else(|p| p.into_inner()) = Some(built);
        });
        self.build = Some(build);
    }

    fn take(&mut self, index: WorldIndex, places: Option<Places>) {
        let places = places.map(Arc::new);
        if let Some(places) = places.clone() {
            let held = Arc::clone(&self.held);
            self.textures = Some(Pool::new(
                4,
                HashMap::<u32, OpenPack>::new,
                move |open, (key, pixels)| {
                    let read = texture::read_texture(open, &places, key, pixels);
                    let bytes = read.as_ref().map_or(0, CpuTexture::bytes);
                    held.fetch_add(bytes, Ordering::Relaxed);
                    read
                },
            ));
        }
        if let Some(middle) = index.middle() {
            self.camera.position = middle + Vec3::Z * START_HEIGHT;
        }
        let index = Arc::new(index);
        self.lights = Some(lights::Lights::new(Arc::clone(&index)));
        let shared = Arc::clone(&index);
        let workers = std::thread::available_parallelism().map_or(4, usize::from) / 2;
        let held = Arc::clone(&self.held);
        self.models = Some(Pool::new(
            workers.clamp(2, 12),
            HashMap::<u32, File>::new,
            move |files, model| {
                let read = model::read_model(files, &shared, places.as_deref(), model);
                held.fetch_add(read.as_ref().map_or(0, CpuModel::bytes), Ordering::Relaxed);
                read
            },
        ));
        let (eye, scale) = (self.camera.position, self.distance_scale());
        self.world = Some(World::new(Arc::clone(&index), eye, scale));
        self.index = Some(index);
    }

    /// Take what a build that ended built.
    pub fn poll(&mut self) {
        let built = (self.build.as_ref())
            .and_then(|b| b.ended.lock().unwrap_or_else(|p| p.into_inner()).take());
        let Some(built) = built else {
            return;
        };
        self.build = None;
        self.ended = Some(match built {
            Built::Index(index, places) => {
                let said = tr!(
                    "The GTA VI world is indexed: {maps} maps stream at rest, {archetypes} archetypes, {models} models",
                    maps = index.report.at_rest,
                    archetypes = index.report.archetypes,
                    models = index.report.models,
                );
                self.take(*index, places);
                (Outcome::Done(Then::Nothing), said)
            }
            Built::Stopped => (
                Outcome::Stopped(Then::Nothing),
                tr!("Stopped before it finished").to_string(),
            ),
            Built::Failed(why) => {
                self.failed = Some(why.clone());
                (
                    Outcome::Failed,
                    tr!("The GTA VI world was not indexed: {why}", why = why),
                )
            }
        });
    }

    /// Whether an index is being built, and how far it is.
    pub fn building(&self) -> Option<f32> {
        (self.build.as_ref()).map(|b| b.progress.snapshot().fraction())
    }

    /// The build while it runs, as the job list draws it.
    pub(in crate::state) fn job(&self) -> Option<RunningJob> {
        let build = self.build.as_ref()?;
        Some(RunningJob {
            kind: JobKind::ArchetypeSearch,
            doing: tr!("Indexing the GTA VI world").to_string(),
            fraction: Some(build.progress.snapshot().fraction()),
            detail: String::new(),
            stopping: build.cancel.is_set(),
        })
    }

    pub(in crate::state) fn stop(&self) {
        if let Some(build) = &self.build {
            build.cancel.set();
        }
    }

    /// How the build ended, as the job list says it, once.
    pub(in crate::state) fn ended(&mut self) -> Option<(Outcome, String)> {
        self.ended.take()
    }

    /// How the streaming stands.
    pub fn streaming(&self) -> Streaming {
        (self.world.as_ref()).map_or_else(Streaming::default, |w| w.latest().streaming)
    }

    /// What was selected last for the camera, after the world's thread is told where the camera
    /// is and which models came to the device, will never come (`None`) or left it (`false`).
    pub fn selected(
        &mut self,
        models: &mut Vec<(ModelRef, Option<bool>)>,
    ) -> Option<Arc<Selected>> {
        let world = self.world.as_ref()?;
        for (model, state) in models.iter() {
            match state {
                Some(false) => self.have.remove(model),
                _ => self.have.insert(*model),
            };
        }
        let told = !models.is_empty();
        world.look(self.camera.position, self.distance_scale());
        // The groups the world-state data has on at this hour and these modes, unless the person chose them.
        if let (false, Some(index)) = (self.groups_chosen, &self.index) {
            let on = index.default_on_in(self.hour, self.enabled.modes);
            if on != self.enabled.groups {
                self.enabled.groups = on;
            }
        }
        let hour = (self.sent.as_ref()).is_none_or(|(hour, _)| *hour != self.hour);
        let enabled = (self.sent.as_ref()).is_none_or(|(_, enabled)| *enabled != self.enabled);
        if hour {
            world.set_hour(self.hour);
        }
        if enabled {
            world.set_enabled(self.enabled.clone());
        }
        if hour || enabled {
            self.sent = Some((self.hour, self.enabled.clone()));
        }
        world.tell(models);
        let latest = world.latest();
        if told || self.wanted_of != latest.version {
            self.wanted_of = latest.version;
            // ⚠ A selection made before a model arrived still waits for it: it is not read twice.
            let wanted: Vec<(f32, ModelRef)> = (latest.wanted.iter().copied())
                .filter(|(_, model)| !self.have.contains(model))
                .collect();
            self.models_waiting = wanted.len();
            if let Some(models) = &self.models {
                models.want(wanted);
            }
        }
        Some(latest)
    }

    /// Ask for `wanted` (how soon, key, longest side seen in pixels) in place of what was asked before.
    pub fn want_textures(&mut self, wanted: Vec<(f32, u64, u32)>) {
        // ⚠ With no pack indexed nothing reads a texture: none waits, or what waits for none never ends.
        self.textures_waiting = if self.textures.is_some() {
            wanted.len()
        } else {
            0
        };
        if let Some(textures) = &self.textures {
            let asked = wanted.into_iter();
            textures.want(
                asked
                    .map(|(soon, key, pixels)| (soon, (key, pixels)))
                    .collect(),
            );
        }
    }

    /// The next texture that arrived, with the pixels asked: `None` where none of it reads, smaller than asked where nothing larger does.
    pub fn take_texture(&mut self) -> Option<(u64, u32, Option<CpuTexture>)> {
        let mut first = true;
        let arrived = (self.textures.as_ref()?).take(|| std::mem::take(&mut first));
        let ((key, pixels), texture) = arrived.into_iter().next()?;
        let bytes = texture.as_ref().map_or(0, CpuTexture::bytes);
        self.held.fetch_sub(bytes, Ordering::Relaxed);
        Some((key, pixels, texture))
    }

    /// The next model that arrived, `None` where it did not read.
    pub fn take_model(&mut self) -> Option<(ModelRef, Option<CpuModel>)> {
        let mut first = true;
        let arrived = (self.models.as_ref()?).take(|| std::mem::take(&mut first));
        let (model, read) = arrived.into_iter().next()?;
        let bytes = read.as_ref().map_or(0, CpuModel::bytes);
        self.held.fetch_sub(bytes, Ordering::Relaxed);
        Some((model, read))
    }

    /// The day the frame is lit by, the lights around the camera, and the exposure a material's own light is given at, at the viewer's hour.
    pub fn sky(
        &mut self,
        days: f32,
        weather: crate::state::drivers::WeatherState,
    ) -> (
        Option<or_renderer::game_shaders::lighting::Day>,
        (Vec<or_renderer::PointLight>, Vec<or_renderer::SpotLight>),
        f32,
    ) {
        let (eye, hour) = (self.camera.position, self.hour);
        let tables = self.root.as_deref().and_then(sky::tables);
        let day =
            (tables.as_ref()).and_then(|tables| sky::day_of(tables, hour, days, weather, eye.y));
        let scale = sky::light_scale(tables.as_deref(), hour, weather, eye.y);
        if let (Some(tables), Some(world)) = (&tables, &self.world) {
            world.set_fade(stream::fade_of(&tables.visual_settings));
        }
        let enabled = self.enabled.clone();
        let lights = self
            .lights
            .as_mut()
            .map_or_else(Default::default, |lights| {
                if let Some(tables) = &tables {
                    lights.set_reach(lights_frame::LodReach::of(&tables.visual_settings));
                }
                lights.set_enabled(&enabled);
                lights.look(eye);
                lights.lights(eye, hour, scale, lights::MOST)
            });
        // The lights take the exposure over pi; a material's own light is a radiance and takes it whole.
        let exposure = match day {
            Some(_) => scale * std::f32::consts::PI,
            None => 1.0,
        };
        (day, lights, exposure)
    }

    /// The water of the install shown once its quads are read, with the ground under it and the day it is seen in.
    pub fn water(&self, weather: crate::state::drivers::WeatherState) -> Option<water::Seen> {
        let root = self.root.as_deref()?;
        water::seen(root, self.hour, weather, self.camera.position.y)
    }

    /// The scale every distance is multiplied by: the person's, and the lens's where it is narrower than the game's.
    fn distance_scale(&self) -> f32 {
        self.lod_scale * or_world::streaming::camera_scale(self.camera.fov_y)
    }
}

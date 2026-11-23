//! The dressed ped of the Wardrobe workspace: what is worn, the files that wear it, and the scene the viewport draws.
//!
//! ⚠ **The engine decides what is worn and this reads the files.** `or_wardrobe::dress` answers which
//! drawable and texture each slot wears; everything here is reading those files on a worker, one
//! request at a time, and handing the viewport what it read. The ped's own drawables come from the
//! mounted install, and without one the preview shows the garment alone.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::SystemTime;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_renderer::OrbitCamera;
use or_title::vfs::MountedGame;
use or_wardrobe::dress::{self, BaseEdit, BaseOutfit, By, Choice, Dressing, Tone, Wear};
use or_wardrobe::{Component, Ped, Prop};

use super::wardrobe_select::in_pack;
use crate::render::dressed::{
    level_name, Aux, Counts, Diffuse, DressedScene, Model, ScenePart, LEVELS,
};
use crate::state::AppState;
use crate::ui::dock::Workspace;

mod alone;
mod assemble;
pub mod catalogue;
mod clips;
pub mod first_person;
pub mod light;
pub mod motion;
pub(crate) mod props;
mod read;
pub mod view;

use motion::Motion;
use read::{describe, mount_install, read_one};
pub(super) use read::{
    diffuse_called_of_bytes, first_texture_near, model_of_bytes, pack_bytes, prop_bones, thumb_with,
};
use view::ViewMode;

/// What the preview says while no game install is chosen: the garment is shown alone, since the ped is drawn from the install.
pub const NO_GAME: &str =
    tr_key!("No game install is chosen, so the preview shows the garment alone");

/// The swatches Tint offers: a colour that multiplies a worn garment's texture, for the eye and not for the pack.
pub const TINTS: &[(&str, [f32; 3])] = &[
    (tr_key!("None"), [1.0, 1.0, 1.0]),
    (tr_key!("Red"), [1.0, 0.55, 0.55]),
    (tr_key!("Orange"), [1.0, 0.75, 0.5]),
    (tr_key!("Yellow"), [1.0, 0.95, 0.5]),
    (tr_key!("Green"), [0.6, 1.0, 0.6]),
    (tr_key!("Teal"), [0.5, 0.95, 0.95]),
    (tr_key!("Blue"), [0.55, 0.65, 1.0]),
    (tr_key!("Purple"), [0.8, 0.6, 1.0]),
    (tr_key!("Grey"), [0.6, 0.6, 0.6]),
];

/// The longest side of a variation's thumbnail.
const THUMB: u32 = 64;

/// Files kept after they leave the scene, so wearing a garment again reads nothing; a decoded texture is megabytes, so it is not many.
const KEPT: usize = 48;

/// Pictures kept for the list once its rows no longer ask for them; each is a few kilobytes.
const KEPT_THUMBS: usize = 256;

/// Pictures of the list read in one batch, so that the scene of the dressed ped is not kept waiting for a screenful of them.
const ROW_BATCH: usize = 6;

static IDS: AtomicU64 = AtomicU64::new(1);

/// A file as it was when read: reading it again is due when this differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        if meta.is_dir() {
            return Some(Self::of_folder(path));
        }
        Some(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }

    /// A source folder as it is, which a garment may name as its model: the bytes of every file in it and the newest time any was written, since the folder's own time says nothing of an edit inside it.
    fn of_folder(dir: &Path) -> Self {
        let (mut len, mut modified) = (0, None);
        let mut folders = vec![dir.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    folders.push(entry.path());
                    continue;
                }
                len += meta.len();
                modified = modified.max(meta.modified().ok());
            }
        }
        Self { len, modified }
    }
}

/// What a read is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Model,
    /// The first texture of a dictionary, drawn with.
    Texture,
    /// The same, cut down to a tile.
    Thumb,
}

/// A file to read, by where it is and how it was last seen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Need {
    /// A file of the pack.
    Pack(Kind, PathBuf, Stamp),
    /// A file in the folder of a freemode ped of the install.
    Base(Kind, Ped, String),
}

impl Need {
    fn kind(&self) -> Kind {
        match self {
            Need::Pack(kind, ..) | Need::Base(kind, ..) => *kind,
        }
    }
}

/// What a read made.
#[derive(Clone)]
enum Loaded {
    Model(Arc<Model>),
    Texture(Arc<Diffuse>),
    Thumb(Arc<Thumb>),
    Failed(String),
}

/// A variation's tile.
pub struct Thumb {
    pub id: u64,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
    /// The dictionary's own size, for the tile's tip.
    pub full: [u32; 2],
}

/// What the ped's own drawables come from.
#[derive(Default)]
enum Base {
    #[default]
    Idle,
    Mounting(Receiver<Result<Arc<Mounted>, String>>),
    Ready(Arc<Mounted>),
    Failed(String),
}

/// The install, mounted, with the variation metadata and the skeleton of both freemode peds read from it.
struct Mounted {
    game: MountedGame,
    outfits: BTreeMap<Ped, Result<BaseOutfit, String>>,
    skeletons: BTreeMap<Ped, Result<or_anim::Skeleton, String>>,
    /// The same skeletons as the bones a skin is bound to by name.
    bones: BTreeMap<Ped, Result<Vec<or_interchange::Bone>, String>>,
}

/// A read in flight: the files it is for, and who reads them.
struct Loading {
    needs: Vec<Need>,
    receiver: Receiver<Vec<(Need, Loaded)>>,
}

/// What the Wardrobe's preview shows and how it is set.
pub struct Dressed {
    /// The ped to dress; the selected garment's when none is chosen.
    pub ped: Option<Ped>,
    /// Garments put on in the outfit, by id.
    pub outfit: Vec<String>,
    /// Wear the outfit with the selected garment on top; off wears the selected garment alone.
    pub outfit_on: bool,
    pub tone: Tone,
    pub heels: bool,
    /// The A of A/B: the ped without the pack.
    pub original: bool,
    /// B is the pack as its previous kept version stated it, in [`Self::earlier`], and not as it is now.
    pub previous: bool,
    /// The pack as it was before its latest change, read from the versions the Wardrobe keeps when that was asked for.
    pub earlier: Option<or_wardrobe::Wardrobe>,
    /// An index of [`TINTS`].
    pub tint: usize,
    /// The variation each garment is worn with, by id.
    pub variation: BTreeMap<String, usize>,
    pub camera: OrbitCamera,
    /// The level of detail drawn: 0 is the high one.
    pub lod: usize,
    /// The camera is placed on the scene the next time one stands.
    pub needs_frame: bool,
    framed_for: Option<Ped>,
    loaded: HashMap<Need, Loaded>,
    /// What the scene and the tiles asked for in the last frame.
    wanted: std::collections::HashSet<Need>,
    loading: Option<Loading>,
    base: Base,
    base_root: Option<PathBuf>,
    scene: Option<Arc<DressedScene>>,
    /// Sentences about what is not on screen and why, for the panel.
    pub notes: Vec<String>,
    /// How high the ped is raised by the heels worn, in metres.
    pub lift: f32,
    /// The tile a drop of files is aimed at, as the window last saw the pointer.
    pub drop_tile: Option<(String, usize)>,
    /// What the 3D view draws.
    pub view: ViewMode,
    /// The key light the plain views are lit by.
    pub light: light::Light,
    /// The alternate model each garment is worn with, by its id and the number after its model's name; a garment not in it is worn with its model.
    pub alternate: BTreeMap<String, usize>,
    /// Which sets of alternate variations are in force.
    pub sets: dress::Sets,
    /// The drawables the sets in force change, as the last scene was made.
    pub driven: Vec<dress::Driven>,
    /// The view from the ped's head, the field of view the engine starts a camera with.
    pub first_person: bool,
    /// Where the head looks in the first person view: the heading and the pitch.
    look: (f32, f32),
    /// The camera of the first person view, once there is a head to put it at.
    first_person_camera: OrbitCamera,
    /// The game's shadows are drawn in the Shaded view.
    pub shadows: bool,
    /// Draw the ped's skeleton over it.
    pub bones: bool,
    /// What was asked of the ped's own slots: taken off, or another of its drawables.
    pub base_edits: BTreeMap<Component, BaseEdit>,
    /// The clip played on the ped, none for its rest pose.
    pub motion: Option<Motion>,
    /// The clips of the dictionary played, for the choice among them.
    pub clips: Vec<(u32, String)>,
    /// Why a clip is not playing.
    pub clip_note: Option<String>,
    reading_clip: Option<clips::Reading>,
    /// The ped the clip was bound to: another ped is another skeleton.
    motion_ped: Option<Ped>,
    last_tick: Option<std::time::Instant>,
    /// A skeleton for a ped that has no install to read it from, for a unit.
    own_skeleton: Option<(Ped, or_anim::Skeleton)>,
    /// The pictures of the pack being drawn, or just drawn.
    pub catalogue: Option<catalogue::Catalogue>,
    /// How many recooks that read a tab again the preview has seen.
    seen_recooked: u64,
    /// The pictures of textures, by file, with the file as it was when read: the tiles of the garment selected and the rows of the list.
    thumbs: HashMap<PathBuf, (Stamp, Arc<Thumb>)>,
    /// The textures the list asked a picture of in its last frame: the first of each row it drew, and every one of the row under the pointer.
    row_wanted: Vec<PathBuf>,
}

impl Default for Dressed {
    fn default() -> Self {
        Self {
            ped: None,
            outfit: Vec::new(),
            outfit_on: false,
            tone: Tone::default(),
            heels: true,
            original: false,
            previous: false,
            earlier: None,
            tint: 0,
            variation: BTreeMap::new(),
            camera: OrbitCamera::default(),
            lod: 0,
            needs_frame: true,
            framed_for: None,
            loaded: HashMap::new(),
            wanted: Default::default(),
            loading: None,
            base: Base::Idle,
            base_root: None,
            scene: None,
            notes: Vec::new(),
            lift: 0.0,
            drop_tile: None,
            view: ViewMode::default(),
            light: light::Light::default(),
            alternate: BTreeMap::new(),
            sets: dress::Sets::default(),
            driven: Vec::new(),
            first_person: false,
            look: first_person::LOOK,
            first_person_camera: OrbitCamera::default(),
            shadows: true,
            bones: false,
            base_edits: BTreeMap::new(),
            motion: None,
            clips: Vec::new(),
            clip_note: None,
            reading_clip: None,
            motion_ped: None,
            last_tick: None,
            own_skeleton: None,
            catalogue: None,
            seen_recooked: 0,
            thumbs: HashMap::new(),
            row_wanted: Vec::new(),
        }
    }
}

impl Dressed {
    /// The scene to draw, none while there is nothing to wear.
    pub fn scene(&self) -> Option<&DressedScene> {
        self.scene.as_deref()
    }

    /// Stand `scene` up as what is drawn, for a unit that has no files to read.
    pub fn show(&mut self, scene: DressedScene) {
        self.scene = Some(Arc::new(scene));
    }

    /// Whether a read of files is under way.
    pub fn is_reading(&self) -> bool {
        self.loading.is_some() || matches!(self.base, Base::Mounting(_))
    }

    /// Whether a clip is being read, or one is playing and so wants a frame again.
    pub fn is_animating(&self) -> bool {
        self.reading_clip.is_some() || self.motion.as_ref().is_some_and(|m| m.transport.playing)
    }

    /// Whether a catalogue of the pack is being drawn, which wants a frame again.
    pub fn is_cataloguing(&self) -> bool {
        self.catalogue.as_ref().is_some_and(|c| !c.is_finished())
    }

    /// The skeleton of `ped`: the install's, once read.
    pub fn skeleton(&self, ped: Ped) -> Option<&or_anim::Skeleton> {
        if let Some((own, skeleton)) = &self.own_skeleton {
            return (*own == ped).then_some(skeleton);
        }
        match &self.base {
            Base::Ready(mounted) => mounted.skeletons.get(&ped)?.as_ref().ok(),
            _ => None,
        }
    }

    /// Stand `skeleton` up as the skeleton of `ped`, for a unit that has no install to read it from.
    pub fn show_skeleton(&mut self, ped: Ped, skeleton: or_anim::Skeleton) {
        self.own_skeleton = Some((ped, skeleton));
    }

    /// The ped's drawables of `slot` and how many it has, once its metadata is read.
    pub fn base_drawables(&self, ped: Ped, slot: Component) -> usize {
        match &self.base {
            Base::Ready(mounted) => match mounted.outfits.get(&ped) {
                Some(Ok(outfit)) => outfit.drawables(slot),
                _ => 0,
            },
            _ => 0,
        }
    }

    /// The slots the ped wears something in, once its metadata is read.
    pub fn base_slots(&self, ped: Ped) -> Vec<Component> {
        match &self.base {
            Base::Ready(mounted) => match mounted.outfits.get(&ped) {
                Some(Ok(outfit)) => outfit.slots().collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Let the ped stand at rest again.
    pub fn stop_clip(&mut self) {
        self.motion = None;
        self.reading_clip = None;
        self.clips.clear();
        self.clip_note = None;
    }

    /// Play clip `key` of the dictionary already read, bound to `ped`'s skeleton.
    pub fn choose_clip(&mut self, ped: Ped, key: u32) {
        let Some(old) = &self.motion else { return };
        let (dictionary, looping) = (Arc::clone(&old.dictionary), old.transport.is_looping());
        let label = (self.clips.iter())
            .find(|(k, _)| *k == key)
            .map_or_else(|| old.label.clone(), |(_, name)| name.clone());
        self.bind_clip(ped, label, dictionary, key, true);
        if let Some(motion) = self.motion.as_mut().filter(|_| looping) {
            motion.transport.set_looping(true);
        }
    }

    fn bind_clip(
        &mut self,
        ped: Ped,
        label: String,
        dictionary: Arc<or_io::gta5::CrClipDictionary>,
        key: u32,
        playing: bool,
    ) {
        let Some(skeleton) = self.skeleton(ped).cloned() else {
            self.clip_note =
                Some(tr!("The ped's skeleton is read from the game install, which needs to be chosen in Settings").into());
            return;
        };
        match Motion::bind(label, dictionary, key, &skeleton) {
            Ok(mut motion) => {
                if let Some(old) = &self.motion {
                    motion.transport.rate = old.transport.rate;
                }
                motion.transport.playing = playing;
                self.clips = motion::clips_of(&motion.dictionary);
                self.motion_ped = Some(ped);
                self.motion = Some(motion);
                self.clip_note = None;
            }
            Err(why) => {
                self.motion = None;
                self.clip_note = Some(why);
            }
        }
    }

    /// Take a dictionary a worker has read, and play its clip.
    fn take_clip(&mut self, ped: Ped) {
        let Some(reading) = &self.reading_clip else {
            return;
        };
        let read = match reading.receiver.try_recv() {
            Ok(read) => read,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(tr!("the read stopped").to_string()),
        };
        let Some(reading) = self.reading_clip.take() else {
            return;
        };
        let preset = reading.wanted.is_some();
        let dictionary = match read {
            Ok(dictionary) => dictionary,
            Err(why) => {
                self.clip_note = Some(format!("{}: {why}", reading.label));
                return;
            }
        };
        let key = match reading.wanted {
            Some(name) => motion::find(&dictionary, name),
            None => motion::clips_of(&dictionary).first().map(|c| c.0),
        };
        match key {
            Some(key) => {
                self.bind_clip(ped, reading.label, dictionary, key, true);
                // ⚠ A preset is a cycle of movement: the clips of the engine's default movement carry no loop flag, and one that stopped at its end showed a ped frozen mid-stride.
                if let Some(motion) = self.motion.as_mut().filter(|_| preset) {
                    motion.transport.set_looping(true);
                }
            }
            None => {
                self.clip_note = Some(tr!(
                    "{reading}: the dictionary holds no such clip",
                    reading = reading.label
                ))
            }
        }
    }

    /// Move the clip on by the time since the last frame, and let it go when the ped is another one.
    fn tick_clip(&mut self, ped: Ped) {
        let now = std::time::Instant::now();
        let dt = self
            .last_tick
            .replace(now)
            .map_or(0.0, |t| (now - t).as_secs_f32().min(0.1));
        if self.motion_ped.is_some_and(|bound| bound != ped) {
            self.stop_clip();
        }
        if let Some(motion) = &mut self.motion {
            motion.tick(dt);
        }
    }

    /// Why the ped's own drawables are not there, none where they are or are being read.
    pub fn base_missing(&self) -> Option<&str> {
        match &self.base {
            Base::Failed(why) => Some(why),
            _ => None,
        }
    }

    /// The tile of the texture at `file`, once read.
    pub fn thumb(&self, file: &Path) -> Option<&Arc<Thumb>> {
        self.thumbs.get(file).map(|(_, thumb)| thumb)
    }

    /// The textures the list wants a picture of in this frame; each is decoded once, and again only when its file changes.
    pub fn ask_row_thumbs(&mut self, files: Vec<PathBuf>) {
        self.row_wanted = files;
    }

    /// Whether `need` is read already: held, or a picture made of the file as it is.
    fn has(&self, need: &Need) -> bool {
        self.loaded.contains_key(need)
            || matches!(need, Need::Pack(Kind::Thumb, path, stamp)
                if self.thumbs.get(path).is_some_and(|(read, _)| read == stamp))
    }

    /// Start over reading the ped's own drawables from the install.
    pub fn retry_base(&mut self) {
        self.base = Base::Idle;
        self.base_root = None;
    }

    pub fn clear_outfit(&mut self) {
        self.outfit.clear();
    }

    /// Put garment `id` in the outfit, or take it out when it is in.
    pub fn toggle_in_outfit(&mut self, id: &str) {
        match self.outfit.iter().position(|g| g == id) {
            Some(at) => {
                self.outfit.remove(at);
            }
            None => self.outfit.push(id.to_string()),
        }
    }

    /// Ask the camera to be placed on the ped again.
    pub fn frame_again(&mut self) {
        self.needs_frame = true;
    }
}

/// One slot's files, as the engine named them and the pack's folder places them.
struct Slot {
    /// The garment that wears it, none for the ped's own drawable.
    garment: Option<String>,
    model: Need,
    texture: Option<Need>,
    /// The anchor of a prop, whose bone the model is carried to; none for a component, whose vertices are already in the skeleton's space.
    anchor: Option<Prop>,
}

/// What this frame asks of the preview, settled from the pack and the choices.
struct Plan {
    ped: Ped,
    dressing: Dressing,
    slots: Vec<Slot>,
    /// The tiles to show: the variations of the garment selected.
    tiles: Vec<Need>,
    /// The pictures the rows of the list want.
    rows: Vec<Need>,
}

impl AppState {
    /// The ped the preview dresses: the one chosen, else the selected garment's, else the male.
    pub fn dressed_ped(&self) -> Ped {
        self.workbench.wardrobe.dressed.ped.unwrap_or_else(|| {
            self.selected_garment()
                .and_then(|(pack, at)| pack.read.as_ref().ok()?.wardrobe.garments[at].ped)
                .unwrap_or(Ped::Male)
        })
    }

    /// Play preset `index` of [`motion::PRESETS`] on the ped, its dictionary read from the install on a worker.
    pub fn play_preset(&mut self, index: usize) {
        let Some(preset) = motion::PRESETS.get(index) else {
            return;
        };
        let (ped, crypto) = (self.dressed_ped(), self.keys.provider());
        let dressed = &mut self.workbench.wardrobe.dressed;
        let Base::Ready(mounted) = &dressed.base else {
            dressed.clip_note = Some(
                tr!("The presets are read from the game install, which needs to be chosen in Settings")
                    .into(),
            );
            return;
        };
        let mounted = Arc::clone(mounted);
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let _ = sender.send(clips::read_preset(&mounted, ped, &crypto));
        });
        dressed.clip_note = None;
        dressed.reading_clip = Some(clips::Reading {
            label: crate::i18n::tr(preset.label).to_string(),
            wanted: Some(preset.clip),
            receiver,
        });
    }

    /// Play the first clip of the `.ycd` file at `path` on the ped, read on a worker.
    pub fn play_clip_file(&mut self, path: PathBuf) {
        let crypto = self.keys.provider();
        let label = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let _ = sender.send(clips::read_file(&path, &crypto));
        });
        let dressed = &mut self.workbench.wardrobe.dressed;
        dressed.clip_note = None;
        dressed.reading_clip = Some(clips::Reading {
            label,
            wanted: None,
            receiver,
        });
    }

    /// The variation `id` is worn with, clamped to the textures it has.
    pub fn dressed_variation(&self, id: &str) -> usize {
        let chosen = self
            .workbench
            .wardrobe
            .dressed
            .variation
            .get(id)
            .copied()
            .unwrap_or(0);
        let held = self
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(id))
            .map_or(0, |g| g.variations.len());
        chosen.min(held.saturating_sub(1))
    }

    /// The choice the preview is showing: the outfit and the selected garment, on the ped.
    fn dressed_choice(&self) -> Choice {
        let dressed = &self.workbench.wardrobe.dressed;
        let mut ids: Vec<String> = if dressed.outfit_on {
            dressed.outfit.clone()
        } else {
            Vec::new()
        };
        if let Some((_, id)) = &self.workbench.wardrobe.selected {
            ids.retain(|g| g != id);
            ids.push(id.clone());
        }
        let mut choice = Choice::new(self.dressed_ped());
        choice.wear = ids
            .into_iter()
            .map(|garment| Wear {
                variation: self.dressed_variation(&garment),
                garment,
            })
            .collect();
        choice.tone = dressed.tone;
        choice.heels = dressed.heels;
        choice.original = dressed.original;
        choice.base = dressed.base_edits.clone();
        choice.alternates = dressed.alternate.clone();
        choice.sets = dressed.sets.clone();
        choice.first_person = dressed.first_person;
        choice
    }

    /// What the preview needs read and drawn, or none while no pack can be read.
    fn dressed_plan(&self) -> Option<Plan> {
        let pack = self.wardrobe_pack()?;
        let read = pack.read.as_ref().ok()?;
        let choice = self.dressed_choice();
        let ped = choice.ped;
        let empty = BaseOutfit::default();
        let base = match &self.workbench.wardrobe.dressed.base {
            Base::Ready(mounted) => match mounted.outfits.get(&ped) {
                Some(Ok(outfit)) => outfit,
                _ => &empty,
            },
            _ => &empty,
        };
        let dressed = &self.workbench.wardrobe.dressed;
        let shown = (dressed.earlier.as_ref()).filter(|_| dressed.previous);
        let dressing = dress::dress(shown.unwrap_or(&read.wardrobe), base, &choice);
        // ⚠ A name that leaves the pack is never stamped or read: a garment whose model does is left off, a texture that does is left out.
        let slot_of = |by: &By, anchor: Option<Prop>| match by {
            By::Garment { id, model, texture } => {
                // The model a step of the mesh doctor replaced, while the viewport is asked to show it.
                let model = self
                    .workbench
                    .wardrobe
                    .mesh
                    .drawn_instead(id, model)
                    .unwrap_or(model);
                let file = |name: &str, kind| {
                    let path = in_pack(&pack.folder, name)?;
                    let stamp = Stamp::of(&path).unwrap_or(Stamp {
                        len: 0,
                        modified: None,
                    });
                    Some(Need::Pack(kind, path, stamp))
                };
                Some(Slot {
                    garment: Some(id.clone()),
                    model: file(model, Kind::Model)?,
                    texture: texture.as_ref().and_then(|t| file(t, Kind::Texture)),
                    anchor,
                })
            }
            By::Base { model, texture } => Some(Slot {
                garment: None,
                model: Need::Base(Kind::Model, ped, model.clone()),
                texture: texture
                    .as_ref()
                    .map(|t| Need::Base(Kind::Texture, ped, t.clone())),
                anchor,
            }),
        };
        let slots = (dressing.worn.iter())
            .filter_map(|worn| slot_of(&worn.by, None))
            .chain((dressing.props.iter()).filter_map(|worn| slot_of(&worn.by, Some(worn.prop))))
            .collect();
        let tiles = self
            .selected_garment()
            .and_then(|(_, at)| read.wardrobe.garments.get(at))
            .map(|g| {
                g.variations
                    .iter()
                    .filter_map(|v| v.texture.as_ref())
                    .filter_map(|t| in_pack(&pack.folder, t))
                    .map(|path| {
                        let stamp = Stamp::of(&path).unwrap_or(Stamp {
                            len: 0,
                            modified: None,
                        });
                        Need::Pack(Kind::Thumb, path, stamp)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rows = (self.workbench.wardrobe.dressed.row_wanted.iter())
            .map(|path| {
                let stamp = Stamp::of(path).unwrap_or(Stamp {
                    len: 0,
                    modified: None,
                });
                Need::Pack(Kind::Thumb, path.clone(), stamp)
            })
            .collect();
        Some(Plan {
            ped,
            dressing,
            slots,
            tiles,
            rows,
        })
    }

    /// Read what the preview lacks, and make the scene the viewport draws of what it has; once a frame.
    pub(super) fn poll_dressed(&mut self) {
        // ⚠ A recook that read a tab again also reads the pack's files again: the files are the cook's to change and the stamps below are only the first way to see it.
        if self.workbench.wardrobe.dressed.seen_recooked != self.recooked {
            let dressed = &mut self.workbench.wardrobe.dressed;
            dressed.seen_recooked = self.recooked;
            dressed
                .loaded
                .retain(|need, _| !matches!(need, Need::Pack(..)));
        }
        self.take_dressed_reads();
        self.poll_catalogue();
        self.poll_dressed_base();
        if self.active_workspace != Workspace::Wardrobe {
            self.workbench.wardrobe.dressed.last_tick = None;
            return;
        }
        let ped = self.dressed_ped();
        let dressed = &mut self.workbench.wardrobe.dressed;
        dressed.take_clip(ped);
        dressed.tick_clip(ped);
        let Some(plan) = self.dressed_plan() else {
            let dressed = &mut self.workbench.wardrobe.dressed;
            dressed.scene = None;
            dressed.notes.clear();
            return;
        };
        let mut wanted: Vec<Need> = Vec::new();
        for slot in &plan.slots {
            wanted.push(slot.model.clone());
            wanted.extend(slot.texture.clone());
        }
        wanted.extend(plan.tiles.iter().cloned());
        self.workbench.wardrobe.dressed.wanted = wanted.iter().chain(&plan.rows).cloned().collect();
        let dressed = &self.workbench.wardrobe.dressed;
        // A file read under an older stamp is no longer the file.
        let mut missing: Vec<Need> = wanted
            .iter()
            .filter(|need| !dressed.has(need))
            .cloned()
            .collect();
        missing.extend(
            (plan.rows.iter())
                .filter(|need| !dressed.has(need) && !missing.contains(need))
                .take(ROW_BATCH)
                .cloned()
                .collect::<Vec<_>>(),
        );
        if !missing.is_empty() && dressed.loading.is_none() {
            let install = match &dressed.base {
                Base::Ready(mounted) => Some(Arc::clone(mounted)),
                _ => None,
            };
            let request = (missing.clone(), install, self.keys.provider());
            self.start_dressed_read(request);
        }
        self.assemble_dressed(&plan);
    }

    fn start_dressed_read(
        &mut self,
        (needs, install, crypto): (Vec<Need>, Option<Arc<Mounted>>, Arc<dyn RpfCryptoProvider>),
    ) {
        let (sender, receiver) = channel();
        let asked = needs.clone();
        std::thread::spawn(move || {
            let read = asked
                .into_iter()
                .map(|need| {
                    let loaded = read_one(&need, install.as_deref(), &crypto);
                    (need, loaded)
                })
                .collect();
            let _ = sender.send(read);
        });
        self.workbench.wardrobe.dressed.loading = Some(Loading { needs, receiver });
    }

    /// Take what a finished read made, and forget the oldest files once there are many.
    fn take_dressed_reads(&mut self) {
        let dressed = &mut self.workbench.wardrobe.dressed;
        let Some(loading) = &dressed.loading else {
            return;
        };
        let read = match loading.receiver.try_recv() {
            Ok(read) => read,
            Err(TryRecvError::Empty) => return,
            // ⚠ A worker that panicked drops its sender: what it was asked for reads as failed, not as waited for.
            Err(TryRecvError::Disconnected) => loading
                .needs
                .iter()
                .map(|need| {
                    (
                        need.clone(),
                        Loaded::Failed(tr!("the read stopped").to_string()),
                    )
                })
                .collect(),
        };
        dressed.loading = None;
        for (need, loaded) in read {
            if let (Need::Pack(Kind::Thumb, path, stamp), Loaded::Thumb(thumb)) = (&need, &loaded) {
                dressed
                    .thumbs
                    .insert(path.clone(), (*stamp, Arc::clone(thumb)));
                continue;
            }
            dressed.loaded.insert(need, loaded);
        }
        // ⚠ The pictures of rows that are no longer asked for go once there are many; one that is asked for and lacks is read again at once.
        if dressed.thumbs.len() > KEPT_THUMBS {
            let asked: std::collections::HashSet<&PathBuf> = (dressed.wanted.iter())
                .filter_map(|need| match need {
                    Need::Pack(Kind::Thumb, path, _) => Some(path),
                    _ => None,
                })
                .collect();
            dressed.thumbs.retain(|path, _| asked.contains(path));
        }
        // ⚠ Only what the scene and the tiles do not ask for is let go: a file they ask for and lack is read again at once.
        if dressed.loaded.len() > KEPT {
            let unwanted: Vec<Need> = (dressed.loaded.keys())
                .filter(|need| !dressed.wanted.contains(*need))
                .cloned()
                .collect();
            for need in unwanted.into_iter().take(dressed.loaded.len() - KEPT) {
                dressed.loaded.remove(&need);
            }
        }
    }

    /// Mount the active install once for the peds' own drawables, and take the mount when it is done.
    fn poll_dressed_base(&mut self) {
        let root = self.prefs.active_game_root().cloned();
        let crypto = self.keys.provider();
        let mount = self.prefs.mount_options();
        let keys = self.keys.has_keys();
        let dressed = &mut self.workbench.wardrobe.dressed;
        if let Base::Mounting(receiver) = &dressed.base {
            match receiver.try_recv() {
                Ok(Ok(mounted)) => dressed.base = Base::Ready(mounted),
                Ok(Err(why)) => dressed.base = Base::Failed(why),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    dressed.base = Base::Failed(tr!("the install could not be read").to_string())
                }
            }
            if !matches!(dressed.base, Base::Mounting(_)) {
                // Everything read under the old base is read again with it.
                dressed
                    .loaded
                    .retain(|need, _| matches!(need, Need::Pack(..)));
            }
        }
        if dressed.base_root != root {
            dressed.base_root = root.clone();
            dressed.base = Base::Idle;
            dressed
                .loaded
                .retain(|need, _| matches!(need, Need::Pack(..)));
        }
        // ⚠ Not before the game's keys are read: an archive that has no key to open it fails the mount, and the failure is not one a retry mends by itself.
        let ready = keys && self.active_workspace == Workspace::Wardrobe;
        if let (Base::Idle, Some(root), true) = (&dressed.base, root, ready) {
            let (sender, receiver) = channel();
            std::thread::spawn(move || {
                let _ = sender.send(mount_install(root, crypto, mount));
            });
            dressed.base = Base::Mounting(receiver);
        }
    }

    /// Step the camera of the dressed ped by what the pointer did, and place it on the ped when it is asked to.
    pub(crate) fn step_dressed_camera(
        &mut self,
        inputs: crate::render::viewport::CameraInputs,
    ) -> &OrbitCamera {
        let ped = self.dressed_ped();
        let dressed = &mut self.workbench.wardrobe.dressed;
        // ⚠ The orbit camera is left as it is while the head looks: it is where it was left when the view comes back.
        if dressed.first_person && dressed.look_from_the_head(ped, &inputs) {
            return dressed.head_camera();
        }
        inputs.apply_to(&mut dressed.camera);
        if inputs.frame_active || inputs.frame_selection {
            dressed.needs_frame = true;
        }
        if dressed.needs_frame {
            if let Some(scene) = dressed.scene.as_ref().filter(|s| !s.parts.is_empty()) {
                dressed.camera.frame_sphere(scene.centre, scene.radius);
                dressed.needs_frame = false;
            }
        }
        &dressed.camera
    }

    /// Whether the active Wardrobe has a ped drawn or being read.
    pub fn dressed_is_ready(&self) -> bool {
        self.workbench.wardrobe.dressed.scene.is_some()
    }

    /// Whether the scene stands for the choice as the last poll saw it: every file it wants read, and the ped's own drawables read or given up on.
    pub fn dressed_is_settled(&self) -> bool {
        let dressed = &self.workbench.wardrobe.dressed;
        let base_done = match dressed.base {
            Base::Ready(_) | Base::Failed(_) => true,
            Base::Idle => dressed.base_root.is_none() || !self.keys.has_keys(),
            Base::Mounting(_) => false,
        };
        base_done
            && dressed.loading.is_none()
            && dressed.wanted.iter().all(|need| dressed.has(need))
            && dressed.scene.is_some()
    }
}

/// What the preview says of a hat that takes the ped's hair off.
fn hair_note(hat: &str) -> String {
    tr!("The ped's hair is off: {hat} states a hair scale of {hides} or less, which is how staged hats hide it; measured on staged hats and not verified in game", hat = hat, hides = or_wardrobe::hair::HIDES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_keeps_the_proportions_of_the_texture_and_averages_its_pixels() {
        let wide = Diffuse {
            id: 0,
            width: 256,
            height: 128,
            rgba: [200u8, 100, 50, 255].repeat(256 * 128),
            srgb: true,
        };
        let tile = thumb_with(&wide, THUMB, [256, 128]);
        assert_eq!(tile.size, [64, 32]);
        assert_eq!(tile.full, [256, 128]);
        assert_eq!(&tile.rgba[..4], &[200, 100, 50, 255]);
        assert_eq!(tile.rgba.len(), 64 * 32 * 4);
    }

    #[test]
    fn a_source_folder_is_stamped_by_what_is_inside_it_and_a_folder_that_cannot_be_cooked_says_why()
    {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("jacket.drawables.src");
        std::fs::create_dir_all(source.join("drawables")).unwrap();
        std::fs::write(source.join("drawables/model.glb"), b"geometry").unwrap();
        let first = Stamp::of(&source).expect("a folder is stamped");
        assert_eq!(first.len, 8);

        std::fs::write(source.join("drawables/model.glb"), b"geometry, edited").unwrap();
        assert_ne!(
            Stamp::of(&source),
            Some(first),
            "an edit inside the folder is a change of it"
        );

        let why = pack_bytes(&source).expect_err("no asset.json, no cook");
        assert!(why.contains("made no file"), "{why}");
    }

    #[test]
    fn a_hat_that_hides_the_hair_is_said_to_do_so_by_the_figure_it_states_and_as_unverified() {
        let note = hair_note("Beanie");
        assert!(note.contains("Beanie"), "{note}");
        assert!(
            note.contains(&or_wardrobe::hair::HIDES.to_string()),
            "{note}"
        );
        assert!(note.contains("not verified in game"), "{note}");
    }

    #[test]
    fn a_ped_that_changes_lets_the_clip_go_and_one_that_does_not_keeps_it() {
        use motion::tests::{dictionary, skeleton};
        let mut dressed = Dressed::default();
        dressed.show_skeleton(Ped::Male, skeleton());
        let dictionary = Arc::new(dictionary(&["walk", "run"], 100));
        let key = motion::find(&dictionary, "walk").unwrap();
        dressed.bind_clip(Ped::Male, "Walk".into(), dictionary, key, true);
        assert!(dressed.is_animating());
        assert_eq!(
            dressed.clips.len(),
            2,
            "the clips of the dictionary are listed"
        );

        dressed.tick_clip(Ped::Male);
        assert!(dressed.motion.is_some(), "the same ped keeps it");
        dressed.tick_clip(Ped::Female);
        assert!(
            dressed.motion.is_none() && dressed.clips.is_empty(),
            "another ped is another skeleton"
        );

        // A ped with no skeleton says why nothing plays.
        let mut bare = Dressed::default();
        let dictionary = Arc::new(dictionary_for_bare());
        bare.bind_clip(Ped::Male, "Walk".into(), dictionary, 1, true);
        assert!(bare
            .clip_note
            .as_deref()
            .is_some_and(|n| n.contains("needs to be chosen in Settings")));
    }

    /// Poll the preview until the clip it was asked to read is read, bound or refused.
    fn read_the_clip(state: &mut AppState) {
        state.active_workspace = Workspace::Wardrobe;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while state.workbench.wardrobe.dressed.reading_clip.is_some() {
            state.poll_dressed();
            assert!(
                std::time::Instant::now() < deadline,
                "the clip was never read"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_clip_file_is_read_and_played_on_the_ped_and_one_that_does_not_read_says_so() {
        use motion::tests::{dictionary, skeleton};
        use or_io::decl::File;
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("walk.ycd");
        dictionary(&["walk", "run"], 100)
            .save_as(&good)
            .expect("a dictionary writes");
        let bad = dir.path().join("bad.ycd");
        std::fs::write(&bad, b"not a clip dictionary").unwrap();

        let mut state = AppState::from_prefs(crate::state::Prefs::default());
        state
            .workbench
            .wardrobe
            .dressed
            .show_skeleton(state.dressed_ped(), skeleton());
        state.play_clip_file(bad);
        read_the_clip(&mut state);
        let dressed = &state.workbench.wardrobe.dressed;
        assert!(dressed.motion.is_none());
        assert!(
            dressed
                .clip_note
                .as_deref()
                .is_some_and(|n| n.contains("bad.ycd")),
            "{:?}",
            dressed.clip_note
        );

        state.play_clip_file(good);
        read_the_clip(&mut state);
        let dressed = &state.workbench.wardrobe.dressed;
        let motion = dressed.motion.as_ref().expect("the first clip plays");
        assert_eq!(motion.label, "walk.ycd");
        assert!(motion.transport.playing);
        assert_eq!(dressed.clips.len(), 2, "the clips of the file are listed");
        assert_eq!(dressed.clip_note, None);
    }

    #[test]
    fn another_clip_of_the_dictionary_keeps_the_loop_the_first_was_given() {
        use motion::tests::{dictionary, skeleton};
        let mut dressed = Dressed::default();
        dressed.show_skeleton(Ped::Male, skeleton());
        let dictionary = Arc::new(dictionary(&["idle", "walk"], 100));
        let idle = motion::find(&dictionary, "idle").unwrap();
        let walk = motion::find(&dictionary, "walk").unwrap();
        dressed.bind_clip(Ped::Male, "Idle".into(), dictionary, idle, true);
        let motion = dressed.motion.as_mut().unwrap();
        motion.transport.set_looping(true);
        motion.transport.rate = 2.0;

        dressed.choose_clip(Ped::Male, walk);
        let motion = dressed.motion.as_ref().unwrap();
        assert_eq!(motion.key, walk);
        assert!(motion.transport.is_looping(), "it loops as the first did");
        assert_eq!(motion.transport.rate, 2.0, "at the speed chosen");
    }

    #[test]
    fn the_bones_are_lines_only_while_asked_for_and_a_clip_moves_them_over_the_rest_pose() {
        use crate::render::skeleton_lines::dressed_bones;
        use motion::tests::{dictionary, skeleton};
        let mut dressed = Dressed::default();
        dressed.show_skeleton(Ped::Male, skeleton());
        assert!(
            dressed_bones(&dressed, Ped::Male).is_empty(),
            "Bones is off"
        );

        dressed.bones = true;
        assert!(
            dressed_bones(&dressed, Ped::Female).is_empty(),
            "another ped's skeleton is not this one's"
        );
        let rest = dressed_bones(&dressed, Ped::Male);
        assert!(!rest.is_empty());

        dressed.lift = 0.5;
        let lifted = dressed_bones(&dressed, Ped::Male);
        assert!(
            lifted
                .iter()
                .zip(&rest)
                .all(|(up, down)| (up.position[2] - down.position[2] - 0.5).abs() < 1e-6),
            "the bones are raised as the ped is"
        );
        dressed.lift = 0.0;

        let dictionary = Arc::new(dictionary(&["walk"], 100));
        let key = motion::find(&dictionary, "walk").unwrap();
        dressed.bind_clip(Ped::Male, "Walk".into(), dictionary, key, true);
        let motion = dressed.motion.as_mut().unwrap();
        motion.tick(0.5);
        let playing = dressed_bones(&dressed, Ped::Male);
        let (ghost, posed) = playing.split_at(playing.len() - rest.len());
        assert_eq!(
            ghost.len(),
            2,
            "the rest pose's one segment stays as a ghost"
        );
        let at: Vec<[f32; 3]> = ghost.iter().map(|v| v.position).collect();
        assert!(
            at.contains(&[0.0, 0.0, 0.0]) && at.contains(&[1.0, 0.0, 0.0]),
            "the ghost is the rest pose: {at:?}"
        );
        assert!(
            posed
                .iter()
                .zip(&rest)
                .any(|(moved, was)| moved.position != was.position),
            "the clip moved a bone"
        );
    }

    fn dictionary_for_bare() -> or_io::gta5::CrClipDictionary {
        or_io::gta5::CrClipDictionary::default()
    }

    #[test]
    fn the_swatches_start_with_none_and_all_are_visible_colours() {
        assert_eq!(TINTS[0], ("None", [1.0, 1.0, 1.0]));
        assert!(TINTS
            .iter()
            .all(|(_, c)| c.iter().all(|v| *v > 0.3 && *v <= 1.0)));
    }

    #[test]
    fn a_recook_that_read_a_tab_again_reads_the_packs_files_again_and_nothing_else_does() {
        let mut state = AppState::from_prefs(crate::state::Prefs::default());
        let held = Need::Pack(
            Kind::Model,
            PathBuf::from("a.ydd"),
            Stamp {
                len: 1,
                modified: None,
            },
        );
        let base = Need::Base(Kind::Model, Ped::Male, "uppr_000_r.ydd".to_string());
        let dressed = &mut state.workbench.wardrobe.dressed;
        dressed
            .loaded
            .insert(held.clone(), Loaded::Failed("old".into()));
        dressed
            .loaded
            .insert(base.clone(), Loaded::Failed("old".into()));

        state.poll_dressed();
        assert!(
            state.workbench.wardrobe.dressed.loaded.contains_key(&held),
            "nothing was recooked"
        );

        state.recooked += 1;
        state.poll_dressed();
        let dressed = &state.workbench.wardrobe.dressed;
        assert!(
            !dressed.loaded.contains_key(&held),
            "the pack's file is read again"
        );
        assert!(
            dressed.loaded.contains_key(&base),
            "the install's file is not the cook's to change"
        );
    }
}

//! `openrage --photo-serve`: the pictures of `openrage --photo` for the files a caller names, one at a time, from a process that stays up.
//!
//! `--photo` starts, imports a whole folder into a clothing project and dresses a ped for every texture, which is
//! a minute for twenty clothes. Here the process starts once and a picture is made of a garment's two files alone:
//! the model read once and kept, the texture read once and kept, the same surfaces, look, camera and passes as `--photo`.
//!
//! The protocol is one JSON object a line on stdin and one a line on stdout (the log goes to stderr):
//!
//! ```text
//! {"cmd":"photo","id":7,"ydd":"a.ydd","ytd":"a_diff_000_a.ytd","type":"jbib","gender":"m","size":256,"out":"7.webp","prio":0}
//! {"cmd":"cancel","id":7}   {"cmd":"ping"}   {"cmd":"stats"}   {"cmd":"shutdown"}
//! ```
//!
//! `type` is the word of the slot in the game's file names (`jbib`, `lowr`, `p_head`, `p_lwrist`...), `gender` is `m` or
//! `f`, `ytd` may be left out (the model is drawn with no diffuse). A photo is answered with
//! `{"id":7,"ok":true,"ms":41.2,"stages":{"parse model":3.1,...}}` once `out` is written, or with `{"id":7,"ok":false,"error":"..."}`.
//! Requests are taken highest `prio` first, the newest of those first, and one that is cancelled before its turn is answered `cancelled`.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use or_renderer::game_shaders::mesh::Image;
use or_renderer::MeshVertex;
use or_wardrobe::{Component, Ped, Prop, Slot};
use serde_json::{json, Value};

use crate::photo::{
    decal_picture, framing, garment_camera, neutral_maps, posed, save, shrink, Look, DECAL,
    SUPERSAMPLE,
};
use crate::render::dressed::{Diffuse, DressedScene, Geometry, ScenePart};
use crate::render::viewport::Alpha;
use crate::render::RenderBridge;
use crate::state::{AppState, DocumentKind};

/// How long a picture of a prop waits for the install to give the ped's skeleton.
const BONES_WAIT: Duration = Duration::from_secs(120);

/// A picture asked for.
struct Request {
    id: i64,
    ydd: PathBuf,
    ytd: Option<PathBuf>,
    kind: String,
    gender: char,
    size: u32,
    out: PathBuf,
    prio: i64,
    /// Degrees about the vertical axis the garment is turned from the look of its slot: 180 shows its back.
    yaw: f32,
    /// Orders what arrived at the same `prio`: the newest first.
    seq: u64,
}

/// A file as it was last seen: a cache entry is good while this holds.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// The files read so far, by path, each with how the file was when it was read.
struct Caches {
    models: lru::LruCache<PathBuf, (Stamp, Arc<or_io::Asset>)>,
    textures: lru::LruCache<PathBuf, (Stamp, Arc<Option<Image>>)>,
}

/// A bone of a ped or prop, by where it is worn, to its place.
type BoneMap = BTreeMap<(Ped, &'static str), glam::Mat4>;

/// Where each bone a prop is worn at is, once the install has said.
enum Bones {
    Unasked,
    Reading(Receiver<Result<BoneMap, String>>, Instant),
    Ready(BoneMap),
    Failed(String),
}

struct Service {
    state: AppState,
    bridge: RenderBridge,
    caches: Caches,
    bones: Bones,
    pending: Vec<Request>,
    next_seq: u64,
    taken: u64,
    total: Duration,
    closing: bool,
}

pub fn run(_args: &[String]) -> i32 {
    match serve() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn serve() -> Result<(), String> {
    let scratch = std::env::temp_dir().join(format!("openrage-photo-serve-{}", std::process::id()));
    let started = Instant::now();
    let state = AppState::new();
    // ⚠ Only once the state has read the user's keys and install: anything written is otherwise saved among the user's own.
    std::env::set_var(
        or_title::config::CONFIG_DIR_VARIABLE,
        scratch.join("config"),
    );
    let mut bridge = crate::self_check::headless_bridge()?;
    let pack = state.game_pack.root().to_path_buf();
    if bridge.set_game_pack_path(&pack) {
        // A white sun and a grey ambient: a picture of a garment is of no hour of the day
        bridge.set_game_light(glam::Vec3::ONE, glam::Vec3::splat(0.3));
    } else {
        eprintln!(
            "no pack of the game's shaders at {}: drawn with the viewport's own shading",
            pack.display()
        );
    }
    let capacity = |n| NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN);
    let mut service = Service {
        state,
        bridge,
        caches: Caches {
            models: lru::LruCache::new(capacity(32)),
            textures: lru::LruCache::new(capacity(64)),
        },
        bones: Bones::Unasked,
        pending: Vec::new(),
        next_seq: 0,
        taken: 0,
        total: Duration::ZERO,
        closing: false,
    };
    eprintln!(
        "photo service ready in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    reply(&json!({"cmd": "ready", "ok": true, "boot_ms": ms(started.elapsed())}));

    let (sender, lines) = channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut open = true;
    while !service.closing {
        if service.pending.is_empty() {
            match lines.recv() {
                Ok(line) => service.accept(&line),
                Err(_) => break,
            }
        }
        while open {
            match lines.try_recv() {
                Ok(line) => service.accept(&line),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => open = false,
            }
        }
        if let Some(request) = service.next() {
            service.answer(&request);
        } else if !open {
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    crate::photo_timing::report();
    Ok(())
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e5).round() / 100.0
}

/// One line on stdout, flushed: a caller reads it as it comes.
fn reply(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn slot_of(kind: &str) -> Option<Slot> {
    Component::ALL
        .into_iter()
        .find(|c| c.name() == kind)
        .map(Slot::Component)
        .or_else(|| Prop::from_name(kind).map(Slot::Prop))
}

impl Service {
    fn accept(&mut self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            reply(&json!({"ok": false, "error": "not a JSON line"}));
            return;
        };
        let id = message["id"].as_i64().unwrap_or(0);
        match message["cmd"].as_str().unwrap_or("photo") {
            "ping" => reply(&json!({"cmd": "ping", "ok": true})),
            "stats" => reply(&json!({
                "cmd": "stats", "ok": true, "pictures": self.taken,
                "mean_ms": ms(self.total / u32::try_from(self.taken.max(1)).unwrap_or(1)),
                "queued": self.pending.len(),
            })),
            "shutdown" => self.closing = true,
            // where the bone a prop is worn at is at rest, as 16 floats column by column: { cmd: "bone", gender, prop: "p_head" }
            "bone" => {
                let ped = match message["gender"].as_str() {
                    Some("f") => Ped::Female,
                    _ => Ped::Male,
                };
                let found = Prop::from_name(message["prop"].as_str().unwrap_or(""))
                    .ok_or_else(|| "no such prop".to_string())
                    .and_then(|prop| self.bone(ped, prop.bone()).map(|m| (prop.bone(), m)));
                match found {
                    Ok((bone, m)) => reply(
                        &json!({"id": id, "ok": true, "bone": bone, "matrix": m.to_cols_array()}),
                    ),
                    Err(why) => reply(&json!({"id": id, "ok": false, "error": why})),
                }
            }
            "cancel" => {
                if let Some(at) = self.pending.iter().position(|r| r.id == id) {
                    self.pending.remove(at);
                    reply(&json!({"id": id, "ok": false, "error": "cancelled"}));
                }
            }
            "photo" => match request_of(&message, self.next_seq) {
                Ok(request) => {
                    self.next_seq += 1;
                    self.pending.push(request);
                }
                Err(why) => reply(&json!({"id": id, "ok": false, "error": why})),
            },
            other => reply(&json!({"id": id, "ok": false, "error": format!("no command {other}")})),
        }
    }

    /// The request to take now: the highest `prio`, the newest of those.
    fn next(&mut self) -> Option<Request> {
        let at =
            (0..self.pending.len()).max_by_key(|&i| (self.pending[i].prio, self.pending[i].seq))?;
        Some(self.pending.swap_remove(at))
    }

    fn answer(&mut self, request: &Request) {
        let start = Instant::now();
        let mut stages = Vec::new();
        let made = self.photograph(request, &mut stages).and_then(|image| {
            let started = Instant::now();
            let saved = save(&image, &request.out);
            stages.push(("save", started.elapsed()));
            saved
        });
        let took = start.elapsed();
        self.taken += 1;
        self.total += took;
        let stages: serde_json::Map<String, Value> = stages
            .into_iter()
            .map(|(name, d)| (name.to_string(), json!(ms(d))))
            .collect();
        match made {
            Ok(()) => {
                reply(&json!({"id": request.id, "ok": true, "ms": ms(took), "stages": stages}))
            }
            Err(why) => {
                reply(&json!({"id": request.id, "ok": false, "error": why, "ms": ms(took)}))
            }
        }
    }

    /// The model of `path` parsed, kept while the file stays as it is.
    fn model(&mut self, path: &Path) -> Result<Arc<or_io::Asset>, String> {
        let stamp = Stamp::of(path)?;
        if let Some((seen, asset)) = self.caches.models.get(path) {
            if *seen == stamp {
                return Ok(Arc::clone(asset));
            }
        }
        let bytes = crate::photo_timing::time("40 model: read file", || std::fs::read(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let asset = crate::photo_timing::time("41 model: parse", || {
            self.state.parse_resource(path, DocumentKind::Ydd, &bytes)
        })?;
        let asset = Arc::new(asset);
        self.caches
            .models
            .put(path.to_path_buf(), (stamp, Arc::clone(&asset)));
        Ok(asset)
    }

    /// The first texture of the dictionary at `path` as the pixels the game's passes read, kept while the file stays as it is.
    fn texture(&mut self, path: &Path) -> Result<Arc<Option<Image>>, String> {
        let stamp = Stamp::of(path)?;
        if let Some((seen, image)) = self.caches.textures.get(path) {
            if *seen == stamp {
                return Ok(Arc::clone(image));
            }
        }
        let bytes = crate::photo_timing::time("42 texture: read file", || std::fs::read(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let asset = crate::photo_timing::time("43 texture: parse", || {
            self.state.parse_resource(path, DocumentKind::Ytd, &bytes)
        })?;
        use crate::state::textures::Texture;
        let textures: Vec<Texture> = match &asset {
            or_io::Asset::TextureDict(dict) => (dict.entries.iter().flatten())
                .map(|t| Texture::Owned(std::borrow::Cow::Borrowed(&**t)))
                .collect(),
            or_io::Asset::TextureDictGen9(dict) => (dict.entries.iter().flatten())
                .map(|t| Texture::Enhanced(std::borrow::Cow::Borrowed(&**t)))
                .collect(),
            _ => Vec::new(),
        };
        let image = crate::photo_timing::time("44 texture: decode", || {
            textures
                .iter()
                .find_map(|t| or_renderer::game_shaders::drawable::first_level(t))
        });
        let image = Arc::new(image);
        self.caches
            .textures
            .put(path.to_path_buf(), (stamp, Arc::clone(&image)));
        Ok(image)
    }

    /// Where `bone` of `ped`'s skeleton is at rest: the install is read for it once, on a thread.
    fn bone(&mut self, ped: Ped, bone: &'static str) -> Result<glam::Mat4, String> {
        if matches!(self.bones, Bones::Unasked) {
            self.bones = match self.state.start_prop_bones() {
                Ok(receiver) => Bones::Reading(receiver, Instant::now()),
                Err(why) => Bones::Failed(why),
            };
        }
        if let Bones::Reading(receiver, since) = &self.bones {
            loop {
                match receiver.try_recv() {
                    Ok(Ok(found)) => {
                        self.bones = Bones::Ready(found);
                        break;
                    }
                    Ok(Err(why)) => {
                        self.bones = Bones::Failed(why);
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.bones = Bones::Failed("the install could not be read".into());
                        break;
                    }
                    Err(TryRecvError::Empty) if since.elapsed() > BONES_WAIT => {
                        return Err("the ped's skeleton was not read in time".into());
                    }
                    Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        }
        match &self.bones {
            Bones::Ready(found) => found
                .get(&(ped, bone))
                .copied()
                .ok_or_else(|| format!("{} has no bone {bone}", ped.model())),
            Bones::Failed(why) => Err(why.clone()),
            _ => Err("the ped's skeleton is not read".into()),
        }
    }

    /// The picture of a garment of `request`, as `--photo` draws one: worn at its slot, from where the slot is best seen.
    fn photograph(
        &mut self,
        request: &Request,
        stages: &mut Vec<(&'static str, Duration)>,
    ) -> Result<image::RgbaImage, String> {
        let mut stage = |name: &'static str, since: Instant| stages.push((name, since.elapsed()));
        let slot = slot_of(&request.kind).ok_or_else(|| format!("{} is no slot", request.kind))?;
        let ped = match request.gender {
            'f' => Ped::Female,
            _ => Ped::Male,
        };
        let started = Instant::now();
        let asset = self.model(&request.ydd)?;
        stage("parse model", started);
        let started = Instant::now();
        let image = match &request.ytd {
            Some(path) => Arc::clone(&self.texture(path)?),
            None => Arc::new(None),
        };
        stage("read texture", started);

        let started = Instant::now();
        let mut surfaces = crate::render::game_scene::surfaces_of_asset(&asset, |parameter, _| {
            parameter
                .eq_ignore_ascii_case("DiffuseSampler")
                .then(|| (*image).clone())
                .flatten()
        })
        .ok_or_else(|| format!("{} holds no drawable", request.ydd.display()))?;
        // ⚠ The game sets a ped component's texture on the effect's own `DiffuseSampler` (the dressed ped does too), whatever the model's parameters are called: a model that names it `TextureSamplerDiffPal`, or lists no texture at all, is drawn with its texture all the same
        if let Some(texture) = image.as_ref() {
            for (_, material) in surfaces.iter_mut() {
                material
                    .textures
                    .entry("DiffuseSampler".to_string())
                    .or_insert_with(|| texture.clone());
            }
        }
        if matches!(
            slot,
            Slot::Component(Component::Lowr | Component::Uppr | Component::Hand)
        ) {
            for (_, material) in surfaces.iter_mut() {
                crate::photo::under_skin(material);
            }
        }
        stage("surfaces", started);
        if let Slot::Prop(prop) = slot {
            let started = Instant::now();
            let at = self.bone(ped, prop.bone())?;
            crate::render::game_scene::carried(&mut surfaces, at);
            stage("prop bone", started);
        }

        let started = Instant::now();
        let mut look = Look::of(Some(slot));
        if request.yaw != 0.0 {
            look.rotation = glam::Quat::from_rotation_z(request.yaw.to_radians()) * look.rotation;
        }
        // The pixels the viewport's own shading reads: a decal's picture is its texture, and hair is drawn by it
        let flat = (look.cut || slot == DECAL)
            .then(|| image.as_ref().as_ref())
            .flatten();
        let scene = scene_of(&surfaces, flat);
        if slot == DECAL {
            return decal_picture(&scene, request.size);
        }
        let worn = posed(&scene, "garment", &look);
        let camera = garment_camera(&worn, "garment").ok_or("the garment drew nothing")?;
        stage("scene", started);

        let started = Instant::now();
        let game = match look.cut {
            true => None,
            false => {
                neutral_maps(&mut surfaces);
                crate::render::game_scene::carried(
                    &mut surfaces,
                    glam::Mat4::from_quat(look.rotation),
                );
                Some(surfaces)
            }
        };
        let alpha = match look.cut {
            true => Alpha::Cut,
            false => Alpha::Worn,
        };
        let large = self.bridge.photograph_garment(
            &worn,
            "garment",
            &camera,
            request.size * SUPERSAMPLE,
            alpha,
            game.as_deref(),
        )?;
        stage("draw", started);
        let started = Instant::now();
        let small = shrink(&large, &framing(&large), request.size);
        stage("frame and shrink", started);
        Ok(small)
    }
}

fn request_of(message: &Value, seq: u64) -> Result<Request, String> {
    let text = |key: &str| message[key].as_str().map(str::to_string);
    let need = |key: &str| text(key).ok_or_else(|| format!("\"{key}\" is missing"));
    let size = message["size"].as_u64().unwrap_or(256).clamp(16, 2048) as u32;
    Ok(Request {
        id: message["id"].as_i64().unwrap_or(0),
        ydd: PathBuf::from(need("ydd")?),
        ytd: text("ytd").map(PathBuf::from),
        kind: need("type")?,
        gender: need("gender")?.chars().next().unwrap_or('m'),
        size,
        out: PathBuf::from(need("out")?),
        prio: message["prio"].as_i64().unwrap_or(0),
        yaw: message["yaw"].as_f64().unwrap_or(0.0) as f32,
        seq,
    })
}

/// The id the next picture's diffuse is uploaded under.
static NEXT_DIFFUSE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The meshes of `surfaces` as the viewport draws a garment, wearing `diffuse` where the picture is of the texture.
fn scene_of(
    surfaces: &[(
        or_renderer::game_shaders::mesh::Surface,
        or_renderer::game_shaders::mesh::Material,
    )],
    diffuse: Option<&Image>,
) -> DressedScene {
    let geometry = (surfaces.iter())
        .filter(|(surface, _)| !surface.positions.is_empty() && !surface.indices.is_empty())
        .map(|(surface, _)| {
            let uvs = surface.uvs.get(&0);
            let vertices = (surface.positions.iter().enumerate())
                .map(|(at, p)| {
                    let normal = surface.normals.get(at).copied().unwrap_or([0.0, 0.0, 1.0]);
                    let uv = uvs.and_then(|u| u.get(at)).copied().unwrap_or([0.0; 2]);
                    MeshVertex::new(*p, normal, uv)
                })
                .collect();
            Geometry {
                vertices,
                indices: surface.indices.clone(),
            }
        })
        .collect();
    let part = ScenePart {
        geometry: Arc::new(geometry),
        // ⚠ Marked sRGB for the viewport's own shading, as the Wardrobe's copy is; the game's passes read the texture's own colour space.
        diffuse: diffuse.map(|d| {
            Arc::new(Diffuse {
                // ⚠ A new id for every picture: the bridge keeps an uploaded texture by its id, so a constant one drew every picture after the first of a process with the first one's texture (a hair cut by another hair's strands)
                id: (1 << 40) + NEXT_DIFFUSE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                width: d.width,
                height: d.height,
                rgba: d.rgba.clone(),
                srgb: true,
            })
        }),
        tint: [1.0; 3],
        lift: 0.0,
        garment: Some("garment".to_string()),
    };
    DressedScene::new(0, vec![part])
}

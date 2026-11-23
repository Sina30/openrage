//! `openrage --photo`, which `openrage-cli photo` starts: a square WebP picture of every model in a
//! folder or in the game, and of every texture variation of every garment in a folder worn by the
//! game's freemode ped, drawn by the viewport's renderer with no window.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::sync::Arc;

use crate::render::dressed::{Diffuse, DressedScene, Geometry, ScenePart};
use crate::render::thumbnails::{camera_for, geometry_for, TILE_YAW};
use crate::render::RenderBridge;
use crate::state::front_door::{NewProject, Template};
use crate::state::models::{models_in, ModelListing};
use crate::state::texture_resolution::wanted_dictionaries;
use crate::state::AppState;
use crate::ui::dock::Workspace;
use or_renderer::game_shaders::mesh::{Image, Material as GameMaterial, Surface};
use or_renderer::MeshVertex;

const USAGE: &str = "\
Usage: openrage-cli photo <FOLDER> --out <DIR> [--size <PX>]
       openrage-cli photo --game [--name <GLOB>] --out <DIR> [--size <PX>]

A <PX>-square WebP picture on a transparent background of each .ydr and .yft, named after it, and
of each texture variation of each .ydd garment, alone, as the freemode ped of the open install
wears it, named after the hash of its shop entry (uniqueNameHash) as an unsigned decimal: the one a
shop file in the folder states, else the one a build of the folder would write.

They are drawn with the game's own shaders where the editor has baked them (Settings, or the
OPENRAGE_GAME_PACK folder), else with the viewport's own shading; hair is always the latter.

  <FOLDER>       every model and garment under the folder; a model's .ytd is looked for beside it,
                 and the garments are read as the Wardrobe's Add clothes reads them
  --game         every model of the install OpenRage has open, its DLC included
  --name <GLOB>  with --game, the file names to take (default prop_*)
  --out <DIR>    where the pictures go; a picture already there is kept
  --size <PX>    the side of a picture (default 128)";

/// Each side is drawn this many times larger and averaged down, which is the only antialiasing.
pub(crate) const SUPERSAMPLE: u32 = 4;

const MODEL_EXTENSIONS: [&str; 2] = ["ydr", "yft"];

/// Where a camera sees a freemode ped's front, turned a little to its left: the ped faces down the Y axis.
const FRONT_YAW: f32 = -std::f32::consts::FRAC_PI_2 + 0.4;

/// The horizontal direction the viewport's key light comes from.
///
/// ⚠ The renderer's own, fixed in the world (`the_key_light_is_the_renderers`): a model seen from
/// anywhere else is lit by the head light and the ambient alone, under half as bright.
const KEY_LIGHT: [f32; 2] = [
    or_renderer::viewport::KEY_LIGHT_TOWARDS.x,
    or_renderer::viewport::KEY_LIGHT_TOWARDS.y,
];

/// The yaw every picture is taken from: the key light's, turned so that the light rakes across the model.
pub(crate) fn lit_yaw() -> f32 {
    KEY_LIGHT[1].atan2(KEY_LIGHT[0]) + 0.6
}

/// How long the files of one picture may take to read; the first garment also waits for the install to mount.
const SETTLE: Duration = Duration::from_secs(180);

#[derive(Debug, PartialEq)]
enum Source {
    Folder(PathBuf),
    Game { name: String },
}

#[derive(Debug, PartialEq)]
struct Options {
    source: Source,
    out: PathBuf,
    size: u32,
}

fn options_from(args: &[String]) -> Result<Options, String> {
    let mut folder = None;
    let mut game = false;
    let mut name = None;
    let mut out = None;
    let mut size = 128;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--game" => game = true,
            "--name" => name = Some(value("--name")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--size" => {
                size = value("--size")?
                    .parse()
                    .ok()
                    .filter(|px| (1..=2048).contains(px))
                    .ok_or("--size takes a side from 1 to 2048")?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path if folder.is_none() => folder = Some(PathBuf::from(path)),
            extra => return Err(format!("one folder only, and {extra} is a second")),
        }
    }
    let out = out.ok_or("--out is required")?;
    let source = match (folder, game) {
        (Some(folder), false) if name.is_none() => Source::Folder(folder),
        (Some(_), false) => return Err("--name is for --game; a folder is taken whole".into()),
        (None, true) => Source::Game {
            name: name.unwrap_or_else(|| "prop_*".into()),
        },
        (Some(_), true) => return Err("a folder or --game, not both".into()),
        (None, false) => return Err("name a folder, or pass --game".into()),
    };
    Ok(Options { source, out, size })
}

pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return 0;
    }
    let options = match options_from(args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("Error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let outcome = photograph_all(&options);
    crate::photo_timing::report();
    match outcome {
        Ok(tally) => {
            println!(
                "{} photographed, {} already there, {} failed",
                tally.taken, tally.kept, tally.failed
            );
            i32::from(tally.failed > 0)
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

#[derive(Default)]
struct Tally {
    taken: usize,
    kept: usize,
    failed: usize,
}

impl Tally {
    /// Count what was done for the picture of `what`, saying why it failed where it did.
    fn record(&mut self, what: &str, made: Result<(), String>) {
        match made {
            Ok(()) => self.taken += 1,
            Err(e) => {
                eprintln!("{what}: {e}");
                self.failed += 1;
            }
        }
        if (self.taken + self.failed).is_multiple_of(100) {
            println!("{} done", self.taken + self.kept + self.failed);
        }
    }
}

pub(crate) fn save(image: &image::RgbaImage, target: &Path) -> Result<(), String> {
    image
        .save_with_format(target, image::ImageFormat::WebP)
        .map_err(|e| format!("cannot write {}: {e}", target.display()))
}

/// A folder removed with everything in it when this is dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn photograph_all(options: &Options) -> Result<Tally, String> {
    std::fs::create_dir_all(&options.out)
        .map_err(|e| format!("cannot make {}: {e}", options.out.display()))?;
    let scratch =
        Scratch(std::env::temp_dir().join(format!("openrage-photo-{}", std::process::id())));
    let mut state = crate::photo_timing::time("01 boot: state", AppState::new);
    // ⚠ Only once the state has read the user's keys and install: the scratch project opened for the
    // garments is otherwise saved among their recent projects, and so is anything else written.
    std::env::set_var(
        or_title::config::CONFIG_DIR_VARIABLE,
        scratch.0.join("config"),
    );
    let mut bridge = crate::photo_timing::time("02 boot: gpu", crate::self_check::headless_bridge)?;
    let pack = state.game_pack.root().to_path_buf();
    match crate::photo_timing::time("03 boot: shader pack", || bridge.set_game_pack_path(&pack)) {
        true => {
            println!("drawn with the game's own shaders, from {}", pack.display());
            // A white sun and a grey ambient: a picture of a garment is of no hour of the day
            bridge.set_game_light(glam::Vec3::ONE, glam::Vec3::splat(0.3));
        }
        false => println!(
            "no pack of the game's shaders at {}, which the editor bakes: drawn with the viewport's own shading",
            pack.display()
        ),
    }
    let mut tally = Tally::default();
    let mut shoot = |state: &mut AppState,
                     name: &str,
                     bytes: Option<Vec<u8>>,
                     companion: &dyn Fn(&str) -> Option<Vec<u8>>| {
        let stem = Path::new(name)
            .file_stem()
            .map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned());
        let target = options.out.join(format!("{stem}.webp"));
        if target.exists() {
            tally.kept += 1;
            return;
        }
        let made = bytes
            .ok_or_else(|| "cannot be read".to_string())
            .and_then(|bytes| photograph(state, &mut bridge, name, &bytes, companion, options.size))
            .and_then(|image| save(&image, &target));
        tally.record(name, made);
    };

    match &options.source {
        Source::Folder(folder) => {
            let mut files = Vec::new();
            files_under(folder, &mut files)
                .map_err(|e| format!("cannot read {}: {e}", folder.display()))?;
            let models: Vec<&PathBuf> = files.iter().filter(|p| is_model(p)).collect();
            println!("{} models under {}", models.len(), folder.display());
            for path in models {
                let companion = |file: &str| std::fs::read(path.with_file_name(file)).ok();
                let name = path.to_string_lossy();
                shoot(&mut state, &name, std::fs::read(path).ok(), &companion);
            }
            if files.iter().any(|p| has_extension(p, "ydd")) {
                let project = scratch.0.join("project");
                photograph_clothes(
                    &mut state,
                    &mut bridge,
                    folder,
                    &project,
                    options,
                    &mut tally,
                )?;
            }
        }
        Source::Game { name } => {
            let root = crate::state::gta_install_root_from_prefs(&state.prefs)
                .ok_or("no install is set; open the game in OpenRage first")?;
            if !state.keys.has_keys() {
                return Err("no keys for the install; open the game in OpenRage first".into());
            }
            let install = or_title::GtaInstall::from_path(&root)
                .ok_or_else(|| format!("{} is not an install", root.display()))?;
            let game = or_title::build_mounted_game(
                &install,
                state.keys.provider(),
                or_title::DlcCutoff::All,
            )
            .map_err(|e| format!("cannot mount {}: {e}", root.display()))?;
            let pattern = or_title::vfs::Pattern::new(name);
            // ⚠ One per file name: a DLC ships a model again under the name of the one it replaces.
            let mut models = BTreeMap::new();
            game.for_each_entry(|facts| {
                if is_model(Path::new(&facts.leaf)) && pattern.matches_path(&facts.leaf) {
                    models
                        .entry(facts.leaf.to_ascii_lowercase())
                        .or_insert_with(|| facts.logical.clone());
                }
            });
            println!("{} models named {name} in {}", models.len(), root.display());
            for logical in models.into_values() {
                let folder = logical.rsplit_once('/').map_or("", |(folder, _)| folder);
                let companion = |file: &str| game.vfs.open(&format!("{folder}/{file}"));
                shoot(&mut state, &logical, game.vfs.open(&logical), &companion);
            }
        }
    }
    Ok(tally)
}

/// The model in `bytes` held with the first of its dictionaries `companion` has, drawn, then let go.
fn photograph(
    state: &mut AppState,
    bridge: &mut RenderBridge,
    name: &str,
    bytes: &[u8],
    companion: &dyn Fn(&str) -> Option<Vec<u8>>,
    size: u32,
) -> Result<image::RgbaImage, String> {
    let path = Path::new(name);
    let model = state.hold_bytes(path, bytes)?;
    let mut held = vec![model];
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    for file in wanted_dictionaries(&stem) {
        let Some(bytes) = companion(&file) else {
            continue;
        };
        if let Ok(id) = state.hold_bytes(&path.with_file_name(&file), &bytes) {
            held.push(id);
            break;
        }
    }
    state.active_document = Some(model);
    let drawn = draw(state, bridge, model, size);
    state.let_go(&held);
    drawn
}

fn draw(
    state: &AppState,
    bridge: &mut RenderBridge,
    model: crate::state::DocumentId,
    size: u32,
) -> Result<image::RgbaImage, String> {
    let doc = state.document(model).ok_or("was not held")?;
    let ModelListing::Models(listed) = models_in(&doc.payload) else {
        return Err("holds no model".into());
    };
    let first = listed.first().ok_or("holds no model")?;
    let mut uploads = geometry_for(state, doc, first.selection_id);
    // The model is turned and not the camera: the light is fixed, and the tile's view is the one that sees a prop's front.
    let rotation = glam::Quat::from_rotation_z(lit_yaw() - TILE_YAW);
    for vertex in uploads.iter_mut().flat_map(|u| &mut u.vertices) {
        vertex.position = (rotation * glam::Vec3::from(vertex.position)).into();
        vertex.normal = (rotation * glam::Vec3::from(vertex.normal)).into();
    }
    let mut camera = camera_for(&uploads).ok_or("has no geometry to draw")?;
    camera.yaw = lit_yaw();
    if let Some((centre, radius)) = crate::render::thumbnails::bounding_sphere(&uploads) {
        close_on(&mut camera, centre, radius);
    }
    let mut game = crate::render::game_scene::surfaces_of_active_document(state);
    if let Some(surfaces) = game.as_mut() {
        crate::render::game_scene::carried(surfaces, glam::Mat4::from_quat(rotation));
    }
    let large = bridge.photograph(
        state,
        &uploads,
        first.selection_id,
        &camera,
        size * SUPERSAMPLE,
        game.as_deref(),
    )?;
    Ok(shrink(&large, &framing(&large), size))
}

/// The square, in `large`'s pixels, around what was drawn with [`MARGIN`] on each side; it may reach past the edges.
pub(crate) fn framing(large: &image::RgbaImage) -> Square {
    let drawn = large.enumerate_pixels().filter(|(_, _, p)| p.0[3] > 0);
    let (mut min, mut max) = ((u32::MAX, u32::MAX), (0, 0));
    for (x, y, _) in drawn {
        (min, max) = ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y)));
    }
    if min.0 > max.0 {
        return Square {
            x: 0,
            y: 0,
            side: large.width(),
        };
    }
    let content = (max.0 - min.0 + 1).max(max.1 - min.1 + 1);
    let side = content + 2 * (content as f32 * MARGIN).ceil() as u32;
    let centre = |lo: u32, hi: u32| (lo + hi + 1) as i64 / 2;
    Square {
        x: centre(min.0, max.0) - i64::from(side / 2),
        y: centre(min.1, max.1) - i64::from(side / 2),
        side,
    }
}

/// What is left clear around a model, as a share of its larger side.
const MARGIN: f32 = 0.04;

pub(crate) struct Square {
    x: i64,
    y: i64,
    side: u32,
}

/// `square` of `large` averaged down to `size` on a side, weighted by alpha so the clear colour does not darken an edge.
///
/// ⚠ A pixel of the square past `large`'s edge counts as clear, so it lowers alpha and leaves colour alone.
pub(crate) fn shrink(large: &image::RgbaImage, square: &Square, size: u32) -> image::RgbaImage {
    let side = u64::from(square.side);
    let span = |i: u32| {
        let lo = u64::from(i) * side / u64::from(size);
        let hi = (u64::from(i + 1) * side / u64::from(size)).max(lo + 1);
        (lo as i64, hi as i64)
    };
    image::RgbaImage::from_fn(size, size, |x, y| {
        let ((x0, x1), (y0, y1)) = (span(x), span(y));
        let mut sum = [0u64; 4];
        for sy in square.y + y0..square.y + y1 {
            for sx in square.x + x0..square.x + x1 {
                let inside = (0..i64::from(large.width())).contains(&sx)
                    && (0..i64::from(large.height())).contains(&sy);
                if !inside {
                    continue;
                }
                let [r, g, b, a] = large.get_pixel(sx as u32, sy as u32).0;
                let a = u64::from(a);
                for (total, channel) in sum.iter_mut().zip([r, g, b]) {
                    *total += u64::from(channel) * a;
                }
                sum[3] += a;
            }
        }
        if sum[3] == 0 {
            return image::Rgba([0; 4]);
        }
        let colour = |c: u64| (c / sum[3]) as u8;
        let area = ((x1 - x0) * (y1 - y0)) as u64;
        image::Rgba([
            colour(sum[0]),
            colour(sum[1]),
            colour(sum[2]),
            (sum[3] / area) as u8,
        ])
    })
}

/// Each texture variation of each garment under `folder`, added to a clothing project made at `project`, worn by the ped the Wardrobe dresses and drawn without it.
fn photograph_clothes(
    state: &mut AppState,
    bridge: &mut RenderBridge,
    folder: &Path,
    project: &Path,
    options: &Options,
    tally: &mut Tally,
) -> Result<(), String> {
    std::fs::create_dir_all(project).map_err(|e| format!("{}: {e}", project.display()))?;
    let mut draft = NewProject::new(Template::Clothing, project.to_path_buf());
    draft.set_name("photo".into());
    crate::photo_timing::time("10 clothing project", || -> Result<(), String> {
        let created = draft.create()?;
        state.load_project_from(created.file);
        settle(state, "the clothing project", |s| s.workbench.is_scanned())
    })?;
    crate::photo_timing::time("11 add clothes", || {
        state.add_clothes(vec![folder.to_path_buf()]);
        settle(state, "adding the clothes", |s| {
            s.wardrobe_is_idle() && s.workbench.scan_is_current()
        })
    })?;
    let module = state
        .wardrobe_module_id()
        .ok_or("the clothing project has no clothing")?;
    let garments: Vec<(String, Option<or_wardrobe::Slot>, Vec<String>)> = state
        .wardrobe_pack()
        .and_then(|pack| pack.read.as_ref().ok())
        .map(|read| {
            let hashes = shop_hashes(&read.wardrobe);
            (read.wardrobe.garments.iter().enumerate())
                .filter(|(_, g)| g.model.is_some())
                .map(|(at, g)| {
                    let names = (picture_names(g).into_iter().enumerate())
                        .map(|(texture, name)| hashes.get(&(at, texture)).cloned().unwrap_or(name))
                        .collect();
                    (g.id.clone(), g.slot, names)
                })
                .collect()
        })
        .unwrap_or_default();
    let variations: usize = garments.iter().map(|(_, _, names)| names.len()).sum();
    println!(
        "{} garments under {}, {variations} variations",
        garments.len(),
        folder.display()
    );
    // The normal and specular maps a garment names, which are no variation's.
    let mut files = Vec::new();
    files_under(folder, &mut files)
        .map_err(|e| format!("cannot read {}: {e}", folder.display()))?;
    files.retain(|p| {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase());
        has_extension(p, "ytd") && !stem.is_some_and(|s| s.contains("_diff_"))
    });
    let beside = state.dictionary_textures(&files);
    state.active_workspace = Workspace::Wardrobe;
    for (id, slot, names) in garments {
        let look = Look::of(slot);
        state.select_garment(&module, Some(id.clone()));
        for (variation, name) in names.iter().enumerate() {
            let target = options.out.join(format!("{name}.webp"));
            if target.exists() {
                tally.kept += 1;
                continue;
            }
            (state.workbench.wardrobe.dressed.variation).insert(id.clone(), variation);
            // ⚠ A prop too waits for the ped: it is placed by the skeleton the same mount reads.
            let settled = crate::photo_timing::time("20 dressed ped", || {
                settle(state, "the dressed ped", AppState::dressed_is_settled)
            });
            if slot == Some(DECAL) {
                let made = (state.garment_alone(&id, variation))
                    .and_then(|alone| decal_picture(&alone, options.size))
                    .and_then(|picture| save(&picture, &target));
                tally.record(name, made);
                continue;
            }
            let prop = matches!(slot, Some(or_wardrobe::Slot::Prop(_)));
            let worn = settled.and_then(|()| match prop {
                true => {
                    (state.garment_alone(&id, variation)).map(|alone| posed(&alone, &id, &look))
                }
                false => {
                    let scene = state.workbench.wardrobe.dressed.scene();
                    crate::photo_timing::time("21 posed", || {
                        scene.map(|scene| posed(scene, &id, &look))
                    })
                    .ok_or_else(|| "the ped was not drawn".to_string())
                }
            });
            let made = worn
                .and_then(|worn| {
                    let camera =
                        crate::photo_timing::time("22 camera", || garment_camera(&worn, &id))
                            .ok_or("the garment drew nothing")?;
                    let px = options.size * SUPERSAMPLE;
                    // ⚠ Hair is coloured by the tint a ped is given, which nothing here sets: it drew black.
                    let mut game = match look.cut {
                        true => None,
                        false => crate::photo_timing::time("23 surfaces", || {
                            state.garment_surfaces(&id, variation, &beside).ok()
                        }),
                    };
                    if let Some(surfaces) = game.as_mut() {
                        neutral_maps(surfaces);
                        crate::render::game_scene::carried(
                            surfaces,
                            glam::Mat4::from_quat(look.rotation),
                        );
                    }
                    let alpha = match look.cut {
                        true => crate::render::viewport::Alpha::Cut,
                        false => crate::render::viewport::Alpha::Worn,
                    };
                    crate::photo_timing::time("24 photograph garment", || {
                        bridge.photograph_garment(&worn, &id, &camera, px, alpha, game.as_deref())
                    })
                })
                .and_then(|large| {
                    let small = crate::photo_timing::time("25 frame and shrink", || {
                        shrink(&large, &framing(&large), options.size)
                    });
                    crate::photo_timing::time("26 save webp", || save(&small, &target))
                });
            tally.record(name, made);
        }
    }
    Ok(())
}

/// How a garment of a slot is shown.
pub(crate) struct Look {
    /// Turns the ped's space so that the lit camera sees the garment from where it is best seen.
    pub(crate) rotation: glam::Quat,
    /// The texture's alpha cuts the garment out, at a half; elsewhere the texture is made opaque.
    ///
    /// ⚠ Hair is a solid cap under shells of strands, and the strands are only where the alpha is:
    /// made opaque, the shells were a low blob over the hair.
    pub(crate) cut: bool,
}

impl Look {
    pub(crate) fn of(slot: Option<or_wardrobe::Slot>) -> Self {
        use or_wardrobe::{Component, Prop, Slot};
        use std::f32::consts::{FRAC_PI_4, PI};
        // The yaw, in the ped's space, of where the garment is seen from; the ped faces down Y, so its left is +X.
        let seen_from = match slot {
            // ⚠ A watch's face is on the outside of the wrist, so a wrist is seen three-quarter from its side.
            Some(Slot::Prop(Prop::LeftWrist)) => -FRAC_PI_4,
            Some(Slot::Prop(Prop::RightWrist)) => -PI + FRAC_PI_4,
            _ => FRONT_YAW,
        };
        Self {
            rotation: glam::Quat::from_rotation_z(lit_yaw() - seen_from),
            cut: slot == Some(Slot::Component(Component::Hair)),
        }
    }
}

/// Garment `id` of `scene` turned `turn` radians about the vertical from where its slot is best seen, as a `size`-pixel square picture on a clear ground, drawn with the viewport's own shading.
///
/// ⚠ The model is turned and not the camera, as for every picture here: the light is fixed in the world.
pub(crate) fn picture_of_garment(
    bridge: &mut RenderBridge,
    scene: &DressedScene,
    id: &str,
    slot: Option<or_wardrobe::Slot>,
    size: u32,
    turn: f32,
) -> Result<image::RgbaImage, String> {
    let mut look = Look::of(slot);
    look.rotation = glam::Quat::from_rotation_z(turn) * look.rotation;
    let worn = posed(scene, id, &look);
    let camera = garment_camera(&worn, id).ok_or("the garment drew nothing")?;
    let alpha = match look.cut {
        true => crate::render::viewport::Alpha::Cut,
        false => crate::render::viewport::Alpha::Worn,
    };
    let large = bridge.photograph_garment(&worn, id, &camera, size * SUPERSAMPLE, alpha, None)?;
    Ok(shrink(&large, &framing(&large), size))
}

/// The parts garment `id` wears in `scene`, as `look` shows them, each texture made opaque or cut out.
///
/// ⚠ A garment's diffuse alpha is not how clear the cloth is: a shirt the game draws solid states 16
/// to 152 in every texel, and drawn as translucent it showed its inside through its front.
pub(crate) fn posed(scene: &DressedScene, id: &str, look: &Look) -> DressedScene {
    let rotation = look.rotation;
    let parts = (scene.parts.iter())
        .filter(|part| part.garment.as_deref() == Some(id))
        .map(|part| ScenePart {
            geometry: Arc::new(
                (part.geometry.iter())
                    .map(|geometry| Geometry {
                        vertices: (geometry.vertices.iter())
                            .map(|vertex| MeshVertex {
                                position: (rotation * glam::Vec3::from(vertex.position)).into(),
                                normal: (rotation * glam::Vec3::from(vertex.normal)).into(),
                                ..*vertex
                            })
                            .collect(),
                        indices: geometry.indices.clone(),
                    })
                    .collect(),
            ),
            diffuse: part.diffuse.as_ref().map(|d| {
                let mut rgba = d.rgba.clone();
                for texel in rgba.as_chunks_mut::<4>().0 {
                    texel[3] = match look.cut && texel[3] < 128 {
                        true => 0,
                        false => u8::MAX,
                    };
                }
                Arc::new(Diffuse {
                    // ⚠ The bridge keeps an uploaded texture by its id, and these are not the pixels it was given under the scene's.
                    id: d.id | OPAQUE_ID,
                    width: d.width,
                    height: d.height,
                    rgba,
                    srgb: d.srgb,
                })
            }),
            tint: part.tint,
            lift: part.lift,
            garment: part.garment.clone(),
        })
        .collect();
    DressedScene::new(scene.key, parts)
}

/// A flat normal map and a dull specular map for a material that has none.
///
/// ⚠ What the game's passes are given for a texture they lack is a texel of nought, which as a normal
/// map tips every normal aside: a garment of a pack that ships no normal map was dark and streaked.
pub(crate) fn neutral_maps(surfaces: &mut [(Surface, GameMaterial)]) {
    let texel = |rgba: [u8; 4]| Image {
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
        srgb: false,
    };
    for (_, material) in surfaces {
        let textures = &mut material.textures;
        textures
            .entry("BumpSampler".into())
            .or_insert_with(|| texel([128, 128, 255, 255]));
        textures
            .entry("SpecSampler".into())
            .or_insert_with(|| texel([48, 48, 48, 255]));
    }
}

/// The decal slot, whose picture is its texture.
///
/// ⚠ A decal's model is a flat card whose texture coordinates all name one texel: the game lays the
/// texture on the torso, and the card drawn alone was one flat colour.
pub(crate) const DECAL: or_wardrobe::Slot =
    or_wardrobe::Slot::Component(or_wardrobe::Component::Decl);

/// The texture of the decal `alone` wears, cropped to what it shows and fitted to `size` on a side.
pub(crate) fn decal_picture(alone: &DressedScene, size: u32) -> Result<image::RgbaImage, String> {
    let diffuse = (alone.parts.iter())
        .find_map(|part| part.diffuse.as_ref())
        .ok_or("has no texture")?;
    let shape = mask(diffuse);
    let d = shape.as_ref().unwrap_or(diffuse);
    let texture = image::RgbaImage::from_raw(d.width, d.height, d.rgba.clone())
        .ok_or("its texture is shorter than its size")?;
    Ok(shrink(&texture, &framing(&texture), size))
}

/// A texture decoded from alpha alone, as a white shape whose alpha is that alpha; none for one that holds colour.
///
/// ⚠ The decoder shows an alpha-only (A8) texture as grey and opaque, which is the right view of it
/// as a picture and loses the shape it is to a decal.
fn mask(d: &Diffuse) -> Option<Diffuse> {
    let texels = d.rgba.as_chunks::<4>().0;
    if !texels
        .iter()
        .all(|[r, g, b, a]| r == g && g == b && *a == u8::MAX)
    {
        return None;
    }
    Some(Diffuse {
        id: d.id | OPAQUE_ID,
        width: d.width,
        height: d.height,
        rgba: texels
            .iter()
            .flat_map(|[r, ..]| [u8::MAX, u8::MAX, u8::MAX, *r])
            .collect(),
        srgb: d.srgb,
    })
}

/// Set in the id of a texture [`posed`] made, which the Wardrobe's own counter never reaches.
const OPAQUE_ID: u64 = 1 << 63;

/// The hash of the shop entry a build writes for each texture, by garment and texture, as an unsigned decimal.
///
/// ⚠ The id a shop file stated where the folder held one, else the name the build gives: the hash is
/// the game's of that text, as the build writes it, and what a shop's script is keyed on.
fn shop_hashes(wardrobe: &or_wardrobe::Wardrobe) -> BTreeMap<(usize, usize), String> {
    let allocation = or_wardrobe::alloc::allocate(wardrobe);
    (or_wardrobe::shop::written(wardrobe, &allocation).into_iter())
        .filter_map(|w| {
            let hash = or_wardrobe::metadata::hashed(&w.id).hash;
            Some(((w.garment?, w.texture), hash.to_string()))
        })
        .collect()
}

/// One name for the picture of each variation where the build writes no shop entry for it: its `.ytd`'s, else the model's with the variation's letter; the model's alone for a garment with none, which is drawn once without a texture.
fn picture_names(garment: &or_wardrobe::Garment) -> Vec<String> {
    let stem = |path: &str| {
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    };
    let model = garment.model.as_deref().and_then(stem).unwrap_or_default();
    if garment.variations.is_empty() {
        return vec![model];
    }
    (garment.variations.iter().enumerate())
        .map(|(at, variation)| {
            (variation.texture.as_deref().and_then(stem))
                .unwrap_or_else(|| format!("{model}_{}", (b'a' + (at % 26) as u8) as char))
        })
        .collect()
}

/// The view from [`lit_yaw`], placed on what garment `id` drew.
pub(crate) fn garment_camera(scene: &DressedScene, id: &str) -> Option<or_renderer::OrbitCamera> {
    let mut min = glam::Vec3::splat(f32::INFINITY);
    let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
    let parts = (scene.parts.iter()).filter(|part| part.garment.as_deref() == Some(id));
    for part in parts {
        for vertex in part.geometry.iter().flat_map(|g| &g.vertices) {
            let p = glam::Vec3::from(vertex.position) + glam::Vec3::Z * part.lift;
            if p.is_finite() {
                (min, max) = (min.min(p), max.max(p));
            }
        }
    }
    if min.x > max.x {
        return None;
    }
    let mut camera = or_renderer::OrbitCamera {
        yaw: lit_yaw(),
        pitch: std::f32::consts::PI / 12.0,
        ..Default::default()
    };
    close_on(&mut camera, (min + max) * 0.5, ((max - min) * 0.5).length());
    Some(camera)
}

/// Place `camera` so that the sphere at `centre` of `radius` fills the view, however small it is.
///
/// ⚠ Not `frame_sphere`, which frames ten centimetres at the least: a ring a centimetre across was a
/// few pixels of the target, and cropped to them it was blocks. The near plane comes in with the
/// camera, which would otherwise cut a ring seen from three centimetres.
fn close_on(camera: &mut or_renderer::OrbitCamera, centre: glam::Vec3, radius: f32) {
    let radius = radius.max(1e-4);
    camera.target = centre;
    camera.distance = 1.4 * radius / (camera.fov_y * 0.5).tan();
    camera.near = (camera.distance - radius) * 0.5;
    camera.far = camera.distance + radius * 4.0;
}

/// Poll the workbench until `done` holds, which the files it reads on workers make true.
fn settle(
    state: &mut AppState,
    what: &str,
    done: impl Fn(&AppState) -> bool,
) -> Result<(), String> {
    let until = Instant::now() + SETTLE;
    loop {
        state.poll_workbench();
        if done(state) {
            return Ok(());
        }
        if Instant::now() > until {
            return Err(format!("{what} was not ready in {} s", SETTLE.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(wanted))
}

fn is_model(path: &Path) -> bool {
    MODEL_EXTENSIONS.iter().any(|m| has_extension(path, m))
}

fn files_under(folder: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_under(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// The skin a body component's empty picture is filled with: the mean of the stock legs' skin (a pale freemode skin).
const SKIN: [u8; 3] = [158, 125, 104];

/// The wearer's skin where the picture of a body component (legs, arms, hands) is empty.
///
/// ⚠ A pack leaves the skin part of such a picture transparent (alpha 0), for the game to fill with the
/// wearer's skin tone, which nothing here is given: the legs of the drawable drew black. A texel that is
/// not opaque is mixed with the skin by its alpha, and made opaque. A material that cuts or blends
/// by that alpha (lace, hair, decals) keeps its alpha.
pub(crate) fn under_skin(material: &mut GameMaterial) {
    // ⚠ A material in the cutout (or blend) bucket draws only where its texture's alpha keeps it, in the game as here: the transparent texels of a torn hem show what is behind (the legs), they are
    // not filled with skin (a flat skin-coloured skirt)
    if material.coverage != or_renderer::game_shaders::mesh::Coverage::Opaque {
        return;
    }
    let effect = material.effect.to_ascii_lowercase();
    if ["alpha", "decal", "hair", "fur", "cutout"]
        .iter()
        .any(|word| effect.contains(word))
    {
        return;
    }
    let Some(diffuse) = material.textures.get_mut("DiffuseSampler") else {
        return;
    };
    for texel in diffuse.rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(texel[3]);
        if alpha == 255 {
            continue;
        }
        for (channel, skin) in texel[..3].iter_mut().zip(SKIN) {
            *channel =
                ((u32::from(*channel) * alpha + u32::from(skin) * (255 - alpha)) / 255) as u8;
        }
        texel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        options_from(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_folder_or_the_game_is_photographed_at_128_unless_asked() {
        assert_eq!(
            parse(&["props", "--out", "pics"]),
            Ok(Options {
                source: Source::Folder("props".into()),
                out: "pics".into(),
                size: 128,
            })
        );
        assert_eq!(
            parse(&["--game", "--out", "pics", "--size", "64"]),
            Ok(Options {
                source: Source::Game {
                    name: "prop_*".into()
                },
                out: "pics".into(),
                size: 64,
            })
        );
    }

    #[test]
    fn what_cannot_be_photographed_is_a_usage_error() {
        assert!(parse(&["props"]).is_err(), "no --out");
        assert!(parse(&["--out", "pics"]).is_err(), "nothing to photograph");
        assert!(parse(&["props", "--game", "--out", "pics"]).is_err());
        assert!(parse(&["props", "--name", "x*", "--out", "pics"]).is_err());
        assert!(parse(&["props", "--out", "pics", "--size", "0"]).is_err());
        assert!(parse(&["props", "more", "--out", "pics"]).is_err());
    }

    #[test]
    fn shrinking_averages_what_was_drawn_and_ignores_the_clear_colour() {
        let mut large = image::RgbaImage::new(2, 2);
        large.put_pixel(0, 0, image::Rgba([200, 100, 50, 255]));
        let whole = Square {
            x: 0,
            y: 0,
            side: 2,
        };
        assert_eq!(
            shrink(&large, &whole, 1).get_pixel(0, 0).0,
            [200, 100, 50, 63],
            "a quarter covered keeps its colour and a quarter of the alpha"
        );
        let blank = image::RgbaImage::new(2, 2);
        assert_eq!(shrink(&blank, &whole, 1).get_pixel(0, 0).0, [0; 4]);
    }

    #[test]
    fn the_framing_is_square_around_what_was_drawn() {
        let mut large = image::RgbaImage::new(100, 100);
        for x in 20..70 {
            for y in 40..50 {
                large.put_pixel(x, y, image::Rgba([255; 4]));
            }
        }
        let square = framing(&large);
        assert_eq!(square.side, 50 + 2 * 2, "the wider side and its margins");
        assert_eq!((square.x, square.y), (45 - 27, 45 - 27), "centred on it");
        let tile = shrink(&large, &square, 54);
        assert_eq!(
            tile.get_pixel(2, 27).0[3],
            255,
            "the left end reaches the margin"
        );
        assert_eq!(tile.get_pixel(1, 27).0[3], 0, "and the margin is clear");
    }

    #[test]
    fn nothing_drawn_frames_the_whole_target() {
        let square = framing(&image::RgbaImage::new(8, 8));
        assert_eq!((square.x, square.y, square.side), (0, 0, 8));
    }

    #[test]
    fn a_variation_is_named_after_its_texture_else_its_model_and_letter() {
        let garment = or_wardrobe::Garment {
            model: Some("male/jbib/pack^jbib_000_u.ydd".into()),
            variations: vec![
                or_wardrobe::Variation {
                    texture: Some("male/jbib/pack^jbib_diff_000_a_uni.ytd".into()),
                    ..Default::default()
                },
                or_wardrobe::Variation::default(),
            ],
            ..Default::default()
        };
        assert_eq!(
            picture_names(&garment),
            ["pack^jbib_diff_000_a_uni", "pack^jbib_000_u_b"]
        );
        let bare = or_wardrobe::Garment {
            variations: Vec::new(),
            ..garment
        };
        assert_eq!(picture_names(&bare), ["pack^jbib_000_u"], "drawn once");
    }

    #[test]
    fn the_camera_looks_at_the_garment_and_not_the_ped_around_it() {
        let part = |z: f32, garment: Option<&str>| ScenePart {
            geometry: std::sync::Arc::new(vec![Geometry {
                vertices: vec![
                    or_renderer::MeshVertex::new([-0.1, 0.0, z], [0.0, 0.0, 1.0], [0.0; 2]),
                    or_renderer::MeshVertex::new([0.1, 0.0, z + 0.2], [0.0, 0.0, 1.0], [0.0; 2]),
                ],
                indices: vec![0, 1],
            }]),
            diffuse: None,
            tint: [1.0; 3],
            lift: 0.0,
            garment: garment.map(str::to_string),
        };
        let scene = DressedScene::new(0, vec![part(1.5, None), part(0.0, Some("shoes"))]);
        let camera = garment_camera(&scene, "shoes").expect("the shoes drew");
        assert!((camera.target - glam::Vec3::new(0.0, 0.0, 0.1)).length() < 1e-5);
        assert_eq!(camera.yaw, lit_yaw(), "from where the key light is");
        assert!(garment_camera(&scene, "hat").is_none());
    }

    #[test]
    fn only_the_garment_is_kept_and_its_texture_is_made_opaque() {
        let part = |garment: Option<&str>| ScenePart {
            geometry: Arc::new(Vec::new()),
            diffuse: Some(Arc::new(Diffuse {
                id: 7,
                width: 1,
                height: 1,
                rgba: vec![10, 20, 30, 40],
                srgb: true,
            })),
            tint: [1.0; 3],
            lift: 0.0,
            garment: garment.map(str::to_string),
        };
        let scene = DressedScene::new(0, vec![part(None), part(Some("shirt"))]);
        let plain = Look {
            rotation: glam::Quat::IDENTITY,
            cut: false,
        };
        let worn = posed(&scene, "shirt", &plain);
        assert_eq!(worn.parts.len(), 1, "the ped's own part is left out");
        let diffuse = worn.parts[0].diffuse.as_ref().expect("a texture");
        assert_eq!(diffuse.rgba, [10, 20, 30, 255]);
        assert_ne!(
            diffuse.id, 7,
            "not the id the scene's pixels were uploaded under"
        );
    }

    #[test]
    fn a_turn_brings_the_peds_front_to_the_lit_side() {
        let vertex = |p: [f32; 3]| MeshVertex::new(p, [0.0, -1.0, 0.0], [0.0; 2]);
        let scene = DressedScene::new(
            0,
            vec![ScenePart {
                geometry: Arc::new(vec![Geometry {
                    vertices: vec![vertex([0.0, -1.0, 0.0])],
                    indices: vec![0],
                }]),
                diffuse: None,
                tint: [1.0; 3],
                lift: 0.0,
                garment: Some("shirt".into()),
            }],
        );
        let worn = posed(&scene, "shirt", &Look::of(None));
        let front = glam::Vec3::from(worn.parts[0].geometry[0].vertices[0].normal);
        let toward = |yaw: f32| glam::Vec3::new(yaw.cos(), yaw.sin(), 0.0);
        let before = glam::Vec3::NEG_Y.dot(toward(FRONT_YAW));
        assert!(before > 0.9, "the unturned camera stood in front");
        assert!(
            (front.dot(toward(lit_yaw())) - before).abs() < 1e-5,
            "the camera stands to the front as it did before the turn"
        );
    }

    #[test]
    fn the_key_light_is_the_renderers() {
        // The pictures are lit from the light the viewport began with, which its shader reads from the camera uniform now that the Wardrobe can set it.
        let shader = include_str!("../crates/runtime/or-renderer/src/viewport.rs");
        assert!(
            shader.contains("let key_dir = camera.key_light.xyz;"),
            "the viewport's shader no longer lights by its key light, and the pictures are lit from where it was"
        );
        let first = or_renderer::viewport::KEY_LIGHT_TOWARDS;
        assert_eq!(KEY_LIGHT, [first.x, first.y]);
    }

    #[test]
    fn a_decal_decoded_as_grey_is_drawn_as_its_shape_and_one_in_colour_is_kept() {
        let diffuse = |rgba: Vec<u8>| Diffuse {
            id: 3,
            width: 2,
            height: 1,
            rgba,
            srgb: false,
        };
        let shape =
            mask(&diffuse(vec![0, 0, 0, 255, 200, 200, 200, 255])).expect("grey and opaque");
        assert_eq!(shape.rgba, [255, 255, 255, 0, 255, 255, 255, 200]);
        assert!(
            mask(&diffuse(vec![10, 20, 30, 255, 0, 0, 0, 255])).is_none(),
            "colour"
        );
        assert!(
            mask(&diffuse(vec![9, 9, 9, 128, 0, 0, 0, 255])).is_none(),
            "an alpha of its own"
        );
    }

    #[test]
    fn a_decal_is_pictured_as_its_texture_cropped_to_what_it_shows() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        for at in [5, 6, 9, 10] {
            rgba[at * 4..at * 4 + 4].copy_from_slice(&[200, 10, 10, 255]);
        }
        let alone = DressedScene::new(
            0,
            vec![ScenePart {
                geometry: Arc::new(Vec::new()),
                diffuse: Some(Arc::new(Diffuse {
                    id: 1,
                    width: 4,
                    height: 4,
                    rgba,
                    srgb: true,
                })),
                tint: [1.0; 3],
                lift: 0.0,
                garment: Some("logo".into()),
            }],
        );
        let picture = decal_picture(&alone, 4).expect("a texture");
        assert_eq!(
            picture.get_pixel(2, 2).0,
            [200, 10, 10, 255],
            "the logo fills the middle"
        );
        assert_eq!(picture.get_pixel(0, 0).0[3], 0, "and the margin is clear");
    }

    #[test]
    fn a_ring_a_centimetre_across_fills_the_view_and_is_not_cut_by_the_near_plane() {
        let mut camera = or_renderer::OrbitCamera::default();
        close_on(&mut camera, glam::Vec3::ZERO, 0.005);
        let edge = camera.distance * (camera.fov_y * 0.5).tan();
        assert!(edge < 0.01, "the view is {edge} m across at the ring");
        assert!(
            camera.near < camera.distance - 0.005,
            "the near plane is in front of it"
        );
        assert!(
            camera.far > camera.distance + 0.005,
            "and the far plane behind it"
        );
    }

    #[test]
    fn a_picture_is_named_by_the_hash_of_the_shop_entry_stated_or_built() {
        use or_wardrobe::{Component, Garment, Ped, Slot, Variation, Wardrobe};
        let mut wardrobe = Wardrobe::new("shirts");
        let mut shirt = Garment::new();
        shirt.ped = Some(Ped::Male);
        shirt.slot = Some(Slot::Component(Component::Jbib));
        shirt.model = Some("jbib_000_u.ydd".into());
        shirt.variations = vec![
            Variation {
                shop_id: Some("CLO_SHIRT_RED".into()),
                ..Variation::default()
            },
            Variation::default(),
        ];
        wardrobe.garments.push(shirt);
        let hashes = shop_hashes(&wardrobe);
        let hash = |name: &str| or_io::hash::string_hash_str(name).to_string();
        assert_eq!(
            hashes[&(0, 0)],
            hash("CLO_SHIRT_RED"),
            "the id somebody chose"
        );
        assert_eq!(
            hashes[&(0, 1)],
            hash("MP_M_SHIRTS_JBIB_000_1"),
            "the name the build gives"
        );
    }
}

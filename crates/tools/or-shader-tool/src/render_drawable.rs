//! A drawable of an install drawn with the game's own passes from a SPIR-V pack: the scene its file extracts to, as the surfaces and materials the renderer draws.

use std::path::Path;

use or_io::codec::texture::TextureSource;
use or_io::container::resource::{ResourceEnvelope, ResourceType, Rsc7};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::GrcTextureG9;
use or_io::gta5::{File as _, PgDictionary};
use or_title::{build_mounted_game, DlcCutoff, GtaInstall, MountedGame};

use or_interchange::{ExtractOptions, LodSelection};
use or_renderer::game_shaders::drawable::{carried, fragment_wheels};
use or_renderer::game_shaders::frame::Rendered;
use or_renderer::game_shaders::mesh::{
    bounds, render_surfaces, Camera, Material, MeshError, MeshRender, Surface,
};
use or_renderer::game_shaders::pack::GamePack;

pub mod census;

pub use or_renderer::game_shaders::drawable::{first_levels, surfaces_of, Textures};

/// The textures of a `.ytd`, read at the layout its RSC7 version names.
pub fn dictionary_textures(bytes: &[u8]) -> Result<Textures, String> {
    let enhanced = Rsc7::peek(bytes)
        .ok()
        .and_then(|seal| ResourceType::from_extension("ytd")?.generation_of_version(seal.version()))
        .is_some_and(|generation| generation.is_gen9());
    let refuse = |e: or_io::Error| format!("the texture dictionary: {e}");
    if enhanced {
        let dictionary = PgDictionary::<GrcTextureG9>::from_bytes(bytes).map_err(refuse)?;
        Ok(first_levels(
            dictionary
                .entries
                .iter()
                .flatten()
                .map(|t| (t.name.to_string(), &**t as &dyn TextureSource)),
        ))
    } else {
        let dictionary = PgDictionary::<GrcTexture>::from_bytes(bytes).map_err(refuse)?;
        Ok(first_levels(
            dictionary
                .entries
                .iter()
                .flatten()
                .map(|t| (t.name.to_string(), &**t as &dyn TextureSource)),
        ))
    }
}

/// A vehicle's wheels at its wheel bones, as the surfaces Shaded draws them; none for a file that is no fragment.
pub fn wheel_surfaces(bytes: &[u8], beside: &Textures) -> Vec<(Surface, Material)> {
    use or_io::decl::{Gen8, Gen9};
    use or_io::gta5::GtaFragType;
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: true,
    };
    let enhanced = Rsc7::peek(bytes)
        .ok()
        .and_then(|seal| ResourceType::from_extension("yft")?.generation_of_version(seal.version()))
        .is_some_and(|generation| generation.is_gen9());
    let wheels = if enhanced {
        GtaFragType::<Gen9>::from_bytes(bytes)
            .ok()
            .map(|f| fragment_wheels(&f.base, &options))
    } else {
        GtaFragType::<Gen8>::from_bytes(bytes)
            .ok()
            .map(|f| fragment_wheels(&f.base, &options))
    };
    let mut out = Vec::new();
    for (scene, at) in wheels.unwrap_or_default() {
        let mut surfaces = surfaces_of(&scene, beside);
        carried(&mut surfaces, at);
        out.extend(surfaces);
    }
    out
}

/// A drawable file and the textures that sit beside it.
pub struct Found {
    /// The path the file has in the install, or on disk.
    pub name: String,
    pub bytes: Vec<u8>,
    /// The `.ytd` of the same name in the same folder or archive, which is where a drawable that does not carry its textures keeps them.
    pub beside: Textures,
}

fn beside_of(bytes: Option<Vec<u8>>) -> Textures {
    bytes
        .and_then(|b| dictionary_textures(&b).ok())
        .unwrap_or_default()
}

/// An install, mounted as the game mounts it, whose drawables can be found and read by path.
pub struct Install {
    game: MountedGame,
    /// What every vehicle shares, read the first time a fragment is.
    shared_vehicle: std::sync::OnceLock<Textures>,
}

impl Install {
    pub fn open(root: &Path) -> Result<Self, String> {
        let install = GtaInstall::from_path(root)
            .ok_or_else(|| format!("{} is not an install root", root.display()))?;
        let crypto = crate::defs::install_crypto(root).map_err(|e| e.to_string())?;
        // The names of a material's constants come from the install's own effects, as the app reads them: without them every constant a material sets goes by a hash, and none of them applies
        if let Ok(load) = or_title::install::load_shader_definitions(
            &install,
            std::sync::Arc::clone(&crypto),
            "gta5",
            "gen8",
        ) {
            or_io::gta5::shaders::shader_defs::install(load.definitions);
        }
        let game = build_mounted_game(&install, crypto, DlcCutoff::All)
            .map_err(|e| format!("mount failed: {e}"))?;
        Ok(Self {
            game,
            shared_vehicle: std::sync::OnceLock::new(),
        })
    }

    /// The path of the file called `name`; the last of those so named is the one the game's mount order serves.
    pub fn find(&self, name: &str) -> Option<String> {
        let mut found = None;
        self.game.for_each_entry(|entry| {
            if entry.leaf.eq_ignore_ascii_case(name) {
                found = Some(entry.logical.clone());
            }
        });
        found
    }

    /// The path of every file with one of `extensions`, in mount order.
    pub fn paths_with(&self, extensions: &[&str]) -> Vec<String> {
        let mut paths = Vec::new();
        self.game.for_each_entry(|entry| {
            let extension = Path::new(&entry.leaf).extension().and_then(|e| e.to_str());
            if extension.is_some_and(|e| extensions.iter().any(|w| e.eq_ignore_ascii_case(w))) {
                paths.push(entry.logical.clone());
            }
        });
        paths
    }

    /// The generic textures every vehicle names (the mud, the detail, the glass), from the dictionaries they are shared through, the first of those holding a name serving it.
    ///
    /// ⚠ Found by name in the mount order, as the game's own parent of every vehicle is, and not beside the vehicle: a DLC that adds a vehicle need not carry them.
    fn shared_vehicle_textures(&self) -> &Textures {
        self.shared_vehicle.get_or_init(|| {
            let mut shared = Textures::new();
            for dictionary in [
                "vehshare",
                "vehshare_worn",
                "vehshare_truck",
                "vehshare_army",
            ] {
                let found = self.find(&format!("{dictionary}.ytd"));
                for (name, image) in beside_of(found.and_then(|p| self.game.vfs.open(&p))) {
                    shared.entry(name).or_insert(image);
                }
            }
            shared
        })
    }

    /// The file at `path`, and the `.ytd` beside it.
    pub fn read(&self, path: &str) -> Result<Found, String> {
        let bytes = self
            .game
            .vfs
            .open(path)
            .ok_or_else(|| format!("cannot read {path}"))?;
        let mut beside = beside_of(
            self.game
                .vfs
                .open(&Path::new(path).with_extension("ytd").to_string_lossy()),
        );
        if path.to_ascii_lowercase().ends_with(".yft") {
            for (name, image) in self.shared_vehicle_textures() {
                beside.entry(name.clone()).or_insert_with(|| image.clone());
            }
        }
        Ok(Found {
            name: path.to_string(),
            bytes,
            beside,
        })
    }
}

/// The drawable `source` names: a loose file, or an install whose archives hold one called `name`.
pub fn read_drawable(source: &Path, name: Option<&str>) -> Result<Found, String> {
    let Some(name) = name else {
        let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
        let beside = beside_of(std::fs::read(source.with_extension("ytd")).ok());
        return Ok(Found {
            name: source.display().to_string(),
            bytes,
            beside,
        });
    };
    let install = Install::open(source)?;
    let path = install
        .find(name)
        .ok_or_else(|| format!("no file of the install is called {name}"))?;
    install.read(&path)
}

/// What drawing a drawable did.
pub struct Drawn {
    pub render: MeshRender,
    /// The effect each surface draws with, in the order of the drawable's primitives.
    pub effects: Vec<String>,
    pub vertices: usize,
}

/// A material names its effect by hash, and the pack's own effects are the names that hash back.
fn install_effect_names(pack: &GamePack) {
    or_io::gta5::shaders::shader_names::install(
        pack.manifest()
            .effects
            .iter()
            .map(|e| (or_io::hash::string_hash_str(&e.name), e.name.clone())),
    );
}

/// Draw the drawable `found` with the passes of `pack` into a square picture of `size` pixels on `device`.
pub fn render_drawable(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    found: &Found,
    size: u32,
) -> Result<Drawn, String> {
    let (name, bytes) = (found.name.as_str(), found.bytes.as_slice());
    install_effect_names(pack);
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: true,
    };
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("drawable");
    let exported = or_cli::export_scene(name, bytes, stem, None, &options)?;
    let surfaces = surfaces_of(&exported.scene, &found.beside);
    let (min, max) = bounds(&surfaces).ok_or_else(|| format!("{name} has no vertex to draw"))?;
    let render = render_surfaces(
        device,
        queue,
        pack,
        &surfaces,
        &Camera::framing(min, max),
        size,
    )
    .map_err(|e| e.to_string())?;
    Ok(Drawn {
        vertices: exported.scene.vertex_count(),
        effects: surfaces.iter().map(|(_, m)| m.effect.clone()).collect(),
        render,
    })
}

/// What to change before a drawable is drawn as Shaded draws it, to see what one input does.
#[derive(Debug, Clone, Default)]
pub struct ShadedOptions {
    /// Draw only the surfaces of this effect.
    pub only_effect: Option<String>,
    /// A texture every surface binds under this parameter, as one texel.
    pub textures: Vec<(String, [u8; 4])>,
    /// A constant every surface sets under this name, as one row of four.
    pub vectors: Vec<(String, [f32; 4])>,
    /// Lay a ground under the drawable.
    pub ground: bool,
    /// The way towards the sun of a day of noon, in place of the light over the camera's shoulder.
    pub sun: Option<[f32; 3]>,
    /// Draw this many more frames of the same picture and report what each costs.
    pub frames: u32,
}

/// The colour the picture of a shaded drawable is cleared to, which is not drawn on.
const SHADED_BACKGROUND: [f32; 4] = [0.1, 0.2, 0.3, 1.0];

/// Draw the drawable `found` as the viewport's Shaded button does, through the game's deferred passes of `pack`, into `renderer`, which is `size` pixels square.
///
/// With `mask`, the picture holds zero wherever the background shows, so that `touched` counts what the drawable covers; without it the picture is as drawn, in sRGB.
pub fn render_drawable_shaded(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &GamePack,
    renderer: &mut or_renderer::viewport::ViewportRenderer,
    found: &Found,
    size: u32,
    mask: bool,
    shaded: &ShadedOptions,
) -> Result<Drawn, String> {
    use or_renderer::viewport::{OrbitCamera, ShadingMode};
    let (name, bytes) = (found.name.as_str(), found.bytes.as_slice());
    install_effect_names(pack);
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: true,
    };
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("drawable");
    let exported = or_cli::export_scene(name, bytes, stem, None, &options)?;
    let mut every = surfaces_of(&exported.scene, &found.beside);
    every.extend(wheel_surfaces(bytes, &found.beside));
    let mut surfaces: Vec<_> = every
        .into_iter()
        .filter(|(_, m)| {
            shaded
                .only_effect
                .as_deref()
                .is_none_or(|only| m.effect == only)
        })
        .collect();
    for (_, material) in &mut surfaces {
        for (name, row) in &shaded.vectors {
            material.vectors.push((name.clone(), vec![*row]));
        }
        for (name, rgba) in &shaded.textures {
            material.textures.insert(
                name.clone(),
                or_renderer::game_shaders::mesh::Image {
                    width: 1,
                    height: 1,
                    rgba: rgba.to_vec(),
                    srgb: false,
                },
            );
        }
    }
    let (min, max) = bounds(&surfaces).ok_or_else(|| format!("{name} has no vertex to draw"))?;
    if shaded.ground {
        or_renderer::game_shaders::drawable::lay_ground_under(&mut surfaces);
    }
    renderer.set_background(SHADED_BACKGROUND);
    if let Some([x, y, z]) = shaded.sun {
        // The day of noon with the sun turned to a direction of its own
        let mut keyframe = or_io::gta5::data::timecycle::Keyframe::default();
        for (name, value) in [
            ("light_direction_override", 1.0),
            ("sun_direction_x", x),
            ("sun_direction_y", y),
            ("sun_direction_z", z),
        ] {
            keyframe.set(name, value);
        }
        renderer.set_day(Some(or_renderer::game_shaders::lighting::Day {
            keyframe,
            hours: 12.0,
            ..Default::default()
        }));
    }
    let (drawn, left_out) = renderer.prepare_deferred(device, queue, pack, &surfaces)?;
    let mut camera = OrbitCamera::default();
    camera.frame_sphere((min + max) * 0.5, (max - min).length() * 0.5);
    camera.yaw = 0.6;
    camera.pitch = 0.3;
    if shaded.frames > 0 {
        let mut encode = std::time::Duration::ZERO;
        let mut stages: Vec<(&'static str, f32, u32)> = Vec::new();
        let started = std::time::Instant::now();
        for _ in 0..shaded.frames {
            let at = std::time::Instant::now();
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.render(&mut encoder, queue, &camera, ShadingMode::Shaded);
            queue.submit([encoder.finish()]);
            encode += at.elapsed();
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|e| e.to_string())?;
            if renderer.collect_gpu_times(device) {
                for (label, ms) in renderer.gpu_stages() {
                    match stages.iter_mut().find(|s| s.0 == label) {
                        Some(s) => (s.1, s.2) = (s.1 + ms, s.2 + 1),
                        None => stages.push((label, ms, 1)),
                    }
                }
            }
        }
        let n = f64::from(shaded.frames);
        eprintln!(
            "frames: {n} drawn, {:.3} ms each, of which {:.3} ms to encode and submit",
            started.elapsed().as_secs_f64() * 1e3 / n,
            encode.as_secs_f64() * 1e3 / n
        );
        // A frame the timer was still reading is not timed, so each stage is averaged over the frames that were
        for (label, total, times) in &stages {
            eprintln!("  gpu {label:<18} {:.3} ms", total / *times as f32);
        }
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render(&mut encoder, queue, &camera, ShadingMode::Shaded);
    let row = (size * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("census-shaded-readback"),
        size: u64::from(row * size),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        renderer.colour_texture().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size),
            },
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    let mapped = buffer.slice(..).get_mapped_range();
    // sRGB of the background, as the target stores it
    let background: Vec<u8> = SHADED_BACKGROUND
        .iter()
        .map(|c| srgb_of((c * 255.0).round() as u8))
        .collect();
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for line in mapped.chunks_exact(row as usize).take(size as usize) {
        for pixel in line[..(size * 4) as usize].as_chunks::<4>().0 {
            let shows = pixel
                .iter()
                .zip(&background)
                .any(|(got, want)| got.abs_diff(*want) > 2);
            pixels.extend_from_slice(if shows || !mask { pixel } else { &[0; 4] });
        }
    }
    let skipped = left_out
        .into_iter()
        .map(|(effect, why)| {
            let error = if why.starts_with("the pack has no effect named") {
                MeshError::NoEffect(effect.clone())
            } else if why.ends_with("has no pass this draws") {
                MeshError::NoPass(effect.clone())
            } else {
                MeshError::Pipeline(why)
            };
            (effect, error)
        })
        .collect();
    Ok(Drawn {
        vertices: exported.scene.vertex_count(),
        effects: surfaces.iter().map(|(_, m)| m.effect.clone()).collect(),
        render: MeshRender {
            rendered: Rendered {
                size,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                bytes: pixels,
            },
            drawn,
            skipped,
        },
    })
}

/// The sRGB encoding of a linear 8-bit channel.
fn srgb_of(linear: u8) -> u8 {
    // The straight segment of the curve ends below one step of eight bits, and a zero encodes below zero and clamps to it
    let e = 1.055 * (f32::from(linear) / 255.0).powf(1.0 / 2.4) - 0.055;
    (e * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Write a render, whose colours are linear, as a PNG, which holds them in sRGB.
pub fn write_png(rendered: Rendered, path: &Path) -> Result<(), String> {
    let mut bytes = rendered.bytes;
    for pixel in bytes.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            *channel = srgb_of(*channel);
        }
    }
    write_encoded_png(rendered.size, bytes, path)
}

/// Write pixels that are in sRGB already, as the viewport's target holds them, as a PNG.
pub fn write_encoded_png(size: u32, bytes: Vec<u8>, path: &Path) -> Result<(), String> {
    image::RgbaImage::from_raw(size, size, bytes)
        .ok_or_else(|| "the render is not four bytes a pixel".to_string())?
        .save(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Cells across a contact sheet.
const SHEET_COLUMNS: usize = 8;
/// Cells down a contact sheet.
const SHEET_ROWS: usize = 6;

/// Lay `pictures` out in contact sheets of eight by six cells, row by row, as `sheet_N.png` in `dir` with a `sheet_N.txt` that names each cell's drawable; `encoded` says they hold sRGB already.
pub fn write_sheets(
    pictures: &[(String, Rendered)],
    encoded: bool,
    dir: &Path,
) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let per_sheet = SHEET_COLUMNS * SHEET_ROWS;
    for (number, group) in pictures.chunks(per_sheet).enumerate() {
        let size = group.first().map_or(1, |(_, r)| r.size) as usize;
        let mut sheet =
            image::RgbaImage::new((SHEET_COLUMNS * size) as u32, (SHEET_ROWS * size) as u32);
        let mut names = String::new();
        for (cell, (path, rendered)) in group.iter().enumerate() {
            let mut bytes = rendered.bytes.clone();
            if !encoded {
                for pixel in bytes.chunks_exact_mut(4) {
                    for channel in &mut pixel[..3] {
                        *channel = srgb_of(*channel);
                    }
                }
            }
            let Some(tile) = image::RgbaImage::from_raw(rendered.size, rendered.size, bytes) else {
                continue;
            };
            let (x, y) = ((cell % SHEET_COLUMNS) * size, (cell / SHEET_COLUMNS) * size);
            image::imageops::replace(&mut sheet, &tile, x as i64, y as i64);
            names.push_str(&format!("{cell:>2}  {path}\n"));
        }
        let png = dir.join(format!("sheet_{number}.png"));
        sheet
            .save(&png)
            .map_err(|e| format!("{}: {e}", png.display()))?;
        let list = dir.join(format!("sheet_{number}.txt"));
        std::fs::write(&list, names).map_err(|e| format!("{}: {e}", list.display()))?;
    }
    Ok(pictures.len().div_ceil(per_sheet))
}

#[cfg(test)]
mod tests;

/// The texel `--texture NAME=r,g,b,a` names: a whole number of 0 to 255 for each channel it states, nought for those left out.
pub fn texel_of(numbers: &[f64]) -> Result<[u8; 4], String> {
    let mut texel = [0u8; 4];
    for (slot, n) in texel.iter_mut().zip(numbers) {
        if n.fract() != 0.0 || !(0.0..=255.0).contains(n) {
            return Err(format!(
                "a texel channel is a whole number of 0 to 255, not {n}"
            ));
        }
        *slot = *n as u8;
    }
    Ok(texel)
}

#[cfg(test)]
mod texel_tests {
    use super::texel_of;

    #[test]
    fn a_texel_takes_whole_bytes_and_refuses_what_a_cast_would_make_nought() {
        assert_eq!(texel_of(&[200.0, 100.0]), Ok([200, 100, 0, 0]));
        assert_eq!(texel_of(&[255.0, 0.0, 7.0, 255.0]), Ok([255, 0, 7, 255]));
        // 0.8 was meant as a fraction and was silently nought, which drew everything black
        assert!(texel_of(&[0.8, 0.8, 0.8, 1.0]).is_err());
        assert!(texel_of(&[256.0]).is_err());
        assert!(texel_of(&[-1.0]).is_err());
    }
}

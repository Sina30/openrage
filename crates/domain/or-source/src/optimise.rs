//! Optimising a textures source: the format, the mips and the size of each texture, written as the exact DDS it becomes.
//!
//! A texture that needs nothing is not touched, so its DDS stays byte for byte. One that does is rebuilt in the cheapest way
//! that keeps what the artist made: a missing chain of mips is added under the blocks of the level that is there, a texture
//! halved to a power of two it already holds mips for loses its top levels and keeps the rest, and only a change of size or of
//! format is encoded afresh. What is written is a DDS, so the compile copies it block for block.
//!
//! ⚠ A texture made from a picture, or from the channels of pictures, has its pixels in the picture and not in a DDS: it is
//! left as it is and said so, since the compile already stores it as BC1 or BC3 by its alpha, with its mips.
//! ⚠ [`apply`] writes into the folder it is given. A caller that must keep the folder as it was runs it on a copy.

use std::collections::BTreeMap;
use std::path::Path;

use or_io::authoring::{automatic_format, encode_level, is_encodable, EncodeQuality};
use or_io::codec::texture::{
    decode_texture_data, generate_mip_chain, max_mip_levels, resize_rgba, to_dds, GrcTextureFormat,
    MipFilter, TextureSource,
};
use serde::Serialize;

use crate::asset::Source;
use crate::json::canonical;
use crate::textures::{
    compile, is_texture_file, mip_levels, surface_of_dds, Mips, Rules, Surface, DIR, KIND, RULES,
};
use crate::Problem;

/// How a texture's size comes down.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Scale {
    #[default]
    Keep,
    /// Each side is multiplied by this, a number above 0 and below 1.
    Ratio(f32),
    /// No side is longer than this, with the shape kept.
    Side(u32),
}

/// What an optimisation may change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// A texture stored uncompressed, and one stored as BC7 where the platform cannot read it, is stored as BC1 when it is opaque and BC3 when it is not.
    pub format: bool,
    /// A texture of more than 4 pixels on a side that stores a single mip is given its chain.
    pub mips: bool,
    /// A side that is not a power of two goes to the nearest one.
    pub power_of_two: bool,
    pub scale: Scale,
    /// The pack is built for Legacy, whose DirectX 10 cards read no BC7.
    pub legacy: bool,
    pub quality: EncodeQuality,
}

impl Options {
    /// What changes no picture's size on purpose: the format, the mips and the power of two.
    pub fn safe(legacy: bool) -> Self {
        Self {
            format: true,
            mips: true,
            power_of_two: true,
            scale: Scale::Keep,
            legacy,
            quality: EncodeQuality::Normal,
        }
    }

    /// Why these options cannot be carried out, none where they can.
    pub fn refusal(&self) -> Option<String> {
        match self.scale {
            Scale::Ratio(r) if !(r.is_finite() && r > 0.0 && r < 1.0) => {
                Some(format!("a ratio is above 0 and below 1, and {r} is not"))
            }
            Scale::Side(n) if n < 4 => {
                Some(format!("a side of {n} pixels is below the 4 of one block"))
            }
            _ => None,
        }
    }
}

/// Whether a DirectX 10 graphics card reads `format`: BC7 and BC6 need DirectX 11.
pub fn readable_on_directx_10(format: GrcTextureFormat) -> bool {
    !matches!(format, GrcTextureFormat::BC7 | GrcTextureFormat::BC6)
}

/// A texture as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Look {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// The engine's name for the format, or `DXT1 or DXT5` where the pixels decide.
    pub format: String,
}

impl Look {
    fn of(width: u32, height: u32, mips: u32, format: &str) -> Self {
        Self {
            width,
            height,
            mips,
            format: format.to_string(),
        }
    }
}

impl std::fmt::Display for Look {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = if self.mips == 1 { "" } else { "s" };
        write!(
            f,
            "{} by {} {} with {} mip{s}",
            self.width, self.height, self.format, self.mips
        )
    }
}

/// How a texture is rebuilt, which says how much of what is stored is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// The blocks that are there are kept, and the mips under them are added.
    Mips,
    /// The top `n` levels are dropped and the others kept as they are.
    Halved(u32),
    /// Decoded, resized where the size changes, and encoded afresh.
    Rebuilt,
}

/// One texture the options change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub texture: String,
    pub was: Look,
    pub now: Look,
    pub how: How,
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kept = match self.how {
            How::Mips => "; the blocks it had are kept",
            How::Halved(_) => "; the levels it keeps are as they were",
            How::Rebuilt => "",
        };
        write!(
            f,
            "{}: {} becomes {}{kept}",
            self.texture, self.was, self.now
        )
    }
}

/// A texture the options leave as it is, with the reason where that is not because it needs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub texture: String,
    pub why: String,
}

/// What an optimisation changes in a textures source, and what it leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub left: Vec<Left>,
}

/// Where a texture's DDS is and what it needs, before any pixel is read.
struct Work {
    file: String,
    change: Change,
    target: Target,
}

struct Target {
    width: u32,
    height: u32,
    mips: u32,
    /// The format stays, or BC1 and BC3 are chosen from the pixels.
    by_alpha: bool,
}

/// The nearest power of two to `n` in the ratio between them, the lower one where they are as near.
fn nearest_power_of_two(n: u32) -> u32 {
    if n <= 1 || n.is_power_of_two() {
        return n.max(1);
    }
    let up = n.next_power_of_two();
    let down = up / 2;
    if u64::from(n) * u64::from(n) < u64::from(down) * u64::from(up) {
        down
    } else {
        up
    }
}

/// The size `scale` and the power of two bring `width` by `height` to.
fn sized(width: u32, height: u32, options: &Options) -> (u32, u32) {
    let by = |factor: f64| {
        let side = |n: u32| ((f64::from(n) * factor).round() as u32).max(1);
        (side(width), side(height))
    };
    let (mut w, mut h) = match options.scale {
        Scale::Keep => (width, height),
        Scale::Ratio(r) => by(f64::from(r)),
        Scale::Side(n) if width.max(height) > n => by(f64::from(n) / f64::from(width.max(height))),
        Scale::Side(_) => (width, height),
    };
    if options.power_of_two {
        (w, h) = (nearest_power_of_two(w), nearest_power_of_two(h));
    }
    if let Scale::Side(n) = options.scale {
        while w.max(h) > n && w.max(h) > 1 {
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
        }
    }
    (w, h)
}

/// What the options make of the texture stored as `surface`, none where it needs nothing.
fn target(surface: &Surface, options: &Options) -> Option<(Target, How)> {
    let (width, height, old) = (surface.width, surface.height, surface.mips);
    let by_alpha = options.format
        && (matches!(
            surface.format,
            GrcTextureFormat::A8R8G8B8 | GrcTextureFormat::A8B8G8R8 | GrcTextureFormat::X8R8G8B8
        ) || (options.legacy && !readable_on_directx_10(surface.format)));
    let (w, h) = sized(width, height, options);
    let same_size = (w, h) == (width, height);
    let wants_mips = options.mips && old < 2 && w.max(h) > 4;

    if same_size && !by_alpha {
        if !wants_mips {
            return None;
        }
        let mips = mip_levels(Mips::To4x4, w, h).max(old);
        return (mips > old).then_some((
            Target {
                width: w,
                height: h,
                mips,
                by_alpha,
            },
            How::Mips,
        ));
    }
    // Halving a texture that stores the levels below its top leaves those levels as they are.
    let halvings = (1..old)
        .find(|k| (width >> k, height >> k) == (w, h) && w << k == width && h << k == height);
    if let (Some(k), false) = (halvings, by_alpha) {
        let kept = old - k;
        if !(options.mips && kept < 2 && w.max(h) > 4) {
            return Some((
                Target {
                    width: w,
                    height: h,
                    mips: kept,
                    by_alpha,
                },
                How::Halved(k),
            ));
        }
    }
    let mips = if old > 1 && old >= max_mip_levels(width, height) {
        max_mip_levels(w, h)
    } else if old >= 2 || options.mips {
        mip_levels(Mips::To4x4, w, h)
    } else {
        1
    };
    Some((
        Target {
            width: w,
            height: h,
            mips,
            by_alpha,
        },
        How::Rebuilt,
    ))
}

impl TextureSource for Surface {
    fn pixel_format(&self) -> GrcTextureFormat {
        self.format
    }
    fn is_srgb(&self) -> bool {
        self.srgb
    }
    fn stored_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn stored_levels(&self) -> u32 {
        self.mips
    }
    fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// What a folder of a textures source holds that an optimisation reads: its rules, and the files of `textures/` by name.
pub(crate) struct Held {
    pub(crate) rules: Rules,
    /// Every texture file's name.
    files: Vec<String>,
}

fn read(dir: &Path) -> Result<Held, Vec<Problem>> {
    let source = Source::open(dir).map_err(|p| vec![p])?;
    if source.manifest.kind != KIND {
        return Err(vec![Problem::new(format!(
            "this is a {} source, and only a textures source is optimised",
            source.manifest.kind
        ))]);
    }
    let text = std::fs::read_to_string(dir.join(RULES))
        .map_err(|e| vec![Problem::new(format!("cannot read: {e}")).in_file(RULES)])?;
    let rules = serde_json::from_str::<Rules>(&text)
        .map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
    let listing = std::fs::read_dir(dir.join(DIR))
        .map_err(|e| vec![Problem::new(format!("cannot list {DIR}/: {e}")).in_file(DIR)])?;
    let mut files: Vec<String> = listing
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| is_texture_file(name))
        .collect();
    files.sort();
    Ok(Held { rules, files })
}

fn is_dds(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
}

fn stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().to_lowercase())
}

/// The textures `options` would change in the source at `dir`, and those it leaves with a reason; nothing is written.
pub fn plan(dir: &Path, options: &Options) -> Result<Plan, Vec<Problem>> {
    analyse(dir, options).map(|(plan, _, _)| plan)
}

/// What a texture of a source is made from, as far as an optimisation is concerned.
pub(crate) enum Made {
    /// A DDS and nothing else: its blocks are the texture.
    Dds { shown: String, file: String },
    /// A picture, or the pictures of a pack, which the compile stores as BC1 or BC3 by its alpha with its mips.
    Pictures { shown: String, why: &'static str },
}

/// Each texture of the source at `dir` that an optimisation or a budget looks at, in the order of its lower-cased name, and the rules and files it read.
pub(crate) fn classify(dir: &Path) -> Result<(Vec<Made>, Held), Vec<Problem>> {
    let held = read(dir)?;
    // A file an entry names as its source or a channel, and every picture, is not the DDS of a texture.
    let mut by_stem: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for file in &held.files {
        by_stem.entry(stem(file)).or_default().push(file);
    }
    // A file an entry names as its source or as a channel of its pack is that entry's input, and no texture of its own where no entry has its name.
    let claimed: Vec<&str> = (held.rules.textures.iter())
        .flat_map(|entry| {
            let channels = entry.pack.iter().flat_map(|pack| {
                [&pack.r, &pack.g, &pack.b, &pack.a]
                    .into_iter()
                    .flatten()
                    .map(|taken| taken.file.as_str())
            });
            entry.source.as_deref().into_iter().chain(channels)
        })
        .collect();
    let mut made = Vec::new();
    for (name, files) in &by_stem {
        let entry = held
            .rules
            .textures
            .iter()
            .find(|e| e.name.to_lowercase() == *name);
        if entry.is_none() && files.iter().all(|f| claimed.contains(f)) {
            continue;
        }
        let Some(dds) = files.iter().find(|f| is_dds(f)) else {
            made.push(Made::Pictures {
                shown: name.clone(),
                why: "it is made from a picture, which the compile stores as BC1 or BC3 by its alpha with its mips; change the picture to change its size",
            });
            continue;
        };
        let shown = entry.map_or_else(
            || dds.trim_end_matches(".dds").to_string(),
            |e| e.name.clone(),
        );
        if files.len() > 1 || entry.is_some_and(|e| e.source.is_some() || e.pack.is_some()) {
            made.push(Made::Pictures {
                shown,
                why: "its pixels come from a picture beside the DDS, which the compile stores as BC1 or BC3 by its alpha with its mips",
            });
            continue;
        }
        made.push(Made::Dds {
            shown,
            file: (*dds).to_string(),
        });
    }
    // A texture made of the channels of pictures has no file of its own name.
    for entry in &held.rules.textures {
        if !by_stem.contains_key(&entry.name.to_lowercase()) {
            made.push(Made::Pictures {
                shown: entry.name.clone(),
                why: "it is made from pictures, which the compile stores as BC1 or BC3 by its alpha with its mips",
            });
        }
    }
    Ok((made, held))
}

fn analyse(dir: &Path, options: &Options) -> Result<(Plan, Vec<Work>, Held), Vec<Problem>> {
    if let Some(why) = options.refusal() {
        return Err(vec![Problem::new(why)]);
    }
    let (made, held) = classify(dir)?;
    let mut plan = Plan::default();
    let mut work = Vec::new();
    for one in made {
        let (shown, dds) = match one {
            Made::Pictures { shown, why } => {
                plan.left.push(Left {
                    texture: shown,
                    why: why.into(),
                });
                continue;
            }
            Made::Dds { shown, file } => (shown, file),
        };
        let bytes = std::fs::read(dir.join(DIR).join(&dds)).map_err(|e| {
            vec![Problem::new(format!("cannot read: {e}")).in_file(format!("{DIR}/{dds}"))]
        })?;
        let surface = match surface_of_dds(&dds, &bytes) {
            Ok(surface) => surface,
            Err(why) => {
                plan.left.push(Left {
                    texture: shown,
                    why,
                });
                continue;
            }
        };
        drop(bytes);
        let Some((target, how)) = target(&surface, options) else {
            continue;
        };
        let format = if target.by_alpha {
            "DXT1 or DXT5".to_string()
        } else {
            format!("{:?}", surface.format)
        };
        // ⚠ A format a DirectX 10 card cannot read is never kept or written in a pack built for Legacy by this, so a texture in one is left as it is, with the way to have it stored as BC1 or BC3.
        if options.legacy && !target.by_alpha && !readable_on_directx_10(surface.format) {
            plan.left.push(Left {
                texture: shown,
                why: format!(
                    "it is stored as {:?}, which a DirectX 10 card cannot read, and none is written in a pack built for Legacy: ask for BC1 or BC3 by alpha to store it as one of those",
                    surface.format
                ),
            });
            continue;
        }
        // Levels that are kept are not encoded, so only a texture that is made afresh needs a format that can be.
        if !target.by_alpha && !matches!(how, How::Halved(_)) && !is_encodable(surface.format) {
            plan.left.push(Left {
                texture: shown,
                why: format!(
                    "it is stored as {:?}, which cannot be encoded again here",
                    surface.format
                ),
            });
            continue;
        }
        let change = Change {
            texture: shown,
            was: Look::of(
                surface.width,
                surface.height,
                surface.mips,
                &format!("{:?}", surface.format),
            ),
            now: Look::of(target.width, target.height, target.mips, &format),
            how,
        };
        plan.changes.push(change.clone());
        work.push(Work {
            file: dds,
            change,
            target,
        });
    }
    Ok((plan, work, held))
}

/// `payload` of `format` as the surface the texture becomes at `target`, with the levels the change keeps taken from `surface`.
fn rebuilt(surface: &Surface, work: &Work, options: &Options) -> Result<Surface, String> {
    let (width, height, format) = (surface.width, surface.height, surface.format);
    let target = &work.target;
    let first = format.mip_level_range(width, height, 0);
    match work.change.how {
        How::Halved(k) => {
            let from = format.mip_level_range(width, height, k).start;
            let size = format.total_size(target.width, target.height, target.mips) as usize;
            let kept = surface
                .payload
                .get(from..from + size)
                .ok_or("the levels it keeps are not all in the file")?;
            return Ok(Surface {
                format,
                srgb: surface.srgb,
                width: target.width,
                height: target.height,
                mips: target.mips,
                payload: kept.to_vec(),
                exact: false,
            });
        }
        How::Mips | How::Rebuilt => {}
    }
    let image = decode_texture_data(format, width, height, &surface.payload[first.clone()])
        .map_err(|e| format!("cannot be decoded: {e}"))?;
    let chosen = match target.by_alpha {
        true => automatic_format(&image),
        false => format,
    };
    let pixels = match (target.width, target.height) == (width, height) {
        true => image.data,
        false => resize_rgba(
            &image.data,
            (width, height),
            (target.width, target.height),
            MipFilter::Lanczos3,
            surface.srgb,
        )
        .map_err(|e| e.to_string())?,
    };
    let chain = generate_mip_chain(
        target.width,
        target.height,
        &pixels,
        Some(target.mips),
        MipFilter::Box,
        surface.srgb,
    )
    .map_err(|e| e.to_string())?;
    // The levels that were stored come across as they were, then the ones under them.
    let (mut payload, kept) = match work.change.how {
        How::Mips => (surface.payload.clone(), surface.mips as usize),
        _ => (Vec::new(), 0),
    };
    for (level, rgba) in chain.iter().enumerate().skip(kept) {
        let (w, h) = chosen.mip_dimensions(target.width, target.height, level as u32);
        let bytes = encode_level(chosen, w, h, rgba, options.quality).map_err(|e| e.to_string())?;
        payload.extend_from_slice(&bytes);
    }
    Ok(Surface {
        format: chosen,
        srgb: surface.srgb,
        width: target.width,
        height: target.height,
        mips: target.mips,
        payload,
        exact: false,
    })
}

/// Bring a rule that names a format or a count of mips the DDS of `texture` no longer has in line with `made`, since a DDS is copied as it is; whether one changed.
pub(crate) fn align_rules(rules: &mut Rules, texture: &str, made: &Surface) -> bool {
    let name = texture.to_lowercase();
    let mut changed = false;
    if let Some(encode) = rules
        .textures
        .iter_mut()
        .find(|e| e.name.to_lowercase() == name)
        .and_then(|e| e.encode.as_mut())
    {
        if let Some(stated) = &encode.format {
            if GrcTextureFormat::from_name(stated) != Some(made.format) {
                encode.format = Some(format!("{:?}", made.format));
                changed = true;
            }
        }
        if encode
            .mips
            .is_some_and(|m| mip_levels(m, made.width, made.height) != made.mips)
        {
            encode.mips = None;
            changed = true;
        }
    }
    changed
}

/// Carry out what [`plan`] says in the source at `dir`, and give what was done; `stop` is asked between textures and ends it with [`Problem`] and no more written.
///
/// Only the DDS of a texture that changes is written, and a rule that would now contradict it is brought in line (a format it
/// states, a count of mips it states). The source is compiled once at the end, so a result that does not compile is an error.
pub fn apply(dir: &Path, options: &Options, stop: &dyn Fn() -> bool) -> Result<Plan, Vec<Problem>> {
    let (mut plan, work, mut held) = analyse(dir, options)?;
    let mut rules_changed = false;
    let mut done = Vec::new();
    for item in &work {
        if stop() {
            return Err(vec![Problem::new("stopped before it was done")]);
        }
        let path = dir.join(DIR).join(&item.file);
        let shown = format!("{DIR}/{}", item.file);
        let fail = |why: String| {
            vec![Problem::new(why)
                .in_file(&shown)
                .of_texture(&item.change.texture)]
        };
        let bytes = std::fs::read(&path).map_err(|e| fail(format!("cannot read: {e}")))?;
        let surface = surface_of_dds(&item.file, &bytes).map_err(fail)?;
        let made = rebuilt(&surface, item, options).map_err(fail)?;
        let dds = to_dds(&made)
            .and_then(|dds| dds.to_bytes())
            .map_err(|e| fail(format!("cannot be written as a DDS: {e}")))?;
        let part = path.with_extension("dds.part");
        std::fs::write(&part, &dds)
            .and_then(|()| std::fs::rename(&part, &path))
            .map_err(|e| fail(format!("cannot write: {e}")))?;
        rules_changed |= align_rules(&mut held.rules, &item.change.texture, &made);
        let mut change = item.change.clone();
        change.now = Look::of(
            made.width,
            made.height,
            made.mips,
            &format!("{:?}", made.format),
        );
        done.push(change);
    }
    if rules_changed {
        let text =
            canonical(&held.rules).map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
        std::fs::write(dir.join(RULES), text)
            .map_err(|e| vec![Problem::new(format!("cannot write: {e}")).in_file(RULES)])?;
    }
    if !done.is_empty() {
        let source = Source::open(dir).map_err(|p| vec![p])?;
        compile(&source)?;
    }
    plan.changes = done;
    Ok(plan)
}

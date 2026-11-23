//! Texture budgeting: a size and a format for each texture of a source that fit a memory target, instead of one blanket cut.
//!
//! [`measure`] gives each texture a ladder of choices, from what it is down to the cheapest, and the PSNR of each against the
//! texture as it is; [`choose`] gives up the least picture per byte saved until the textures fit, across as many sources as are
//! asked; [`apply`] writes exactly the rung chosen for each texture. Measuring and choosing write nothing, so they are the dry run
//! of a plan that [`apply`] carries out once it is accepted.
//!
//! ⚠ **A rung is measured at fast quality and written at the quality asked for.** The PSNR a plan states is therefore a floor of
//! the picture it writes. A level the texture stores already is never encoded again: a halving that keeps the format drops the top
//! levels and keeps the others as they are, and measures what those levels hold.
//!
//! ⚠ **The bytes are those of the mips**, which is what a texture asks the graphics card for, and not the pages of a resource.
//!
//! ponytail: the choice is greedy by the PSNR lost per byte saved, which is optimal when each ladder is convex and close to it
//! when it is not; an exact choice is a knapsack over the ladders, to be written if a plan is seen to leave a lot of slack.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::Path;

use or_io::authoring::{encode_level, is_encodable, rgba_psnr, EncodeQuality};
use or_io::codec::texture::{
    decode_texture_data, generate_mip_chain, max_mip_levels, resize_rgba, to_dds, GrcTextureFormat,
    MipFilter, RgbaImage,
};
use serde::Serialize;

use crate::asset::Source;
use crate::json::canonical;
use crate::optimise::{align_rules, classify, Made};
use crate::textures::{compile, mip_levels, surface_of_dds, Mips, Surface, DIR, RULES};
use crate::Problem;

/// What a budget may do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// The quality a rung is written at; a rung is measured at fast quality whatever this is.
    pub quality: EncodeQuality,
    /// The shortest side a texture is offered at.
    pub smallest: u32,
    /// The pack is built for Legacy, whose DirectX 10 cards read no BC7: a texture stored so is offered only as BC1 or BC3.
    pub legacy: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            quality: EncodeQuality::Normal,
            smallest: 16,
            legacy: true,
        }
    }
}

/// A texture as it could be stored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rung {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    /// The engine's name for the format.
    pub format: String,
    /// The bytes of the mips.
    pub bytes: u64,
    /// The PSNR in decibels of this against the texture as it is, none where they are the same picture.
    pub psnr: Option<f64>,
}

impl std::fmt::Display for Rung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} by {} {} with {} mip{}, {} bytes",
            self.width,
            self.height,
            self.format,
            self.mips,
            if self.mips == 1 { "" } else { "s" },
            self.bytes
        )?;
        match self.psnr {
            Some(db) => write!(f, ", {db:.1} dB"),
            None => Ok(()),
        }
    }
}

/// The choices of one texture, the first being what it is and the rest cheaper and each less true to it than the one before.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ladder {
    pub texture: String,
    /// Its DDS, inside the source's `textures/` folder.
    pub file: String,
    pub rungs: Vec<Rung>,
}

/// A texture a budget leaves as it is, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub texture: String,
    pub why: String,
}

/// What a source can give up.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Measured {
    pub ladders: Vec<Ladder>,
    pub left: Vec<Left>,
}

impl Measured {
    /// What the textures take as they are.
    pub fn bytes(&self) -> u64 {
        self.ladders.iter().map(|l| l.rungs[0].bytes).sum()
    }
}

/// How many halvings take `from` to `to`, where `to` is a power-of-two fraction of `from`.
fn halvings(from: u32, to: u32) -> Option<u32> {
    (to != 0 && from.is_multiple_of(to) && (from / to).is_power_of_two())
        .then(|| (from / to).trailing_zeros())
}

/// The mips a texture of this size is given where it is built afresh: the chain it had, or the shipped one, or one level.
fn mips_for(was: &Surface, width: u32, height: u32) -> u32 {
    if was.mips > 1 && was.mips >= max_mip_levels(was.width, was.height) {
        max_mip_levels(width, height)
    } else if was.mips >= 2 {
        mip_levels(Mips::To4x4, width, height)
    } else {
        1
    }
}

fn rung_of(surface: &Surface, psnr: Option<f64>) -> Rung {
    Rung {
        width: surface.width,
        height: surface.height,
        mips: surface.mips,
        format: format!("{:?}", surface.format),
        bytes: surface.payload.len() as u64,
        psnr,
    }
}

/// The top mip of a surface, decoded.
fn top_of(surface: &Surface) -> Result<RgbaImage, String> {
    let range = surface
        .format
        .mip_level_range(surface.width, surface.height, 0);
    let payload = surface
        .payload
        .get(range)
        .ok_or("its top mip is not in the file")?;
    let mut image = decode_texture_data(surface.format, surface.width, surface.height, payload)
        .map_err(|e| format!("cannot be decoded: {e}"))?;
    image.is_srgb = surface.srgb;
    Ok(image)
}

/// `was` as it would be stored at `width` by `height` as `format`.
fn candidate(
    was: &Surface,
    image: &RgbaImage,
    width: u32,
    height: u32,
    format: GrcTextureFormat,
    quality: EncodeQuality,
) -> Result<Surface, String> {
    let k = halvings(was.width, width)
        .zip(halvings(was.height, height))
        .map(|(a, b)| a.min(b))
        .filter(|k| (was.width >> k, was.height >> k) == (width, height));
    // A texture that stores the levels below its top keeps them: a halving of the same format drops the top ones.
    let kept = match k {
        Some(0) => Some((was.mips, was.payload.as_slice())),
        Some(k) if was.mips > k => {
            let from = was.format.mip_level_range(was.width, was.height, k).start;
            let mips = was.mips - k;
            let size = was.format.total_size(width, height, mips) as usize;
            let kept = was
                .payload
                .get(from..from + size)
                .ok_or("the levels it keeps are not all in the file")?;
            Some((mips, kept))
        }
        _ => None,
    };
    if let Some((mips, payload)) = kept {
        let as_it_is = |payload: Vec<u8>, format| Surface {
            format,
            srgb: was.srgb,
            width,
            height,
            mips,
            payload,
            exact: false,
        };
        if format == was.format && k != Some(0) {
            return Ok(as_it_is(payload.to_vec(), format));
        }
        // The colour blocks of a BC3 or BC2 texture are those of BC1, so an opaque one drops its alpha blocks and loses nothing.
        if format == GrcTextureFormat::DXT1
            && matches!(was.format, GrcTextureFormat::DXT5 | GrcTextureFormat::DXT3)
        {
            if let Some(colour) = colour_blocks(payload) {
                return Ok(as_it_is(colour, format));
            }
        }
    }
    let pixels = if (width, height) == (was.width, was.height) {
        image.data.clone()
    } else {
        resize_rgba(
            &image.data,
            (was.width, was.height),
            (width, height),
            MipFilter::Lanczos3,
            was.srgb,
        )
        .map_err(|e| e.to_string())?
    };
    let mips = mips_for(was, width, height);
    let chain = generate_mip_chain(width, height, &pixels, Some(mips), MipFilter::Box, was.srgb)
        .map_err(|e| e.to_string())?;
    let mut payload = Vec::new();
    for (level, rgba) in chain.iter().enumerate() {
        let (w, h) = format.mip_dimensions(width, height, level as u32);
        payload.extend_from_slice(
            &encode_level(format, w, h, rgba, quality).map_err(|e| e.to_string())?,
        );
    }
    Ok(Surface {
        format,
        srgb: was.srgb,
        width,
        height,
        mips,
        payload,
        exact: false,
    })
}

/// The colour half of every 16-byte block of `payload`, none where a block would read otherwise as BC1 than as BC3: BC1 takes a first colour that is not above the second for three colours and a clear one, BC3 never does.
fn colour_blocks(payload: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len() / 2);
    for block in payload.as_chunks::<16>().0 {
        let colour = &block[8..];
        if u16::from_le_bytes([colour[0], colour[1]]) <= u16::from_le_bytes([colour[2], colour[3]])
        {
            return None;
        }
        out.extend_from_slice(colour);
    }
    payload.len().is_multiple_of(16).then_some(out)
}

/// The PSNR of `surface` against `original`, the original size being what it is held to; none where they are the same picture.
fn psnr_of(surface: &Surface, original: &RgbaImage) -> Result<Option<f64>, String> {
    let mut seen = top_of(surface)?;
    if (seen.width, seen.height) != (original.width, original.height) {
        let data = resize_rgba(
            &seen.data,
            (seen.width, seen.height),
            (original.width, original.height),
            MipFilter::Lanczos3,
            surface.srgb,
        )
        .map_err(|e| e.to_string())?;
        seen = RgbaImage {
            width: original.width,
            height: original.height,
            data,
            is_srgb: original.is_srgb,
        };
    }
    let db = rgba_psnr(original, &seen)
        .map_err(|e| e.to_string())?
        .overall;
    Ok(db.is_finite().then_some(db))
}

/// The formats a texture stored as `was` may be written as, besides keeping its own.
fn formats_for(was: GrcTextureFormat, opaque: bool, options: &Options) -> Vec<GrcTextureFormat> {
    use GrcTextureFormat::{A8B8G8R8, A8R8G8B8, BC7, DXT1, DXT3, DXT5, X8R8G8B8};
    let by_alpha = if opaque { DXT1 } else { DXT5 };
    let mut formats = Vec::new();
    let keeps = match was {
        A8R8G8B8 | A8B8G8R8 | X8R8G8B8 => false,
        BC7 => !options.legacy,
        _ => true,
    };
    if keeps {
        formats.push(was);
    }
    match was {
        DXT5 | DXT3 if opaque => formats.push(DXT1),
        A8R8G8B8 | A8B8G8R8 | X8R8G8B8 => formats.push(by_alpha),
        BC7 if options.legacy => formats.push(by_alpha),
        _ => {}
    }
    formats.retain(|f| is_encodable(*f) || *f == was);
    formats.dedup();
    formats
}

fn ladder_of(shown: &str, file: &str, was: &Surface, options: &Options) -> Result<Ladder, String> {
    if !(was.width.is_power_of_two() && was.height.is_power_of_two()) {
        return Err("its size is not a power of two, which the optimiser brings it to before it is budgeted".into());
    }
    let image = top_of(was)?;
    let opaque = image.data.as_chunks::<4>().0.iter().all(|t| t[3] == 255);
    let first = rung_of(was, None);
    let mut rungs: Vec<Rung> = Vec::new();
    let formats = formats_for(was.format, opaque, options);
    let mut k = 0;
    while was.width >> k >= 1 && was.height >> k >= 1 {
        let (w, h) = (was.width >> k, was.height >> k);
        if k > 0 && w.min(h) < options.smallest.max(4) {
            break;
        }
        for format in &formats {
            if k == 0 && *format == was.format {
                continue;
            }
            let made = candidate(was, &image, w, h, *format, EncodeQuality::Fast)?;
            if made.payload.len() as u64 >= first.bytes {
                continue;
            }
            let psnr = psnr_of(&made, &image)?;
            rungs.push(rung_of(&made, psnr));
        }
        k += 1;
    }
    // Largest first; one that is smaller and no worse than another makes the other no choice at all.
    rungs.sort_by_key(|r| std::cmp::Reverse(r.bytes));
    let score = |r: &Rung| r.psnr.unwrap_or(f64::INFINITY);
    let mut kept: Vec<Rung> = vec![first];
    for rung in rungs {
        while kept.len() > 1
            && kept
                .last()
                .is_some_and(|l| score(l) <= score(&rung) && l.bytes >= rung.bytes)
        {
            kept.pop();
        }
        if kept.last().is_none_or(|l| rung.bytes < l.bytes) {
            kept.push(rung);
        }
    }
    Ok(Ladder {
        texture: shown.to_string(),
        file: file.to_string(),
        rungs: kept,
    })
}

/// What each DDS texture of the source at `dir` could be stored as, with the PSNR of each choice; nothing is written.
///
/// `stop` is asked between textures and ends the measure with a [`Problem`].
pub fn measure(
    dir: &Path,
    options: &Options,
    stop: &dyn Fn() -> bool,
) -> Result<Measured, Vec<Problem>> {
    let (made, _) = classify(dir)?;
    let mut out = Measured::default();
    for one in made {
        match one {
            Made::Pictures { shown, why } => out.left.push(Left {
                texture: shown,
                why: why.to_string(),
            }),
            Made::Dds { shown, file } => {
                if stop() {
                    return Err(vec![Problem::new("stopped before it was done")]);
                }
                let path = dir.join(DIR).join(&file);
                let bytes = std::fs::read(&path).map_err(|e| {
                    vec![Problem::new(format!("cannot read: {e}")).in_file(format!("{DIR}/{file}"))]
                })?;
                let ladder = surface_of_dds(&file, &bytes)
                    .and_then(|surface| ladder_of(&shown, &file, &surface, options));
                match ladder {
                    Ok(ladder) => out.ladders.push(ladder),
                    Err(why) => out.left.push(Left {
                        texture: shown,
                        why,
                    }),
                }
            }
        }
    }
    Ok(out)
}

/// What a budget chose for some sources.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chosen {
    pub target: u64,
    /// What the textures take as they are.
    pub before: u64,
    /// What they take once every pick is made.
    pub after: u64,
    /// Whether `after` is within the target; a target below what the ladders can give up is not met, and everything they can give up is given.
    pub met: bool,
    /// For each source of the request, the rung chosen for each of its ladders; 0 is the texture as it is.
    pub picks: Vec<Vec<usize>>,
}

impl Chosen {
    /// The part of the target that is left unused, none where it is not met: the plan stops below the target by at most the saving of the step it took last.
    pub fn slack(&self) -> f64 {
        if self.met && self.target > 0 {
            (self.target - self.after) as f64 / self.target as f64
        } else {
            0.0
        }
    }
}

struct Step {
    cost: f64,
    set: usize,
    ladder: usize,
}

impl PartialEq for Step {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Step {}
impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Step {
    /// The cheapest step is the greatest, so that a max-heap pops it.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.set.cmp(&self.set))
            .then(other.ladder.cmp(&self.ladder))
    }
}

/// The decibels lost per byte saved by the step from rung `at` to the one after it.
fn cost_of(ladder: &Ladder, at: usize) -> Option<f64> {
    let (a, b) = (ladder.rungs.get(at)?, ladder.rungs.get(at + 1)?);
    let db = |r: &Rung| r.psnr.unwrap_or(100.0).min(100.0);
    Some((db(a) - db(b)).max(0.0) / (a.bytes - b.bytes) as f64)
}

/// The rung of each ladder of `sets` that fits `target` bytes in all with the least lost: the steps that lose least per byte saved are taken first.
pub fn choose(sets: &[&Measured], target: u64) -> Chosen {
    let before: u64 = sets.iter().map(|m| m.bytes()).sum();
    let mut picks: Vec<Vec<usize>> = sets.iter().map(|m| vec![0; m.ladders.len()]).collect();
    let mut total = before;
    let mut heap = BinaryHeap::new();
    let push = |heap: &mut BinaryHeap<Step>, picks: &[Vec<usize>], set: usize, ladder: usize| {
        if let Some(cost) = cost_of(&sets[set].ladders[ladder], picks[set][ladder]) {
            heap.push(Step { cost, set, ladder });
        }
    };
    for (set, m) in sets.iter().enumerate() {
        for ladder in 0..m.ladders.len() {
            push(&mut heap, &picks, set, ladder);
        }
    }
    let mut taken: Vec<(usize, usize)> = Vec::new();
    while total > target {
        let Some(step) = heap.pop() else { break };
        let ladder = &sets[step.set].ladders[step.ladder];
        let at = picks[step.set][step.ladder];
        total -= ladder.rungs[at].bytes - ladder.rungs[at + 1].bytes;
        picks[step.set][step.ladder] = at + 1;
        taken.push((step.set, step.ladder));
        push(&mut heap, &picks, step.set, step.ladder);
    }
    // The last steps may have gone further than the target asked: give back the dearest of them that still fits.
    if total <= target {
        let mut again = true;
        while again {
            again = false;
            let mut best: Option<(usize, f64)> = None;
            for (n, (set, ladder)) in taken.iter().enumerate() {
                let (set, ladder) = (*set, *ladder);
                let at = picks[set][ladder];
                let l = &sets[set].ladders[ladder];
                if at == 0 || taken[n + 1..].contains(&(set, ladder)) {
                    continue;
                }
                let back = l.rungs[at - 1].bytes - l.rungs[at].bytes;
                if total + back <= target {
                    let lost = cost_of(l, at - 1).unwrap_or(0.0);
                    if best.is_none_or(|(_, c)| lost > c) {
                        best = Some((n, lost));
                    }
                }
            }
            if let Some((n, _)) = best {
                let (set, ladder) = taken[n];
                let at = picks[set][ladder];
                let l = &sets[set].ladders[ladder];
                total += l.rungs[at - 1].bytes - l.rungs[at].bytes;
                picks[set][ladder] = at - 1;
                taken.remove(n);
                again = true;
            }
        }
    }
    Chosen {
        target,
        before,
        after: total,
        met: total <= target,
        picks,
    }
}

/// A texture the budget changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub texture: String,
    pub was: Rung,
    pub now: Rung,
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} becomes {}", self.texture, self.was, self.now)
    }
}

/// The picture a rung of `ladder` stores, at most `longest` pixels on a side, for a person to put beside the one it was.
pub fn picture(
    dir: &Path,
    ladder: &Ladder,
    rung: usize,
    longest: u32,
) -> Result<RgbaImage, String> {
    let file = ladder.file.as_str();
    let bytes =
        std::fs::read(dir.join(DIR).join(file)).map_err(|e| format!("cannot read {file}: {e}"))?;
    let was = surface_of_dds(file, &bytes)?;
    let image = top_of(&was)?;
    let made = match rung {
        0 => was,
        n => {
            let wanted = ladder.rungs.get(n).ok_or("the ladder has no such rung")?;
            let format =
                GrcTextureFormat::from_name(&wanted.format).ok_or("its format is unknown")?;
            candidate(
                &was,
                &image,
                wanted.width,
                wanted.height,
                format,
                EncodeQuality::Fast,
            )?
        }
    };
    let mut seen = top_of(&made)?;
    // Held to the size it was, so that two pictures side by side are two looks at one texture.
    if (seen.width, seen.height) != (image.width, image.height) {
        seen = RgbaImage {
            width: image.width,
            height: image.height,
            data: resize_rgba(
                &seen.data,
                (seen.width, seen.height),
                (image.width, image.height),
                MipFilter::Lanczos3,
                made.srgb,
            )
            .map_err(|e| e.to_string())?,
            is_srgb: image.is_srgb,
        };
    }
    let longest = longest.max(1);
    if seen.width.max(seen.height) > longest {
        let (w, h) = (seen.width, seen.height);
        let scale = f64::from(longest) / f64::from(w.max(h));
        let (nw, nh) = (
            ((f64::from(w) * scale) as u32).max(1),
            ((f64::from(h) * scale) as u32).max(1),
        );
        seen = RgbaImage {
            width: nw,
            height: nh,
            data: resize_rgba(&seen.data, (w, h), (nw, nh), MipFilter::Box, made.srgb)
                .map_err(|e| e.to_string())?,
            is_srgb: seen.is_srgb,
        };
    }
    Ok(seen)
}

/// Write the rung `picks` chose for each ladder of `measured` into the source at `dir`; the textures that gave nothing up are not touched.
///
/// A texture that is no longer as it was measured is refused. `stop` is asked between textures and ends it with a [`Problem`]
/// before the first is written. The source is compiled once at the end, so a result that does not compile is an error.
pub fn apply(
    dir: &Path,
    measured: &Measured,
    picks: &[usize],
    options: &Options,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Change>, Vec<Problem>> {
    if picks.len() != measured.ladders.len() {
        return Err(vec![Problem::new(
            "a plan names a rung for each texture it measured",
        )]);
    }
    let (_, mut held) = classify(dir)?;
    let mut done = Vec::new();
    let mut written: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    let undo = |written: &[(std::path::PathBuf, Vec<u8>)]| {
        for (path, was) in written {
            let _ = std::fs::write(path, was);
        }
    };
    let mut rules_changed = false;
    for (ladder, pick) in measured.ladders.iter().zip(picks) {
        if *pick == 0 {
            continue;
        }
        if stop() {
            undo(&written);
            return Err(vec![Problem::new("stopped before it was done")]);
        }
        let shown = format!("{DIR}/{}", ladder.file);
        let fail = |why: String| {
            vec![Problem::new(why)
                .in_file(&shown)
                .of_texture(&ladder.texture)]
        };
        let path = dir.join(DIR).join(&ladder.file);
        let bytes = std::fs::read(&path).map_err(|e| {
            undo(&written);
            fail(format!("cannot read: {e}"))
        })?;
        let was = surface_of_dds(&ladder.file, &bytes).map_err(|e| {
            undo(&written);
            fail(e)
        })?;
        let first = &ladder.rungs[0];
        if (was.width, was.height, was.mips, format!("{:?}", was.format))
            != (first.width, first.height, first.mips, first.format.clone())
        {
            undo(&written);
            return Err(fail("it is not as it was when it was measured".to_string()));
        }
        let rung = ladder.rungs.get(*pick).ok_or_else(|| {
            undo(&written);
            fail(format!("it has no rung {pick}"))
        })?;
        let format = GrcTextureFormat::from_name(&rung.format).ok_or_else(|| {
            undo(&written);
            fail(format!("{} is not a format this build knows", rung.format))
        })?;
        let image = top_of(&was).map_err(|e| {
            undo(&written);
            fail(e)
        })?;
        let made = candidate(
            &was,
            &image,
            rung.width,
            rung.height,
            format,
            options.quality,
        )
        .map_err(|e| {
            undo(&written);
            fail(e)
        })?;
        let dds = to_dds(&made).and_then(|d| d.to_bytes()).map_err(|e| {
            undo(&written);
            fail(format!("cannot be written as a DDS: {e}"))
        })?;
        let part = path.with_extension("dds.part");
        std::fs::write(&part, &dds)
            .and_then(|()| std::fs::rename(&part, &path))
            .map_err(|e| {
                undo(&written);
                fail(format!("cannot write: {e}"))
            })?;
        written.push((path, bytes));
        rules_changed |= align_rules(&mut held.rules, &ladder.texture, &made);
        done.push(Change {
            texture: ladder.texture.clone(),
            was: first.clone(),
            now: Rung {
                bytes: made.payload.len() as u64,
                ..rung.clone()
            },
        });
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
    Ok(done)
}

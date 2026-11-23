//! Cropping a textures source to a window of its pictures, exactly: the blocks of an aligned power-of-two part, copied level by level.
//!
//! A window that is a power-of-two fraction of a power-of-two texture, lying where such a fraction does, is whole blocks of every
//! level of the mips down to four texels, so the crop is a copy and no block is encoded again. What was stored is what is kept:
//! the same format, the same colour space, and the levels the cropped size has under the convention shipped textures follow, which
//! stop at four texels. A texture with a side of four texels or fewer is left as it is, since a part of it is none a crop can make.
//!
//! ⚠ A texture made from pictures, or from the channels of pictures, has its pixels in the pictures, which are not cropped here: it
//! is refused by name, and nothing is written. [`crop`] writes into the folder it is given; a caller that must keep the folder as it
//! was runs it on a copy.

use std::path::Path;

use or_interchange::doctor::uv::Window;
use or_io::codec::texture::{to_dds, GrcTextureFormat};
use serde::Serialize;

use crate::asset::Source;
use crate::json::canonical;
use crate::optimise::Look;
use crate::textures::{
    compile, is_texture_file, mip_levels, surface_of_dds, Rules, Surface, DIR, KIND, RULES,
};
use crate::Problem;

/// One texture the crop changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cropped {
    pub texture: String,
    pub was: Look,
    pub now: Look,
}

/// A texture left as it is, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub texture: String,
    pub why: String,
}

/// What a crop did to a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub cropped: Vec<Cropped>,
    pub left: Vec<Left>,
}

/// Where a window begins in a texture and how large it is, in pixels: its corner, then its width and height.
type Part = ((u32, u32), (u32, u32));

/// The pixels of `window` in a surface of `width` by `height`, or why the texture has none.
fn pixels(window: &Window, width: u32, height: u32) -> Result<Part, String> {
    let part = |side: u32, axis: &or_interchange::doctor::uv::Axis| -> Result<(u32, u32), String> {
        if !side.is_power_of_two() {
            return Err(format!("a side of {side} pixels is not a power of two"));
        }
        let size = side >> axis.power.min(31);
        Ok((axis.index * size, size))
    };
    let ((x, w), (y, h)) = (part(width, &window.x)?, part(height, &window.y)?);
    if w < 4 || h < 4 {
        return Err(format!(
            "the window is {w} by {h} pixels of {width} by {height}, and a crop keeps four at least on a side"
        ));
    }
    Ok(((x, y), (w, h)))
}

/// `surface` cropped to `window`, level by level.
fn cropped(surface: &Surface, window: &Window) -> Result<Surface, String> {
    let format = surface.format;
    let ((x0, y0), (cw, ch)) = pixels(window, surface.width, surface.height)?;
    if format.bytes_per_block() == 0 || !(format.block_size() == 4 || format.block_size() == 1) {
        return Err(format!("the format {format:?} is not one a crop copies"));
    }
    // The levels the cropped size has under the convention shipped textures follow, and no more than were stored.
    let wanted = cw.min(ch).ilog2().saturating_sub(1).max(1);
    let levels = wanted.min(surface.mips);
    let block = format.block_size();
    let bytes = format.bytes_per_block() as usize;
    let mut payload = Vec::new();
    for level in 0..levels {
        let (sw, _) = format.mip_dimensions(surface.width, surface.height, level);
        let from = format.mip_level_range(surface.width, surface.height, level);
        let level_bytes = surface
            .payload
            .get(from)
            .ok_or("a level it keeps is not in the file")?;
        let (dw, dh) = format.mip_dimensions(cw, ch, level);
        let (px, py) = (x0 >> level, y0 >> level);
        // The unit of a row: a block of four rows of texels, or a texel.
        let (across, down, begin_across, begin_down) = match block {
            4 => (dw.div_ceil(4), dh.div_ceil(4), px / 4, py / 4),
            _ => (dw, dh, px, py),
        };
        let row_len = match block {
            4 => sw.div_ceil(4) as usize,
            _ => sw as usize,
        };
        for row in 0..down as usize {
            let start = ((begin_down as usize + row) * row_len + begin_across as usize) * bytes;
            let end = start + across as usize * bytes;
            payload.extend_from_slice(
                level_bytes
                    .get(start..end)
                    .ok_or("a row of the window is past the level")?,
            );
        }
    }
    Ok(Surface {
        format,
        srgb: surface.srgb,
        width: cw,
        height: ch,
        mips: levels,
        payload,
        exact: surface.exact,
    })
}

/// Whether `format` is one the exact crop stores as it is, with its name for a sentence.
fn name_of(format: GrcTextureFormat) -> String {
    format!("{format:?}")
}

/// Crop every texture of the textures source at `dir` to `window`, writing the DDS of each that changes; `stop` is asked between textures.
///
/// Refused, with nothing written, a source with a texture made from pictures and a texture the window cannot be cut from; a texture of four texels or fewer on a side is left as it is and listed.
pub fn crop(dir: &Path, window: &Window, stop: &dyn Fn() -> bool) -> Result<Report, Vec<Problem>> {
    let source = Source::open(dir).map_err(|p| vec![p])?;
    if source.manifest.kind != KIND {
        return Err(vec![Problem::new(format!(
            "this is a {} source, and only a textures source is cropped",
            source.manifest.kind
        ))]);
    }
    let text = std::fs::read_to_string(dir.join(RULES))
        .map_err(|e| vec![Problem::new(format!("cannot read: {e}")).in_file(RULES)])?;
    let mut rules = serde_json::from_str::<Rules>(&text)
        .map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
    if let Some(made) = rules.textures.iter().find(|e| e.pack.is_some()) {
        return Err(vec![Problem::new(
            "a texture made from the channels of pictures has its pixels in them, which a crop does not reach: crop the pictures",
        )
        .of_texture(&made.name)]);
    }
    let mut files: Vec<String> = std::fs::read_dir(dir.join(DIR))
        .map_err(|e| vec![Problem::new(format!("cannot list {DIR}/: {e}")).in_file(DIR)])?
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| is_texture_file(name))
        .collect();
    files.sort();
    if let Some(picture) = files
        .iter()
        .find(|f| !f.to_ascii_lowercase().ends_with(".dds"))
    {
        return Err(vec![Problem::new(
            "a texture made from a picture has its pixels in it, which a crop does not reach: crop the picture",
        )
        .in_file(format!("{DIR}/{picture}"))]);
    }

    // Everything is worked out before anything is written, so a texture that cannot be cropped leaves the others as they were.
    let mut work: Vec<(String, Surface, Surface)> = Vec::new();
    let mut report = Report {
        cropped: Vec::new(),
        left: Vec::new(),
    };
    for file in &files {
        let shown = format!("{DIR}/{file}");
        let bytes = std::fs::read(dir.join(DIR).join(file))
            .map_err(|e| vec![Problem::new(format!("cannot read: {e}")).in_file(&shown)])?;
        let surface =
            surface_of_dds(file, &bytes).map_err(|e| vec![Problem::new(e).in_file(&shown)])?;
        let texture = file
            .rsplit_once('.')
            .map_or(file.as_str(), |(stem, _)| stem)
            .to_string();
        if surface.width.min(surface.height) <= 4 {
            report.left.push(Left {
                texture,
                why: format!(
                    "it is {} by {} pixels, too small for any part of it to be cut out",
                    surface.width, surface.height
                ),
            });
            continue;
        }
        let made = cropped(&surface, window)
            .map_err(|e| vec![Problem::new(e).in_file(&shown).of_texture(&texture)])?;
        report.cropped.push(Cropped {
            texture,
            was: Look {
                width: surface.width,
                height: surface.height,
                mips: surface.mips,
                format: name_of(surface.format),
            },
            now: Look {
                width: made.width,
                height: made.height,
                mips: made.mips,
                format: name_of(made.format),
            },
        });
        work.push((file.clone(), surface, made));
    }
    let mut rules_changed = false;
    for (file, _, made) in &work {
        if stop() {
            return Err(vec![Problem::new("stopped before it was done")]);
        }
        let path = dir.join(DIR).join(file);
        let shown = format!("{DIR}/{file}");
        let dds = to_dds(made).and_then(|dds| dds.to_bytes()).map_err(|e| {
            vec![Problem::new(format!("cannot be written as a DDS: {e}")).in_file(&shown)]
        })?;
        let part = path.with_extension("dds.part");
        std::fs::write(&part, &dds)
            .and_then(|()| std::fs::rename(&part, &path))
            .map_err(|e| vec![Problem::new(format!("cannot write: {e}")).in_file(&shown)])?;
        // A rule that names a count of mips the DDS no longer has is brought in line, since a DDS is copied as it is.
        let name = file
            .rsplit_once('.')
            .map_or(file.as_str(), |(s, _)| s)
            .to_lowercase();
        if let Some(encode) = rules
            .textures
            .iter_mut()
            .find(|e| e.name.to_lowercase() == name)
            .and_then(|e| e.encode.as_mut())
        {
            if encode
                .mips
                .is_some_and(|m| mip_levels(m, made.width, made.height) != made.mips)
            {
                encode.mips = None;
                rules_changed = true;
            }
        }
    }
    if rules_changed {
        let text =
            canonical(&rules).map_err(|e| vec![Problem::new(e.to_string()).in_file(RULES)])?;
        std::fs::write(dir.join(RULES), text)
            .map_err(|e| vec![Problem::new(format!("cannot write: {e}")).in_file(RULES)])?;
    }
    if !work.is_empty() {
        compile(&Source::open(dir).map_err(|p| vec![p])?)?;
    }
    Ok(report)
}

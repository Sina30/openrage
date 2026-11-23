//! Re-encoding only the blocks of a DDS's mips that an edited image changed, so that the mips the
//! file shipped with survive everywhere else.
//!
//! Compression works in independent 4x4 blocks. The image is compared with the decode of the DDS, and
//! so are the mip chains each makes, block by block: a block whose pixels are the same in both is
//! copied from the DDS, and the rest are encoded from the image's own mips.

use or_io::authoring::{encode_level, EncodeQuality};
use or_io::codec::texture::GrcTextureFormat;
use or_io::codec::texture::{decode_texture_data, generate_mip_chain, MipFilter, RgbaImage};

use crate::textures::Surface;

/// How the blocks that do change are made.
pub(crate) struct Settings {
    pub filter: MipFilter,
    pub quality: EncodeQuality,
    /// The colour space the mips are filtered in, and the flag the texture states.
    pub srgb: bool,
}

/// The channels of an RGBA image that `format` keeps: one it does not keep cannot make a block differ.
fn stored(format: GrcTextureFormat) -> [bool; 4] {
    match format {
        GrcTextureFormat::DXT5A => [true, false, false, false],
        GrcTextureFormat::DXN => [true, true, false, false],
        GrcTextureFormat::L8 => [true, true, true, false],
        GrcTextureFormat::A8 => [false, false, false, true],
        _ => [true; 4],
    }
}

/// `base` with every block that `image` changes encoded afresh; `None` where `image` is not made to
/// `base`'s size, or `base`'s pixels cannot be decoded to compare with, which only a full encode answers.
pub(crate) fn changed_blocks(
    base: &Surface,
    image: &RgbaImage,
    settings: &Settings,
) -> Result<Option<Surface>, String> {
    let (width, height, format) = (base.width, base.height, base.format);
    if (image.width, image.height) != (width, height) {
        return Ok(None);
    }
    let first = format.mip_level_range(width, height, 0);
    let Ok(old) = decode_texture_data(format, width, height, &base.payload[first]) else {
        return Ok(None);
    };
    let chain = |pixels: &[u8]| {
        generate_mip_chain(
            width,
            height,
            pixels,
            Some(base.mips),
            settings.filter,
            settings.srgb,
        )
        .map_err(|e| e.to_string())
    };
    let (was, now) = (chain(&old.data)?, chain(&image.data)?);

    let keep = stored(format);
    let unit = format.block_size().max(1);
    let size = format.bytes_per_block() as usize;
    let mut payload = base.payload.clone();
    let mut changed = false;
    for level in 0..base.mips {
        let (w, h) = format.mip_dimensions(width, height, level);
        let at = format.mip_level_range(width, height, level).start;
        let (before, after) = (&was[level as usize], &now[level as usize]);
        let differs = |x0: u32, y0: u32, x1: u32, y1: u32| {
            (y0..y1).any(|y| {
                (x0..x1).any(|x| {
                    let i = ((y * w + x) * 4) as usize;
                    (0..4).any(|c| keep[c] && before[i + c] != after[i + c])
                })
            })
        };

        // A level that is not a whole number of blocks across is a handful of blocks that clamp at the
        // edge, and is encoded whole when anything in it moved.
        if w % unit != 0 || h % unit != 0 {
            if differs(0, 0, w, h) {
                let bytes = encode_level(format, w, h, after, settings.quality)
                    .map_err(|e| e.to_string())?;
                payload[at..at + bytes.len()].copy_from_slice(&bytes);
                changed = true;
            }
            continue;
        }

        let across = w / unit;
        let dirty: Vec<(u32, u32)> = (0..h / unit)
            .flat_map(|by| (0..across).map(move |bx| (bx, by)))
            .filter(|&(bx, by)| differs(bx * unit, by * unit, (bx + 1) * unit, (by + 1) * unit))
            .collect();
        if dirty.is_empty() {
            continue;
        }
        // The dirty blocks side by side as one strip: blocks are independent, so they encode as they would in place.
        let strip_width = dirty.len() as u32 * unit;
        let row = (unit * 4) as usize;
        let mut strip = vec![0u8; strip_width as usize * unit as usize * 4];
        for (i, &(bx, by)) in dirty.iter().enumerate() {
            for y in 0..unit {
                let from = (((by * unit + y) * w + bx * unit) * 4) as usize;
                let to = ((y * strip_width + i as u32 * unit) * 4) as usize;
                strip[to..to + row].copy_from_slice(&after[from..from + row]);
            }
        }
        let bytes = encode_level(format, strip_width, unit, &strip, settings.quality)
            .map_err(|e| e.to_string())?;
        if bytes.len() != dirty.len() * size {
            return Err(format!(
                "{} blocks of {format:?} encoded to {} bytes, not {}",
                dirty.len(),
                bytes.len(),
                dirty.len() * size
            ));
        }
        for (i, &(bx, by)) in dirty.iter().enumerate() {
            let to = at + (by * across + bx) as usize * size;
            payload[to..to + size].copy_from_slice(&bytes[i * size..(i + 1) * size]);
        }
        changed = true;
    }

    Ok(Some(Surface {
        format,
        srgb: settings.srgb,
        width,
        height,
        mips: base.mips,
        payload,
        exact: !changed,
    }))
}

//! The flattened image of a Photoshop file: the composite it stores after its layers, 8 or 16 bits a
//! channel, in grayscale or RGB, raw or run-length packed. Layers are not composited here.

use image::{DynamicImage, ImageBuffer};

use crate::codec::texture::MAX_DECODE_DIMENSION;
use crate::error::{IoError, Result};

fn refuse(why: impl std::fmt::Display) -> IoError {
    IoError::InvalidData {
        context: format!("failed to read the PSD: {why}"),
    }
}

struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(refuse("the file ends before its image data does"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().unwrap_or_default(),
        ))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }
}

/// The row `packed` unpacks to, appended to `out`: PackBits, where a count byte of `n` copies the
/// next `n + 1` bytes and one of `-n` repeats the next byte `1 - n` times.
fn unpack(packed: &[u8], row: usize, out: &mut Vec<u8>) -> Result<()> {
    let start = out.len();
    let mut at = 0;
    while at < packed.len() && out.len() - start < row {
        let count = packed[at] as i8;
        at += 1;
        match count {
            0..=127 => {
                let n = usize::from(count.unsigned_abs()) + 1;
                out.extend_from_slice(
                    packed
                        .get(at..at + n)
                        .ok_or_else(|| refuse("a packed row ends early"))?,
                );
                at += n;
            }
            -127..=-1 => {
                let byte = *packed
                    .get(at)
                    .ok_or_else(|| refuse("a packed row ends early"))?;
                at += 1;
                out.resize(out.len() + 1 + usize::from(count.unsigned_abs()), byte);
            }
            -128 => {}
        }
    }
    if out.len() - start == row {
        Ok(())
    } else {
        Err(refuse(format!(
            "a packed row holds {} bytes, not the {row} its width needs",
            out.len() - start
        )))
    }
}

/// The layer count of a Layer and Mask Information section, in its layer info or, for a 16-bit
/// document, in the `Lr16` block after the global mask. It is negative when the first extra channel
/// of the merged image is that image's transparency.
fn layer_count(section: &[u8]) -> Result<i16> {
    let mut at = Bytes(section);
    let info = at.u32()? as usize;
    if info >= 2 {
        return Ok(at.u16()? as i16);
    }
    at.take(info)?;
    let mask = at.u32()? as usize;
    at.take(mask)?;
    loop {
        let signature = at.take(4)?;
        if signature != b"8BIM" && signature != b"8B64" {
            return Err(refuse("a block of its layer section has no signature"));
        }
        let key = at.take(4)?;
        let len = at.u32()? as usize;
        if key == b"Lr16" {
            return Ok(Bytes(at.take(len)?).u16()? as i16);
        }
        at.take(len.next_multiple_of(4))?;
    }
}

/// `colour` with the white it was blended against taken back out: Photoshop stores a texel of colour
/// `c` and alpha `a` as `max - (max - c) * a / max`, with `max` the value of a full channel.
fn unmatte(colour: u16, alpha: u16, opaque: u16) -> u16 {
    if alpha == 0 {
        return colour;
    }
    let (alpha, max) = (u64::from(alpha), u64::from(opaque));
    let num = (u64::from(colour) + alpha).saturating_sub(max) * max;
    ((num * 2 + alpha) / (alpha * 2)).min(max) as u16
}

/// The merged image of `bytes`, with the channel after the colour as alpha where the file has one.
/// Where the layer count is negative the colour is stored blended against white and comes out as
/// straight alpha.
/// The depth the header states: 16 bits an integer, 32 bits of light, or 8.
pub(super) fn depth(bytes: &[u8]) -> Result<super::texture_import::Depth> {
    use super::texture_import::Depth;
    let mut file = Bytes(bytes);
    if file.take(4)? != b"8BPS" {
        return Err(refuse("it does not begin with 8BPS"));
    }
    file.take(18)?;
    Ok(match file.u16()? {
        16 => Depth::Sixteen,
        32 => Depth::Float,
        _ => Depth::Eight,
    })
}

pub(super) fn flatten(bytes: &[u8]) -> Result<DynamicImage> {
    let mut file = Bytes(bytes);
    if file.take(4)? != b"8BPS" {
        return Err(refuse("it does not begin with 8BPS"));
    }
    match file.u16()? {
        1 => {}
        2 => return Err(refuse("a large document (PSB) is not read")),
        other => {
            return Err(refuse(format!(
                "version {other} is not one this build reads"
            )))
        }
    }
    file.take(6)?;
    let channels = usize::from(file.u16()?);
    let (height, width) = (file.u32()?, file.u32()?);
    let depth = file.u16()?;
    let mode = file.u16()?;
    if width == 0 || height == 0 || width > MAX_DECODE_DIMENSION || height > MAX_DECODE_DIMENSION {
        return Err(refuse(format!(
            "{width}x{height} is not a size of at most {MAX_DECODE_DIMENSION} a side"
        )));
    }
    let bytes_per = match depth {
        8 => 1,
        16 => 2,
        other => {
            return Err(refuse(format!(
                "{other} bits a channel is not read; save it as 8 or 16"
            )))
        }
    };
    let colour = match mode {
        1 => 1,
        3 => 3,
        other => {
            let name = match other {
                0 => "bitmap",
                2 => "indexed colour",
                4 => "CMYK",
                7 => "multichannel",
                8 => "duotone",
                9 => "Lab",
                _ => "an unknown mode",
            };
            return Err(refuse(format!(
                "{name} is not read; save it as grayscale or RGB"
            )));
        }
    };
    if channels < colour {
        return Err(refuse(format!(
            "{channels} channels are fewer than its colour mode needs"
        )));
    }
    // The colour mode data and the image resources are not the merged image, and the layers are only
    // read for their count.
    for _ in 0..2 {
        let skip = file.u32()? as usize;
        file.take(skip)?;
    }
    let layers = file.u32()? as usize;
    let layers = file.take(layers)?;
    let (w, h) = (width as usize, height as usize);
    let row = w * bytes_per;
    // Planes are stored one channel after another; only the colour and the alpha are read.
    let wanted = channels.min(colour + 1);
    let planes = match file.u16()? {
        0 => file.take(wanted * h * row)?.to_vec(),
        1 => {
            let counts = (0..channels * h)
                .map(|_| file.u16().map(usize::from))
                .collect::<Result<Vec<_>>>()?;
            let mut planes = Vec::with_capacity(wanted * h * row);
            for count in &counts[..wanted * h] {
                unpack(file.take(*count)?, row, &mut planes)?;
            }
            planes
        }
        other => {
            return Err(refuse(format!(
                "compression {other} (zip) is not read; save it with the default compression"
            )))
        }
    };

    let at = |channel: usize, texel: usize| {
        let i = channel * h * row + texel * bytes_per;
        if bytes_per == 1 {
            u16::from(planes[i])
        } else {
            u16::from_be_bytes([planes[i], planes[i + 1]])
        }
    };
    let opaque = if bytes_per == 1 { 255 } else { u16::MAX };
    let matted = wanted > colour && layer_count(layers).is_ok_and(|count| count < 0);
    let mut texels = Vec::with_capacity(w * h * 4);
    for texel in 0..w * h {
        let alpha = if wanted > colour {
            at(colour, texel)
        } else {
            opaque
        };
        let rgb = if colour == 1 {
            [at(0, texel); 3]
        } else {
            [at(0, texel), at(1, texel), at(2, texel)]
        };
        let rgb = if matted {
            rgb.map(|c| unmatte(c, alpha, opaque))
        } else {
            rgb
        };
        texels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    }
    let size = |e: &str| refuse(format!("{e} does not fit its size"));
    Ok(if bytes_per == 1 {
        let data = texels.iter().map(|&v| v as u8).collect();
        DynamicImage::ImageRgba8(
            ImageBuffer::from_raw(width, height, data).ok_or_else(|| size("the image"))?,
        )
    } else {
        DynamicImage::ImageRgba16(
            ImageBuffer::from_raw(width, height, texels).ok_or_else(|| size("the image"))?,
        )
    })
}

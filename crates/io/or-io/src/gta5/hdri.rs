use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HdriError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Radiance .hdr file (missing #?RADIANCE magic)")]
    BadMagic,
    #[error("malformed header: {0}")]
    BadHeader(String),
    #[error("unexpected EOF while reading scanlines")]
    UnexpectedEof,
    #[error("unsupported format `{0}` (only 32-bit_rle_rgbe is supported)")]
    UnsupportedFormat(String),
}

pub struct HdriImage {
    pub width: u32,
    pub height: u32,
    pub rgba_f32: Vec<f32>,
}

impl HdriImage {
    pub fn to_rgba16f_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgba_f32.len() * 2);
        for &v in &self.rgba_f32 {
            let bits = crate::half::f32_to_half(v);
            out.extend_from_slice(&bits.to_le_bytes());
        }
        out
    }
}

pub fn load_radiance_hdr(path: impl AsRef<Path>) -> Result<HdriImage, HdriError> {
    let file = std::fs::File::open(path)?;
    decode_radiance_hdr(BufReader::new(file))
}

pub fn decode_radiance_hdr<R: Read>(mut reader: BufReader<R>) -> Result<HdriImage, HdriError> {
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;
    let trimmed = first_line.trim_end();
    if !(trimmed.starts_with("#?RADIANCE") || trimmed.starts_with("#?RGBE")) {
        return Err(HdriError::BadMagic);
    }
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(HdriError::BadHeader("unexpected EOF in header".into()));
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some(rest) = l.strip_prefix("FORMAT=") {
            if rest != "32-bit_rle_rgbe" && rest != "32-bit_rle_xyze" {
                return Err(HdriError::UnsupportedFormat(rest.to_string()));
            }
        }
    }

    let mut res_line = String::new();
    reader.read_line(&mut res_line)?;
    let res = res_line.trim();
    let parts: Vec<&str> = res.split_whitespace().collect();
    if parts.len() != 4 {
        return Err(HdriError::BadHeader(format!(
            "expected `-Y H +X W` resolution line, got `{res}`"
        )));
    }
    if parts[0] != "-Y" || parts[2] != "+X" {
        return Err(HdriError::BadHeader(format!(
            "only -Y +X orientation supported, got `{res}`"
        )));
    }
    let height: u32 = parts[1]
        .parse()
        .map_err(|e| HdriError::BadHeader(format!("invalid height: {e}")))?;
    let width: u32 = parts[3]
        .parse()
        .map_err(|e| HdriError::BadHeader(format!("invalid width: {e}")))?;

    let mut rgba_f32 = Vec::with_capacity((width * height * 4) as usize);
    let mut scanline = vec![0u8; (width * 4) as usize];
    for _y in 0..height {
        decode_scanline(&mut reader, width, &mut scanline)?;
        for x in 0..width as usize {
            let r = scanline[x];
            let g = scanline[x + width as usize];
            let b = scanline[x + 2 * width as usize];
            let e = scanline[x + 3 * width as usize];
            let (rf, gf, bf) = rgbe_to_rgb(r, g, b, e);
            rgba_f32.push(rf);
            rgba_f32.push(gf);
            rgba_f32.push(bf);
            rgba_f32.push(1.0);
        }
    }

    Ok(HdriImage {
        width,
        height,
        rgba_f32,
    })
}

fn decode_scanline<R: Read>(
    reader: &mut BufReader<R>,
    width: u32,
    out: &mut [u8],
) -> Result<(), HdriError> {
    debug_assert_eq!(out.len(), (width * 4) as usize);
    if !(8..=0x7fff).contains(&width) {
        return decode_uncompressed(reader, width as usize, out);
    }
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let is_new_rle = header[0] == 0x02
        && header[1] == 0x02
        && (header[2] as u16) << 8 | (header[3] as u16) == width as u16;
    if !is_new_rle {
        out[0] = header[0];
        out[width as usize] = header[1];
        out[2 * width as usize] = header[2];
        out[3 * width as usize] = header[3];
        return decode_uncompressed_skipping_first(reader, width as usize, out);
    }
    for c in 0..4 {
        let mut x = 0usize;
        while x < width as usize {
            let mut count_byte = [0u8; 1];
            reader.read_exact(&mut count_byte)?;
            let count = count_byte[0];
            if count > 128 {
                let run = (count - 128) as usize;
                if x + run > width as usize {
                    return Err(HdriError::BadHeader(format!(
                        "RLE run overflows scanline: {run}+{x} > {width}"
                    )));
                }
                let mut value = [0u8; 1];
                reader.read_exact(&mut value)?;
                for i in 0..run {
                    out[c * width as usize + x + i] = value[0];
                }
                x += run;
            } else {
                let run = count as usize;
                if x + run > width as usize {
                    return Err(HdriError::BadHeader(format!(
                        "literal run overflows scanline: {run}+{x} > {width}"
                    )));
                }
                let dst = &mut out[c * width as usize + x..c * width as usize + x + run];
                reader.read_exact(dst)?;
                x += run;
            }
        }
    }
    Ok(())
}

fn decode_uncompressed<R: Read>(
    reader: &mut BufReader<R>,
    width: usize,
    out: &mut [u8],
) -> Result<(), HdriError> {
    let mut buf = vec![0u8; width * 4];
    reader.read_exact(&mut buf)?;
    for x in 0..width {
        out[x] = buf[x * 4];
        out[x + width] = buf[x * 4 + 1];
        out[x + 2 * width] = buf[x * 4 + 2];
        out[x + 3 * width] = buf[x * 4 + 3];
    }
    Ok(())
}

fn decode_uncompressed_skipping_first<R: Read>(
    reader: &mut BufReader<R>,
    width: usize,
    out: &mut [u8],
) -> Result<(), HdriError> {
    if width == 0 {
        return Ok(());
    }
    let remaining = width - 1;
    let mut buf = vec![0u8; remaining * 4];
    reader.read_exact(&mut buf)?;
    for i in 0..remaining {
        let x = i + 1;
        out[x] = buf[i * 4];
        out[x + width] = buf[i * 4 + 1];
        out[x + 2 * width] = buf[i * 4 + 2];
        out[x + 3 * width] = buf[i * 4 + 3];
    }
    Ok(())
}

fn rgbe_to_rgb(r: u8, g: u8, b: u8, e: u8) -> (f32, f32, f32) {
    if e == 0 {
        return (0.0, 0.0, 0.0);
    }
    let exp = (e as i32 - 128) - 8;
    let scale = 2f32.powi(exp);
    (
        (r as f32 + 0.5) * scale,
        (g as f32 + 0.5) * scale,
        (b as f32 + 0.5) * scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn decode_minimal_2x2_hdr() {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"#?RADIANCE\n");
        data.extend_from_slice(b"FORMAT=32-bit_rle_rgbe\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"-Y 2 +X 2\n");
        for _ in 0..4 {
            data.extend_from_slice(&[128, 128, 128, 128]);
        }
        let img = decode_radiance_hdr(BufReader::new(data.as_slice())).unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.rgba_f32.len(), 16);
        for px in img.rgba_f32.chunks(4) {
            assert!((px[0] - 0.5019).abs() < 0.01);
            assert!((px[1] - 0.5019).abs() < 0.01);
            assert!((px[2] - 0.5019).abs() < 0.01);
            assert_eq!(px[3], 1.0);
        }
    }

    #[test]
    fn rejects_non_radiance_magic() {
        let data = b"NOT A RADIANCE FILE\n\n-Y 1 +X 1\n";
        let r = decode_radiance_hdr(BufReader::new(data.as_slice()));
        assert!(matches!(r, Err(HdriError::BadMagic)));
    }

    #[test]
    fn f16_packs_round_numbers_correctly() {
        use crate::half::f32_to_half;
        assert_eq!(f32_to_half(1.0), 0x3C00);
        assert_eq!(f32_to_half(0.5), 0x3800);
        assert_eq!(f32_to_half(2.0), 0x4000);
        assert_eq!(f32_to_half(0.0), 0x0000);
        assert_eq!(f32_to_half(-1.0), 0xBC00);
    }

    #[test]
    fn to_rgba16f_bytes_doubles_in_size() {
        let img = HdriImage {
            width: 1,
            height: 1,
            rgba_f32: vec![1.0, 0.5, 0.0, 1.0],
        };
        let bytes = img.to_rgba16f_bytes();
        assert_eq!(bytes.len(), 8);
        // Channel 0 = 1.0 → 0x3C00 little-endian.
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x3C);
    }
}

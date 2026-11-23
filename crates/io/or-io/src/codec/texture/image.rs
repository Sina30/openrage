use super::format::GrcTextureFormat;
use crate::error::{IoError, Result};
use crate::half_to_f32;

#[cfg(test)]
#[path = "legacy_bcn.rs"]
mod legacy;

#[derive(Debug, Clone)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub is_srgb: bool,
}

pub const MAX_DECODE_DIMENSION: u32 = 16384;

fn validate_decode_dimensions(width: u32, height: u32) -> Result<()> {
    if width > MAX_DECODE_DIMENSION || height > MAX_DECODE_DIMENSION {
        return Err(IoError::InvalidData {
            context: format!(
                "texture dimensions {width}x{height} exceed the maximum \
                 decodable size of {MAX_DECODE_DIMENSION}; the header is \
                 corrupt or this is not a texture"
            ),
        });
    }
    Ok(())
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width as usize) * (height as usize) * 4;
        Self {
            width,
            height,
            data: vec![0u8; size],
            is_srgb: false,
        }
    }

    pub fn with_srgb(mut self, is_srgb: bool) -> Self {
        self.is_srgb = is_srgb;
        self
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let idx = ((y * self.width + x) * 4) as usize;
        self.data[idx] = rgba[0];
        self.data[idx + 1] = rgba[1];
        self.data[idx + 2] = rgba[2];
        self.data[idx + 3] = rgba[3];
    }

    pub fn fill(&mut self, rgba: [u8; 4]) {
        for chunk in self.data.as_chunks_mut::<4>().0.iter_mut() {
            chunk.copy_from_slice(&rgba);
        }
    }

    pub fn apply_channel_view(&mut self, view: ChannelView) {
        if view == ChannelView::Rgba {
            return;
        }
        for px in self.data.as_chunks_mut::<4>().0.iter_mut() {
            let out = view.map([px[0], px[1], px[2], px[3]]);
            px.copy_from_slice(&out);
        }
    }

    pub fn with_channel_view(mut self, view: ChannelView) -> Self {
        self.apply_channel_view(view);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    R,
    G,
    B,
    A,
}

impl Channel {
    fn index(self) -> usize {
        match self {
            Self::R => 0,
            Self::G => 1,
            Self::B => 2,
            Self::A => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChannelView {
    #[default]
    Rgba,
    Grey(Channel),
    NormalMapRg,
    AlphaOnly,
}

impl ChannelView {
    pub fn preview_default(format: GrcTextureFormat) -> Self {
        match format {
            GrcTextureFormat::DXT5A | GrcTextureFormat::EacR11 => Self::Grey(Channel::R),
            GrcTextureFormat::DXN | GrcTextureFormat::EacRg11 => Self::NormalMapRg,
            GrcTextureFormat::A8 => Self::AlphaOnly,
            _ => Self::Rgba,
        }
    }

    fn map(self, px: [u8; 4]) -> [u8; 4] {
        match self {
            Self::Rgba => px,
            Self::Grey(channel) => {
                let v = px[channel.index()];
                [v, v, v, 255]
            }
            Self::AlphaOnly => [px[3], px[3], px[3], 255],
            Self::NormalMapRg => {
                let x = (px[0] as f32 / 255.0) * 2.0 - 1.0;
                let y = (px[1] as f32 / 255.0) * 2.0 - 1.0;
                let z = (1.0 - x * x - y * y).max(0.0).sqrt();
                [px[0], px[1], ((z * 0.5 + 0.5) * 255.0 + 0.5) as u8, 255]
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct HdrImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tonemap {
    #[default]
    ClampSrgb,
    Reinhard,
}

impl HdrImage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0.0; (width as usize) * (height as usize) * 4],
        }
    }

    pub fn tonemap(&self, mode: Tonemap) -> RgbaImage {
        let mut out = RgbaImage::new(self.width, self.height);
        for (dst, src) in out
            .data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.data.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                let v = match mode {
                    Tonemap::ClampSrgb => src[c],
                    Tonemap::Reinhard => {
                        let x = src[c].max(0.0);
                        x / (1.0 + x)
                    }
                };
                dst[c] = super::mipmap::linear_to_srgb_u8(v);
            }
            dst[3] = (src[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        out.is_srgb = true;
        out
    }
}

pub fn decode_texture_data(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Result<RgbaImage> {
    use GrcTextureFormat as F;

    validate_decode_dimensions(width, height)?;

    if matches!(
        format,
        F::BC6
            | F::R16F
            | F::R32F
            | F::G16R16F
            | F::G32R32F
            | F::A16B16G16R16F
            | F::A16B16G16R16FNoExpand
            | F::A32B32G32R32F
            | F::R11G11B10F
    ) {
        return Ok(decode_texture_hdr(format, width, height, data)?.tonemap(Tonemap::default()));
    }

    let mut image = RgbaImage::new(width, height);

    match format {
        F::DXT1 => decode_bc_blocks(data, width, height, &mut image, 8, "BC1", bc1_block)?,
        F::DXT3 => decode_bc_blocks(data, width, height, &mut image, 16, "BC2", bc2_block)?,
        F::DXT5 => decode_bc_blocks(data, width, height, &mut image, 16, "BC3", bc3_block)?,
        F::DXT5A => decode_bc_blocks(data, width, height, &mut image, 8, "BC4", bc4_block)?,
        F::DXN => decode_bc_blocks(data, width, height, &mut image, 16, "BC5", bc5_block)?,
        F::BC7 => decode_bc_blocks(data, width, height, &mut image, 16, "BC7", bc7_block)?,

        F::Etc2Rgb => decode_packed_bgra(
            data,
            width,
            height,
            &mut image,
            "ETC2 RGB",
            texture2ddecoder::decode_etc2_rgb,
        )?,
        F::Etc2Rgba => decode_packed_bgra(
            data,
            width,
            height,
            &mut image,
            "ETC2 RGBA",
            texture2ddecoder::decode_etc2_rgba8,
        )?,
        F::EacR11 => decode_packed_bgra(
            data,
            width,
            height,
            &mut image,
            "EAC R11",
            texture2ddecoder::decode_eacr,
        )?,
        F::EacRg11 => decode_packed_bgra(
            data,
            width,
            height,
            &mut image,
            "EAC RG11",
            texture2ddecoder::decode_eacrg,
        )?,

        F::A8R8G8B8 => decode_packed(data, width, height, &mut image, 4, |p| {
            [p[2], p[1], p[0], p[3]]
        })?,
        F::X8R8G8B8 => decode_packed(data, width, height, &mut image, 4, |p| {
            [p[2], p[1], p[0], 255]
        })?,
        F::A8B8G8R8 | F::A8B8G8R8Snorm => decode_packed(data, width, height, &mut image, 4, |p| {
            [p[0], p[1], p[2], p[3]]
        })?,
        F::A2B10G10R10 | F::A2B10G10R10ATI => {
            decode_packed(data, width, height, &mut image, 4, |p| {
                let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                [
                    ten_to_eight(v & 0x3FF),
                    ten_to_eight((v >> 10) & 0x3FF),
                    ten_to_eight((v >> 20) & 0x3FF),
                    two_to_eight((v >> 30) as u8 & 0x3),
                ]
            })?
        }
        F::G16R16 => decode_packed(data, width, height, &mut image, 4, |p| [p[1], p[3], 0, 255])?,

        F::A16B16G16R16 => decode_packed(data, width, height, &mut image, 8, |p| {
            [p[1], p[3], p[5], p[7]]
        })?,

        F::R5G6B5 => decode_packed(data, width, height, &mut image, 2, |p| {
            rgb565_to_rgba(u16::from_le_bytes([p[0], p[1]]))
        })?,
        F::A1R5G5B5 => decode_packed(data, width, height, &mut image, 2, |p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let r = ((v >> 10) & 0x1F) as u8;
            let g = ((v >> 5) & 0x1F) as u8;
            let b = (v & 0x1F) as u8;
            [
                (r << 3) | (r >> 2),
                (g << 3) | (g >> 2),
                (b << 3) | (b >> 2),
                if v & 0x8000 != 0 { 255 } else { 0 },
            ]
        })?,
        F::A4R4G4B4 => decode_packed(data, width, height, &mut image, 2, |p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let expand = |n: u16| {
                let n = (n & 0xF) as u8;
                (n << 4) | n
            };
            [expand(v >> 8), expand(v >> 4), expand(v), expand(v >> 12)]
        })?,
        F::L16 => decode_packed(data, width, height, &mut image, 2, |p| {
            [p[1], p[1], p[1], 255]
        })?,
        F::G8R8 => decode_packed(data, width, height, &mut image, 2, |p| [p[0], p[1], 0, 255])?,

        F::L8 => decode_packed(data, width, height, &mut image, 1, |p| {
            [p[0], p[0], p[0], 255]
        })?,
        F::A8 => decode_packed(data, width, height, &mut image, 1, |p| [0, 0, 0, p[0]])?,

        F::D16 | F::D24S8 | F::D32F | F::D32FS8 | F::X24G8 | F::X32S8 => {
            return Err(IoError::InvalidData {
                context: format!("{format:?} is a depth/stencil surface, not a sampleable image"),
            })
        }
        F::None | F::Null => {
            return Err(IoError::InvalidData {
                context: "texture has no pixel format".to_string(),
            })
        }

        other => {
            tracing::warn!(
                format = ?other,
                width,
                height,
                "no decoder for texture format; substituting magenta placeholder"
            );
            image.fill([255, 0, 255, 255]);
        }
    }

    Ok(image)
}

pub fn decode_texture_hdr(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Result<HdrImage> {
    use GrcTextureFormat as F;

    validate_decode_dimensions(width, height)?;

    let mut out = HdrImage::new(width, height);
    let pixels = (width as usize) * (height as usize);

    match format {
        F::BC6 => {
            let blocks_x = width.div_ceil(4) as usize;
            let blocks_y = height.div_ceil(4) as usize;
            let required = blocks_x * blocks_y * 16;
            if data.len() < required {
                return Err(IoError::InsufficientData {
                    expected: required,
                    actual: data.len(),
                });
            }

            let mut scratch = [0f32; 16 * 3];
            for by in 0..blocks_y {
                for bx in 0..blocks_x {
                    let offset = (by * blocks_x + bx) * 16;
                    bcdec_rs::bc6h_float(&data[offset..offset + 16], &mut scratch, 4 * 3, false);
                    for row in 0..4.min(height as usize - by * 4) {
                        for col in 0..4.min(width as usize - bx * 4) {
                            let src = (row * 4 + col) * 3;
                            let dst = ((by * 4 + row) * width as usize + bx * 4 + col) * 4;
                            out.data[dst] = scratch[src];
                            out.data[dst + 1] = scratch[src + 1];
                            out.data[dst + 2] = scratch[src + 2];
                            out.data[dst + 3] = 1.0;
                        }
                    }
                }
            }
        }

        F::R16F => hdr_from_packed(data, pixels, 2, &mut out, |p| [half(p, 0), 0.0, 0.0, 1.0])?,
        F::G16R16F => hdr_from_packed(data, pixels, 4, &mut out, |p| {
            [half(p, 0), half(p, 1), 0.0, 1.0]
        })?,
        F::A16B16G16R16F | F::A16B16G16R16FNoExpand => {
            hdr_from_packed(data, pixels, 8, &mut out, |p| {
                [half(p, 0), half(p, 1), half(p, 2), half(p, 3)]
            })?
        }
        F::R32F => hdr_from_packed(data, pixels, 4, &mut out, |p| [single(p, 0), 0.0, 0.0, 1.0])?,
        F::G32R32F => hdr_from_packed(data, pixels, 8, &mut out, |p| {
            [single(p, 0), single(p, 1), 0.0, 1.0]
        })?,
        F::A32B32G32R32F => hdr_from_packed(data, pixels, 16, &mut out, |p| {
            [single(p, 0), single(p, 1), single(p, 2), single(p, 3)]
        })?,
        F::R11G11B10F => hdr_from_packed(data, pixels, 4, &mut out, |p| {
            let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            [
                float11_to_f32((v & 0x7FF) as u16),
                float11_to_f32(((v >> 11) & 0x7FF) as u16),
                float10_to_f32(((v >> 22) & 0x3FF) as u16),
                1.0,
            ]
        })?,

        other => {
            return Err(IoError::InvalidData {
                context: format!("{other:?} is not an HDR format; use decode_texture_data"),
            })
        }
    }

    Ok(out)
}

fn hdr_from_packed<F>(
    data: &[u8],
    pixels: usize,
    stride: usize,
    out: &mut HdrImage,
    f: F,
) -> Result<()>
where
    F: Fn(&[u8]) -> [f32; 4],
{
    let expected = pixels * stride;
    if data.len() < expected {
        return Err(IoError::InsufficientData {
            expected,
            actual: data.len(),
        });
    }
    for i in 0..pixels {
        let px = f(&data[i * stride..(i + 1) * stride]);
        out.data[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    Ok(())
}

#[inline]
fn half(p: &[u8], index: usize) -> f32 {
    half_to_f32(u16::from_le_bytes([p[index * 2], p[index * 2 + 1]]))
}

#[inline]
fn single(p: &[u8], index: usize) -> f32 {
    let b = index * 4;
    f32::from_le_bytes([p[b], p[b + 1], p[b + 2], p[b + 3]])
}

fn float11_to_f32(bits: u16) -> f32 {
    let exponent = (bits >> 6) & 0x1F;
    let mantissa = (bits & 0x3F) as f32 / 64.0;
    match exponent {
        0 => mantissa * 2f32.powi(-14),
        0x1F => f32::INFINITY,
        e => (1.0 + mantissa) * 2f32.powi(e as i32 - 15),
    }
}

fn float10_to_f32(bits: u16) -> f32 {
    let exponent = (bits >> 5) & 0x1F;
    let mantissa = (bits & 0x1F) as f32 / 32.0;
    match exponent {
        0 => mantissa * 2f32.powi(-14),
        0x1F => f32::INFINITY,
        e => (1.0 + mantissa) * 2f32.powi(e as i32 - 15),
    }
}

type BcScratch = [u8; 4 * 4 * 4];

fn decode_bc_blocks<F>(
    data: &[u8],
    width: u32,
    height: u32,
    image: &mut RgbaImage,
    block_bytes: usize,
    label: &str,
    decode_block: F,
) -> Result<()>
where
    F: Fn(&[u8], &mut BcScratch),
{
    if width == 0 || height == 0 {
        return Ok(());
    }

    let blocks_x = width.div_ceil(4) as usize;
    let blocks_y = height.div_ceil(4) as usize;
    let required = blocks_x * blocks_y * block_bytes;

    if data.len() < required {
        return Err(IoError::InsufficientData {
            expected: required,
            actual: data.len(),
        });
    }

    let pitch = (width as usize) * 4;
    let mut scratch: BcScratch = [0u8; 4 * 4 * 4];

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let offset = (by * blocks_x + bx) * block_bytes;
            decode_block(&data[offset..offset + block_bytes], &mut scratch);

            let x0 = bx * 4;
            let y0 = by * 4;
            let copy_w = (width as usize - x0).min(4);
            let copy_h = (height as usize - y0).min(4);

            for row in 0..copy_h {
                let src = row * 16;
                let dst = (y0 + row) * pitch + x0 * 4;
                image.data[dst..dst + copy_w * 4].copy_from_slice(&scratch[src..src + copy_w * 4]);
            }
        }
    }

    tracing::trace!(
        label,
        width,
        height,
        blocks = blocks_x * blocks_y,
        "decoded"
    );
    Ok(())
}

fn bc1_block(block: &[u8], out: &mut BcScratch) {
    bcdec_rs::bc1(block, out, 4 * 4);
}

fn bc2_block(block: &[u8], out: &mut BcScratch) {
    bcdec_rs::bc2(block, out, 4 * 4);
}

fn bc3_block(block: &[u8], out: &mut BcScratch) {
    bcdec_rs::bc3(block, out, 4 * 4);
}

fn bc7_block(block: &[u8], out: &mut BcScratch) {
    bcdec_rs::bc7(block, out, 4 * 4);
}

fn bc4_block(block: &[u8], out: &mut BcScratch) {
    let mut red = [0u8; 16];
    bcdec_rs::bc4(block, &mut red, 4, false);
    for (i, v) in red.iter().enumerate() {
        out[i * 4] = *v;
        out[i * 4 + 1] = 0;
        out[i * 4 + 2] = 0;
        out[i * 4 + 3] = 255;
    }
}

fn bc5_block(block: &[u8], out: &mut BcScratch) {
    let mut rg = [0u8; 32];
    bcdec_rs::bc5(block, &mut rg, 4 * 2, false);
    for i in 0..16 {
        out[i * 4] = rg[i * 2];
        out[i * 4 + 1] = rg[i * 2 + 1];
        out[i * 4 + 2] = 0;
        out[i * 4 + 3] = 255;
    }
}

fn decode_packed_bgra<F>(
    data: &[u8],
    width: u32,
    height: u32,
    image: &mut RgbaImage,
    label: &str,
    decoder: F,
) -> Result<()>
where
    F: Fn(&[u8], usize, usize, &mut [u32]) -> core::result::Result<(), &'static str>,
{
    let pixels = (width as usize) * (height as usize);
    let mut buf = vec![0u32; pixels];
    decoder(data, width as usize, height as usize, &mut buf).map_err(|msg| {
        IoError::InvalidStructure {
            offset: 0,
            message: format!("{label} decode failed ({width}x{height}): {msg}"),
        }
    })?;
    for (i, packed) in buf.iter().enumerate() {
        let off = i * 4;
        image.data[off] = ((packed >> 16) & 0xFF) as u8;
        image.data[off + 1] = ((packed >> 8) & 0xFF) as u8;
        image.data[off + 2] = (packed & 0xFF) as u8;
        image.data[off + 3] = ((packed >> 24) & 0xFF) as u8;
    }
    Ok(())
}

fn decode_packed<F>(
    data: &[u8],
    width: u32,
    height: u32,
    image: &mut RgbaImage,
    stride: usize,
    f: F,
) -> Result<()>
where
    F: Fn(&[u8]) -> [u8; 4],
{
    let pixels = (width as usize) * (height as usize);
    let expected = pixels * stride;
    if data.len() < expected {
        return Err(IoError::InsufficientData {
            expected,
            actual: data.len(),
        });
    }
    for i in 0..pixels {
        let px = f(&data[i * stride..(i + 1) * stride]);
        image.data[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    Ok(())
}

fn rgb565_to_rgba(rgb565: u16) -> [u8; 4] {
    let r5 = ((rgb565 >> 11) & 0x1F) as u8;
    let g6 = ((rgb565 >> 5) & 0x3F) as u8;
    let b5 = (rgb565 & 0x1F) as u8;

    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);

    [r, g, b, 255]
}

#[inline]
fn ten_to_eight(v: u32) -> u8 {
    (v >> 2) as u8
}

#[inline]
fn two_to_eight(v: u8) -> u8 {
    // 0,1,2,3 -> 0,85,170,255
    v * 85
}

pub fn encode_texture_data(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    rgba: &RgbaImage,
) -> Result<Vec<u8>> {
    if rgba.width != width || rgba.height != height {
        return Err(IoError::InvalidData {
            context: "Image dimensions don't match requested size".to_string(),
        });
    }

    match format {
        GrcTextureFormat::A8R8G8B8 => {
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    let rgba = rgba.get_pixel(x, y);
                    data.push(rgba[2]);
                    data.push(rgba[1]);
                    data.push(rgba[0]);
                    data.push(rgba[3]);
                }
            }
            Ok(data)
        }
        GrcTextureFormat::A8B8G8R8 => {
            Ok(rgba.data.clone())
        }
        GrcTextureFormat::L8 => {
            let mut data = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                for x in 0..width {
                    let rgba = rgba.get_pixel(x, y);
                    let l = ((rgba[0] as u32 + rgba[1] as u32 + rgba[2] as u32) / 3) as u8;
                    data.push(l);
                }
            }
            Ok(data)
        }
        GrcTextureFormat::A8 => {
            let mut data = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                for x in 0..width {
                    let rgba = rgba.get_pixel(x, y);
                    data.push(rgba[3]);
                }
            }
            Ok(data)
        }
        _ => {
            Err(IoError::InvalidData {
                context: format!("Encoding to {:?} not supported. Use original compressed data for lossless roundtrip.", format),
            })
        }
    }
}

#[cfg(test)]
mod tests;

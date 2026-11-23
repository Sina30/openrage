use texpresso::{Algorithm, Format as BlockFormat, Params};

use crate::codec::texture::{
    encode_texture_data, generate_mip_chain_with, max_mip_levels, AlphaRule, GrcTextureFormat,
    MipFilter, RgbaImage,
};
use crate::error::{IoError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodeQuality {
    Fast,
    #[default]
    Normal,
    High,
}

impl EncodeQuality {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "fast" => Some(Self::Fast),
            "normal" | "default" => Some(Self::Normal),
            "high" | "slow" => Some(Self::High),
            _ => None,
        }
    }

    fn params(self) -> Params {
        Params {
            algorithm: match self {
                Self::Fast => Algorithm::RangeFit,
                Self::Normal => Algorithm::ClusterFit,
                Self::High => Algorithm::IterativeClusterFit,
            },
            ..Params::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextureBuildOptions {
    pub mip_levels: Option<u32>,
    pub filter: MipFilter,
    pub srgb: bool,
    pub quality: EncodeQuality,
    pub allow_npot: bool,
    /// How the colour of the mips follows the alpha of the image.
    pub alpha: AlphaRule,
}

impl Default for TextureBuildOptions {
    fn default() -> Self {
        Self {
            mip_levels: None,
            filter: MipFilter::default(),
            srgb: true,
            quality: EncodeQuality::default(),
            allow_npot: false,
            alpha: AlphaRule::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedTexture {
    pub data: Vec<u8>,
    pub mip_count: u8,
    pub stride: u32,
}

pub fn encode_level(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    rgba: &[u8],
    quality: EncodeQuality,
) -> Result<Vec<u8>> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(IoError::InvalidData {
            context: format!(
                "encode_level got {} bytes, expected {} for a {}x{} RGBA8 level",
                rgba.len(),
                expected,
                width,
                height
            ),
        });
    }

    if let Some(block) = block_format(format) {
        let mut out = vec![0u8; block.compressed_size(width as usize, height as usize)];
        block.compress(
            rgba,
            width as usize,
            height as usize,
            quality.params(),
            &mut out,
        );
        return Ok(out);
    }

    if format == GrcTextureFormat::BC7 {
        return encode_bc7(width, height, rgba, quality);
    }

    let image = RgbaImage {
        width,
        height,
        data: rgba.to_vec(),
        is_srgb: false,
    };
    encode_texture_data(format, width, height, &image)
}

fn block_format(format: GrcTextureFormat) -> Option<BlockFormat> {
    match format {
        GrcTextureFormat::DXT1 => Some(BlockFormat::Bc1),
        GrcTextureFormat::DXT3 => Some(BlockFormat::Bc2),
        GrcTextureFormat::DXT5 => Some(BlockFormat::Bc3),
        GrcTextureFormat::DXT5A => Some(BlockFormat::Bc4),
        GrcTextureFormat::DXN => Some(BlockFormat::Bc5),
        _ => None,
    }
}

pub fn is_encodable(format: GrcTextureFormat) -> bool {
    if block_format(format).is_some() {
        return true;
    }
    matches!(
        format,
        GrcTextureFormat::A8R8G8B8
            | GrcTextureFormat::A8B8G8R8
            | GrcTextureFormat::L8
            | GrcTextureFormat::A8
    ) || (cfg!(feature = "bc7-encode") && format == GrcTextureFormat::BC7)
}

#[cfg(feature = "bc7-encode")]
fn encode_bc7(width: u32, height: u32, rgba: &[u8], quality: EncodeQuality) -> Result<Vec<u8>> {
    use intel_tex_2::{bc7, RgbaSurface};

    let (padded, pw, ph) = pad_to_block_multiple(rgba, width, height, 4);

    let surface = RgbaSurface {
        data: &padded,
        width: pw,
        height: ph,
        stride: pw * 4,
    };
    let settings = match quality {
        EncodeQuality::Fast => bc7::alpha_ultra_fast_settings(),
        EncodeQuality::Normal => bc7::alpha_basic_settings(),
        EncodeQuality::High => bc7::alpha_slow_settings(),
    };
    Ok(bc7::compress_blocks(&settings, &surface))
}

#[cfg(not(feature = "bc7-encode"))]
fn encode_bc7(_width: u32, _height: u32, _rgba: &[u8], _quality: EncodeQuality) -> Result<Vec<u8>> {
    Err(IoError::InvalidData {
        context: "BC7 encoding requires the `bc7-encode` feature of or-io, which is off by \
                  default because it links Intel's ISPC texture compressor"
            .to_string(),
    })
}

#[cfg_attr(not(feature = "bc7-encode"), allow(dead_code))]
fn pad_to_block_multiple(rgba: &[u8], width: u32, height: u32, block: u32) -> (Vec<u8>, u32, u32) {
    let pw = width.div_ceil(block) * block;
    let ph = height.div_ceil(block) * block;
    if pw == width && ph == height {
        return (rgba.to_vec(), width, height);
    }

    let mut out = vec![0u8; (pw as usize) * (ph as usize) * 4];
    for y in 0..ph {
        let sy = y.min(height - 1);
        for x in 0..pw {
            let sx = x.min(width - 1);
            let si = ((sy * width + sx) * 4) as usize;
            let di = ((y * pw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    (out, pw, ph)
}

pub fn encode_texture_with_mips(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    rgba: &RgbaImage,
    mip_levels: u32,
    filter: MipFilter,
) -> Result<EncodedTexture> {
    encode_texture_with_options(
        format,
        width,
        height,
        rgba,
        &TextureBuildOptions {
            mip_levels: Some(mip_levels),
            filter,
            srgb: rgba.is_srgb,
            ..TextureBuildOptions::default()
        },
    )
}

pub fn encode_texture_with_options(
    format: GrcTextureFormat,
    width: u32,
    height: u32,
    rgba: &RgbaImage,
    opts: &TextureBuildOptions,
) -> Result<EncodedTexture> {
    if rgba.width != width || rgba.height != height {
        return Err(IoError::InvalidData {
            context: format!(
                "image is {}x{} but {}x{} was requested",
                rgba.width, rgba.height, width, height
            ),
        });
    }
    if !is_encodable(format) {
        return Err(IoError::InvalidData {
            context: format!("no encoder for {format:?}"),
        });
    }
    if !opts.allow_npot && (!width.is_power_of_two() || !height.is_power_of_two()) {
        return Err(IoError::InvalidData {
            context: format!(
                "{width}x{height} is not power-of-two; GTA V expects power-of-two textures. \
                 Set `allow_npot` to override."
            ),
        });
    }

    let levels = opts
        .mip_levels
        .unwrap_or_else(|| max_mip_levels(width, height))
        .clamp(1, max_mip_levels(width, height));

    let chain = generate_mip_chain_with(
        width,
        height,
        &rgba.data,
        Some(levels),
        opts.filter,
        opts.srgb,
        opts.alpha,
    )?;

    let mut data = Vec::with_capacity(format.total_size(width, height, levels) as usize);
    for (level, pixels) in chain.iter().enumerate() {
        let (w, h) = format.mip_dimensions(width, height, level as u32);
        data.extend_from_slice(&encode_level(format, w, h, pixels, opts.quality)?);
    }

    Ok(EncodedTexture {
        data,
        mip_count: levels as u8,
        stride: format.row_pitch(width),
    })
}

/// The block format to compress an image into when the user has not chosen.
///
/// ⚠ Keyed on whether *any* pixel is non-opaque rather than on the source
/// format: a PNG with an all-255 alpha channel is an opaque image, and BC1
/// stores it in half the bytes BC3 would.
pub fn automatic_format(rgba: &RgbaImage) -> GrcTextureFormat {
    if rgba
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] != 0xFF)
    {
        GrcTextureFormat::DXT5
    } else {
        GrcTextureFormat::DXT1
    }
}

/// What to compress into: a format the caller named, or one the pixels decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatChoice {
    #[default]
    Automatic,
    Fixed(GrcTextureFormat),
}

impl FormatChoice {
    pub fn resolve(self, rgba: &RgbaImage) -> GrcTextureFormat {
        match self {
            Self::Automatic => automatic_format(rgba),
            Self::Fixed(format) => format,
        }
    }

    /// `Automatic` is always encodable: [`automatic_format`] returns only BC1
    /// or BC3, neither of which sits behind an optional feature.
    pub fn is_encodable(self) -> bool {
        match self {
            Self::Automatic => true,
            Self::Fixed(format) => is_encodable(format),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PsnrReport {
    pub overall: f64,
    pub per_channel: [f64; 4],
}

pub fn rgba_psnr(a: &RgbaImage, b: &RgbaImage) -> Result<PsnrReport> {
    if a.width != b.width || a.height != b.height {
        return Err(IoError::InvalidData {
            context: format!(
                "cannot compare a {}x{} image with a {}x{} one",
                a.width, a.height, b.width, b.height
            ),
        });
    }

    let mut sums = [0f64; 4];
    for (pa, pb) in a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0.iter())
    {
        for c in 0..4 {
            let d = pa[c] as f64 - pb[c] as f64;
            sums[c] += d * d;
        }
    }

    let n = (a.width as f64) * (a.height as f64);
    let to_db = |mse: f64| {
        if mse <= 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0f64 * 255.0 / mse).log10()
        }
    };

    Ok(PsnrReport {
        overall: to_db(sums.iter().sum::<f64>() / (n * 4.0)),
        per_channel: [
            to_db(sums[0] / n),
            to_db(sums[1] / n),
            to_db(sums[2] / n),
            to_db(sums[3] / n),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::texture::decode_texture_data;

    fn quadrants(size: u32) -> RgbaImage {
        let mut img = RgbaImage::new(size, size);
        let half = size / 2;
        for y in 0..size {
            for x in 0..size {
                let px = match (x < half, y < half) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [128, 128, 128, 64],
                };
                img.set_pixel(x, y, px);
            }
        }
        img
    }

    #[test]
    fn compressed_sizes_match_format_math() {
        let img = quadrants(16);
        for (format, expected) in [
            (GrcTextureFormat::DXT1, 16 * 16 / 2),
            (GrcTextureFormat::DXT3, 16 * 16),
            (GrcTextureFormat::DXT5, 16 * 16),
            (GrcTextureFormat::DXT5A, 16 * 16 / 2),
            (GrcTextureFormat::DXN, 16 * 16),
        ] {
            let out = encode_level(format, 16, 16, &img.data, EncodeQuality::Normal).unwrap();
            assert_eq!(out.len(), expected, "{format:?} produced the wrong size");
            assert_eq!(out.len(), format.mip_size(16, 16) as usize);
        }
    }

    #[test]
    fn channel_order_survives_encode_decode() {
        let img = quadrants(16);
        let mut formats = vec![
            GrcTextureFormat::DXT1,
            GrcTextureFormat::DXT3,
            GrcTextureFormat::DXT5,
            GrcTextureFormat::A8R8G8B8,
            GrcTextureFormat::A8B8G8R8,
        ];
        if cfg!(feature = "bc7-encode") {
            formats.push(GrcTextureFormat::BC7);
        }
        for format in formats {
            let encoded = encode_level(format, 16, 16, &img.data, EncodeQuality::High).unwrap();
            let decoded = decode_texture_data(format, 16, 16, &encoded).unwrap();

            let red = decoded.get_pixel(3, 3);
            assert!(
                red[0] > 200,
                "{format:?}: red quadrant lost its red: {red:?}"
            );
            assert!(red[1] < 60 && red[2] < 60, "{format:?}: red bled: {red:?}");

            let green = decoded.get_pixel(12, 3);
            assert!(green[1] > 200, "{format:?}: green quadrant: {green:?}");
            assert!(
                green[0] < 60 && green[2] < 60,
                "{format:?}: green bled: {green:?}"
            );

            let blue = decoded.get_pixel(3, 12);
            assert!(blue[2] > 200, "{format:?}: blue quadrant: {blue:?}");
            assert!(
                blue[0] < 60 && blue[1] < 60,
                "{format:?}: blue bled: {blue:?}"
            );
        }
    }

    #[test]
    fn bc3_preserves_alpha() {
        let img = quadrants(16);
        let encoded = encode_level(
            GrcTextureFormat::DXT5,
            16,
            16,
            &img.data,
            EncodeQuality::High,
        )
        .unwrap();
        let decoded = decode_texture_data(GrcTextureFormat::DXT5, 16, 16, &encoded).unwrap();
        assert!(decoded.get_pixel(3, 3)[3] > 240, "opaque quadrant");
        let translucent = decoded.get_pixel(12, 12)[3];
        assert!(
            (translucent as i32 - 64).abs() <= 8,
            "translucent quadrant drifted to {translucent}"
        );
    }

    #[test]
    fn bc4_reads_the_red_channel() {
        let mut img = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.set_pixel(x, y, [(x * 16) as u8, 0, 0, 255]);
            }
        }
        let encoded = encode_level(
            GrcTextureFormat::DXT5A,
            16,
            16,
            &img.data,
            EncodeQuality::High,
        )
        .unwrap();
        let decoded = decode_texture_data(GrcTextureFormat::DXT5A, 16, 16, &encoded).unwrap();
        assert!(decoded.get_pixel(0, 0)[0] < 24);
        assert!(decoded.get_pixel(15, 0)[0] > 230);
    }

    #[test]
    fn bc5_keeps_red_and_green_distinct() {
        // The DXN trap: swapping the two 8-byte halves swaps red and green and
        // survives a symmetric round-trip.
        let mut img = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.set_pixel(x, y, [(x * 16) as u8, 200, 0, 255]);
            }
        }
        let encoded = encode_level(
            GrcTextureFormat::DXN,
            16,
            16,
            &img.data,
            EncodeQuality::High,
        )
        .unwrap();
        let decoded = decode_texture_data(GrcTextureFormat::DXN, 16, 16, &encoded).unwrap();

        assert!(decoded.get_pixel(0, 0)[0] < 24, "R should start low");
        assert!(decoded.get_pixel(15, 0)[0] > 230, "R should ramp up");
        for x in 0..16 {
            let g = decoded.get_pixel(x, 8)[1];
            assert!((g as i32 - 200).abs() <= 8, "G drifted to {g} at x={x}");
        }
    }

    #[test]
    fn psnr_meets_the_workplan_thresholds() {
        let mut img = RgbaImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                img.set_pixel(x, y, [(x * 4) as u8, (y * 4) as u8, 128, 255]);
            }
        }
        let mut cases = vec![
            (GrcTextureFormat::DXT1, 30.0),
            (GrcTextureFormat::DXT5, 32.0),
            (GrcTextureFormat::DXN, 40.0),
        ];
        if cfg!(feature = "bc7-encode") {
            cases.push((GrcTextureFormat::BC7, 40.0));
        }
        for (format, floor) in cases {
            let encoded = encode_level(format, 64, 64, &img.data, EncodeQuality::High).unwrap();
            let decoded = decode_texture_data(format, 64, 64, &encoded).unwrap();

            let report = rgba_psnr(&img, &decoded).unwrap();
            let measured = if format == GrcTextureFormat::DXN {
                report.per_channel[0].min(report.per_channel[1])
            } else {
                report.per_channel[0]
                    .min(report.per_channel[1])
                    .min(report.per_channel[2])
            };
            assert!(
                measured >= floor,
                "{format:?} scored {measured:.1} dB, below the {floor} dB floor"
            );
        }
    }

    #[test]
    fn encoding_is_reproducible() {
        let img = quadrants(32);
        for format in [
            GrcTextureFormat::DXT1,
            GrcTextureFormat::DXT5,
            GrcTextureFormat::DXN,
        ] {
            let a = encode_level(format, 32, 32, &img.data, EncodeQuality::Normal).unwrap();
            let b = encode_level(format, 32, 32, &img.data, EncodeQuality::Normal).unwrap();
            assert_eq!(a, b, "{format:?} is not reproducible");
        }
    }

    #[test]
    fn packed_chain_matches_format_total_size() {
        let img = quadrants(64);
        let encoded =
            encode_texture_with_mips(GrcTextureFormat::DXT5, 64, 64, &img, 7, MipFilter::Box)
                .unwrap();
        assert_eq!(encoded.mip_count, 7);
        assert_eq!(
            encoded.data.len(),
            GrcTextureFormat::DXT5.total_size(64, 64, 7) as usize
        );
        assert_eq!(encoded.stride, GrcTextureFormat::DXT5.row_pitch(64));
    }

    #[test]
    fn npot_is_rejected_unless_allowed() {
        let img = RgbaImage::new(24, 16);
        let strict = TextureBuildOptions::default();
        assert!(
            encode_texture_with_options(GrcTextureFormat::DXT1, 24, 16, &img, &strict).is_err()
        );

        let lenient = TextureBuildOptions {
            allow_npot: true,
            ..TextureBuildOptions::default()
        };
        assert!(
            encode_texture_with_options(GrcTextureFormat::DXT1, 24, 16, &img, &lenient).is_ok()
        );
    }

    #[test]
    fn unsupported_format_names_the_feature() {
        let img = quadrants(8);
        let err = encode_level(GrcTextureFormat::BC7, 8, 8, &img.data, EncodeQuality::Fast);
        #[cfg(feature = "bc7-encode")]
        assert!(err.is_ok());
        #[cfg(not(feature = "bc7-encode"))]
        {
            let message = err.unwrap_err().to_string();
            assert!(message.contains("bc7-encode"), "unhelpful error: {message}");
        }
    }

    #[test]
    fn psnr_is_infinite_for_identical_images() {
        let img = quadrants(8);
        let report = rgba_psnr(&img, &img).unwrap();
        assert!(report.overall.is_infinite());
    }

    #[test]
    fn padding_replicates_edge_texels() {
        let rgba: Vec<u8> = [[1u8, 2, 3, 4], [5, 6, 7, 8]].concat();
        let (padded, pw, ph) = pad_to_block_multiple(&rgba, 2, 1, 4);
        assert_eq!((pw, ph), (4, 4));
        assert_eq!(&padded[0..4], &[1, 2, 3, 4]);
        assert_eq!(&padded[4..8], &[5, 6, 7, 8]);
        assert_eq!(&padded[8..12], &[5, 6, 7, 8]);
        assert_eq!(&padded[16..20], &[1, 2, 3, 4]);
    }
}

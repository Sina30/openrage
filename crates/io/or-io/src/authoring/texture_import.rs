//! Reading an image file into the pixels a texture is built from.
//!
//! [`crate::codec::texture::png`] is the interchange that also writes back out; every
//! other format here is an input only, because exporting to one would lose
//! either the alpha channel or the exactness a round trip is judged on.
//!
//! ⚠ **No format here states its colour space in a way this reader trusts**,
//! so all of them report sRGB and the caller overrides it. A normal map or a
//! packed mask imported at its face value is resampled through a gamma curve
//! that was never in the data, and the symptom is a mip chain that looks
//! subtly wrong rather than one that fails.
//!
//! The game's formats hold 8 bits a channel, so an image of more is brought down by a rule the
//! caller states ([`Levels`]) and never by a guess: a 16-bit image is scaled over its full range
//! unless a clamp is stated, and floating-point light (EXR, HDR, a float TIFF) is refused until a
//! clamp or a tone map is.

use std::path::Path;

use image::{DynamicImage, ImageFormat};

use super::psd;
use crate::codec::texture::{decode_texture_data, format_of_dds, HdrImage, RgbaImage, Tonemap};
use crate::dds::Dds;
use crate::error::{IoError, Result};

/// An image file a texture can be imported from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Png,
    Jpeg,
    Bmp,
    Tga,
    Tiff,
    Dds,
    /// The flattened image a Photoshop file stores after its layers.
    Psd,
    /// Radiance HDR, floating-point light.
    Hdr,
    /// OpenEXR, floating-point light.
    Exr,
}

impl SourceFormat {
    pub const ALL: [SourceFormat; 9] = [
        Self::Png,
        Self::Jpeg,
        Self::Bmp,
        Self::Tga,
        Self::Tiff,
        Self::Dds,
        Self::Psd,
        Self::Hdr,
        Self::Exr,
    ];

    /// Every extension an import dialog should offer.
    pub const EXTENSIONS: [&'static str; 11] = [
        "png", "jpg", "jpeg", "bmp", "tga", "tif", "tiff", "dds", "psd", "hdr", "exr",
    ];

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "bmp" => Some(Self::Bmp),
            "tga" => Some(Self::Tga),
            "tif" | "tiff" => Some(Self::Tiff),
            "dds" => Some(Self::Dds),
            "psd" => Some(Self::Psd),
            "hdr" => Some(Self::Hdr),
            "exr" => Some(Self::Exr),
            _ => None,
        }
    }

    pub fn of_path(path: &Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::Tga => "TGA",
            Self::Tiff => "TIFF",
            Self::Dds => "DDS",
            Self::Psd => "PSD",
            Self::Hdr => "HDR",
            Self::Exr => "EXR",
        }
    }
}

/// How many bits a channel of a source image states, which decides what a rule may ask of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Depth {
    /// 8 bits, or a DDS: taken as it is.
    Eight,
    /// 16 bits an integer: scaled over its full range.
    Sixteen,
    /// Floating-point light, which 8 bits cannot hold.
    Float,
}

/// How a source of more than 8 bits a channel is brought down to 8; an 8-bit source ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Levels {
    /// The part of the source's range that is kept, stretched over the 8 bits and cut outside: a
    /// 16-bit value is a fraction of 65535, floating-point light is the light itself.
    pub clamp: Option<(f32, f32)>,
    /// Floating-point light made display-referred as `x / (1 + x)` after being scaled by two to
    /// this many stops.
    pub reinhard: Option<f32>,
    /// Whether the texture is stored sRGB, which floating-point (linear) light is encoded for.
    pub srgb: bool,
}

/// A source image brought to 8 bits, and the depth it had.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub image: RgbaImage,
    pub depth: Depth,
}

/// Decode a source image's bytes to RGBA8.
///
/// ⚠ A `.dds` is decoded from **mip 0** and re-compressed from there, not
/// copied through. Its payload is already in a GPU format, and passing those
/// bytes straight into a dictionary would carry a pixel format, a mip count
/// and a row pitch that the entry's own header does not describe.
///
/// A 16-bit image is scaled over its full range, and floating-point light is refused: use
/// [`decode_levels`] to state how it comes down.
pub fn decode(format: SourceFormat, bytes: &[u8]) -> Result<RgbaImage> {
    decode_format(format, bytes, &Levels::default()).map(|d| d.image)
}

/// `bytes` as the image its extension names, brought to 8 bits by `levels`.
pub fn decode_levels(path: &Path, bytes: &[u8], levels: &Levels) -> Result<Decoded> {
    decode_format(of_path_or_refuse(path)?, bytes, levels)
}

/// How many bits a channel of the image in `bytes` states, read from its header and not by decoding its pixels.
pub fn depth_of(path: &Path, bytes: &[u8]) -> Result<Depth> {
    use image::ImageDecoder;
    let format = of_path_or_refuse(path)?;
    let kind = match format {
        SourceFormat::Dds => return Ok(Depth::Eight),
        SourceFormat::Psd => return psd::depth(bytes),
        SourceFormat::Png => ImageFormat::Png,
        SourceFormat::Jpeg => ImageFormat::Jpeg,
        SourceFormat::Bmp => ImageFormat::Bmp,
        SourceFormat::Tga => ImageFormat::Tga,
        SourceFormat::Tiff => ImageFormat::Tiff,
        SourceFormat::Hdr => ImageFormat::Hdr,
        SourceFormat::Exr => ImageFormat::OpenExr,
    };
    let decoder = image::ImageReader::with_format(std::io::Cursor::new(bytes), kind)
        .into_decoder()
        .map_err(|e| IoError::InvalidData {
            context: format!("failed to read the header of {}: {e}", format.label()),
        })?;
    Ok(match decoder.color_type() {
        image::ColorType::L16
        | image::ColorType::La16
        | image::ColorType::Rgb16
        | image::ColorType::Rgba16 => Depth::Sixteen,
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => Depth::Float,
        _ => Depth::Eight,
    })
}

fn of_path_or_refuse(path: &Path) -> Result<SourceFormat> {
    SourceFormat::of_path(path).ok_or_else(|| IoError::InvalidData {
        context: format!(
            "{}: expected one of {}",
            path.display(),
            SourceFormat::EXTENSIONS.map(|e| format!(".{e}")).join(", ")
        ),
    })
}

pub fn decode_path(path: &Path, bytes: &[u8]) -> Result<RgbaImage> {
    decode(of_path_or_refuse(path)?, bytes)
}

pub fn load(path: impl AsRef<Path>) -> Result<RgbaImage> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| IoError::FileReadFailed {
        path: path.to_path_buf(),
        source,
    })?;
    decode_path(path, &bytes)
}

fn decode_format(format: SourceFormat, bytes: &[u8], levels: &Levels) -> Result<Decoded> {
    let from = |kind| {
        image::load_from_memory_with_format(bytes, kind).map_err(|e| IoError::InvalidData {
            context: format!("failed to decode {}: {e}", format.label()),
        })
    };
    let image = match format {
        SourceFormat::Dds => {
            return Ok(Decoded {
                image: decode_dds(&Dds::from_bytes(bytes)?)?,
                depth: Depth::Eight,
            })
        }
        SourceFormat::Psd => psd::flatten(bytes)?,
        SourceFormat::Png => from(ImageFormat::Png)?,
        SourceFormat::Jpeg => from(ImageFormat::Jpeg)?,
        SourceFormat::Bmp => from(ImageFormat::Bmp)?,
        SourceFormat::Tga => from(ImageFormat::Tga)?,
        SourceFormat::Tiff => from(ImageFormat::Tiff)?,
        SourceFormat::Hdr => from(ImageFormat::Hdr)?,
        SourceFormat::Exr => from(ImageFormat::OpenExr)?,
    };
    bring_to_eight(image, levels, format.label())
}

/// The kept part of `x` between `lo` and `hi`, as a fraction of it.
fn window(x: f32, (lo, hi): (f32, f32)) -> f32 {
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn bring_to_eight(image: DynamicImage, levels: &Levels, label: &str) -> Result<Decoded> {
    let refuse = |why: &str| IoError::InvalidData {
        context: format!("{label}: {why}"),
    };
    if let Some((lo, hi)) = levels.clamp {
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return Err(refuse(&format!("a clamp of {lo} to {hi} is not a range")));
        }
    }
    let (width, height) = (image.width(), image.height());
    let (data, depth) = match image {
        DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => {
            if levels.reinhard.is_some() {
                return Err(refuse(
                    "has 16 bits an integer, which is not light a tone map applies to",
                ));
            }
            let data = image.into_rgba16().into_raw();
            let scaled = |v: u16| ((u32::from(v) + 128) / 257) as u8;
            let eight = match levels.clamp {
                None => data.iter().map(|&v| scaled(v)).collect(),
                // The colour is windowed and alpha, which is coverage, is scaled as it is.
                Some(range) => data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|t| {
                        let kept =
                            |v: u16| (window(f32::from(v) / 65535.0, range) * 255.0 + 0.5) as u8;
                        [kept(t[0]), kept(t[1]), kept(t[2]), scaled(t[3])]
                    })
                    .collect(),
            };
            (eight, Depth::Sixteen)
        }
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
            let how = match (levels.clamp, levels.reinhard) {
                (None, None) => return Err(refuse(
                    "holds floating-point light, which 8 bits cannot: state a clamp or a tone map",
                )),
                (Some(_), Some(_)) => {
                    return Err(refuse(
                        "is given a clamp and a tone map, and light is brought down by one",
                    ))
                }
                (range, stops) => (range, stops),
            };
            let mut light = HdrImage {
                width,
                height,
                data: image.into_rgba32f().into_raw(),
            };
            for texel in light.data.as_chunks_mut::<4>().0 {
                for v in &mut texel[..3] {
                    let x = if v.is_nan() { 0.0 } else { *v };
                    *v = match how {
                        (Some(range), _) => window(x, range),
                        (None, Some(stops)) => {
                            let x = (x * stops.exp2()).max(0.0);
                            x / (1.0 + x)
                        }
                        (None, None) => 0.0,
                    };
                }
            }
            let data = if levels.srgb {
                light.tonemap(Tonemap::ClampSrgb).data
            } else {
                let eight = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                light.data.iter().map(|&v| eight(v)).collect()
            };
            (data, Depth::Float)
        }
        other => (other.into_rgba8().into_raw(), Depth::Eight),
    };
    Ok(Decoded {
        image: RgbaImage {
            width,
            height,
            data,
            is_srgb: true,
        },
        depth,
    })
}

fn decode_dds(dds: &Dds) -> Result<RgbaImage> {
    let (format, srgb) = format_of_dds(dds)?;
    let width = dds.header.width.max(1);
    let height = dds.header.height.max(1);
    let mip_count = dds.header.mip_map_count.max(1);
    let expected = format.total_size(width, height, mip_count) as usize;
    if dds.data.len() < expected {
        return Err(IoError::InvalidData {
            context: format!(
                "DDS payload is {} bytes but {width}x{height} {format:?} with {mip_count} \
                 mips needs {expected}",
                dds.data.len(),
            ),
        });
    }
    let level = format.mip_size(width, height) as usize;
    let mut image = decode_texture_data(format, width, height, &dds.data[..level])?;
    image.is_srgb = srgb;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::texture::encode_png;

    fn gradient() -> RgbaImage {
        let mut image = RgbaImage::new(8, 4);
        for y in 0..4 {
            for x in 0..8 {
                image.set_pixel(x, y, [(x * 32) as u8, (y * 64) as u8, 17, 255]);
            }
        }
        image
    }

    /// ⚠ **The fixture is encoded from RGB, not RGBA.** JPEG has no alpha
    /// channel, and `image` refuses an `Rgba8` buffer for it outright rather
    /// than dropping the channel — so a fixture built the obvious way makes
    /// this suite fail in its own setup. The gradient's alpha is opaque
    /// everywhere, so nothing under test is lost.
    fn reencode(format: ImageFormat) -> Vec<u8> {
        let source = gradient();
        let rgb: Vec<u8> = source
            .data
            .chunks(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_raw(source.width, source.height, rgb)
            .expect("the buffer is width * height * 3")
            .write_to(&mut out, format)
            .expect("the encoder is compiled in");
        out.into_inner()
    }

    #[test]
    fn every_offered_extension_names_a_format_that_decodes() {
        for extension in SourceFormat::EXTENSIONS {
            assert!(
                SourceFormat::from_extension(extension).is_some(),
                "an import dialog offers .{extension} and nothing decodes it"
            );
        }
        for format in SourceFormat::ALL {
            assert!(
                SourceFormat::EXTENSIONS
                    .iter()
                    .any(|e| SourceFormat::from_extension(e) == Some(format)),
                "{format:?} can be decoded and no dialog offers it"
            );
        }
    }

    /// ⚠ JPEG, BMP and TGA have to decode to the same shape PNG does, or
    /// offering them is a menu entry rather than a feature.
    #[test]
    fn jpeg_bmp_and_tga_decode_to_the_same_shape_png_does() {
        let png = decode(SourceFormat::Png, &encode_png(&gradient()).unwrap()).unwrap();
        assert_eq!(png.data, gradient().data, "PNG is the lossless baseline");

        for (format, encoded) in [
            (SourceFormat::Bmp, reencode(ImageFormat::Bmp)),
            (SourceFormat::Tga, reencode(ImageFormat::Tga)),
            (SourceFormat::Jpeg, reencode(ImageFormat::Jpeg)),
        ] {
            let decoded = decode(format, &encoded)
                .unwrap_or_else(|e| panic!("{format:?} should decode: {e}"));
            assert_eq!(
                (decoded.width, decoded.height),
                (8, 4),
                "{format:?} lost the dimensions"
            );
            assert_eq!(decoded.data.len(), 8 * 4 * 4, "{format:?} is not RGBA8");
            assert!(decoded.is_srgb);
        }
    }

    #[test]
    fn a_tiff_decodes_through_the_one_list_under_either_extension() {
        let encoded = reencode(ImageFormat::Tiff);
        for path in ["art/albedo.tif", "art/albedo.TIFF"] {
            let decoded = decode_path(Path::new(path), &encoded)
                .unwrap_or_else(|e| panic!("{path} should decode: {e}"));
            assert_eq!((decoded.width, decoded.height), (8, 4), "{path}");
            assert_eq!(decoded.data, gradient().data, "{path}: TIFF is lossless");
            assert!(decoded.is_srgb, "{path}");
        }
    }

    #[test]
    fn an_extension_nothing_decodes_names_what_is_accepted() {
        let error = decode_path(Path::new("art/wheel.xcf"), b"")
            .unwrap_err()
            .to_string();
        assert!(error.contains(".png"), "unhelpful error: {error}");
        assert!(error.contains(".dds"), "unhelpful error: {error}");
    }

    #[test]
    fn bytes_that_are_not_the_named_format_are_rejected() {
        let error = decode(SourceFormat::Jpeg, b"not a jpeg at all")
            .unwrap_err()
            .to_string();
        assert!(error.contains("JPEG"), "unhelpful error: {error}");
    }

    fn encoded(image: DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut out, format)
            .expect("the encoder is compiled in");
        out.into_inner()
    }

    fn deep16(values: [u16; 4]) -> Vec<u8> {
        let buffer = image::ImageBuffer::from_raw(1, 1, values.to_vec()).expect("one texel");
        encoded(DynamicImage::ImageRgba16(buffer), ImageFormat::Png)
    }

    fn light(values: [f32; 3], format: ImageFormat) -> Vec<u8> {
        let buffer = image::ImageBuffer::from_raw(1, 1, values.to_vec()).expect("one texel");
        encoded(DynamicImage::ImageRgb32F(buffer), format)
    }

    fn colour(bytes: &[u8], format: SourceFormat, levels: &Levels) -> (Decoded, [u8; 3]) {
        let decoded = decode_format(format, bytes, levels).expect("it decodes");
        let data = &decoded.image.data;
        let rgb = [data[0], data[1], data[2]];
        (decoded, rgb)
    }

    #[test]
    fn the_depth_of_an_image_is_read_from_its_header() {
        let eight = encoded(DynamicImage::ImageRgba8(gradient_rgba()), ImageFormat::Png);
        let sixteen = deep16([1, 2, 3, 4]);
        let float = light([0.5, 0.5, 0.5], ImageFormat::Hdr);
        assert_eq!(depth_of(Path::new("a.png"), &eight).unwrap(), Depth::Eight);
        assert_eq!(
            depth_of(Path::new("a.png"), &sixteen).unwrap(),
            Depth::Sixteen
        );
        assert_eq!(depth_of(Path::new("a.hdr"), &float).unwrap(), Depth::Float);
        let mut psd16 = vec![0u8; 26];
        psd16[..4].copy_from_slice(b"8BPS");
        psd16[22..24].copy_from_slice(&16u16.to_be_bytes());
        assert_eq!(
            depth_of(Path::new("a.psd"), &psd16).unwrap(),
            Depth::Sixteen
        );
        assert!(depth_of(Path::new("a.png"), b"not a png").is_err());
        assert!(depth_of(Path::new("a.xyz"), &eight).is_err());
    }

    fn gradient_rgba() -> image::RgbaImage {
        let source = gradient();
        image::RgbaImage::from_raw(source.width, source.height, source.data).expect("sized")
    }

    #[test]
    fn a_16_bit_image_is_scaled_over_its_full_range_and_a_clamp_cuts_a_window_of_it() {
        let png = deep16([0x00FF, 0x8080, 0xFFFF, 0xFFFF]);
        let (decoded, rgb) = colour(&png, SourceFormat::Png, &Levels::default());
        assert_eq!(decoded.depth, Depth::Sixteen);
        assert_eq!(
            rgb,
            [1, 128, 255],
            "rounded, not shifted: 0x00FF is nearer 1 than 0"
        );

        // The lower half of the range kept: the middle value is the top, and above it is cut.
        let half = Levels {
            clamp: Some((0.0, 0.5)),
            ..Levels::default()
        };
        let (kept, rgb) = colour(&png, SourceFormat::Png, &half);
        assert_eq!(
            rgb,
            [2, 255, 255],
            "0x00FF stretched over half the range is 0x01FE"
        );
        assert_eq!(
            kept.image.data[3], 255,
            "alpha is coverage, which a window leaves alone"
        );

        let bad = Levels {
            clamp: Some((0.5, 0.5)),
            ..Levels::default()
        };
        let error = decode_format(SourceFormat::Png, &png, &bad)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a range"), "{error}");
        let tone = Levels {
            reinhard: Some(0.0),
            ..Levels::default()
        };
        let error = decode_format(SourceFormat::Png, &png, &tone)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not light"), "{error}");
    }

    #[test]
    fn floating_point_light_is_refused_until_a_clamp_or_a_tone_map_is_stated() {
        let samples = [
            (SourceFormat::Hdr, light([0.5, 2.0, 0.0], ImageFormat::Hdr)),
            (
                SourceFormat::Exr,
                light([0.5, 2.0, 0.0], ImageFormat::OpenExr),
            ),
            (
                SourceFormat::Tiff,
                light([0.5, 2.0, 0.0], ImageFormat::Tiff),
            ),
        ];
        for (format, bytes) in &samples {
            let error = decode(*format, bytes).unwrap_err().to_string();
            assert!(
                error.contains("floating-point light") && error.contains("clamp"),
                "{format:?}: {error}"
            );
            let both = Levels {
                clamp: Some((0.0, 1.0)),
                reinhard: Some(0.0),
                ..Levels::default()
            };
            assert!(decode_format(*format, bytes, &both).is_err(), "{format:?}");
        }
    }

    #[test]
    fn light_is_clamped_or_tone_mapped_then_encoded_for_the_colour_space_the_texture_has() {
        let bytes = light([0.5, 2.0, 0.0], ImageFormat::OpenExr);
        let clamp = |srgb| Levels {
            clamp: Some((0.0, 1.0)),
            srgb,
            ..Levels::default()
        };
        // Linear: half the light is half the byte, and more than the range is the top.
        let (decoded, rgb) = colour(&bytes, SourceFormat::Exr, &clamp(false));
        assert_eq!(decoded.depth, Depth::Float);
        assert_eq!(rgb, [128, 255, 0]);
        // sRGB: the same light is brighter in the bytes, since the curve gives the dark more of them.
        let (_, rgb) = colour(&bytes, SourceFormat::Exr, &clamp(true));
        assert_eq!(rgb, [188, 255, 0]);

        // Reinhard is x / (1 + x): 0.5 is a third and 2.0 two thirds, and two stops up is four times the light.
        let reinhard = |stops| Levels {
            reinhard: Some(stops),
            ..Levels::default()
        };
        let (_, rgb) = colour(&bytes, SourceFormat::Exr, &reinhard(0.0));
        assert_eq!(rgb, [85, 170, 0]);
        let (_, rgb) = colour(&bytes, SourceFormat::Exr, &reinhard(2.0));
        assert_eq!(rgb, [170, 227, 0]);

        // A window of the light stretches it: 0.5 to 2.0 is the whole byte range.
        let window = Levels {
            clamp: Some((0.5, 2.0)),
            ..Levels::default()
        };
        let (_, rgb) = colour(&bytes, SourceFormat::Exr, &window);
        assert_eq!(rgb, [0, 255, 0]);
    }

    /// A one-row PSD, planar, in the layout Photoshop writes it.
    fn psd(width: u32, depth: u16, mode: u16, planes: &[Vec<u8>], packed: bool) -> Vec<u8> {
        psd_layers(width, depth, mode, planes, packed, &[9u8; 5])
    }

    /// [`psd`] with the bytes of its Layer and Mask Information section given.
    fn psd_layers(
        width: u32,
        depth: u16,
        mode: u16,
        planes: &[Vec<u8>],
        packed: bool,
        layers: &[u8],
    ) -> Vec<u8> {
        let mut out = b"8BPS".to_vec();
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&(planes.len() as u16).to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&depth.to_be_bytes());
        out.extend_from_slice(&mode.to_be_bytes());
        // Colour mode data, image resources and layers: each a length and the bytes it counts.
        for section in [&[1u8, 2, 3][..], &[][..], layers] {
            out.extend_from_slice(&(section.len() as u32).to_be_bytes());
            out.extend_from_slice(section);
        }
        if packed {
            out.extend_from_slice(&1u16.to_be_bytes());
            // A run where a plane repeats one byte, and the bytes as they are otherwise.
            let rows: Vec<Vec<u8>> = planes
                .iter()
                .map(|plane| {
                    if plane.iter().all(|b| *b == plane[0]) {
                        vec![(1 - plane.len() as i16) as u8, plane[0]]
                    } else {
                        let mut row = vec![(plane.len() - 1) as u8];
                        row.extend_from_slice(plane);
                        row
                    }
                })
                .collect();
            for row in &rows {
                out.extend_from_slice(&(row.len() as u16).to_be_bytes());
            }
            rows.into_iter().for_each(|row| out.extend(row));
        } else {
            out.extend_from_slice(&0u16.to_be_bytes());
            planes.iter().for_each(|plane| out.extend_from_slice(plane));
        }
        out
    }

    #[test]
    fn a_psd_reads_as_the_merged_image_it_stores_raw_or_packed() {
        let planes = [
            vec![10, 20, 30],
            vec![40, 50, 60],
            vec![70, 80, 90],
            vec![255, 128, 0],
        ];
        for packed in [false, true] {
            let decoded = decode(SourceFormat::Psd, &psd(3, 8, 3, &planes, packed)).unwrap();
            assert_eq!((decoded.width, decoded.height), (3, 1));
            assert_eq!(&decoded.data[..4], &[10, 40, 70, 255], "packed: {packed}");
            assert_eq!(&decoded.data[4..8], &[20, 50, 80, 128]);
            assert_eq!(&decoded.data[8..], &[30, 60, 90, 0]);
        }
        // Three channels are opaque, and a gray one is all three colours.
        let opaque = decode(SourceFormat::Psd, &psd(3, 8, 3, &planes[..3], false)).unwrap();
        assert!(opaque.data.chunks(4).all(|p| p[3] == 255));
        let gray = decode(SourceFormat::Psd, &psd(3, 8, 1, &planes[..1], true)).unwrap();
        assert_eq!(&gray.data[..4], &[10, 10, 10, 255]);
    }

    #[test]
    fn a_16_bit_psd_is_a_16_bit_image_and_scales_like_one() {
        let words = |v: [u16; 2]| v.iter().flat_map(|w| w.to_be_bytes()).collect::<Vec<u8>>();
        let planes = [
            words([0, 0xFFFF]),
            words([0x8080, 0]),
            words([0xFFFF, 0x8080]),
        ];
        let bytes = psd(2, 16, 3, &planes, false);
        let decoded = decode_format(SourceFormat::Psd, &bytes, &Levels::default()).unwrap();
        assert_eq!(decoded.depth, Depth::Sixteen);
        assert_eq!(
            &decoded.image.data[..8],
            &[0, 128, 255, 255, 255, 0, 128, 255]
        );
    }

    /// The Layer and Mask Information of a file with no layers but a count, which is negative when the
    /// merged image carries its transparency.
    fn layer_info(count: i16) -> Vec<u8> {
        let mut out = 2u32.to_be_bytes().to_vec();
        out.extend_from_slice(&count.to_be_bytes());
        out
    }

    /// The same count where a 16-bit document keeps it: no layer info, no global mask, then an `Lr16` block.
    fn lr16_layer_info(count: i16) -> Vec<u8> {
        let mut out = vec![0; 8];
        out.extend_from_slice(b"8BIMLr16");
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out
    }

    #[test]
    fn a_psd_with_transparency_has_its_white_taken_back_out() {
        // Photoshop stores the merged colour of a texel of alpha a as 255 - (255 - c) * a / 255, which the
        // negative layer count marks. The first two texels are ones from Photoshop files and the colours
        // ImageMagick reads for them; an opaque texel stays and so does a clear one.
        let planes = [
            vec![225, 239, 7, 255],
            vec![228, 230, 8, 255],
            vec![238, 226, 9, 255],
            vec![32, 35, 255, 0],
        ];
        let expected: [[u8; 4]; 4] = [
            [16, 40, 120, 32],
            [138, 73, 44, 35],
            [7, 8, 9, 255],
            [255, 255, 255, 0],
        ];
        for packed in [false, true] {
            for count in [-1, -26] {
                let bytes = psd_layers(4, 8, 3, &planes, packed, &layer_info(count));
                let decoded = decode(SourceFormat::Psd, &bytes).unwrap();
                let texels: Vec<[u8; 4]> = decoded
                    .data
                    .chunks(4)
                    .map(|p| p.try_into().unwrap())
                    .collect();
                assert_eq!(texels, expected, "packed: {packed}, layer count {count}");
            }
        }
    }

    #[test]
    fn a_16_bit_psd_keeps_its_layer_count_in_the_lr16_block_and_is_unmatted_too() {
        // Black under alpha 0x8080 is stored as 0xFFFF - 0x8080, and comes back as black.
        let words = |v: [u16; 2]| v.iter().flat_map(|w| w.to_be_bytes()).collect::<Vec<u8>>();
        let planes = [
            words([0xFFFF - 0x8080, 0xFFFF]),
            words([0xFFFF - 0x8080, 0xFFFF]),
            words([0xFFFF - 0x8080, 0xFFFF]),
            words([0x8080, 0xFFFF]),
        ];
        let bytes = psd_layers(2, 16, 3, &planes, false, &lr16_layer_info(-3));
        let decoded = decode(SourceFormat::Psd, &bytes).unwrap();
        assert_eq!(&decoded.data, &[0, 0, 0, 128, 255, 255, 255, 255]);
    }

    #[test]
    fn an_extra_channel_of_a_file_with_no_negative_count_is_straight_alpha() {
        let planes = [vec![225], vec![228], vec![238], vec![32]];
        for layers in [layer_info(3), lr16_layer_info(3), layer_info(0), vec![]] {
            let bytes = psd_layers(1, 8, 3, &planes, false, &layers);
            let decoded = decode(SourceFormat::Psd, &bytes).unwrap();
            assert_eq!(&decoded.data, &[225, 228, 238, 32], "{layers:?}");
        }
    }

    #[test]
    fn a_psd_this_build_does_not_read_says_what_it_is() {
        let planes = [vec![1, 2, 3]];
        let refused = |bytes: Vec<u8>| decode(SourceFormat::Psd, &bytes).unwrap_err().to_string();
        assert!(refused(psd(3, 8, 4, &planes, false)).contains("CMYK"));
        assert!(refused(psd(3, 32, 3, &planes, false)).contains("32 bits"));
        let mut zip = psd(3, 8, 1, &planes, false);
        let at = zip.len() - 3 - 2;
        zip[at..at + 2].copy_from_slice(&2u16.to_be_bytes());
        assert!(refused(zip).contains("zip"));
        let mut big = psd(3, 8, 1, &planes, false);
        big[4..6].copy_from_slice(&2u16.to_be_bytes());
        assert!(refused(big).contains("PSB"));
        assert!(refused(b"not a psd".to_vec()).contains("8BPS"));
        let cut = psd(
            3,
            8,
            3,
            &[vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]],
            false,
        );
        assert!(refused(cut[..cut.len() - 1].to_vec()).contains("ends before"));
    }
}

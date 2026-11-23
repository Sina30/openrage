//! Texture pixels: the format tables, BCn and packed decode, mip chains and
//! PNG, over any texture that implements [`TextureSource`].

pub mod format;
pub mod image;
pub mod mipmap;
pub mod png;

#[cfg(feature = "dds")]
pub use format::format_of_dds;
pub use format::{fourcc, DecodeSupport, DxgiFormat, GrcDX11InfoBits, GrcTextureFormat};
pub use image::{
    decode_texture_data, decode_texture_hdr, encode_texture_data, Channel, ChannelView, HdrImage,
    RgbaImage, Tonemap, MAX_DECODE_DIMENSION,
};
pub use mipmap::{
    generate_mip_chain, generate_mip_chain_with, max_mip_levels, resize_rgba, AlphaRule, MipFilter,
};
pub use png::{decode_png, encode_png, load_png, save_png};

use crate::error::{IoError, Result};

/// What the codec reads of a texture: its pixel format and its stored mip chain.
pub trait TextureSource {
    fn pixel_format(&self) -> GrcTextureFormat;
    fn is_srgb(&self) -> bool;
    /// The first stored mip's width and height.
    fn stored_size(&self) -> (u32, u32);
    fn stored_levels(&self) -> u32;
    /// Every stored mip, largest first.
    fn payload(&self) -> &[u8];
}

/// One stored mip of `texture`, decoded to RGBA8 and marked with its colour space.
pub fn decode_level<T: TextureSource + ?Sized>(texture: &T, level: u32) -> Result<RgbaImage> {
    let levels = texture.stored_levels().max(1);
    if level >= levels {
        return Err(IoError::InvalidData {
            context: format!("no mip level {level}: {levels} are stored"),
        });
    }
    let format = texture.pixel_format();
    let (width, height) = texture.stored_size();
    let range = format.mip_level_range(width, height, level);
    let payload = texture.payload();
    let Some(bytes) = payload.get(range.clone()) else {
        return Err(IoError::InvalidData {
            context: format!(
                "mip level {level} needs bytes {range:?} and the payload is {} bytes",
                payload.len()
            ),
        });
    };
    let (w, h) = format.mip_dimensions(width, height, level);
    let mut image = decode_texture_data(format, w, h, bytes)?;
    image.is_srgb = texture.is_srgb();
    Ok(image)
}

/// `texture`'s stored mips as a `.dds`; sRGB, or a format no FourCC names, takes the DX10 header.
#[cfg(feature = "dds")]
pub fn to_dds<T: TextureSource + ?Sized>(texture: &T) -> Result<crate::dds::Dds> {
    use crate::dds::{
        caps, header_flags, pixel_format_flags, Dds, DdsHeader, DdsHeaderDxt10, DdsPixelFormat,
        FOURCC_DX10,
    };

    let (width, height) = texture.stored_size();
    let mip_count = texture.stored_levels();
    let format = texture.pixel_format();
    let payload = texture.payload();
    let expected = format.total_size(width, height, mip_count) as usize;
    if payload.len() < expected {
        return Err(IoError::InvalidData {
            context: format!(
                "payload is {} bytes but {width}x{height} {format:?} with {mip_count} mips \
                 needs {expected}",
                payload.len(),
            ),
        });
    }

    let compressed = format.is_compressed();
    let mut flags =
        header_flags::CAPS | header_flags::HEIGHT | header_flags::WIDTH | header_flags::PIXELFORMAT;
    flags |= if compressed {
        header_flags::LINEARSIZE
    } else {
        header_flags::PITCH
    };
    let mut surface_caps = caps::TEXTURE;
    if mip_count > 1 {
        flags |= header_flags::MIPMAPCOUNT;
        surface_caps |= caps::COMPLEX | caps::MIPMAP;
    }

    let srgb = texture.is_srgb();
    let legacy_fourcc = if srgb { None } else { format.to_fourcc() };
    let fourcc = |four_cc| DdsPixelFormat {
        flags: pixel_format_flags::FOURCC,
        four_cc,
        ..DdsPixelFormat::default()
    };
    let masks =
        |flags, rgb_bit_count, [r_bit_mask, g_bit_mask, b_bit_mask, a_bit_mask]: [u32; 4]| {
            DdsPixelFormat {
                flags,
                rgb_bit_count,
                r_bit_mask,
                g_bit_mask,
                b_bit_mask,
                a_bit_mask,
                ..DdsPixelFormat::default()
            }
        };
    let rgba = pixel_format_flags::RGB | pixel_format_flags::ALPHAPIXELS;
    let pixel_format = match (legacy_fourcc, format) {
        (Some(four_cc), _) => fourcc(four_cc),
        (None, _) if compressed || srgb => fourcc(FOURCC_DX10),
        (None, GrcTextureFormat::A8R8G8B8) => masks(
            rgba,
            32,
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000],
        ),
        (None, GrcTextureFormat::A8B8G8R8) => masks(
            rgba,
            32,
            [0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000],
        ),
        (None, GrcTextureFormat::L8) => masks(pixel_format_flags::LUMINANCE, 8, [0xFF, 0, 0, 0]),
        (None, GrcTextureFormat::A8) => masks(pixel_format_flags::ALPHA, 8, [0, 0, 0, 0xFF]),
        (None, GrcTextureFormat::R16F) => fourcc(FOURCC_DX10),
        (None, other) => {
            return Err(IoError::InvalidData {
                context: format!("no DDS pixel format mapping for {other:?}"),
            })
        }
    };
    let header = DdsHeader {
        flags,
        height,
        width,
        pitch_or_linear_size: if compressed {
            format.mip_size(width, height)
        } else {
            format.row_pitch(width)
        },
        mip_map_count: mip_count,
        pixel_format,
        caps: surface_caps,
        ..DdsHeader::default()
    };
    let dxt10 = header.names_dx10().then(|| DdsHeaderDxt10 {
        dxgi_format: format.to_dxgi(srgb) as u32,
        resource_dimension: 3,
        misc_flag: 0,
        array_size: 1,
        misc_flags2: 0,
    });
    Ok(Dds {
        header,
        dxt10,
        data: payload[..expected].to_vec(),
    })
}

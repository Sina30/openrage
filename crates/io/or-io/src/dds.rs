//! DirectDraw Surface files: the header, the DX10 extension and the payload
//! after them. Only a texture format knows its block size, so the payload's
//! length is the caller's to check.

use std::path::Path;

use crate::error::{IoError, Result};

/// `DDS `, little-endian.
pub const DDS_MAGIC: u32 = 0x2053_4444;
/// `DX10`: the FourCC that says a [`DdsHeaderDxt10`] follows the header.
pub const FOURCC_DX10: u32 = 0x3031_5844;

pub mod header_flags {
    pub const CAPS: u32 = 0x1;
    pub const HEIGHT: u32 = 0x2;
    pub const WIDTH: u32 = 0x4;
    pub const PITCH: u32 = 0x8;
    pub const PIXELFORMAT: u32 = 0x1000;
    pub const MIPMAPCOUNT: u32 = 0x2_0000;
    pub const LINEARSIZE: u32 = 0x8_0000;
}

pub mod pixel_format_flags {
    pub const ALPHAPIXELS: u32 = 0x1;
    pub const ALPHA: u32 = 0x2;
    pub const FOURCC: u32 = 0x4;
    pub const RGB: u32 = 0x40;
    pub const LUMINANCE: u32 = 0x2_0000;
}

pub mod caps {
    pub const COMPLEX: u32 = 0x8;
    pub const TEXTURE: u32 = 0x1000;
    pub const MIPMAP: u32 = 0x40_0000;
}

pub mod caps2 {
    pub const CUBEMAP: u32 = 0x200;
    pub const VOLUME: u32 = 0x20_0000;
}

/// `misc_flag` in the DX10 header: the array is a cube's faces.
pub const DX10_MISC_TEXTURECUBE: u32 = 0x4;
/// `resource_dimension` in the DX10 header for a volume texture.
pub const DX10_DIMENSION_TEXTURE3D: u32 = 4;

/// How the payload's pixels are encoded: a FourCC, or channel masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsPixelFormat {
    pub size: u32,
    pub flags: u32,
    pub four_cc: u32,
    pub rgb_bit_count: u32,
    pub r_bit_mask: u32,
    pub g_bit_mask: u32,
    pub b_bit_mask: u32,
    pub a_bit_mask: u32,
}

impl DdsPixelFormat {
    pub const SIZE: usize = 32;
}

impl Default for DdsPixelFormat {
    fn default() -> Self {
        Self {
            size: Self::SIZE as u32,
            flags: 0,
            four_cc: 0,
            rgb_bit_count: 0,
            r_bit_mask: 0,
            g_bit_mask: 0,
            b_bit_mask: 0,
            a_bit_mask: 0,
        }
    }
}

/// The header after the magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsHeader {
    pub size: u32,
    pub flags: u32,
    pub height: u32,
    pub width: u32,
    pub pitch_or_linear_size: u32,
    pub depth: u32,
    pub mip_map_count: u32,
    pub reserved1: [u32; 11],
    pub pixel_format: DdsPixelFormat,
    pub caps: u32,
    pub caps2: u32,
    pub caps3: u32,
    pub caps4: u32,
    pub reserved2: u32,
}

impl DdsHeader {
    pub const SIZE: usize = 124;

    /// Whether a [`DdsHeaderDxt10`] follows this header.
    pub fn names_dx10(&self) -> bool {
        self.pixel_format.flags & pixel_format_flags::FOURCC != 0
            && self.pixel_format.four_cc == FOURCC_DX10
    }
}

impl Default for DdsHeader {
    fn default() -> Self {
        Self {
            size: Self::SIZE as u32,
            flags: 0,
            height: 0,
            width: 0,
            pitch_or_linear_size: 0,
            depth: 0,
            mip_map_count: 0,
            reserved1: [0; 11],
            pixel_format: DdsPixelFormat::default(),
            caps: 0,
            caps2: 0,
            caps3: 0,
            caps4: 0,
            reserved2: 0,
        }
    }
}

/// The DX10 extension: a DXGI format where no FourCC names one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DdsHeaderDxt10 {
    pub dxgi_format: u32,
    pub resource_dimension: u32,
    pub misc_flag: u32,
    pub array_size: u32,
    pub misc_flags2: u32,
}

impl DdsHeaderDxt10 {
    pub const SIZE: usize = 20;
}

/// A whole `.dds`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dds {
    pub header: DdsHeader,
    /// Present exactly when the header's pixel format names DX10.
    pub dxt10: Option<DdsHeaderDxt10>,
    pub data: Vec<u8>,
}

impl Dds {
    /// Whether the payload is one 2D surface and its mips: no cube faces, volume slices or array layers.
    pub fn is_one_surface(&self) -> bool {
        self.header.caps2 & (caps2::CUBEMAP | caps2::VOLUME) == 0
            && self.dxt10.is_none_or(|x| {
                x.array_size <= 1
                    && x.misc_flag & DX10_MISC_TEXTURECUBE == 0
                    && x.resource_dimension != DX10_DIMENSION_TEXTURE3D
            })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| IoError::FileReadFailed {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let Some(magic) = bytes.first_chunk::<4>().map(|m| u32::from_le_bytes(*m)) else {
            return Err(invalid(format!(
                "file is {} bytes, too short to hold a DDS header",
                bytes.len()
            )));
        };
        let Some(header) = bytes.get(4..4 + DdsHeader::SIZE) else {
            return Err(invalid(format!(
                "file is {} bytes, too short to hold a DDS header",
                bytes.len()
            )));
        };
        if magic != DDS_MAGIC {
            return Err(invalid(format!(
                "not a DDS file: magic is {magic:#010X}, expected {DDS_MAGIC:#010X}"
            )));
        }
        let header = read_header(header);
        if header.size != DdsHeader::SIZE as u32 {
            return Err(invalid(format!(
                "DDS header declares size {}, expected {}",
                header.size,
                DdsHeader::SIZE
            )));
        }

        let mut payload = 4 + DdsHeader::SIZE;
        let dxt10 = if header.names_dx10() {
            let ext = bytes
                .get(payload..payload + DdsHeaderDxt10::SIZE)
                .ok_or_else(|| invalid("DDS declares a DX10 header but the file ends first"))?;
            payload += DdsHeaderDxt10::SIZE;
            let [dxgi_format, resource_dimension, misc_flag, array_size, misc_flags2] = words(ext);
            Some(DdsHeaderDxt10 {
                dxgi_format,
                resource_dimension,
                misc_flag,
                array_size,
                misc_flags2,
            })
        } else {
            None
        };

        Ok(Self {
            header,
            dxt10,
            data: bytes[payload..].to_vec(),
        })
    }

    /// Refused when the DX10 header's presence disagrees with the pixel format.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if self.header.names_dx10() != self.dxt10.is_some() {
            return Err(invalid(if self.dxt10.is_some() {
                "a DX10 header is given but the pixel format does not name DX10"
            } else {
                "the pixel format names DX10 but no DX10 header is given"
            }));
        }
        let h = &self.header;
        let pf = &h.pixel_format;
        let mut out = Vec::with_capacity(
            4 + DdsHeader::SIZE
                + DdsHeaderDxt10::SIZE * usize::from(self.dxt10.is_some())
                + self.data.len(),
        );
        let mut put = |w: u32| out.extend_from_slice(&w.to_le_bytes());
        put(DDS_MAGIC);
        for w in [
            h.size,
            h.flags,
            h.height,
            h.width,
            h.pitch_or_linear_size,
            h.depth,
            h.mip_map_count,
        ] {
            put(w);
        }
        h.reserved1.iter().for_each(|&w| put(w));
        for w in [
            pf.size,
            pf.flags,
            pf.four_cc,
            pf.rgb_bit_count,
            pf.r_bit_mask,
            pf.g_bit_mask,
            pf.b_bit_mask,
            pf.a_bit_mask,
            h.caps,
            h.caps2,
            h.caps3,
            h.caps4,
            h.reserved2,
        ] {
            put(w);
        }
        if let Some(x) = &self.dxt10 {
            for w in [
                x.dxgi_format,
                x.resource_dimension,
                x.misc_flag,
                x.array_size,
                x.misc_flags2,
            ] {
                put(w);
            }
        }
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, self.to_bytes()?).map_err(|source| IoError::FileWriteFailed {
            path: path.to_path_buf(),
            source,
        })
    }
}

fn invalid(context: impl Into<String>) -> IoError {
    IoError::InvalidData {
        context: context.into(),
    }
}

/// `N` little-endian words from the front of `bytes`, which holds at least `4 * N`.
fn words<const N: usize>(bytes: &[u8]) -> [u32; N] {
    std::array::from_fn(|i| {
        let at = i * 4;
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    })
}

fn read_header(bytes: &[u8]) -> DdsHeader {
    let w: [u32; DdsHeader::SIZE / 4] = words(bytes);
    DdsHeader {
        size: w[0],
        flags: w[1],
        height: w[2],
        width: w[3],
        pitch_or_linear_size: w[4],
        depth: w[5],
        mip_map_count: w[6],
        reserved1: std::array::from_fn(|i| w[7 + i]),
        pixel_format: DdsPixelFormat {
            size: w[18],
            flags: w[19],
            four_cc: w[20],
            rgb_bit_count: w[21],
            r_bit_mask: w[22],
            g_bit_mask: w[23],
            b_bit_mask: w[24],
            a_bit_mask: w[25],
        },
        caps: w[26],
        caps2: w[27],
        caps3: w[28],
        caps4: w[29],
        reserved2: w[30],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dds(dxt10: bool) -> Dds {
        let mut header = DdsHeader {
            flags: header_flags::CAPS | header_flags::WIDTH | header_flags::HEIGHT,
            width: 4,
            height: 4,
            mip_map_count: 1,
            caps: caps::TEXTURE,
            ..DdsHeader::default()
        };
        header.reserved1[3] = 0x1234_5678;
        header.pixel_format.flags = pixel_format_flags::FOURCC;
        header.pixel_format.four_cc = if dxt10 { FOURCC_DX10 } else { 0x3154_5844 };
        Dds {
            header,
            dxt10: dxt10.then_some(DdsHeaderDxt10 {
                dxgi_format: 98,
                resource_dimension: 3,
                array_size: 1,
                ..DdsHeaderDxt10::default()
            }),
            data: (0..16).collect(),
        }
    }

    #[test]
    fn every_field_round_trips() {
        for dxt10 in [false, true] {
            let dds = dds(dxt10);
            let bytes = dds.to_bytes().unwrap();
            let extension = if dxt10 { DdsHeaderDxt10::SIZE } else { 0 };
            assert_eq!(
                bytes.len(),
                4 + DdsHeader::SIZE + extension + dds.data.len()
            );
            assert_eq!(Dds::from_bytes(&bytes).unwrap(), dds);
        }
    }

    #[test]
    fn a_dx10_header_the_pixel_format_does_not_name_is_refused() {
        let mut dds = dds(false);
        dds.dxt10 = Some(DdsHeaderDxt10::default());
        assert!(dds.to_bytes().is_err());
        let mut dds = self::dds(true);
        dds.dxt10 = None;
        assert!(dds.to_bytes().is_err());
    }

    #[test]
    fn a_cube_a_volume_or_an_array_is_not_one_surface() {
        assert!(dds(false).is_one_surface() && dds(true).is_one_surface());
        let mut cube = dds(false);
        cube.header.caps2 = caps2::CUBEMAP;
        let mut volume = dds(false);
        volume.header.caps2 = caps2::VOLUME;
        let mut array = dds(true);
        array.dxt10.as_mut().unwrap().array_size = 6;
        for dds in [cube, volume, array] {
            assert!(!dds.is_one_surface(), "{:?}", dds.header);
        }
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = dds(false).to_bytes().unwrap();
        bytes[0] = b'X';
        let message = Dds::from_bytes(&bytes).unwrap_err().to_string();
        assert!(message.contains("not a DDS file"), "unhelpful: {message}");
    }

    #[test]
    fn a_wrong_header_size_is_rejected() {
        let mut dds = dds(false);
        dds.header.size = 120;
        let message = Dds::from_bytes(&dds.to_bytes().unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("declares size 120"),
            "unhelpful: {message}"
        );
    }

    #[test]
    fn a_short_file_is_rejected() {
        assert!(Dds::from_bytes(&[0u8; 16]).is_err());
        let bytes = dds(true).to_bytes().unwrap();
        let message = Dds::from_bytes(&bytes[..4 + DdsHeader::SIZE + 8])
            .unwrap_err()
            .to_string();
        assert!(message.contains("DX10"), "unhelpful: {message}");
    }
}

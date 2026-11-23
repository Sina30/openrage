use std::ops::Range;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrcTextureFormat {
    #[default]
    None = 0,

    R5G6B5 = 1,

    A8R8G8B8 = 2,

    R16F = 3,

    R32F = 4,

    A2B10G10R10 = 5,

    A2B10G10R10ATI = 6,

    A16B16G16R16F = 7,

    G16R16 = 8,

    G16R16F = 9,

    A32B32G32R32F = 10,

    A16B16G16R16FNoExpand = 11,

    A16B16G16R16 = 12,

    L8 = 13,

    L16 = 14,

    G8R8 = 15,

    A1R5G5B5 = 17,

    D24S8 = 18,

    A4R4G4B4 = 19,

    G32R32F = 20,

    D16 = 22,

    D32F = 24,

    X8R8G8B8 = 25,

    Null = 26,

    X24G8 = 27,

    A8 = 28,

    R11G11B10F = 29,

    D32FS8 = 30,

    X32S8 = 31,

    DXT1 = 32,

    DXT3 = 33,

    DXT5 = 34,

    DXT5A = 35,

    DXN = 36,

    BC6 = 37,

    BC7 = 38,

    A8B8G8R8Snorm = 39,

    A8B8G8R8 = 40,

    Etc2Rgb = 41,
    Etc2Rgba = 42,
    EacR11 = 43,
    EacRg11 = 44,
}

impl From<u32> for GrcTextureFormat {
    fn from(value: u32) -> Self {
        match value {
            0 => return Self::None,

            21 => return Self::A8R8G8B8,
            22 => return Self::X8R8G8B8,
            23 => return Self::R5G6B5,
            25 => return Self::A1R5G5B5,
            26 => return Self::A4R4G4B4,
            28 => return Self::A8,
            31 => return Self::A2B10G10R10,
            32 => return Self::A8B8G8R8,
            34 => return Self::G16R16,
            35 => return Self::A2B10G10R10,
            36 => return Self::A16B16G16R16,
            50 => return Self::L8,
            75 => return Self::D24S8,
            77 => return Self::D24S8,
            80 => return Self::D16,
            81 => return Self::L16,
            82 => return Self::D32F,
            83 => return Self::D24S8,
            111 => return Self::R16F,
            112 => return Self::G16R16F,
            113 => return Self::A16B16G16R16F,
            114 => return Self::R32F,
            115 => return Self::G32R32F,
            116 => return Self::A32B32G32R32F,
            _ => {}
        }

        // D3D9 FourCC codes (little-endian)
        match value {
            0x31545844 => Self::DXT1,
            0x32545844 => Self::DXT3,
            0x33545844 => Self::DXT3,
            0x34545844 => Self::DXT5,
            0x35545844 => Self::DXT5,
            0x31495441 => Self::DXT5A,
            0x32495441 => Self::DXN,
            0x55344342 => Self::DXT5A,
            0x55354342 => Self::DXN,
            0x20364342 => Self::BC6,
            0x20374342 => Self::BC7,
            0x38385247 => Self::G8R8,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecodeSupport {
    Exact,
    Tonemapped,
    NotSampleable,
    Empty,
    Missing,
}

impl DecodeSupport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Tonemapped => "tonemapped",
            Self::NotSampleable => "not-sampleable",
            Self::Empty => "empty",
            Self::Missing => "MISSING",
        }
    }

    pub fn is_decodable(self) -> bool {
        matches!(self, Self::Exact | Self::Tonemapped)
    }
}

impl GrcTextureFormat {
    pub fn block_size(&self) -> u32 {
        match self {
            Self::DXT1
            | Self::DXT3
            | Self::DXT5
            | Self::DXT5A
            | Self::DXN
            | Self::BC6
            | Self::BC7
            | Self::Etc2Rgb
            | Self::Etc2Rgba
            | Self::EacR11
            | Self::EacRg11 => 4,
            _ => 1,
        }
    }

    pub fn bytes_per_block(&self) -> u32 {
        match self {
            Self::None | Self::Null => 0,
            Self::L8 | Self::A8 => 1,
            Self::R5G6B5
            | Self::A1R5G5B5
            | Self::A4R4G4B4
            | Self::R16F
            | Self::L16
            | Self::G8R8
            | Self::D16 => 2,
            Self::A8R8G8B8
            | Self::R32F
            | Self::A2B10G10R10
            | Self::A2B10G10R10ATI
            | Self::G16R16
            | Self::G16R16F
            | Self::D24S8
            | Self::D32F
            | Self::X8R8G8B8
            | Self::X24G8
            | Self::R11G11B10F
            | Self::A8B8G8R8Snorm
            | Self::A8B8G8R8 => 4,
            Self::DXT1 | Self::DXT5A => 8,
            Self::DXT3 | Self::DXT5 | Self::DXN | Self::BC6 | Self::BC7 => 16,
            Self::A16B16G16R16F
            | Self::A16B16G16R16FNoExpand
            | Self::A16B16G16R16
            | Self::G32R32F
            | Self::D32FS8
            | Self::X32S8 => 8,
            Self::A32B32G32R32F => 16,
            Self::Etc2Rgb | Self::EacR11 => 8,
            Self::Etc2Rgba | Self::EacRg11 => 16,
        }
    }

    pub fn is_compressed(&self) -> bool {
        matches!(
            self,
            Self::DXT1
                | Self::DXT3
                | Self::DXT5
                | Self::DXT5A
                | Self::DXN
                | Self::BC6
                | Self::BC7
                | Self::Etc2Rgb
                | Self::Etc2Rgba
                | Self::EacR11
                | Self::EacRg11
        )
    }

    pub fn decode_support(&self) -> DecodeSupport {
        match self {
            Self::None | Self::Null => DecodeSupport::Empty,

            Self::D16 | Self::D24S8 | Self::D32F | Self::D32FS8 | Self::X24G8 | Self::X32S8 => {
                DecodeSupport::NotSampleable
            }

            Self::R16F
            | Self::R32F
            | Self::G16R16F
            | Self::G32R32F
            | Self::A16B16G16R16F
            | Self::A16B16G16R16FNoExpand
            | Self::A32B32G32R32F
            | Self::R11G11B10F
            | Self::BC6 => DecodeSupport::Tonemapped,

            Self::R5G6B5
            | Self::A8R8G8B8
            | Self::A2B10G10R10
            | Self::A2B10G10R10ATI
            | Self::G16R16
            | Self::A16B16G16R16
            | Self::L8
            | Self::L16
            | Self::G8R8
            | Self::A1R5G5B5
            | Self::A4R4G4B4
            | Self::X8R8G8B8
            | Self::A8
            | Self::A8B8G8R8
            | Self::A8B8G8R8Snorm
            | Self::DXT1
            | Self::DXT3
            | Self::DXT5
            | Self::DXT5A
            | Self::DXN
            | Self::BC7
            | Self::Etc2Rgb
            | Self::Etc2Rgba
            | Self::EacR11
            | Self::EacRg11 => DecodeSupport::Exact,
        }
    }

    pub fn to_raw_format(&self) -> u32 {
        match self {
            Self::None | Self::Null => 0,

            Self::A8R8G8B8 => 21,
            Self::X8R8G8B8 => 22,
            Self::R5G6B5 => 23,
            Self::A1R5G5B5 => 25,
            Self::A4R4G4B4 => 26,
            Self::A8 => 28,
            Self::A2B10G10R10 => 31,
            Self::A2B10G10R10ATI => 31,
            Self::A8B8G8R8 => 32,
            Self::A8B8G8R8Snorm => 32,
            Self::G16R16 => 34,
            Self::A16B16G16R16 => 36,
            Self::L8 => 50,
            Self::G8R8 => 0x38385247,
            Self::L16 => 81,
            Self::D24S8 => 75,
            Self::D16 => 80,
            Self::D32F => 82,
            Self::D32FS8 => 83,
            Self::X24G8 => 75,
            Self::X32S8 => 83,
            Self::R11G11B10F => 0,
            Self::G32R32F => 115,
            Self::R16F => 111,
            Self::G16R16F => 112,
            Self::A16B16G16R16F => 113,
            Self::A16B16G16R16FNoExpand => 113,
            Self::R32F => 114,
            Self::A32B32G32R32F => 116,

            Self::DXT1 => 0x31545844,
            Self::DXT3 => 0x33545844,
            Self::DXT5 => 0x35545844,
            Self::DXT5A => 0x31495441,
            Self::DXN => 0x32495441,
            Self::BC6 => 0x20364342,
            Self::BC7 => 0x20374342,
            Self::Etc2Rgb => 0x32435445,
            Self::Etc2Rgba => 0x41324345,
            Self::EacR11 => 0x52434145,
            Self::EacRg11 => 0x47524145,
        }
    }
}

impl From<GrcTextureFormat> for u32 {
    fn from(fmt: GrcTextureFormat) -> u32 {
        fmt.to_raw_format()
    }
}

impl GrcTextureFormat {
    /// The bytes of one level, or `None` where they do not fit 32 bits.
    pub fn checked_mip_size(&self, width: u32, height: u32) -> Option<u32> {
        let block_size = self.block_size();
        let bpb = self.bytes_per_block();

        if bpb == 0 {
            return Some(0);
        }

        let (across, down) = if block_size > 1 {
            (
                width.div_ceil(block_size).max(1),
                height.div_ceil(block_size).max(1),
            )
        } else {
            (width, height)
        };
        across.checked_mul(down)?.checked_mul(bpb)
    }

    /// The bytes of `mip_levels` levels, or `None` where they do not fit 32 bits.
    pub fn checked_total_size(&self, width: u32, height: u32, mip_levels: u32) -> Option<u32> {
        self.checked_physical_size(width, height, 1, mip_levels, 1)
    }

    /// The bytes of every level, slice and layer, or `None` where they do not fit 32 bits.
    pub fn checked_physical_size(
        &self,
        width: u32,
        height: u32,
        depth: u32,
        mip_count: u32,
        layers: u32,
    ) -> Option<u32> {
        let mut total = 0u32;
        let (mut w, mut h, mut d) = (width, height, depth);

        for level in 0..mip_count {
            let slice = self.checked_mip_size(w, h)?.checked_mul(d.max(1))?;
            total = total.checked_add(slice)?;

            let next = (
                (w / 2).max(1),
                (h / 2).max(1),
                if depth > 1 { (d / 2).max(1) } else { d },
            );
            // A header may state billions of levels: every one past the smallest is the same size.
            if next == (w, h, d) {
                total = total.checked_add(slice.checked_mul(mip_count - level - 1)?)?;
                break;
            }
            (w, h, d) = next;
        }

        total.checked_mul(layers.max(1))
    }

    /// One level's bytes, or `u32::MAX` where they do not fit 32 bits, which no payload holds.
    pub fn mip_size(&self, width: u32, height: u32) -> u32 {
        self.checked_mip_size(width, height).unwrap_or(u32::MAX)
    }

    /// `mip_levels` levels' bytes, or `u32::MAX` where they do not fit 32 bits, which no payload holds.
    pub fn total_size(&self, width: u32, height: u32, mip_levels: u32) -> u32 {
        self.checked_total_size(width, height, mip_levels)
            .unwrap_or(u32::MAX)
    }

    /// Every level, slice and layer's bytes, or `u32::MAX` where they do not fit 32 bits, which no payload holds.
    pub fn calculate_physical_size(
        &self,
        width: u32,
        height: u32,
        depth: u32,
        mip_count: u32,
        layers: u32,
    ) -> u32 {
        self.checked_physical_size(width, height, depth, mip_count, layers)
            .unwrap_or(u32::MAX)
    }

    pub fn row_pitch(&self, width: u32) -> u32 {
        let block_size = self.block_size();
        let bpb = self.bytes_per_block();

        if bpb == 0 {
            return 0;
        }

        if block_size > 1 {
            let blocks_x = width.div_ceil(block_size).max(1);
            blocks_x * bpb
        } else {
            width * bpb
        }
    }

    /// What a Legacy texture's `mip_stride` states: a block-compressed format's bytes per row of texels, a quarter of its row of blocks, and any other format's row.
    pub fn legacy_mip_stride(&self, width: u32) -> u32 {
        let row = self.row_pitch(width);
        if self.is_compressed() {
            row / 4
        } else {
            row
        }
    }

    pub fn aligned_row_pitch(&self, width: u32, alignment: u32) -> u32 {
        let pitch = self.row_pitch(width);
        if alignment == 0 || pitch == 0 {
            return pitch;
        }
        (pitch + alignment - 1) & !(alignment - 1)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "DXT1" | "BC1" => Some(Self::DXT1),
            "DXT3" | "BC2" => Some(Self::DXT3),
            "DXT5" | "BC3" => Some(Self::DXT5),
            "DXT5A" | "BC4" | "ATI1" => Some(Self::DXT5A),
            "DXN" | "BC5" | "ATI2" | "3DC" => Some(Self::DXN),
            "BC6" | "BC6H" => Some(Self::BC6),
            "BC7" => Some(Self::BC7),
            "A8R8G8B8" | "BGRA8" | "BGRA" => Some(Self::A8R8G8B8),
            "A8B8G8R8" | "RGBA8" | "RGBA" => Some(Self::A8B8G8R8),
            "L8" | "R8" | "GRAY" => Some(Self::L8),
            "A8" | "ALPHA" => Some(Self::A8),
            _ => None,
        }
    }

    pub fn mip_dimensions(&self, width: u32, height: u32, level: u32) -> (u32, u32) {
        ((width >> level).max(1), (height >> level).max(1))
    }

    pub fn mip_level_range(&self, width: u32, height: u32, level: u32) -> Range<usize> {
        let mut offset = 0usize;
        for l in 0..level {
            let (w, h) = self.mip_dimensions(width, height, l);
            offset += self.mip_size(w, h) as usize;
        }
        let (w, h) = self.mip_dimensions(width, height, level);
        offset..offset + self.mip_size(w, h) as usize
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxgiFormat {
    Unknown = 0,

    R32G32B32A32Float = 2,
    R16G16B16A16Float = 10,
    R16G16B16A16Unorm = 11,
    R32G32Float = 16,
    R11G11B10Float = 26,
    R8G8B8A8Unorm = 28,
    R8G8B8A8UnormSrgb = 29,
    R8G8B8A8Snorm = 31,
    B8G8R8A8Unorm = 87,
    B8G8R8A8UnormSrgb = 91,
    B8G8R8X8Unorm = 88,
    B8G8R8X8UnormSrgb = 93,
    R10G10B10A2Unorm = 24,
    R16G16Unorm = 35,
    R16G16Float = 34,
    R32Float = 41,
    R16Float = 54,
    R16Unorm = 56,
    R8Unorm = 61,
    R8G8Unorm = 49,
    A8Unorm = 65,
    B5G6R5Unorm = 85,
    B5G5R5A1Unorm = 86,
    B4G4R4A4Unorm = 115,

    D32Float = 40,
    D24UnormS8Uint = 45,
    D16Unorm = 55,
    D32FloatS8X24Uint = 20,

    Bc1Unorm = 71,
    Bc1UnormSrgb = 72,
    Bc2Unorm = 74,
    Bc2UnormSrgb = 75,
    Bc3Unorm = 77,
    Bc3UnormSrgb = 78,
    Bc4Unorm = 80,
    Bc4Snorm = 81,
    Bc5Unorm = 83,
    Bc5Snorm = 84,
    Bc6hUf16 = 95,
    Bc6hSf16 = 96,
    Bc7Unorm = 98,
    Bc7UnormSrgb = 99,
}

impl DxgiFormat {
    pub fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::Unknown,
            2 => Self::R32G32B32A32Float,
            10 => Self::R16G16B16A16Float,
            11 => Self::R16G16B16A16Unorm,
            16 => Self::R32G32Float,
            20 => Self::D32FloatS8X24Uint,
            24 => Self::R10G10B10A2Unorm,
            26 => Self::R11G11B10Float,
            28 => Self::R8G8B8A8Unorm,
            29 => Self::R8G8B8A8UnormSrgb,
            31 => Self::R8G8B8A8Snorm,
            34 => Self::R16G16Float,
            35 => Self::R16G16Unorm,
            40 => Self::D32Float,
            41 => Self::R32Float,
            45 => Self::D24UnormS8Uint,
            49 => Self::R8G8Unorm,
            54 => Self::R16Float,
            55 => Self::D16Unorm,
            56 => Self::R16Unorm,
            61 => Self::R8Unorm,
            65 => Self::A8Unorm,
            71 => Self::Bc1Unorm,
            72 => Self::Bc1UnormSrgb,
            74 => Self::Bc2Unorm,
            75 => Self::Bc2UnormSrgb,
            77 => Self::Bc3Unorm,
            78 => Self::Bc3UnormSrgb,
            80 => Self::Bc4Unorm,
            81 => Self::Bc4Snorm,
            83 => Self::Bc5Unorm,
            84 => Self::Bc5Snorm,
            85 => Self::B5G6R5Unorm,
            86 => Self::B5G5R5A1Unorm,
            87 => Self::B8G8R8A8Unorm,
            88 => Self::B8G8R8X8Unorm,
            91 => Self::B8G8R8A8UnormSrgb,
            93 => Self::B8G8R8X8UnormSrgb,
            95 => Self::Bc6hUf16,
            96 => Self::Bc6hSf16,
            98 => Self::Bc7Unorm,
            99 => Self::Bc7UnormSrgb,
            115 => Self::B4G4R4A4Unorm,
            _ => Self::Unknown,
        }
    }

    pub fn to_raw(self) -> u32 {
        self as u32
    }

    pub fn is_srgb(self) -> bool {
        matches!(
            self,
            Self::Bc1UnormSrgb
                | Self::Bc2UnormSrgb
                | Self::Bc3UnormSrgb
                | Self::Bc7UnormSrgb
                | Self::R8G8B8A8UnormSrgb
                | Self::B8G8R8A8UnormSrgb
                | Self::B8G8R8X8UnormSrgb
        )
    }

    pub fn to_linear(self) -> Self {
        match self {
            Self::Bc1UnormSrgb => Self::Bc1Unorm,
            Self::Bc2UnormSrgb => Self::Bc2Unorm,
            Self::Bc3UnormSrgb => Self::Bc3Unorm,
            Self::Bc7UnormSrgb => Self::Bc7Unorm,
            Self::R8G8B8A8UnormSrgb => Self::R8G8B8A8Unorm,
            Self::B8G8R8A8UnormSrgb => Self::B8G8R8A8Unorm,
            Self::B8G8R8X8UnormSrgb => Self::B8G8R8X8Unorm,
            other => other,
        }
    }

    pub fn to_srgb(self) -> Option<Self> {
        match self {
            Self::Bc1Unorm => Some(Self::Bc1UnormSrgb),
            Self::Bc2Unorm => Some(Self::Bc2UnormSrgb),
            Self::Bc3Unorm => Some(Self::Bc3UnormSrgb),
            Self::Bc7Unorm => Some(Self::Bc7UnormSrgb),
            Self::R8G8B8A8Unorm => Some(Self::R8G8B8A8UnormSrgb),
            Self::B8G8R8A8Unorm => Some(Self::B8G8R8A8UnormSrgb),
            Self::B8G8R8X8Unorm => Some(Self::B8G8R8X8UnormSrgb),
            other if other.is_srgb() => Some(other),
            _ => None,
        }
    }
}

pub mod fourcc {
    pub const DXT1: u32 = 0x31545844;
    pub const DXT2: u32 = 0x32545844;
    pub const DXT3: u32 = 0x33545844;
    pub const DXT4: u32 = 0x34545844;
    pub const DXT5: u32 = 0x35545844;
    pub const ATI1: u32 = 0x31495441;
    pub const ATI2: u32 = 0x32495441;
    pub const BC4U: u32 = 0x55344342;
    pub const BC4S: u32 = 0x53344342;
    pub const BC5U: u32 = 0x55354342;
    pub const BC5S: u32 = 0x53354342;

    pub fn to_string(code: u32) -> String {
        let bytes = code.to_le_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }

    pub fn is_fourcc(value: u32) -> bool {
        matches!(
            value,
            DXT1 | DXT2 | DXT3 | DXT4 | DXT5 | ATI1 | ATI2 | BC4U | BC4S | BC5U | BC5S
        )
    }
}

impl GrcTextureFormat {
    pub fn to_dxgi(&self, srgb: bool) -> DxgiFormat {
        let base = match self {
            Self::None | Self::Null => return DxgiFormat::Unknown,

            Self::R5G6B5 => DxgiFormat::B5G6R5Unorm,
            Self::A8R8G8B8 => DxgiFormat::B8G8R8A8Unorm,
            Self::R16F => DxgiFormat::R16Float,
            Self::R32F => DxgiFormat::R32Float,
            Self::A2B10G10R10 | Self::A2B10G10R10ATI => DxgiFormat::R10G10B10A2Unorm,
            Self::A16B16G16R16F | Self::A16B16G16R16FNoExpand => DxgiFormat::R16G16B16A16Float,
            Self::G16R16 => DxgiFormat::R16G16Unorm,
            Self::G16R16F => DxgiFormat::R16G16Float,
            Self::A32B32G32R32F => DxgiFormat::R32G32B32A32Float,
            Self::A16B16G16R16 => DxgiFormat::R16G16B16A16Unorm,
            Self::L8 => DxgiFormat::R8Unorm,
            Self::L16 => DxgiFormat::R16Unorm,
            Self::G8R8 => DxgiFormat::R8G8Unorm,
            Self::A1R5G5B5 => DxgiFormat::B5G5R5A1Unorm,
            Self::D24S8 => DxgiFormat::D24UnormS8Uint,
            Self::A4R4G4B4 => DxgiFormat::B4G4R4A4Unorm,
            Self::G32R32F => DxgiFormat::R32G32Float,
            Self::D16 => DxgiFormat::D16Unorm,
            Self::D32F => DxgiFormat::D32Float,
            Self::X8R8G8B8 => DxgiFormat::B8G8R8X8Unorm,
            Self::X24G8 => return DxgiFormat::Unknown,
            Self::A8 => DxgiFormat::A8Unorm,
            Self::R11G11B10F => DxgiFormat::R11G11B10Float,
            Self::D32FS8 => DxgiFormat::D32FloatS8X24Uint,
            Self::X32S8 => return DxgiFormat::Unknown,

            Self::DXT1 => DxgiFormat::Bc1Unorm,
            Self::DXT3 => DxgiFormat::Bc2Unorm,
            Self::DXT5 => DxgiFormat::Bc3Unorm,
            Self::DXT5A => DxgiFormat::Bc4Unorm,
            Self::DXN => DxgiFormat::Bc5Unorm,
            Self::BC6 => DxgiFormat::Bc6hUf16,
            Self::BC7 => DxgiFormat::Bc7Unorm,

            Self::A8B8G8R8Snorm => DxgiFormat::R8G8B8A8Snorm,
            Self::A8B8G8R8 => DxgiFormat::R8G8B8A8Unorm,
            Self::Etc2Rgb | Self::Etc2Rgba | Self::EacR11 | Self::EacRg11 => {
                return DxgiFormat::Unknown
            }
        };

        if srgb {
            base.to_srgb().unwrap_or(base)
        } else {
            base
        }
    }

    pub fn from_dxgi(dxgi: DxgiFormat) -> (Self, bool) {
        let is_srgb = dxgi.is_srgb();
        let format = match dxgi.to_linear() {
            DxgiFormat::Unknown => Self::None,

            DxgiFormat::B5G6R5Unorm => Self::R5G6B5,
            DxgiFormat::B8G8R8A8Unorm | DxgiFormat::B8G8R8A8UnormSrgb => Self::A8R8G8B8,
            DxgiFormat::R16Float => Self::R16F,
            DxgiFormat::R32Float => Self::R32F,
            DxgiFormat::R10G10B10A2Unorm => Self::A2B10G10R10,
            DxgiFormat::R16G16B16A16Float => Self::A16B16G16R16F,
            DxgiFormat::R16G16Unorm => Self::G16R16,
            DxgiFormat::R16G16Float => Self::G16R16F,
            DxgiFormat::R32G32B32A32Float => Self::A32B32G32R32F,
            DxgiFormat::R16G16B16A16Unorm => Self::A16B16G16R16,
            DxgiFormat::R8Unorm => Self::L8,
            DxgiFormat::R16Unorm => Self::L16,
            DxgiFormat::R8G8Unorm => Self::G8R8,
            DxgiFormat::B5G5R5A1Unorm => Self::A1R5G5B5,
            DxgiFormat::D24UnormS8Uint => Self::D24S8,
            DxgiFormat::B4G4R4A4Unorm => Self::A4R4G4B4,
            DxgiFormat::R32G32Float => Self::G32R32F,
            DxgiFormat::D16Unorm => Self::D16,
            DxgiFormat::D32Float => Self::D32F,
            DxgiFormat::B8G8R8X8Unorm | DxgiFormat::B8G8R8X8UnormSrgb => Self::X8R8G8B8,
            DxgiFormat::A8Unorm => Self::A8,
            DxgiFormat::R11G11B10Float => Self::R11G11B10F,
            DxgiFormat::D32FloatS8X24Uint => Self::D32FS8,
            DxgiFormat::R8G8B8A8Snorm => Self::A8B8G8R8Snorm,
            DxgiFormat::R8G8B8A8Unorm | DxgiFormat::R8G8B8A8UnormSrgb => Self::A8B8G8R8,

            DxgiFormat::Bc1Unorm | DxgiFormat::Bc1UnormSrgb => Self::DXT1,
            DxgiFormat::Bc2Unorm | DxgiFormat::Bc2UnormSrgb => Self::DXT3,
            DxgiFormat::Bc3Unorm | DxgiFormat::Bc3UnormSrgb => Self::DXT5,
            DxgiFormat::Bc4Unorm | DxgiFormat::Bc4Snorm => Self::DXT5A,
            DxgiFormat::Bc5Unorm | DxgiFormat::Bc5Snorm => Self::DXN,
            DxgiFormat::Bc6hUf16 | DxgiFormat::Bc6hSf16 => Self::BC6,
            DxgiFormat::Bc7Unorm | DxgiFormat::Bc7UnormSrgb => Self::BC7,
        };

        (format, is_srgb)
    }

    pub fn from_fourcc(code: u32) -> Option<Self> {
        match code {
            fourcc::DXT1 => Some(Self::DXT1),
            fourcc::DXT2 | fourcc::DXT3 => Some(Self::DXT3),
            fourcc::DXT4 | fourcc::DXT5 => Some(Self::DXT5),
            fourcc::ATI1 | fourcc::BC4U | fourcc::BC4S => Some(Self::DXT5A),
            fourcc::ATI2 | fourcc::BC5U | fourcc::BC5S => Some(Self::DXN),
            _ => None,
        }
    }

    pub fn to_fourcc(&self) -> Option<u32> {
        match self {
            Self::DXT1 => Some(fourcc::DXT1),
            Self::DXT3 => Some(fourcc::DXT3),
            Self::DXT5 => Some(fourcc::DXT5),
            Self::DXT5A => Some(fourcc::ATI1),
            Self::DXN => Some(fourcc::ATI2),
            _ => None,
        }
    }

    pub fn from_any(value: u32) -> (Self, bool) {
        let result = Self::from(value);
        if result != Self::None || value == 0 {
            return (result, false);
        }

        let dxgi = DxgiFormat::from_raw(value);
        if dxgi != DxgiFormat::Unknown {
            return Self::from_dxgi(dxgi);
        }

        (Self::None, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrcDX11InfoBits {
    pub is_srgb: bool,
    pub read_write_access: u8,
    pub owns_backing_store: bool,
    pub has_been_deleted: bool,
    pub dynamic: bool,
    pub dirty: bool,
}

impl From<u8> for GrcDX11InfoBits {
    fn from(byte: u8) -> Self {
        Self {
            is_srgb: (byte & 0x01) != 0,
            read_write_access: (byte >> 1) & 0x07,
            owns_backing_store: (byte & 0x10) != 0,
            has_been_deleted: (byte & 0x20) != 0,
            dynamic: (byte & 0x40) != 0,
            dirty: (byte & 0x80) != 0,
        }
    }
}

impl From<GrcDX11InfoBits> for u8 {
    fn from(bits: GrcDX11InfoBits) -> u8 {
        let mut byte = 0u8;
        if bits.is_srgb {
            byte |= 0x01;
        }
        byte |= (bits.read_write_access & 0x07) << 1;
        if bits.owns_backing_store {
            byte |= 0x10;
        }
        if bits.has_been_deleted {
            byte |= 0x20;
        }
        if bits.dynamic {
            byte |= 0x40;
        }
        if bits.dirty {
            byte |= 0x80;
        }
        byte
    }
}

/// A `.dds`'s pixel format and whether it is sRGB; refused when no texture format is its equivalent.
#[cfg(feature = "dds")]
pub fn format_of_dds(dds: &crate::dds::Dds) -> crate::Result<(GrcTextureFormat, bool)> {
    use crate::dds::pixel_format_flags;
    use crate::IoError;

    let (format, srgb) = match &dds.dxt10 {
        Some(dxt10) => GrcTextureFormat::from_dxgi(DxgiFormat::from_raw(dxt10.dxgi_format)),
        None => {
            let pf = &dds.header.pixel_format;
            if pf.flags & pixel_format_flags::FOURCC != 0 {
                let Some(format) = GrcTextureFormat::from_fourcc(pf.four_cc) else {
                    return Err(IoError::InvalidData {
                        context: format!("unsupported DDS FourCC {:#010X}", pf.four_cc),
                    });
                };
                (format, false)
            } else if pf.flags & pixel_format_flags::RGB != 0 && pf.rgb_bit_count == 32 {
                if pf.r_bit_mask == 0x00FF_0000 {
                    (GrcTextureFormat::A8R8G8B8, false)
                } else {
                    (GrcTextureFormat::A8B8G8R8, false)
                }
            } else if pf.flags & pixel_format_flags::LUMINANCE != 0 && pf.rgb_bit_count == 8 {
                (GrcTextureFormat::L8, false)
            } else if pf.flags & pixel_format_flags::ALPHA != 0 && pf.rgb_bit_count == 8 {
                (GrcTextureFormat::A8, false)
            } else {
                return Err(IoError::InvalidData {
                    context: format!(
                        "unsupported DDS pixel format: flags {:#X}, {} bpp",
                        pf.flags, pf.rgb_bit_count
                    ),
                });
            }
        }
    };
    if format == GrcTextureFormat::None {
        return Err(IoError::InvalidData {
            context: "DDS pixel format is not one OpenRage can represent".to_string(),
        });
    }
    Ok((format, srgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxgi_for_etc2_family_returns_unknown() {
        for f in [
            GrcTextureFormat::Etc2Rgb,
            GrcTextureFormat::Etc2Rgba,
            GrcTextureFormat::EacR11,
            GrcTextureFormat::EacRg11,
        ] {
            assert_eq!(f.to_dxgi(false), DxgiFormat::Unknown);
            assert_eq!(f.to_dxgi(true), DxgiFormat::Unknown);
        }
    }

    #[test]
    fn test_fourcc_to_rage() {
        assert_eq!(
            GrcTextureFormat::from_fourcc(fourcc::DXT1),
            Some(GrcTextureFormat::DXT1)
        );
        assert_eq!(
            GrcTextureFormat::from_fourcc(fourcc::DXT5),
            Some(GrcTextureFormat::DXT5)
        );
        assert_eq!(
            GrcTextureFormat::from_fourcc(fourcc::ATI1),
            Some(GrcTextureFormat::DXT5A)
        );
        assert_eq!(
            GrcTextureFormat::from_fourcc(fourcc::ATI2),
            Some(GrcTextureFormat::DXN)
        );
        assert_eq!(GrcTextureFormat::from_fourcc(0xDEADBEEF), None);
    }

    #[test]
    fn test_from_any_d3d9() {
        assert_eq!(
            GrcTextureFormat::from_any(21),
            (GrcTextureFormat::A8R8G8B8, false)
        );
        assert_eq!(
            GrcTextureFormat::from_any(32),
            (GrcTextureFormat::A8B8G8R8, false)
        );
        assert_eq!(
            GrcTextureFormat::from_any(50),
            (GrcTextureFormat::L8, false)
        );
        assert_eq!(
            GrcTextureFormat::from_any(114),
            (GrcTextureFormat::R32F, false)
        );
    }

    #[test]
    fn test_from_any_fourcc() {
        assert_eq!(
            GrcTextureFormat::from_any(fourcc::DXT1),
            (GrcTextureFormat::DXT1, false)
        );
        assert_eq!(
            GrcTextureFormat::from_any(fourcc::DXT5),
            (GrcTextureFormat::DXT5, false)
        );
    }

    #[test]
    fn test_from_any_dxgi() {
        assert_eq!(
            GrcTextureFormat::from_any(72),
            (GrcTextureFormat::DXT1, true)
        );
        assert_eq!(
            GrcTextureFormat::from_any(98),
            (GrcTextureFormat::BC7, false)
        );
        assert_eq!(
            GrcTextureFormat::from_any(95),
            (GrcTextureFormat::BC6, false)
        );
    }

    #[test]
    fn test_fourcc_string() {
        assert_eq!(fourcc::to_string(fourcc::DXT1), "DXT1");
        assert_eq!(fourcc::to_string(fourcc::DXT5), "DXT5");
        assert_eq!(fourcc::to_string(fourcc::ATI1), "ATI1");
        assert_eq!(fourcc::to_string(fourcc::ATI2), "ATI2");
    }

    #[test]
    fn test_info_bits_roundtrip() {
        let original = GrcDX11InfoBits {
            is_srgb: true,
            read_write_access: 3,
            owns_backing_store: true,
            has_been_deleted: false,
            dynamic: true,
            dirty: false,
        };

        let byte: u8 = original.into();
        let decoded = GrcDX11InfoBits::from(byte);

        assert_eq!(decoded.is_srgb, original.is_srgb);
        assert_eq!(decoded.read_write_access, original.read_write_access);
        assert_eq!(decoded.owns_backing_store, original.owns_backing_store);
        assert_eq!(decoded.has_been_deleted, original.has_been_deleted);
        assert_eq!(decoded.dynamic, original.dynamic);
        assert_eq!(decoded.dirty, original.dirty);
    }

    #[test]
    fn test_info_bits_srgb_only() {
        let bits = GrcDX11InfoBits::from(0x01);
        assert!(bits.is_srgb);
        assert_eq!(bits.read_write_access, 0);
        assert!(!bits.owns_backing_store);
    }

    /// ⚠ A header states its own sides and level count, so no size here may panic or wrap.
    #[test]
    fn a_size_that_does_not_fit_32_bits_saturates_and_never_wraps_or_panics() {
        let wide = GrcTextureFormat::A32B32G32R32F;
        assert_eq!(wide.mip_size(65535, 65535), u32::MAX);
        assert_eq!(GrcTextureFormat::BC7.mip_size(65535, 65535), u32::MAX);
        assert_eq!(
            GrcTextureFormat::DXT5.mip_size(u32::MAX, u32::MAX),
            u32::MAX
        );
        assert_eq!(wide.total_size(65535, 65535, 3), u32::MAX);
        assert_eq!(wide.calculate_physical_size(4096, 4096, 64, 1, 1), u32::MAX);
        assert_eq!(
            wide.calculate_physical_size(16, 16, 1, 1, u32::MAX),
            u32::MAX
        );

        assert_eq!(wide.checked_mip_size(65535, 65535), None);
        assert_eq!(wide.checked_mip_size(4096, 4096), Some(4096 * 4096 * 16));
        assert_eq!(wide.checked_total_size(65535, 65535, 3), None);
        assert_eq!(wide.checked_physical_size(16, 16, 1, 1, u32::MAX), None);
        // A billion levels past the 1x1 one are one multiplication, not a billion additions.
        let dxt1 = GrcTextureFormat::DXT1;
        assert_eq!(dxt1.checked_total_size(1, 1, u32::MAX), None);
        assert_eq!(dxt1.checked_total_size(1, 1, 1000), Some(8000));
        assert_eq!(dxt1.checked_total_size(4, 4, 0), Some(0));
        // 64x64 down to 1x1, summed level by level from the block counts.
        let by_hand: u32 = (0..7u32)
            .map(|level| {
                let side = (64u32 >> level).max(1);
                side.div_ceil(4).pow(2) * 8
            })
            .sum();
        assert_eq!(dxt1.checked_total_size(64, 64, 7), Some(by_hand));
        assert_eq!(dxt1.total_size(64, 64, 7), by_hand);
        assert_eq!(dxt1.calculate_physical_size(64, 64, 1, 7, 6), 6 * by_hand);
    }
}

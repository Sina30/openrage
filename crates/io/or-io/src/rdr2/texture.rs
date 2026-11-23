//! A texture dictionary (`.ytd`): a `pgDictionary` of RDR2's textures.

use std::sync::{Arc, OnceLock};

use super::shared::PgDictionary;
use crate::codec::texture::{decode_level, save_png, DxgiFormat, GrcTextureFormat, TextureSource};
use crate::container::rpf::rsc8::Rsc8;
use crate::decl::*;
use crate::desc::{value, Pack, TypeName};
use crate::error::Error;

/// A dictionary of RDR2 textures.
pub type TextureDictionary = PgDictionary<GrcTextureRdr2>;

/// The shape of a texture.
#[record(enum, width = u8)]
pub enum TextureDimension {
    /// Texture2D
    Texture2D = 1,
    /// TextureCube
    TextureCube = 2,
    /// Texture3D
    Texture3D = 3,
}

/// How a texture's pixels are arranged in memory.
#[record(enum, width = u8)]
pub enum TileMode {
    /// Depth
    Depth = 4,
    /// Linear
    Linear = 8,
    /// Display
    Display = 10,
    /// Standard
    Standard = 13,
    /// RenderTarget
    RenderTarget = 14,
    /// VolumeStandard
    VolumeStandard = 19,
    /// VolumeRenderTarget
    VolumeRenderTarget = 20,
    /// Auto
    Auto = 255,
}

/// An RDR2 texture's pixel format: a DXGI format number, under DXGI's names.
#[record(enum, width = u8)]
pub enum PixelFormat {
    /// UNKNOWN
    Unknown = 0,
    /// R32G32B32A32_FLOAT
    R32G32B32A32Float = 2,
    /// R16G16B16A16_FLOAT
    R16G16B16A16Float = 10,
    /// R16G16B16A16_UNORM
    R16G16B16A16Unorm = 11,
    /// R10G10B10A2_UNORM
    R10G10B10A2Unorm = 24,
    /// R11G11B10_FLOAT
    R11G11B10Float = 26,
    /// R8G8B8A8_UNORM
    R8G8B8A8Unorm = 28,
    /// R8G8B8A8_UNORM_SRGB
    R8G8B8A8UnormSrgb = 29,
    /// R32_FLOAT
    R32Float = 41,
    /// R16_FLOAT
    R16Float = 54,
    /// R16_UNORM
    R16Unorm = 56,
    /// R8_UNORM
    R8Unorm = 61,
    /// A8_UNORM
    A8Unorm = 65,
    /// BC1_UNORM
    Bc1Unorm = 71,
    /// BC1_UNORM_SRGB
    Bc1UnormSrgb = 72,
    /// BC2_UNORM
    Bc2Unorm = 74,
    /// BC2_UNORM_SRGB
    Bc2UnormSrgb = 75,
    /// BC3_UNORM
    Bc3Unorm = 77,
    /// BC3_UNORM_SRGB
    Bc3UnormSrgb = 78,
    /// BC4_UNORM
    Bc4Unorm = 80,
    /// BC5_UNORM
    Bc5Unorm = 83,
    /// B5G5R5A1_UNORM
    B5G5R5A1Unorm = 86,
    /// B8G8R8A8_UNORM
    B8G8R8A8Unorm = 87,
    /// B8G8R8A8_UNORM_SRGB
    B8G8R8A8UnormSrgb = 91,
    /// BC6H_UF16
    Bc6hUf16 = 95,
    /// BC7_UNORM
    Bc7Unorm = 98,
    /// BC7_UNORM_SRGB
    Bc7UnormSrgb = 99,
}

/// The shader resource view an RDR2 texture embeds.
#[record(size = 0x40, target = "rdr2")]
pub struct ShaderResourceViewRdr2 {
    #[note("a runtime address, carried")]
    vft: u64,
    #[note("two 16-bit halves that vary by texture, carried whole")]
    after_vft: u64,
    /// Direct3D's view dimension as the engine packs it: 0x401 for a 2D texture, 0x601 for an array, 0x802 for a cube.
    pub view_dimension: u64,
    #[note("5 in the views written from scratch, carried")]
    #[init(5)]
    after_view_dimension: u64,
    #[note("carried")]
    second_after_view_dimension: u64,
    #[note("carried")]
    third_after_view_dimension: u64,
    #[note("carried")]
    fourth_after_view_dimension: u64,
    #[note("carried")]
    fifth_after_view_dimension: u64,
}

/// An RDR2 texture: the Enhanced header, then a view and words of its own.
#[record(size = 0xB0, target = "rdr2")]
pub struct GrcTextureRdr2 {
    #[note("a runtime address, carried")]
    vft: u64,
    #[note("blocks in the allocation: its length over the low twelve bits of `block_stride`")]
    block_count: u32,
    #[note("bytes one block of `format` takes: 8 for BC1, 16 for BC3, with flag bits above the low twelve")]
    block_stride: u32,
    pub flags: u32,
    #[note("zero in every texture measured, carried")]
    after_flags: u32,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub dimension: Open<TextureDimension>,
    pub format: Open<PixelFormat>,
    pub tile_mode: Open<TileMode>,
    pub anti_alias_type: u8,
    pub mip_levels: u8,
    #[note("carried")]
    after_mip_levels: u8,
    #[note("carried")]
    second_after_mip_levels: u8,
    #[note("carried")]
    third_after_mip_levels: u8,
    #[note("1 in every texture, as the Enhanced texture's usage count is")]
    #[init(1)]
    usage_count: u16,
    pub name: String,
    #[note("a pointer at the embedded `view`, or null in a texture whose view was never built; carried")]
    view_ptr: Ptr<Opaque>,
    #[phys]
    #[len(product = "block_count * block_stride")]
    pub pixels: Vec<u8>,
    #[note("carried")]
    after_pixels: u32,
    #[note("0 or 0x01000000, carried")]
    second_after_pixels: u32,
    #[note("carried")]
    third_after_pixels: u64,
    #[note("carried")]
    fourth_after_pixels: u64,
    #[note("carried")]
    fifth_after_pixels: u64,
    #[note("carried")]
    sixth_after_pixels: u64,
    pub view: ShaderResourceViewRdr2,
    #[note("carried")]
    after_view: u64,
}

impl TextureSource for GrcTextureRdr2 {
    fn pixel_format(&self) -> GrcTextureFormat {
        GrcTextureFormat::from_dxgi(DxgiFormat::from_raw(u32::from(self.format.raw()))).0
    }

    fn is_srgb(&self) -> bool {
        DxgiFormat::from_raw(u32::from(self.format.raw())).is_srgb()
    }

    fn stored_size(&self) -> (u32, u32) {
        (u32::from(self.width).max(1), u32::from(self.height).max(1))
    }

    fn stored_levels(&self) -> u32 {
        u32::from(self.mip_levels.max(1))
    }

    fn payload(&self) -> &[u8] {
        &self.pixels
    }
}

impl GrcTextureRdr2 {
    /// Writes the first stored mip to `path` as a PNG; a swizzled tile mode is refused.
    pub fn to_png(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
        // A PC texture is `Standard` and stores the payload row by row, as CodeX reads it; the other
        // modes are a console's.
        if !matches!(
            self.tile_mode.get(),
            TileMode::Auto | TileMode::Linear | TileMode::Standard
        ) {
            return Err(Error::InvalidData {
                context: format!(
                    "texture {:?} has tile mode {:?}, whose swizzled payload cannot be decoded",
                    self.name, self.tile_mode
                ),
            });
        }
        save_png(path, &decode_level(self, 0)?)
    }
}

/// The built-in description of a dictionary of RDR2 textures, built once.
fn description() -> Result<Arc<Pack>, Error> {
    static PACK: OnceLock<Result<Arc<Pack>, String>> = OnceLock::new();
    PACK.get_or_init(|| {
        describe("rdr2", "rdr2", &[root::<Raw<TextureDictionary>>])
            .map(Arc::new)
            .map_err(|problems| problems.join("; "))
    })
    .clone()
    .map_err(|problems| Error::Description {
        ext: "ytd",
        problems,
    })
}

impl File for TextureDictionary {
    const EXTENSION: &'static str = "ytd";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let rsc = Rsc8::open(bytes).map_err(|e| Error::NotAResource(e.to_string()))?;
        let data = Aligned::from_bytes(rsc.data());
        let image = Image::new(data.bytes(), rsc.virtual_size());
        let name = TypeName(intern(&<Raw<TextureDictionary> as Described>::name()));
        let pack = description()?;
        let tree = value::read(&pack, name, image, 0).map_err(|e| Error::Read {
            ext: "ytd",
            why: e.to_string(),
        })?;
        Self::from_element(&tree)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Err(Error::Save {
            ext: "ytd",
            why: "an RSC8 resource is read, not yet written".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::string_hash;

    fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
        bytes[at..at + value.len()].copy_from_slice(value);
    }

    /// A dictionary of one 8x8 BC1 texture named `rock`, laid out as the game writes it: the
    /// dictionary, its codes, its pointers and the texture in the virtual bytes, the pixels beside them.
    fn dictionary() -> Vec<u8> {
        let mut virt = vec![0u8; 0x120];
        put(&mut virt, 0x20, &0x5000_0040u64.to_le_bytes());
        put(&mut virt, 0x28, &1u16.to_le_bytes());
        put(&mut virt, 0x28 + 2, &1u16.to_le_bytes());
        put(&mut virt, 0x30, &0x5000_0050u64.to_le_bytes());
        put(&mut virt, 0x38, &1u16.to_le_bytes());
        put(&mut virt, 0x38 + 2, &1u16.to_le_bytes());
        put(&mut virt, 0x40, &string_hash(b"rock").to_le_bytes());
        put(&mut virt, 0x50, &0x5000_0060u64.to_le_bytes());
        let t = 0x60;
        put(&mut virt, t + 8, &4u32.to_le_bytes());
        put(&mut virt, t + 12, &8u32.to_le_bytes());
        put(&mut virt, t + 0x18, &8u16.to_le_bytes());
        put(&mut virt, t + 0x1A, &8u16.to_le_bytes());
        put(&mut virt, t + 0x1C, &1u16.to_le_bytes());
        put(&mut virt, t + 0x1E, &[1, 71, 13, 0, 1]);
        put(&mut virt, t + 0x26, &1u16.to_le_bytes());
        put(&mut virt, t + 0x28, &0x5000_0110u64.to_le_bytes());
        put(
            &mut virt,
            t + 0x30,
            &(0x5000_0000u64 + t as u64 + 0x68).to_le_bytes(),
        );
        put(&mut virt, t + 0x38, &0x6000_0000u64.to_le_bytes());
        put(&mut virt, 0x110, b"rock\0");
        let mut body = virt;
        body.extend((0..32u8).collect::<Vec<_>>());
        let mut file = Vec::new();
        for word in [0x3843_5352u32, 0x1F << 8, 0x120, 32] {
            file.extend_from_slice(&word.to_le_bytes());
        }
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn a_dictionary_names_its_textures_by_hash_and_they_decode() {
        let dict = TextureDictionary::from_bytes(&dictionary()).expect("a dictionary");
        assert_eq!(dict.codes, vec![string_hash(b"rock")]);
        let rock = dict.entries[0].as_ref().expect("a texture");
        assert_eq!(
            (rock.name.as_str(), rock.width, rock.height),
            ("rock", 8, 8)
        );
        assert_eq!(rock.pixels, (0..32u8).collect::<Vec<_>>());
        assert_eq!(rock.tile_mode.get(), TileMode::Standard);
        let image = decode_level(&**rock, 0).expect("decodes");
        assert_eq!((image.width, image.height), (8, 8));
    }

    #[test]
    fn a_dictionary_whose_pixels_lie_outside_the_resource_is_refused() {
        let mut file = dictionary();
        file.truncate(file.len() - 16);
        file[12..16].copy_from_slice(&16u32.to_le_bytes());
        assert!(TextureDictionary::from_bytes(&file).is_err());
    }

    #[test]
    fn a_dictionary_is_read_and_not_yet_written() {
        let dict = TextureDictionary::from_bytes(&dictionary()).expect("a dictionary");
        assert!(dict.to_bytes().is_err());
    }
}

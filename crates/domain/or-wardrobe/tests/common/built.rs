//! Files built to have the fault a rule looks for: a dictionary of textures of a chosen size, mips and format.

use or_io::codec::texture::{max_mip_levels, GrcTextureFormat};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;

/// One texture of a dictionary built here.
pub struct Tex {
    pub name: String,
    pub width: u16,
    pub height: u16,
    /// How many mips it stores; the whole chain down to one pixel where none is said.
    pub mips: Option<u8>,
    pub format: GrcTextureFormat,
}

impl Tex {
    /// A texture of `width` by `height` in `format`, with every mip.
    pub fn new(name: impl Into<String>, width: u16, height: u16, format: GrcTextureFormat) -> Self {
        Self {
            name: name.into(),
            width,
            height,
            mips: None,
            format,
        }
    }

    pub fn with_mips(self, mips: u8) -> Self {
        Self {
            mips: Some(mips),
            ..self
        }
    }

    /// How many mips it stores.
    pub fn stored(&self) -> u8 {
        self.mips
            .unwrap_or_else(|| max_mip_levels(self.width.into(), self.height.into()) as u8)
    }
}

/// The bytes of a Legacy `.ytd` holding `textures`, whose pixels are all zero.
pub fn ytd(textures: &[Tex]) -> Vec<u8> {
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    for t in textures {
        let size = t
            .format
            .total_size(t.width.into(), t.height.into(), t.stored().into())
            as usize;
        let unit = (or_io::gta5::texture_dict::gen8::PAYLOAD_SIZE_MASK
            & or_io::gta5::texture_dict::gen8::PAYLOAD_SIZE_MASK.wrapping_neg())
            as usize;
        dictionary.insert(GrcTexture {
            name: t.name.clone(),
            width: t.width,
            height: t.height,
            depth: 1,
            mip_stride: t.format.legacy_mip_stride(t.width.into()) as u16,
            format: t.format.to_raw_format(),
            mip_count: t.stored(),
            backing_store: vec![0; size.next_multiple_of(unit)],
            resource_type_and_conversion_flags: or_io::gta5::texture_dict::gen8::PROCESSED,
            ..GrcTexture::default()
        });
    }
    dictionary
        .to_bytes()
        .expect("a dictionary of textures saves")
}

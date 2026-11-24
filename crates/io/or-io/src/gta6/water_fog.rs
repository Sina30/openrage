//! The medium of GTA VI's water by place: the fog atlas's textures, each an unsigned BC6H texture on
//! the grid a [`super::GridAtlas`] states, read as floats.

use crate::codec::texture::{decode_texture_hdr, DxgiFormat, GrcTextureFormat};
use crate::gta6::ttpf::Texture;

/// A fog texture's top level, row by row from its north edge, each texel's red, green and blue.
#[derive(Debug, Clone, PartialEq)]
pub struct FogTexels {
    width: u32,
    height: u32,
    texels: Vec<[f32; 3]>,
}

impl FogTexels {
    /// The top level of an unsigned BC6H texture; `None` for another format or too few bytes.
    pub fn of(texture: &Texture) -> Option<Self> {
        if texture.format.dxgi() != DxgiFormat::Bc6hUf16 {
            return None;
        }
        let image = decode_texture_hdr(
            GrcTextureFormat::BC6,
            texture.width,
            texture.height,
            &texture.pixels,
        )
        .ok()?;
        let texels = (image.data.as_chunks::<4>().0.iter())
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        Some(Self {
            width: texture.width,
            height: texture.height,
            texels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Every texel, row by row from the north edge.
    pub fn texels(&self) -> &[[f32; 3]] {
        &self.texels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gta6::ttpf::Format;

    #[test]
    fn an_unsigned_bc6h_texture_reads_as_floats_and_another_format_or_too_few_bytes_as_none() {
        let texture = Texture {
            width: 8,
            height: 4,
            format: Format::of_dxgi(95).expect("BC6H_UF16"),
            levels: 1,
            faces: 1,
            pixels: vec![0; 32],
        };
        let texels = FogTexels::of(&texture).expect("two blocks");
        assert_eq!((texels.width(), texels.height()), (8, 4));
        assert_eq!(texels.texels(), &[[0.0; 3]; 32][..]);
        let short = Texture {
            pixels: vec![0; 16],
            ..texture.clone()
        };
        assert!(FogTexels::of(&short).is_none(), "one block of two");
        let other = Texture {
            format: Format::of_dxgi(96).expect("BC6H_SF16"),
            ..texture
        };
        assert!(FogTexels::of(&other).is_none());
    }
}

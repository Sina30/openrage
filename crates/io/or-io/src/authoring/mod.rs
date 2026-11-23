//! Texture authoring: BCn encode, mip chains built for encoding, fitting and
//! image import. BC7 needs the `bc7-encode` feature.

pub mod encode;
pub mod fit;
mod psd;
pub mod texture_import;

pub use encode::{
    automatic_format, encode_level, encode_texture_with_mips, encode_texture_with_options,
    is_encodable, rgba_psnr, EncodeQuality, EncodedTexture, FormatChoice, PsnrReport,
    TextureBuildOptions,
};
pub use fit::{fit_within, halvings_to_fit};

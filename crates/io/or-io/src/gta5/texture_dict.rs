//! `.ytd`: textures keyed by name hash. The dictionary is shared; the texture
//! is declared per target, because Enhanced rebuilt its header from scratch.

use crate::decl::*;
use crate::Generation;

/// The Legacy texture and the header it embeds.
pub mod gen8 {
    use super::super::shared::PgDictionary;
    use crate::codec::texture::{decode_level, save_png, GrcTextureFormat, TextureSource};
    use crate::decl::*;
    use crate::Generation;

    impl Resource for PgDictionary<GrcTexture> {
        const EXT: &'static str = "ytd";
        const TARGET: Generation = Generation::Gen8;
    }

    /// The mask that turns a Legacy texture's `physical_size_and_template_type` into a byte count.
    pub const PAYLOAD_SIZE_MASK: u32 = 0x07FF_FF80;

    /// The conversion flag in `resource_type_and_conversion_flags` that a processed texture states.
    pub const PROCESSED: u8 = 0x80;

    /// `grcCellGcmTextureWrapper`: the console texture header every Legacy texture embeds at 0x10.
    #[record(size = 0x18, target = "gen8")]
    pub struct GrcCellGcmTextureWrapper {
        pub format: u8,
        pub mipmap: u8,
        pub dimension: u8,
        pub cubemap: u8,
        pub remap: u32,
        /// ⚠ Not the texture's size: these three words match its width,
        /// height and depth in none of the corpus's 290 Legacy textures.
        pub width: u16,
        pub height: u16,
        pub depth: u16,
        pub location: u8,
        padding: u8,
        pub pitch: u32,
        pub offset: u32,
    }

    /// A Legacy texture: `grcTexture`, `grcTexturePC` and `grcTextureDX11` as one object.
    #[record(size = 0x90, target = "gen8")]
    pub struct GrcTexture {
        pub base: PgBase,
        pub texture: GrcCellGcmTextureWrapper,
        pub name: String,
        #[init(1)]
        ref_count: u16,
        pub resource_type_and_conversion_flags: u8,
        #[pad_after(4)]
        pub layer_count: u8,
        cached_texture_ptr: Ptr<Opaque>,
        /// Masked with [`PAYLOAD_SIZE_MASK`], the pixel payload's byte count;
        /// the other bits are the template type.
        physical_size_and_template_type: u32,
        pub handle_index: u32,
        #[pad_after(4)]
        pub extra_flags: u32,
        pub width: u16,
        pub height: u16,
        pub depth: u16,
        pub mip_stride: u16,
        /// A Direct3D 9 format: a FourCC for a block-compressed texture.
        pub format: u32,
        pub image_type: u8,
        pub mip_count: u8,
        pub cut_mip_levels: u8,
        pub info_bits: u8,
        next: Ptr<Opaque>,
        previous: Ptr<Opaque>,
        #[phys]
        #[len(
            masked_or_mip_chain = "physical_size_and_template_type",
            mask = 0x07FF_FF80
        )]
        pub backing_store: Vec<u8>,
        shader_resource_view: Ptr<Opaque>,
        extra_data: Ptr<Opaque>,
        #[pad_after(4)]
        pub stereo_rt_mode: i32,
    }

    use super::super::shared::PgBase;

    impl TextureSource for GrcTexture {
        fn pixel_format(&self) -> GrcTextureFormat {
            GrcTextureFormat::from(self.format)
        }

        fn is_srgb(&self) -> bool {
            self.info_bits & 0x01 != 0
        }

        fn stored_size(&self) -> (u32, u32) {
            let cut = u32::from(self.cut_mip_levels);
            (
                (u32::from(self.width) >> cut).max(1),
                (u32::from(self.height) >> cut).max(1),
            )
        }

        fn stored_levels(&self) -> u32 {
            u32::from(self.mip_count.saturating_sub(self.cut_mip_levels).max(1))
        }

        fn payload(&self) -> &[u8] {
            &self.backing_store
        }
    }

    impl GrcTexture {
        /// Writes the first stored mip to `path` as a PNG.
        pub fn to_png(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
            save_png(path, &decode_level(self, 0)?)
        }

        /// A texture named after the file's stem, compressed with its whole mip chain: BC3 if any pixel is translucent, BC1 if none.
        #[cfg(feature = "authoring")]
        pub fn from_png(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
            let authored = super::Authored::from_png(path.as_ref())?;
            let mut backing_store = authored.data;
            // The masked word states only whole multiples of the mask's lowest bit.
            let unit = (PAYLOAD_SIZE_MASK & PAYLOAD_SIZE_MASK.wrapping_neg()) as usize;
            backing_store.resize(backing_store.len().next_multiple_of(unit), 0);
            Ok(Self {
                name: authored.name,
                width: authored.width,
                height: authored.height,
                depth: 1,
                mip_stride: authored.stride,
                format: authored.format.to_raw_format(),
                mip_count: authored.mip_count,
                info_bits: u8::from(authored.srgb),
                backing_store,
                resource_type_and_conversion_flags: PROCESSED,
                ..Default::default()
            })
        }

        /// Replaces the pixels with `image` encoded as `format`, keeping the name and the usage classification.
        #[cfg(feature = "authoring")]
        pub fn set_pixels(
            &mut self,
            format: GrcTextureFormat,
            image: &crate::codec::texture::RgbaImage,
            options: &crate::authoring::TextureBuildOptions,
        ) -> crate::Result<()> {
            let name = self.name.clone();
            let refused = |why: String| crate::Error::InvalidData {
                context: format!("texture {name:?}: {why}"),
            };
            let (Ok(width), Ok(height)) = (u16::try_from(image.width), u16::try_from(image.height))
            else {
                return Err(refused(format!(
                    "{}x{} does not fit a texture's 16-bit sides",
                    image.width, image.height
                )));
            };
            let encoded = crate::authoring::encode_texture_with_options(
                format,
                image.width,
                image.height,
                image,
                options,
            )?;
            let stride = format.legacy_mip_stride(image.width);
            let mip_stride = u16::try_from(stride)
                .map_err(|_| refused(format!("a {stride}-byte row does not fit 16 bits")))?;
            let mut backing_store = encoded.data;
            // The masked word states only whole multiples of the mask's lowest bit.
            let unit = (PAYLOAD_SIZE_MASK & PAYLOAD_SIZE_MASK.wrapping_neg()) as usize;
            backing_store.resize(backing_store.len().next_multiple_of(unit), 0);
            self.width = width;
            self.height = height;
            self.depth = 1;
            self.format = format.to_raw_format();
            self.image_type = 0;
            self.layer_count = 0;
            self.mip_count = encoded.mip_count;
            self.cut_mip_levels = 0;
            self.mip_stride = mip_stride;
            self.info_bits = if options.srgb {
                self.info_bits | 0x01
            } else {
                self.info_bits & !0x01
            };
            self.backing_store = backing_store;
            Ok(())
        }

        /// The pixel payload's byte count, which a save states in its masked bits.
        pub fn payload_size(&self) -> usize {
            self.backing_store.len()
        }

        /// The template type: the bits of `physical_size_and_template_type` outside [`PAYLOAD_SIZE_MASK`].
        pub fn template_type(&self) -> u32 {
            self.__carry
                .hidden::<u32>(std::mem::offset_of!(
                    Raw<Self>,
                    physical_size_and_template_type
                ))
                .map_or(0, |word| word & !PAYLOAD_SIZE_MASK)
        }

        /// Sets the template type; bits under [`PAYLOAD_SIZE_MASK`] are the payload's count and are ignored.
        pub fn set_template_type(&mut self, template_type: u32) {
            let at = std::mem::offset_of!(Raw<Self>, physical_size_and_template_type);
            let count = self
                .__carry
                .hidden::<u32>(at)
                .map_or(0, |w| w & PAYLOAD_SIZE_MASK);
            self.__carry
                .hide(at, count | (template_type & !PAYLOAD_SIZE_MASK));
        }
    }
}

/// The Enhanced texture, the view it embeds, and the enums it holds.
#[cfg(feature = "gen9")]
pub mod gen9 {
    use super::super::shared::PgDictionary;
    use crate::codec::texture::{
        decode_level, save_png, DxgiFormat, GrcTextureFormat, TextureSource,
    };
    use crate::decl::*;
    use crate::Generation;

    impl Resource for PgDictionary<GrcTextureG9> {
        const EXT: &'static str = "ytd";
        const TARGET: Generation = Generation::Gen9;
    }

    /// The shape of an Enhanced texture.
    #[record(enum, width = u8)]
    pub enum TextureDimension {
        /// Texture2D
        Texture2D = 1,
        /// TextureCube
        TextureCube = 2,
        /// Texture3D
        Texture3D = 3,
    }

    /// How an Enhanced texture's pixels are arranged in memory.
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

    /// An Enhanced texture's pixel format: a DXGI format number, under DXGI's names.
    #[record(enum, width = u8)]
    pub enum PixelFormat {
        /// Unknown
        Unknown = 0,
        /// R8G8B8A8_UNORM
        R8G8B8A8Unorm = 28,
        /// R8G8B8A8_UNORM_SRGB
        R8G8B8A8UnormSrgb = 29,
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
        /// B5G6R5_UNORM
        B5G6R5Unorm = 85,
        /// B5G5R5A1_UNORM
        B5G5R5A1Unorm = 86,
        /// B8G8R8A8_UNORM
        B8G8R8A8Unorm = 87,
        /// B8G8R8A8_UNORM_SRGB
        B8G8R8A8UnormSrgb = 91,
        /// BC6H_UF16
        Bc6hUf16 = 95,
        /// BC6H_SF16
        Bc6hSf16 = 96,
        /// BC7_UNORM
        Bc7Unorm = 98,
        /// BC7_UNORM_SRGB
        Bc7UnormSrgb = 99,
    }

    // Each value a note below states is what or-io-verify's
    // `the_carried_enhanced_texture_words_hold_what_their_notes_state` counts
    // over the 147 Enhanced textures the corpus holds, in `.ytd`, `.ydr`,
    // `.ydd`, `.yft` and `.ypt` files; it fails on a value a note does not state.

    /// The shader resource view an Enhanced texture embeds.
    #[record(size = 0x20, target = "gen9")]
    pub struct ShaderResourceViewG9 {
        #[note("a runtime address, carried")]
        vft: u64,
        #[note("zero in every view: no other value to name it by, so carried")]
        after_vft: u64,
        /// Direct3D's `ViewDimension`, as the engine packs it: 0x41 in every 2D texture.
        pub view_dimension: u16,
        #[note("0xFFFF in every view: no other value to name it by, so carried")]
        #[init(0xFFFF)]
        after_view_dimension: u16,
        #[note(
            "0xFFFFFFFF in every view, Direct3D's all-levels value, beside mip counts from 1 to 11: \
             no view holds another value to say which count it is, so carried"
        )]
        #[init(u32::MAX)]
        second_after_view_dimension: u32,
        #[note("zero in every view: no other value to name it by, so carried")]
        third_after_view_dimension: u64,
    }

    /// An Enhanced texture. ⚠ Not a variation on the Legacy one: a different header.
    #[record(size = 0x80, target = "gen9")]
    pub struct GrcTextureG9 {
        #[note("a runtime address, carried")]
        vft: u64,
        #[note("blocks in the payload: written at save as its length over `block_stride`")]
        block_count: u32,
        #[note("bytes one block of `format` takes: 8 for BC1, 16 for BC3")]
        block_stride: u32,
        pub flags: u32,
        #[note("zero in every texture: no other value to name it by, so carried")]
        after_flags: u32,
        pub width: u16,
        pub height: u16,
        pub depth: u16,
        pub dimension: Open<TextureDimension>,
        pub format: Open<PixelFormat>,
        pub tile_mode: Open<TileMode>,
        pub anti_alias_type: u8,
        pub mip_levels: u8,
        #[note(
            "0x28 in 39 textures and zero in 108, and textures alike in format, size, mip levels, \
             flags and usage_data hold each: nothing measured predicts it, so carried"
        )]
        after_mip_levels: u8,
        #[note("zero in every texture: no other value to name it by, so carried")]
        second_after_mip_levels: u8,
        #[note("zero in every texture: no other value to name it by, so carried")]
        third_after_mip_levels: u8,
        #[note("1 in every texture, as the Legacy texture's reference count is")]
        #[init(1)]
        ref_count: u16,
        pub name: String,
        #[note("the texture's own `view`, before and after every save")]
        #[init(Member::to("view"))]
        view_ptr: Member<ShaderResourceViewG9>,
        #[phys]
        #[len(product = "block_count * block_stride")]
        pub pixels: Vec<u8>,
        pub usage_data: u32,
        #[note(
            "2 in every texture, each a dictionary entry: no texture in another role is measured \
             to name it by, so carried"
        )]
        #[init(2)]
        after_usage_data: u32,
        #[note("zero in every texture: no other value to name it by, so carried")]
        second_after_usage_data: u64,
        #[note("zero in every texture: no other value to name it by, so carried")]
        third_after_usage_data: u64,
        pub view: ShaderResourceViewG9,
        #[note("zero in every texture: no other value to name it by, so carried")]
        after_view: u64,
    }

    impl TextureSource for GrcTextureG9 {
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

    impl GrcTextureG9 {
        /// Writes the first stored mip to `path` as a PNG; a swizzled tile mode is refused.
        pub fn to_png(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
            // Only Auto and Linear store the payload row by row; every other mode swizzles it.
            if !matches!(self.tile_mode.get(), TileMode::Auto | TileMode::Linear) {
                return Err(crate::Error::InvalidData {
                    context: format!(
                        "texture {:?} has tile mode {:?}, whose swizzled payload cannot be decoded",
                        self.name, self.tile_mode
                    ),
                });
            }
            save_png(path, &decode_level(self, 0)?)
        }

        /// A 2D texture named after the file's stem, compressed with its whole mip chain: BC3 if any pixel is translucent, BC1 if none.
        #[cfg(feature = "authoring")]
        pub fn from_png(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
            let authored = super::Authored::from_png(path.as_ref())?;
            let dxgi = authored.format.to_dxgi(authored.srgb) as u32;
            let format = u8::try_from(dxgi).map_err(|_| crate::Error::InvalidData {
                context: format!("{:?} has no Enhanced pixel format", authored.format),
            })?;
            let mut texture = Self {
                name: authored.name,
                width: authored.width,
                height: authored.height,
                depth: 1,
                dimension: Open::new(TextureDimension::Texture2D),
                format: Open::new(<PixelFormat as OpenEnum>::from_raw(format)),
                tile_mode: Open::new(TileMode::Auto),
                mip_levels: authored.mip_count,
                pixels: authored.data,
                view: ShaderResourceViewG9 {
                    view_dimension: 0x41,
                    ..Default::default()
                },
                ..Default::default()
            };
            texture.set_block_stride(authored.format.bytes_per_block());
            Ok(texture)
        }

        /// Bytes one block of `format` takes, which a save divides the payload's length by.
        pub fn block_stride(&self) -> u32 {
            self.__carry
                .hidden::<u32>(std::mem::offset_of!(Raw<Self>, block_stride))
                .copied()
                .unwrap_or(0)
        }

        /// Sets the block stride; a save refuses a payload that is not a whole number of blocks.
        pub fn set_block_stride(&mut self, stride: u32) {
            self.__carry
                .hide(std::mem::offset_of!(Raw<Self>, block_stride), stride);
        }
    }
}

/// A PNG encoded for a new texture of either target.
#[cfg(feature = "authoring")]
struct Authored {
    name: String,
    width: u16,
    height: u16,
    format: crate::codec::texture::GrcTextureFormat,
    srgb: bool,
    mip_count: u8,
    stride: u16,
    data: Vec<u8>,
}

#[cfg(feature = "authoring")]
impl Authored {
    fn from_png(path: &std::path::Path) -> crate::Result<Self> {
        use crate::authoring::{
            automatic_format, encode_texture_with_options, TextureBuildOptions,
        };
        let refused = |why: String| crate::Error::InvalidData {
            context: format!("{}: {why}", path.display()),
        };
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| refused("no file name to name the texture by".to_string()))?
            .to_string();
        let image = crate::codec::texture::load_png(path)?;
        let (Ok(width), Ok(height)) = (u16::try_from(image.width), u16::try_from(image.height))
        else {
            return Err(refused(format!(
                "{}x{} does not fit a texture's 16-bit sides",
                image.width, image.height
            )));
        };
        let format = automatic_format(&image);
        let options = TextureBuildOptions::default();
        let encoded =
            encode_texture_with_options(format, image.width, image.height, &image, &options)?;
        let stride = format.legacy_mip_stride(image.width);
        let stride = u16::try_from(stride)
            .map_err(|_| refused(format!("a {stride}-byte row does not fit 16 bits")))?;
        Ok(Self {
            name,
            width,
            height,
            format,
            srgb: options.srgb,
            mip_count: encoded.mip_count,
            stride,
            data: encoded.data,
        })
    }
}

/// A dictionary the game would misread: why, and which texture.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("the dictionary's hashes are not ascending, so a lookup would find the wrong texture")]
    HashesUnsorted,
    #[error("the dictionary holds {hashes} hashes and {entries} textures")]
    Lengths { hashes: usize, entries: usize },
    #[error("texture {hash:#010x} states a zero-byte payload beside a live pointer, so its pixels would be lost")]
    UnstatedPayload { hash: u32 },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// The dictionary's order, and each texture `lost` says would lose its pixels.
fn check<T: Owned>(
    image: Image<'_>,
    root: usize,
    lost: fn(&View<'_, Raw<T>>) -> Result<bool, ViewError>,
) -> Result<(), Broken>
where
    Raw<T>: Record,
{
    let dict = View::<Raw<super::shared::PgDictionary<T>>>::at_offset(image, root)?;
    let codes = dict.codes()?;
    let entries = dict.entries()?;
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            hashes: codes.len(),
            entries: entries.len(),
        });
    }
    if !codes.windows(2).all(|w| w[0] <= w[1]) {
        return Err(Broken::HashesUnsorted);
    }
    for (i, code) in codes.iter().enumerate() {
        if let Some(texture) = entries.follow(i)? {
            if lost(&texture)? {
                return Err(Broken::UnstatedPayload { hash: *code });
            }
        }
    }
    Ok(())
}

/// Every texture in `image`'s dictionary, checked for what the game assumes and never checks.
#[cfg_attr(not(feature = "gen9"), allow(unused_variables))]
pub fn validate(image: Image<'_>, root: usize, target: Generation) -> Result<(), Broken> {
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        return check::<gen9::GrcTextureG9>(image, root, |t| {
            Ok(!t.pixels.is_null() && t.block_count as usize * t.block_stride as usize == 0)
        });
    }
    // A zero under the mask states the mip chain, so only a chain of nothing loses pixels.
    check::<gen8::GrcTexture>(image, root, |t| {
        Ok(!t.backing_store.is_null() && t.backing_store()?.as_slice().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::super::shared::PgDictionary;
    use super::gen8::GrcTexture;
    use crate::codec::texture::{decode_png, load_png, save_png, RgbaImage};
    use crate::container::resource::{ResourceEnvelope, Rsc7};
    use crate::decl::File;

    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir().expect("scratch directory")
    }

    /// Four flat quadrants, red, green, blue and a translucent grey, so a swapped or lost channel shows.
    fn quadrants() -> RgbaImage {
        let mut image = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let px = match (x < 8, y < 8) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [128, 128, 128, 64],
                };
                image.set_pixel(x, y, px);
            }
        }
        image
    }

    /// The largest difference in each of R, G, B and A.
    fn per_channel(a: &RgbaImage, b: &RgbaImage) -> [u8; 4] {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let mut worst = [0u8; 4];
        for (pa, pb) in a
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.data.as_chunks::<4>().0.iter())
        {
            for c in 0..4 {
                worst[c] = worst[c].max(pa[c].abs_diff(pb[c]));
            }
        }
        worst
    }

    #[cfg(feature = "authoring")]
    #[test]
    fn a_known_image_survives_from_png_save_open_to_png_per_channel() {
        let known = quadrants();
        let dir = scratch();
        let source = dir.path().join("stone.png");
        save_png(&source, &known).unwrap();

        let legacy = GrcTexture::from_png(&source).unwrap();
        assert_eq!(legacy.name, "stone");
        let ytd = dir.path().join("legacy.ytd");
        PgDictionary {
            codes: vec![crate::hash::string_hash_str(&legacy.name)],
            entries: vec![Some(Box::new(legacy))],
            ..Default::default()
        }
        .save_as(&ytd)
        .unwrap();
        let reopened = PgDictionary::<GrcTexture>::open(&ytd).unwrap();
        let out = dir.path().join("legacy.png");
        reopened.get("stone").unwrap().to_png(&out).unwrap();
        let legacy_worst = per_channel(&known, &load_png(&out).unwrap());

        #[cfg(feature = "gen9")]
        let enhanced_worst = {
            use super::gen9::GrcTextureG9;
            let enhanced = GrcTextureG9::from_png(&source).unwrap();
            assert_eq!(enhanced.name, "stone");
            let ytd = dir.path().join("enhanced.ytd");
            PgDictionary {
                codes: vec![crate::hash::string_hash_str(&enhanced.name)],
                entries: vec![Some(Box::new(enhanced))],
                ..Default::default()
            }
            .save_as(&ytd)
            .unwrap();
            let reopened = PgDictionary::<GrcTextureG9>::open(&ytd).unwrap();
            let out = dir.path().join("enhanced.png");
            reopened.entries[0]
                .as_deref()
                .unwrap()
                .to_png(&out)
                .unwrap();
            per_channel(&known, &load_png(&out).unwrap())
        };
        #[cfg(not(feature = "gen9"))]
        let enhanced_worst = [0u8; 4];

        // BC3 stores a flat colour to within one step of 5:6:5 and alpha to within one of eight levels.
        for (edition, worst) in [("Legacy", legacy_worst), ("Enhanced", enhanced_worst)] {
            for (channel, delta) in ["R", "G", "B", "A"].iter().zip(worst) {
                assert!(delta <= 8, "{edition} {channel} drifted by {delta}");
            }
        }
    }

    #[cfg(feature = "gen9")]
    #[test]
    fn an_enhanced_texture_with_a_swizzled_tile_mode_is_refused_by_to_png() {
        use super::gen9::{GrcTextureG9, PixelFormat, TileMode};
        use crate::decl::Open;
        let texture = |tile_mode| GrcTextureG9 {
            name: "tiled".to_string(),
            width: 4,
            height: 4,
            depth: 1,
            format: Open::new(PixelFormat::R8G8B8A8Unorm),
            tile_mode: Open::new(tile_mode),
            mip_levels: 1,
            pixels: vec![0x80; 4 * 4 * 4],
            ..Default::default()
        };
        let dir = scratch();
        let out = dir.path().join("tiled.png");
        for decodable in [TileMode::Auto, TileMode::Linear] {
            texture(decodable)
                .to_png(&out)
                .unwrap_or_else(|e| panic!("{decodable:?}: {e}"));
        }
        for swizzled in [TileMode::Standard, TileMode::Display, TileMode::Other(7)] {
            let error = texture(swizzled).to_png(&out).unwrap_err().to_string();
            assert!(
                error.contains("tiled") && error.contains(&format!("{swizzled:?}")),
                "{swizzled:?}: {error}"
            );
        }
    }

    #[test]
    fn every_corpus_texture_decodes_through_to_png() {
        let Some(files) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ytd")
            .all_targets()
            .find_or_skip("every corpus texture's to_png")
        else {
            return;
        };
        let dir = scratch();
        let ytd = super::super::formats::format("ytd").unwrap();
        // One file's textures decode on their own, to a png of their own.
        let one = |path: &std::path::PathBuf| {
            let file = path.file_name().unwrap().to_string_lossy();
            let out = dir.path().join(format!("{file}.png"));
            let (mut legacy, mut enhanced) = (0usize, 0usize);
            let check = |name: &str, size: (u32, u32), written: crate::Result<()>| {
                written.unwrap_or_else(|e| panic!("{name}: {e}"));
                let image = decode_png(&std::fs::read(&out).unwrap()).unwrap();
                assert_eq!((image.width, image.height), size, "{name}");
            };
            let bytes = std::fs::read(path).unwrap();
            let version = Rsc7::peek(&bytes)
                .unwrap_or_else(|e| panic!("{file}: {e}"))
                .version();
            if ytd.target_of_version(version).is_err() {
                return (0, 0, Some(format!("{file} (version {version})")));
            }
            let opened = crate::open(path).unwrap_or_else(|e| panic!("{file}: {e}"));
            match opened {
                crate::Asset::TextureDict(dict) => {
                    for t in dict.entries.iter().flatten() {
                        let size = crate::codec::texture::TextureSource::stored_size(&**t);
                        check(&format!("{file}:{}", t.name), size, t.to_png(&out));
                        legacy += 1;
                    }
                }
                #[cfg(feature = "gen9")]
                crate::Asset::TextureDictGen9(dict) => {
                    for t in dict.entries.iter().flatten() {
                        let size = crate::codec::texture::TextureSource::stored_size(&**t);
                        check(&format!("{file}:{}", t.name), size, t.to_png(&out));
                        enhanced += 1;
                    }
                }
                _ => panic!("{file} did not open as a texture dictionary"),
            }
            (legacy, enhanced, None)
        };
        // The cores share the files; the counts fold in file order.
        let next = std::sync::atomic::AtomicUsize::new(0);
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut parts: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(path) = files.get(at) else {
                                return mine;
                            };
                            mine.push((at, one(path)));
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|run| run.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
                .collect()
        });
        parts.sort_by_key(|(at, _)| *at);
        let (mut legacy, mut enhanced) = (0usize, 0usize);
        let mut undeclared = Vec::new();
        for (_, (l, e, skipped)) in parts {
            legacy += l;
            enhanced += e;
            undeclared.extend(skipped);
        }
        println!(
            "{legacy} Legacy and {enhanced} Enhanced textures in {} files decode through to_png; \
             at a version no layout declares: {}",
            files.len() - undeclared.len(),
            undeclared.join(", ")
        );
        assert!(legacy > 0, "no Legacy texture was decoded");
        #[cfg(feature = "gen9")]
        assert!(enhanced > 0, "no Enhanced texture was decoded");
    }
}

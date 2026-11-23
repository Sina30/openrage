//! A value a conversion drops, in the words a modder reads: which shader, texture or
//! mesh holds it, and what it is, with none of the declarations' paths.
//!
//! ⚠ `openrage-cli convert retarget` prints the same `Dropped` values through their
//! `Display`, for engine readers. Every `match` here names every variant, so a new
//! kind of loss does not compile until it has words, and the two cannot drift.

use or_io::codec::mesh::GrcFvfChannel;
use or_io::gta5::retarget::{Dropped, Lost, Part, Place, Spot, Word};

/// The levels of detail a drawable holds, by their index.
const LEVELS: [&str; 4] = ["high", "medium", "low", "very low"];

/// `shader 0: the flags`, as a line of the conversion's question names the value dropped.
pub fn dropped(dropped: &Dropped) -> String {
    format!("{}: {}", place(&dropped.at), lost(&dropped.lost))
}

fn place(at: &Place) -> String {
    match &at.drawable {
        Some(part) => format!("{} \u{2014} {}", drawable(part), spot(&at.spot)),
        None => spot(&at.spot),
    }
}

fn drawable(part: &Part) -> String {
    match part {
        Part::Entry(hash) => format!("Drawable {hash:#010x}"),
        Part::Common => "The drawable".to_string(),
        Part::Cloth => "The cloth drawable".to_string(),
        Part::Extra(index) => format!("Extra drawable {index}"),
        Part::EnvironmentCloth(index) => format!("Environment cloth {index}"),
        Part::Child {
            level,
            child,
            damaged,
        } => {
            let damaged = if *damaged { ", damaged" } else { "" };
            format!("Child {child} of physics level {level}{damaged}")
        }
    }
}

fn spot(at: &Spot) -> String {
    match at {
        Spot::Shader(index) => format!("Shader {index}"),
        Spot::Geometry {
            lod,
            model,
            geometry,
        } => {
            let level = match lod {
                Some(lod) => (LEVELS.get(*lod))
                    .map_or_else(|| format!("level {lod}"), |level| format!("{level} detail")),
                None => "high detail, the copy kept apart".to_string(),
            };
            format!("Geometry {geometry} of model {model} at {level}")
        }
        Spot::Texture(name) => format!("Texture {name}"),
    }
}

fn lost(what: &Lost) -> String {
    match what {
        Lost::Entry { hash, texture } => {
            let kind = if *texture {
                "texture parameter"
            } else {
                "parameter"
            };
            format!("{kind} {hash:#010x}, not in the shader database for Enhanced")
        }
        Lost::TextureParameter(hash) => {
            format!("texture parameter {hash:#010x}, not in the shader database for Legacy")
        }
        Lost::Constant(hash) => {
            format!("parameter {hash:#010x}, not in the shader database for Legacy")
        }
        Lost::Uneven(hash, bytes) => {
            format!("parameter {hash:#010x}, whose {bytes} bytes are not a whole number of values")
        }
        Lost::Sampler(hash) => format!("sampler {hash:#010x}, which Legacy cannot hold"),
        Lost::Other(hash) => format!("parameter {hash:#010x}, of a kind Legacy cannot hold"),
        Lost::Slot(slot) => format!("texture slot {slot}, which no Legacy parameter names"),
        Lost::Channel(channel, size) => format!(
            "{} ({size:?}), which Enhanced has no slot for",
            channel_words(*channel)
        ),
        Lost::Element(slot, format) => {
            format!("vertex data in slot {slot} ({format:?}), which Legacy has no channel for")
        }
        Lost::Reference(name, word) => {
            format!(
                "{}, which its reference to texture {name} states",
                words(*word)
            )
        }
        Lost::Word(word) => words(*word).to_string(),
    }
}

fn channel_words(channel: GrcFvfChannel) -> String {
    use GrcFvfChannel::*;
    match channel {
        Position => "positions".to_string(),
        Weight => "bone weights".to_string(),
        Binding => "bone indices".to_string(),
        Normal => "normals".to_string(),
        Diffuse => "colour 0".to_string(),
        Specular => "colour 1".to_string(),
        Tex0 | Tex1 | Tex2 | Tex3 | Tex4 | Tex5 | Tex6 | Tex7 => {
            format!("texcoord {}", channel as u8 - Tex0 as u8)
        }
        Tangent0 => "tangent 0".to_string(),
        Tangent1 => "tangent 1".to_string(),
    }
}

fn words(word: Word) -> &'static str {
    match word {
        Word::VertexData => "a second copy of the vertices",
        Word::VertexBufferFlags => "the vertex buffer's flags",
        Word::LockPtr => "a copy of the vertices kept beside the buffer's",
        Word::FvfPastSixteen => "the vertex format bits past its sixteen channels",
        Word::FvfFlags => "the vertex format's flags",
        Word::DynamicOrder => "the vertex format's dynamic order",
        Word::ChannelSizes => "the encodings of the channels it does not carry",
        Word::IndexCountTopByte => "the top byte of the index count",
        Word::Registers => "the registers of its parameters",
        Word::MaterialHash => "the material hash",
        Word::BasisHashCode => "the basis hash code",
        Word::MaterialHashCode => "the material hash code",
        Word::PhysMtlDeprecated => "the deprecated physics material",
        Word::Flags => "the flags",
        Word::IsInstanced => "the instanced mark",
        Word::UserFlags => "the user flags",
        Word::SortKeyDeprecated => "the deprecated sort key",
        Word::Texture => "the console texture header",
        Word::ResourceTypeAndConversionFlags => "the resource type and conversion flags",
        Word::LayerCount => "the layer count",
        Word::PhysicalSizeAndTemplateType => "the physical size and template type",
        Word::HandleIndex => "the handle index",
        Word::ExtraFlags => "the extra flags",
        Word::TemplateType => "the template type",
        Word::InfoBitsPastSrgb => "the info bits other than sRGB",
        Word::StereoRtMode => "the stereo render target mode",
        Word::MipStride => "the stored row pitch",
        Word::PayloadPastMipChain => "the bytes stored after the last mip level",
        Word::VertexBindFlags => "the vertex buffer's bind flags",
        Word::VertexSrv => "the vertex buffer's shader resource view",
        Word::InfoData => "the extra words of the vertex declaration",
        Word::IndexSize => "the size of an index",
        Word::IndexBindFlags => "the index buffer's bind flags",
        Word::IndexSrv => "the index buffer's shader resource view",
        Word::Preset => "the shader preset",
        Word::Samplers => "the sampler states",
        Word::Others => "the other parameters",
        Word::BufferCopies => "the per-thread buffer copies",
        Word::Width => "the width",
        Word::Height => "the height",
        Word::Depth => "the depth",
        Word::Dimension => "the dimension",
        Word::Format => "the pixel format",
        Word::TileMode => "the tile mode",
        Word::AntiAliasType => "the anti-aliasing type",
        Word::MipLevels => "the number of mip levels",
        Word::UsageData => "the usage data",
        Word::AfterMipLevels => "the data after the mip levels",
    }
}

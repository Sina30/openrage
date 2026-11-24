//! `rpf scan-texture` on a GTA VI drawable or fragment: the texture its shader group embeds
//! replaced from an image, re-encoded or resized, or a texture embedded (`--add`) or unembedded
//! (`--remove`), written as a loose resource read back before it is kept.

use std::io::Cursor;

use or_io::authoring::{encode_texture_with_options, FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage};
use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Sections};
use or_io::gta6::hash::{name_hash32, name_hash64};
use or_io::gta6::headerless::{
    embed_texture, key_word, read_drawable_head, reshape_embedded_texture, unembed_texture,
    write_embedded_texture, EmbeddedTexture, HeldTexture, ModelTexture,
};

use super::texture::EncodeArgs;

/// The key a texture is named by: `hash_` and sixteen hex digits, else the hash of its name.
fn key_of(name: &str) -> Option<u64> {
    let name = name.trim();
    match name.get(..5).filter(|p| p.eq_ignore_ascii_case("hash_")) {
        Some(_) => u64::from_str_radix(&name[5..], 16).ok(),
        None => (!name.is_empty()).then(|| name_hash64(name.as_bytes())),
    }
}

type Found = Result<Option<EmbeddedTexture>, String>;

/// The texture of `key` the drawable `s` holds embeds, or of its fragment's where `fragment`;
/// `None` where it embeds none of that key.
fn embedded(s: &Rpf9Sections, fragment: bool, key: u64) -> Found {
    let head = read_drawable_head(&s.virtual_section, Some(&s.physical), fragment)
        .map_err(|e| e.to_string())?;
    Ok((head.textures.into_iter()).flatten().find(|t| t.key == key))
}

/// What `rpf scan-texture` asks of a drawable's embedded texture.
pub(crate) enum Change {
    Replace,
    Add,
    Remove,
}

/// The embedded texture `name` of the drawable `sections` hold, or of its fragment's where
/// `fragment`, edited as `change` says from `image`, as a loose resource read back, and what was
/// done.
pub(crate) fn edit(
    sections: &Rpf9Sections,
    fragment: bool,
    name: &str,
    image: Option<&RgbaImage>,
    encode: &EncodeArgs,
    change: Change,
) -> Result<(Vec<u8>, String), String> {
    let key = key_of(name).ok_or("name a texture, or hash_ and sixteen hex digits")?;
    let mut out = sections.clone();
    let (said, pixels) = match change {
        Change::Remove => {
            if embedded(sections, fragment, key)?.is_none() {
                return Err(not_embedded(sections, fragment, name));
            }
            unembed_texture(&mut out, fragment, key).map_err(|e| e.to_string())?;
            let said = format!(
                "embedded {name} removed: the shader slots keep its key, which a texture pack must now hold"
            );
            (said, None)
        }
        Change::Add => {
            if embedded(sections, fragment, key)?.is_some() {
                return Err(format!(
                    "{name} is already embedded; replace it from an image instead"
                ));
            }
            let image = image.ok_or("--add needs --image <file>")?;
            // A typed name states its own hash; a key alone takes the one a record of the body
            // states.
            let word = match name
                .trim()
                .get(..5)
                .filter(|p| p.eq_ignore_ascii_case("hash_"))
            {
                None => Some(name_hash32(name.trim().as_bytes())),
                Some(_) => key_word(&sections.virtual_section, key),
            };
            let word = word.ok_or_else(|| {
                format!(
                    "no name or bound entry states the 32-bit name hash of {name}: name it instead"
                )
            })?;
            let format = match encode.format {
                Some(format) => FormatChoice::Fixed(format),
                None => FormatChoice::Automatic,
            }
            .resolve(image);
            let options = encode.build_options(image.is_srgb);
            let (texture, pixels) = planned(key, format, image, &options)?;
            embed_texture(&mut out, fragment, key, word, &texture).map_err(|e| e.to_string())?;
            let said = format!(
                "{name} embedded: {}x{} {format:?}, {} mips",
                texture.width, texture.height, texture.mip_levels
            );
            (said, Some(pixels))
        }
        Change::Replace => {
            let found = embedded(sections, fragment, key)?;
            let found = found.ok_or_else(|| not_embedded(sections, fragment, name))?;
            let (said, pixels) = replaced(&mut out, fragment, (key, name), &found, image, encode)?;
            (said, Some(pixels))
        }
    };
    let saved = write_rpf9_resource(&out).map_err(|e| e.to_string())?;
    // The file is written only once it reads back as the edit made it.
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    let again = embedded(&back, fragment, key)?;
    let read = again.as_ref().and_then(|t| t.texture.texture.as_ref().ok());
    if read.map(|x| &x.pixels) != pixels.as_ref() {
        return Err(format!("{name}: the written texture reads back otherwise"));
    }
    Ok((saved, said))
}

/// Why `name` is not edited: the drawable embeds no texture of its key, and which it embeds.
fn not_embedded(sections: &Rpf9Sections, fragment: bool, name: &str) -> String {
    let head = read_drawable_head(
        &sections.virtual_section,
        Some(&sections.physical),
        fragment,
    );
    let held: Vec<String> = (head.iter().flat_map(|h| &h.textures))
        .flatten()
        .map(|t| format!("hash_{:016X}", t.key))
        .collect();
    format!(
        "the drawable embeds no texture named '{name}'. Embedded: {}",
        held.join(", ")
    )
}

/// `image` encoded as `format` with `options`, tiled as the embedded texture of `key`, and its
/// levels top first as a read gives them back.
fn planned(
    key: u64,
    format: GrcTextureFormat,
    image: &RgbaImage,
    options: &TextureBuildOptions,
) -> Result<(ModelTexture, Vec<u8>), String> {
    let side = |pixels: u32| {
        u16::try_from(pixels).map_err(|_| format!("a side of {pixels} does not fit a texture"))
    };
    let encoded = encode_texture_with_options(format, image.width, image.height, image, options)
        .map_err(|e| e.to_string())?;
    let held = HeldTexture {
        name: format!("hash_{key:016x}"),
        width: side(image.width)?,
        height: side(image.height)?,
        format,
        srgb: options.srgb,
        mips: encoded.mip_count,
        pixels: encoded.data,
    };
    let texture = held.tiled(None).map_err(|e| e.to_string())?;
    Ok((texture, held.pixels))
}

/// The embedded texture `found` of `key` replaced in `out` from `image`, or re-encoded from its own
/// pixels, in its own shape or the one `encode` asks; what was done and the levels written.
fn replaced(
    out: &mut Rpf9Sections,
    fragment: bool,
    (key, name): (u64, &str),
    found: &EmbeddedTexture,
    image: Option<&RgbaImage>,
    encode: &EncodeArgs,
) -> Result<(String, Vec<u8>), String> {
    let t = &found.texture;
    let read = (t.texture.as_ref()).map_err(|e| format!("{name} does not read: {e}"))?;
    if t.dimension != 1 || t.depth > 1 {
        return Err(format!(
            "{name} is no 2D texture of one slice, and none such is embedded, so how one is replaced is not known"
        ));
    }
    let decoded;
    let image = match image {
        Some(image) => image,
        None if encode.format.is_some() || encode.mips.is_some() => {
            decoded = decode_level(read, 0).map_err(|e| e.to_string())?;
            &decoded
        }
        None => {
            return Err(
                "an embedded texture is replaced from --image <file>, or re-encoded with --format or --mips"
                    .into(),
            )
        }
    };
    let (width, height, mips) = (u32::from(t.width), u32::from(t.height), read.levels);
    let format = read.format.format();
    let own = (image.width, image.height) == (width, height)
        && encode.format.is_none_or(|f| f == format)
        && encode.mips.is_none_or(|m| m == mips);
    if !own {
        // Another shape moves the physical section's objects, which a re-pack places anew.
        let format = encode.format.unwrap_or(format);
        let options = encode.build_options(read.format.is_srgb());
        let (texture, pixels) = planned(key, format, image, &options)?;
        reshape_embedded_texture(out, fragment, key, &texture).map_err(|e| e.to_string())?;
        let said = format!(
            "embedded {name}: {width}x{height}, {mips} mips -> {}x{} {format:?}, {} mips",
            texture.width, texture.height, texture.mip_levels
        );
        return Ok((said, pixels));
    }
    let options = TextureBuildOptions {
        mip_levels: Some(mips),
        allow_npot: true,
        ..encode.build_options(read.format.is_srgb())
    };
    let encoded = encode_texture_with_options(format, width, height, image, &options)
        .map_err(|e| e.to_string())?;
    if u32::from(encoded.mip_count) != mips || encoded.data.len() != read.pixels.len() {
        return Err(format!(
            "{name} encodes to other mips than it holds, which would move the sections"
        ));
    }
    let (v, p) = (&out.virtual_section, &mut out.physical);
    write_embedded_texture(v, p, fragment, key, &encoded.data).map_err(|e| e.to_string())?;
    let said = format!("embedded {name} replaced in its own size, format and mips");
    Ok((said, encoded.data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_texture_is_named_by_its_key_or_by_the_hash_of_its_name() {
        assert_eq!(key_of("hash_00000000000000FF"), Some(0xFF));
        assert_eq!(key_of(" Zq_Paint "), Some(name_hash64(b"zq_paint")));
        assert_eq!(key_of("hash_zz"), None);
        assert_eq!(key_of("  "), None);
    }
}

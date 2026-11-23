//! `rpf scan-texture` on a GTA VI drawable or fragment: the texture its shader group embeds
//! replaced from an image in its own size, format and mips, as the app replaces it, written as a
//! loose resource read back before it is kept.

use std::io::Cursor;

use or_io::authoring::{encode_texture_with_options, TextureBuildOptions};
use or_io::codec::texture::RgbaImage;
use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Sections};
use or_io::gta6::hash::name_hash64;
use or_io::gta6::headerless::{read_drawable_head, write_embedded_texture};

use super::texture::EncodeArgs;

/// The key a texture is named by: `hash_` and sixteen hex digits, else the hash of its name.
fn key_of(name: &str) -> Option<u64> {
    let name = name.trim();
    match name.get(..5).filter(|p| p.eq_ignore_ascii_case("hash_")) {
        Some(_) => u64::from_str_radix(&name[5..], 16).ok(),
        None => (!name.is_empty()).then(|| name_hash64(name.as_bytes())),
    }
}

/// The embedded texture `name` of the drawable `sections` hold, or of its fragment's where
/// `fragment`, encoded from `image`, as a loose resource read back, and what was done.
pub(crate) fn edit(
    sections: &Rpf9Sections,
    fragment: bool,
    name: &str,
    image: Option<&RgbaImage>,
    encode: &EncodeArgs,
) -> Result<(Vec<u8>, String), String> {
    let pixels_of = |s: &Rpf9Sections, key: u64| {
        let head = read_drawable_head(&s.virtual_section, Some(&s.physical), fragment)
            .map_err(|e| e.to_string())?;
        let held: Vec<String> = (head.textures.iter())
            .flatten()
            .map(|t| format!("hash_{:016X}", t.key))
            .collect();
        let found = (head.textures.into_iter()).flatten().find(|t| t.key == key);
        found.ok_or_else(|| {
            format!(
                "the drawable embeds no texture named '{name}'. Embedded: {}",
                held.join(", ")
            )
        })
    };
    let key = key_of(name).ok_or("name a texture, or hash_ and sixteen hex digits")?;
    let embedded = pixels_of(sections, key)?;
    let t = &embedded.texture;
    let read = (t.texture.as_ref()).map_err(|e| format!("{name} does not read: {e}"))?;
    if t.dimension != 1 || t.depth > 1 {
        return Err(format!(
            "{name} is no 2D texture of one slice, and none such is embedded, so how one is replaced is not known"
        ));
    }
    let image = image.ok_or("an embedded texture is replaced from --image <file>")?;
    let (width, height, mips) = (u32::from(t.width), u32::from(t.height), read.levels);
    let format = read.format.format();
    let other = (image.width, image.height) != (width, height)
        || encode.format.is_some_and(|f| f != format)
        || encode.mips.is_some_and(|m| m != mips);
    if other {
        return Err(format!(
            "{name} is embedded and keeps its {width}x{height} {format:?} with {mips} mips: another shape moves the physical section's objects, which this verb does not re-pack"
        ));
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
    let mut out = sections.clone();
    let (v, p) = (&out.virtual_section, &mut out.physical);
    write_embedded_texture(v, p, fragment, key, &encoded.data).map_err(|e| e.to_string())?;
    let saved = write_rpf9_resource(&out).map_err(|e| e.to_string())?;
    // The file is written only once its texture reads back as the one encoded.
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    let again = pixels_of(&back, key)?;
    if again.texture.texture.as_ref().ok().map(|x| &x.pixels) != Some(&encoded.data) {
        return Err(format!("{name}: the written texture reads back otherwise"));
    }
    Ok((
        saved,
        format!("embedded {name} replaced in its own size, format and mips"),
    ))
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

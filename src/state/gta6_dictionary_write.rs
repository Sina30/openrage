//! A scanned GTA VI texture dictionary's texture given another size, format or mips, or added or
//! removed, through GTA V's texture commands; its save plans both sections anew from the textures.

use or_io::authoring::{encode_texture_with_options, FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage, TextureSource};
use or_io::container::rpf::Rpf9Sections;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::shared::PgDictionary;
use or_io::gta6::headerless::{write_dictionary_textures, HeldTexture, TexturePlanError};
use or_io::gta6::slice_sources::slice_images;

use super::document::Document;
use super::gta6_edit::is_scanned_dictionary;
use super::texture_edit::TextureEdit;
use super::DocPayload;

/// A texture of a scanned dictionary's document, as its save writes it.
pub(super) type Held = HeldTexture;

fn held_of(texture: &GrcTexture) -> Held {
    HeldTexture {
        name: texture.name.clone(),
        width: texture.width,
        height: texture.height,
        format: texture.pixel_format(),
        srgb: texture.is_srgb(),
        mips: texture.mip_count,
        pixels: texture.backing_store.clone(),
    }
}

/// What a person reads of why a scanned dictionary's textures are not planned.
fn said(e: TexturePlanError) -> String {
    match e {
        TexturePlanError::Format { format: f } => tr!(
            "{format} is no format a GTA VI dictionary holds a texture in.",
            format = format!("{:?}", f)
        ),
        TexturePlanError::Unread { name } => tr!(
            "{name} does not read, so its pixels are not replaced.",
            name = name
        ),
        e => e.to_string(),
    }
}

/// Every texture of `dict`, as its save writes it.
// ponytail: every texture's pixels are copied for the worker; only the edited ones if a dictionary's size makes it tell.
pub(super) fn held(dict: &PgDictionary<GrcTexture>) -> Vec<Held> {
    dict.entries.iter().flatten().map(|t| held_of(t)).collect()
}

/// The texture `name` of the scanned dictionary `doc`, where it reads and is no volume, the slices it holds one after another, and the one the preview shows.
fn reshapeable<'a>(
    doc: &'a mut Document,
    name: &str,
) -> Result<(&'a mut GrcTexture, u32, u32), String> {
    let slices = match &doc.payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => dict.get(name),
        _ => None,
    };
    let slices = slices.map(|t| super::ttpf_document::scanned_slices(doc, t));
    let shown = super::ttpf_document::shown_scanned_slice(doc, name);
    let held = match &mut doc.payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => dict.get_mut(name),
        _ => None,
    };
    let (Some(texture), Some(slices)) = (held, slices) else {
        return Err(tr!("{name} is not in this dictionary", name = name));
    };
    if texture.image_type == super::ttpf_document::VOLUME_IMAGE {
        return Err(tr!(
            "{name} is a volume, so it keeps its size, format and mips: its levels lie across its z-slabs.",
            name = name
        ));
    }
    if texture.backing_store.is_empty() {
        return Err(tr!(
            "{name} does not read, so its pixels are not replaced.",
            name = name
        ));
    }
    Ok((texture, slices, shown))
}

/// One image for each of the `slices` `texture` holds: `image` for slice `shown`, every other its own top mip resampled to the image's size; with no image, each its own top mip.
fn sources_of(
    texture: &GrcTexture,
    (slices, shown): (u32, u32),
    image: Option<&RgbaImage>,
    options: &TextureBuildOptions,
) -> Result<Vec<RgbaImage>, String> {
    if let Some(image) = image.filter(|_| slices <= 1) {
        return Ok(vec![image.clone()]);
    }
    // One copy of the texture, which holds each slice's bytes in turn.
    let mut one = texture.clone();
    let each = texture.backing_store.len() / slices.max(1) as usize;
    let tops = (0..slices.max(1) as usize)
        .map(|s| {
            one.backing_store = texture.backing_store[each * s..each * (s + 1)].to_vec();
            decode_level(&one, 0).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let shown = shown.min(slices.saturating_sub(1)) as usize;
    slice_images(tops, image, shown, options.filter).map_err(|e| e.to_string())
}

/// `texture` encoded from `sources`, one image for each slice it holds, as `format` with `options`' mips, once a save could write it; the texture it was.
fn reshaped(
    texture: &mut GrcTexture,
    format: GrcTextureFormat,
    sources: &[RgbaImage],
    options: &TextureBuildOptions,
) -> Result<GrcTexture, String> {
    let before = texture.clone();
    let first = (sources.first()).ok_or_else(|| {
        let name = texture.name.clone();
        tr!(
            "{name} does not read, so its pixels are not replaced.",
            name = name
        )
    })?;
    let encoded = (texture.set_pixels(format, first, options))
        .map_err(|e| e.to_string())
        .and_then(|()| {
            // A reshape keeps the texture's own colour space and image type, as a pack's
            // reshape does.
            texture.info_bits = (texture.info_bits & !0x01) | (before.info_bits & 0x01);
            texture.image_type = before.image_type;
            if sources.len() > 1 {
                // The slices one after another, each its levels exactly, as the save reads them.
                let mut store = Vec::new();
                for source in sources {
                    let (w, h) = (source.width, source.height);
                    let one = encode_texture_with_options(format, w, h, source, options);
                    store.extend(one.map_err(|e| e.to_string())?.data);
                }
                texture.backing_store = store;
            }
            held_of(texture).tiled(None).map(|_| ()).map_err(said)
        });
    if let Err(why) = encoded {
        *texture = before;
        return Err(why);
    }
    Ok(before)
}

/// Texture `name` of the scanned dictionary `doc` re-encoded as `choice` with `options`' mips: the
/// texture it was and what changed; `None` for any other document.
pub(super) fn reencode(
    doc: &mut Document,
    name: &str,
    choice: FormatChoice,
    options: &TextureBuildOptions,
) -> Option<Result<(GrcTexture, TextureEdit), String>> {
    if !is_scanned_dictionary(doc) {
        return None;
    }
    Some(reshapeable(doc, name).and_then(|(texture, slices, shown)| {
        let sources = sources_of(texture, (slices, shown), None, options)?;
        let format = (sources.first()).map_or(texture.pixel_format(), |rgba| choice.resolve(rgba));
        let before = reshaped(texture, format, &sources, options)?;
        let mips = u32::from(texture.mip_count);
        let name = name.to_string();
        Ok((before, TextureEdit::Reencoded { name, format, mips }))
    }))
}

/// Texture `name` of the scanned dictionary `doc` from `image`: in its own shape where the image
/// has its size and `choice` and `options` keep its format and mips, else given the image's size,
/// that format, or its own for an automatic one, and those mips; `None` for any other document.
pub(super) fn import(
    doc: &mut Document,
    name: &str,
    image: &RgbaImage,
    choice: FormatChoice,
    options: &TextureBuildOptions,
) -> Option<Result<(GrcTexture, TextureEdit), String>> {
    if !is_scanned_dictionary(doc) {
        return None;
    }
    let own = match &doc.payload {
        DocPayload::Resource(or_io::Asset::TextureDict(dict)) => dict.get(name),
        _ => None,
    };
    let same = own.is_none_or(|t| {
        let size = (image.width, image.height) == (u32::from(t.width), u32::from(t.height));
        let format = matches!(choice, FormatChoice::Fixed(f) if f != t.pixel_format());
        let mips = (options.mip_levels).is_some_and(|m| m != u32::from(t.mip_count));
        size && !format && !mips
    });
    if same {
        return super::gta6_edit::replace_scanned_texture(doc, name, image, choice, options);
    }
    Some(reshapeable(doc, name).and_then(|(texture, slices, shown)| {
        let format = match choice {
            FormatChoice::Fixed(format) => format,
            FormatChoice::Automatic => texture.pixel_format(),
        };
        let sources = sources_of(texture, (slices, shown), Some(image), options)?;
        let before = reshaped(texture, format, &sources, options)?;
        let edit = TextureEdit::Imported {
            name: name.to_string(),
            format,
            mips: u32::from(texture.mip_count),
            replaced: true,
        };
        Ok((before, edit))
    }))
}

/// Whether a scanned GTA VI dictionary takes `texture` as a new one: a name it stores in lower
/// case, and a format and levels its save tiles, as a replace checks when it is made.
pub(super) fn can_add(texture: &GrcTexture) -> Result<(), String> {
    let name = &texture.name;
    if !name.is_ascii() || name.contains('\0') {
        return Err(tr!(
            "A GTA VI dictionary stores a texture's name in ASCII with no NUL, which {name} is not.",
            name = name
        ));
    }
    held_of(texture).tiled(None).map(drop).map_err(said)
}

/// `held` written over the scanned dictionary `sections` hold: in place where every texture keeps
/// its shape and none is added or removed, else both sections planned anew from the textures.
pub(super) fn write(sections: &mut Rpf9Sections, held: &[Held]) -> Result<(), String> {
    write_dictionary_textures(sections, held).map_err(said)
}

#[cfg(test)]
#[path = "gta6_dictionary_write_tests.rs"]
mod tests;

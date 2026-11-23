//! `rpf scan-texture`: a scanned or loose GTA VI texture dictionary's texture replaced from an
//! image, re-encoded, resized or added, as `texture import` and `texture reencode` edit a `.ytd`,
//! or a drawable's or fragment's embedded texture replaced, embedded or unembedded, written as a
//! loose resource that `rpf scan-open` reads at offset 0.

use std::fs;
use std::io::Cursor;

use or_io::authoring::texture_import;
use or_io::authoring::{encode_texture_with_options, FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage};
use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Role, Rpf9Sections,
};
use or_io::gta6::headerless::{read_dictionary, write_dictionary_textures, Entry, HeldTexture};

const USAGE: &str = "usage: rpf scan-texture <archive.rpf|file> <offset> <texture> <out> \
                     [--image <file>] [--add] [--remove] [--slice <n>] [--format <format>] \
                     [--mips <n>] [--filter box|lanczos3] [--quality fast|normal|high] \
                     [--srgb|--linear] [--allow-npot] [--limit <bytes>]";

/// A texture of the dictionary as its save writes it, the slices it holds, whether it is a
/// volume, and its pixels as read, which a re-encode decodes.
struct Held {
    texture: HeldTexture,
    slices: usize,
    volume: bool,
    read: Option<or_io::gta6::Texture>,
}

/// What one invocation asks of the texture it names.
struct Ask {
    image: Option<RgbaImage>,
    add: bool,
    slice: usize,
    encode: super::texture::EncodeArgs,
}

pub(crate) fn cmd_scan_texture(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let (image, args) = super::rpf::split_value(&args, "--image")?;
    let (slice, args) = super::rpf::split_value(&args, "--slice")?;
    let add = args.iter().any(|a| a == "--add");
    let remove = args.iter().any(|a| a == "--remove");
    let args: Vec<String> = (args.into_iter())
        .filter(|a| a != "--add" && a != "--remove")
        .collect();
    let (encode, words) = super::texture::parse_flags(&args)?;
    let [path, offset, name, out] = words.as_slice() else {
        return Err(USAGE.to_string());
    };
    let offset = super::rpf::parse_offset(offset)?;
    if super::navmesh_edit::same_file(path, out) {
        return Err(format!("the output is the input {path}: name another file"));
    }
    let slice = match slice {
        Some(word) => word
            .parse::<usize>()
            .map_err(|_| format!("--slice expects a number, got '{word}'"))?,
        None => 0,
    };
    let image = match image {
        Some(file) => Some(texture_import::load(&file).map_err(|e| format!("{file}: {e}"))?),
        None => None,
    };
    let ask = Ask {
        image,
        add,
        slice,
        encode,
    };
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    let sections = Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| format!("the body at {offset:#X} is not a resource"))?;
    let role = match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    };
    let fragment = match role {
        Some(Rpf9Role::Drawable) => Some(false),
        Some(Rpf9Role::DrawableWithBound) => Some(true),
        _ => None,
    };
    use super::gta6_embedded_texture::Change;
    let change = match (ask.add, remove) {
        (true, true) => return Err("--add and --remove ask for opposite edits: give one".into()),
        (true, false) => Change::Add,
        (false, true) => Change::Remove,
        (false, false) => Change::Replace,
    };
    let (saved, said) = match fragment {
        None if remove => {
            return Err("--remove takes a drawable's or a fragment's embedded texture".into())
        }
        Some(_) if ask.slice != 0 => {
            return Err(format!(
                "a drawable's embedded texture is one 2D slice, so it has no slice {}",
                ask.slice
            ))
        }
        Some(fragment) => super::gta6_embedded_texture::edit(
            &sections,
            fragment,
            name,
            ask.image.as_ref(),
            &ask.encode,
            change,
        )?,
        None => edit(&sections, name, &ask)?,
    };
    fs::write(out, &saved).map_err(|e| format!("write({out}): {e}"))?;
    println!("{path} at {offset:#X} -> {out} ({} bytes)", saved.len());
    println!("  {said}");
    println!("This checks the body's bytes. It does not test that GTA VI loads them.");
    Ok(())
}

/// Every texture of the dictionary `sections` hold, as a save writes it.
fn textures(sections: &Rpf9Sections) -> Result<Vec<Held>, String> {
    let read = read_dictionary(&sections.virtual_section, Some(&sections.physical))
        .map_err(|e| e.to_string())?;
    let mut held = Vec::new();
    for (_, entry) in read.entries {
        match entry {
            Ok(Entry::Texture(t)) => {
                let read = t.texture.ok();
                let format = read.as_ref().map(|x| x.format);
                let texture = HeldTexture {
                    name: t.name,
                    width: t.width,
                    height: t.height,
                    format: format.map(|f| f.format()).unwrap_or_default(),
                    srgb: format.is_some_and(|f| f.is_srgb()),
                    mips: read.as_ref().map_or(t.mip_levels, |x| x.levels as u8),
                    pixels: read.as_ref().map(|x| x.pixels.clone()).unwrap_or_default(),
                };
                held.push(Held {
                    texture,
                    slices: read.as_ref().map_or(1, |x| x.faces.max(1) as usize),
                    volume: t.dimension == 3,
                    read,
                });
            }
            Ok(_) => return Err("this dictionary holds entries that are no textures".into()),
            // An entry that does not read is kept as the body holds it.
            Err(_) => {}
        }
    }
    if held.is_empty() {
        return Err("this body is no texture dictionary".into());
    }
    Ok(held)
}

/// `image` encoded as `format` with `options`, its levels top first, and how many it holds.
fn encoded(
    format: GrcTextureFormat,
    image: &RgbaImage,
    options: &TextureBuildOptions,
) -> Result<(Vec<u8>, u8), String> {
    let e = encode_texture_with_options(format, image.width, image.height, image, options)
        .map_err(|e| e.to_string())?;
    Ok((e.data, e.mip_count))
}

/// A side of an image as a texture's 16-bit side.
fn side(pixels: u32) -> Result<u16, String> {
    u16::try_from(pixels).map_err(|_| format!("a side of {pixels} does not fit a texture"))
}

/// The dictionary `sections` hold with the texture `name` edited as `ask` says, as a loose
/// resource read back, and what was done; a refusal writes nothing.
fn edit(sections: &Rpf9Sections, name: &str, ask: &Ask) -> Result<(Vec<u8>, String), String> {
    let mut held = textures(sections)?;
    let at = (held.iter()).position(|h| h.texture.name.eq_ignore_ascii_case(name));
    let said = match (at, ask.add) {
        (Some(_), true) => {
            return Err(format!(
                "{name} is already in this dictionary; replace it from an image instead"
            ))
        }
        (None, false) => {
            let names: Vec<&str> = held.iter().map(|h| h.texture.name.as_str()).collect();
            return Err(format!(
                "no texture named '{name}'. Held: {}",
                names.join(", ")
            ));
        }
        (None, true) => {
            let texture = added(name, ask)?;
            let said = format!(
                "{} added: {}x{} {:?}, {} mips",
                texture.name, texture.width, texture.height, texture.format, texture.mips
            );
            held.push(Held {
                texture,
                slices: 1,
                volume: false,
                read: None,
            });
            said
        }
        (Some(at), false) => edited(&mut held[at], ask)?,
    };
    let textures: Vec<HeldTexture> = held.iter().map(|h| h.texture.clone()).collect();
    let mut out = sections.clone();
    write_dictionary_textures(&mut out, &textures).map_err(|e| e.to_string())?;
    let saved = write_rpf9_resource(&out).map_err(|e| e.to_string())?;
    // The file is written only once its textures read back as the ones the edit made.
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    let read = self::textures(&back)?;
    for h in textures.iter().filter(|h| !h.pixels.is_empty()) {
        let again = read
            .iter()
            .find(|r| r.texture.name.eq_ignore_ascii_case(&h.name));
        if again.is_none_or(|r| r.texture.pixels != h.pixels) {
            return Err(format!(
                "{}: the written texture reads back otherwise",
                h.name
            ));
        }
    }
    Ok((saved, said))
}

/// A new texture `name` from the image `ask` gives.
fn added(name: &str, ask: &Ask) -> Result<HeldTexture, String> {
    if name.trim().is_empty() || !name.is_ascii() || name.contains('\0') {
        return Err(format!(
            "a GTA VI dictionary stores a texture's name in ASCII with no NUL, which {name:?} is not"
        ));
    }
    let image = ask.image.as_ref().ok_or("--add needs --image <file>")?;
    let format = match ask.encode.format {
        Some(format) => FormatChoice::Fixed(format),
        None => FormatChoice::Automatic,
    }
    .resolve(image);
    let options = ask.encode.build_options(image.is_srgb);
    let (pixels, mips) = encoded(format, image, &options)?;
    Ok(HeldTexture {
        name: name.to_ascii_lowercase(),
        width: side(image.width)?,
        height: side(image.height)?,
        format,
        srgb: options.srgb,
        mips,
        pixels,
    })
}

/// The texture `h` replaced, re-encoded or resized as `ask` says, and what was done.
fn edited(h: &mut Held, ask: &Ask) -> Result<String, String> {
    let (format, mips) = (ask.encode.format, ask.encode.mips);
    let decoded;
    let image = match (&ask.image, &h.read) {
        (Some(image), _) => image,
        _ if format.is_none() && mips.is_none() => {
            return Err("without --image, a re-encode needs --format or --mips".into())
        }
        (None, Some(read)) => {
            decoded = decode_level(read, 0).map_err(|e| e.to_string())?;
            &decoded
        }
        (None, None) => {
            return Err(format!(
                "{} does not read, so it is not re-encoded",
                h.texture.name
            ))
        }
    };
    let t = &mut h.texture;
    if t.pixels.is_empty() {
        return Err(format!(
            "{} does not read, so its pixels are not replaced",
            t.name
        ));
    }
    let own = (t.width, t.height, t.format, t.mips);
    let same = (image.width, image.height) == (u32::from(t.width), u32::from(t.height))
        && format.is_none_or(|f| f == t.format)
        && mips.is_none_or(|m| m == u32::from(t.mips));
    if same && ask.image.is_some() {
        // One slice of a cube, an array or a volume is replaced alone, in its own shape.
        let slice = ask.slice;
        if slice >= h.slices {
            return Err(format!(
                "{} holds {} slices, so there is no slice {slice}",
                t.name, h.slices
            ));
        }
        let options = TextureBuildOptions {
            mip_levels: Some(u32::from(t.mips)),
            allow_npot: true,
            ..ask.encode.build_options(t.srgb)
        };
        let (pixels, mips) = encoded(t.format, image, &options)?;
        let each = t.pixels.len() / h.slices;
        if mips != t.mips || pixels.len() != each {
            return Err(format!(
                "{} encodes to other mips than it holds, which would move the sections",
                t.name
            ));
        }
        t.pixels[slice * each..(slice + 1) * each].copy_from_slice(&pixels);
        return Ok(match h.slices {
            1 => format!("{} replaced in its own size, format and mips", t.name),
            n => format!(
                "slice {slice} of {n} of {} replaced in its own size, format and mips",
                t.name
            ),
        });
    }
    if h.slices > 1 || h.volume {
        return Err(format!(
            "{} holds several slices, so it keeps its size, format and mips: only a 2D texture of one slice takes others",
            t.name
        ));
    }
    // A reshape keeps the texture's own colour space.
    let options = ask.encode.build_options(t.srgb);
    let format = format.unwrap_or(t.format);
    let (pixels, mips) = encoded(format, image, &options)?;
    (t.width, t.height) = (side(image.width)?, side(image.height)?);
    (t.format, t.mips, t.pixels) = (format, mips, pixels);
    Ok(format!(
        "{}: {}x{} {:?}, {} mips -> {}x{} {:?}, {} mips",
        t.name, own.0, own.1, own.2, own.3, t.width, t.height, t.format, t.mips
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::{write_dictionary, DictionaryModel, ModelTexture};

    /// A dictionary of one 64x64 BC1 texture of one level, "zq_paint", its blocks a pattern.
    fn paint() -> Rpf9Sections {
        let blocks: Vec<u8> = (0..16 * 16 * 8)
            .map(|i: usize| (i * 13 + i / 7) as u8)
            .collect();
        let paint = ModelTexture::tiled("zq_paint", 64, 64, 71, &[blocks]).expect("it tiles");
        let model = DictionaryModel {
            textures: vec![paint],
            capacity: 1,
        };
        write_dictionary(&model).expect("planned")
    }

    fn image(side: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage {
            width: side,
            height: side,
            data: colour.repeat((side * side) as usize),
            is_srgb: false,
        }
    }

    fn ask(image: Option<RgbaImage>, add: bool, words: &[&str]) -> Ask {
        let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        let (encode, _) = super::super::texture::parse_flags(&words).expect("flags");
        Ask {
            image,
            add,
            slice: 0,
            encode,
        }
    }

    /// A texture read back: its name, width, height, mips and pixels.
    type Shown = (String, u16, u16, u8, Vec<u8>);

    /// The textures the loose resource `saved` holds, by name.
    fn held_in(saved: &[u8]) -> Vec<Shown> {
        let (find, bytes) = read_rpf9_body(Cursor::new(saved), 0, u64::MAX).expect("it reads");
        let back = Rpf9Sections::of(&find, &bytes).expect("a resource");
        let mut held: Vec<_> = (textures(&back).expect("textures").into_iter())
            .map(|h| {
                let t = h.texture;
                (t.name, t.width, t.height, t.mips, t.pixels)
            })
            .collect();
        held.sort();
        held
    }

    #[test]
    fn an_image_of_its_own_size_replaces_a_texture_in_place() {
        let sections = paint();
        let (saved, said) = edit(
            &sections,
            "ZQ_PAINT",
            &ask(Some(image(64, [200, 10, 10, 255])), false, &[]),
        )
        .expect("replaced");
        assert!(said.contains("in its own size"), "{said}");
        let held = held_in(&saved);
        assert_eq!((held[0].1, held[0].2, held[0].3), (64, 64, 1));
        let was = held_in(&write_rpf9_resource(&sections).expect("written"));
        assert_ne!(held[0].4, was[0].4, "the pixels changed");
    }

    #[test]
    fn a_resize_a_re_encode_and_an_added_texture_plan_the_dictionary_anew() {
        let sections = paint();
        let resized = ask(Some(image(32, [0, 0, 255, 255])), false, &["--mips", "2"]);
        let (saved, said) = edit(&sections, "zq_paint", &resized).expect("resized");
        assert!(said.contains("-> 32x32"), "{said}");
        let held = held_in(&saved);
        assert_eq!((held[0].1, held[0].2, held[0].3), (32, 32, 2));

        let more = ask(None, false, &["--mips", "3"]);
        let (saved, _) = edit(&sections, "zq_paint", &more).expect("re-encoded");
        assert_eq!(held_in(&saved)[0].3, 3);

        let added = ask(Some(image(16, [9, 9, 9, 255])), true, &["--format", "DXT1"]);
        let (saved, said) = edit(&sections, "ZQ_New", &added).expect("added");
        assert!(said.starts_with("zq_new added: 16x16"), "{said}");
        let names: Vec<String> = held_in(&saved).into_iter().map(|t| t.0).collect();
        assert_eq!(names, ["zq_new", "zq_paint"]);
    }

    #[test]
    fn a_name_held_twice_one_not_held_and_a_re_encode_of_nothing_are_refused() {
        let sections = paint();
        let twice = ask(Some(image(16, [1, 2, 3, 255])), true, &[]);
        let why = edit(&sections, "zq_paint", &twice).expect_err("refused");
        assert!(why.contains("already in this dictionary"), "{why}");
        let why =
            edit(&sections, "zq_none", &ask(None, false, &["--mips", "1"])).expect_err("refused");
        assert!(why.contains("Held: zq_paint"), "{why}");
        let why = edit(&sections, "zq_paint", &ask(None, false, &[])).expect_err("refused");
        assert!(why.contains("--format or --mips"), "{why}");
    }
}

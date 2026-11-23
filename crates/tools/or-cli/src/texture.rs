use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tracing::error;

use or_io::authoring::texture_import::{self, SourceFormat};
use or_io::authoring::{rgba_psnr, EncodeQuality, TextureBuildOptions};
use or_io::codec::texture::{
    decode_level, save_png, to_dds, GrcTextureFormat, MipFilter, RgbaImage, TextureSource,
};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::GrcTextureG9;
use or_io::gta5::{File as _, PgDictionary};

#[derive(Debug, Default)]
pub(crate) struct EncodeArgs {
    pub(crate) format: Option<GrcTextureFormat>,
    pub(crate) mips: Option<u32>,
    filter: MipFilter,
    quality: EncodeQuality,
    srgb: Option<bool>,
    allow_npot: bool,
    pub(crate) output: Option<PathBuf>,
}

impl EncodeArgs {
    pub(crate) fn build_options(&self, srgb_default: bool) -> TextureBuildOptions {
        TextureBuildOptions {
            mip_levels: self.mips,
            filter: self.filter,
            srgb: self.srgb.unwrap_or(srgb_default),
            quality: self.quality,
            allow_npot: self.allow_npot,
            ..TextureBuildOptions::default()
        }
    }
}

pub(crate) fn parse_flags(args: &[String]) -> Result<(EncodeArgs, Vec<String>), String> {
    let mut parsed = EncodeArgs::default();
    let mut positional = Vec::new();
    let mut i = 0;

    fn value<'a>(args: &'a [String], i: usize, flag: &str) -> Result<&'a str, String> {
        args.get(i + 1)
            .map(String::as_str)
            .ok_or_else(|| format!("{flag} needs a value"))
    }

    while i < args.len() {
        match args[i].as_str() {
            "--format" | "-f" => {
                let raw = value(args, i, "--format")?;
                parsed.format = Some(GrcTextureFormat::from_name(raw).ok_or_else(|| {
                    format!("unknown texture format '{raw}'; try DXT1, DXT5, DXT5A, DXN or BC7")
                })?);
                i += 2;
            }
            "--mips" | "-m" => {
                let raw = value(args, i, "--mips")?;
                parsed.mips = Some(
                    raw.parse::<u32>()
                        .map_err(|_| format!("--mips expects a number, got '{raw}'"))?,
                );
                i += 2;
            }
            "--filter" => {
                let raw = value(args, i, "--filter")?;
                parsed.filter = MipFilter::parse(raw)
                    .ok_or_else(|| format!("unknown filter '{raw}'; try box or lanczos3"))?;
                i += 2;
            }
            "--quality" | "-q" => {
                let raw = value(args, i, "--quality")?;
                parsed.quality = EncodeQuality::parse(raw)
                    .ok_or_else(|| format!("unknown quality '{raw}'; try fast, normal or high"))?;
                i += 2;
            }
            "--srgb" => {
                parsed.srgb = Some(true);
                i += 1;
            }
            "--linear" => {
                parsed.srgb = Some(false);
                i += 1;
            }
            "--allow-npot" => {
                parsed.allow_npot = true;
                i += 1;
            }
            "-o" | "--output" => {
                parsed.output = Some(PathBuf::from(value(args, i, "--output")?));
                i += 2;
            }
            // ⚠ The second word after the file is a texture name (import) or an output path: a name that starts with '-' (a pack's "-Name- Skirt") is not an option
            other if other.starts_with('-') && positional.len() != 1 => {
                return Err(format!("unknown option '{other}'"));
            }
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }

    Ok((parsed, positional))
}

fn read_ytd(path: &Path) -> Result<PgDictionary<GrcTexture>, String> {
    let data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    PgDictionary::<GrcTexture>::from_bytes(&data)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

/// A `.ytd` at the layout its RSC7 version names.
pub(crate) enum Listing {
    Legacy(PgDictionary<GrcTexture>),
    Enhanced(PgDictionary<GrcTextureG9>),
}

/// A texture as the reports list it, from either target.
pub(crate) struct Listed<'a> {
    pub name: &'a str,
    pub width: u16,
    pub height: u16,
    pub mip_count: u8,
    pub cut_mip_levels: u8,
    pub pixels: &'a dyn TextureSource,
}

impl Listing {
    fn read(path: &Path) -> Result<Self, String> {
        let data =
            std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_bytes(path, &data)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }

    pub(crate) fn from_bytes(path: &Path, data: &[u8]) -> Result<Self, or_io::Error> {
        if crate::target_of(path, data).is_gen9() {
            PgDictionary::<GrcTextureG9>::from_bytes(data).map(Self::Enhanced)
        } else {
            PgDictionary::<GrcTexture>::from_bytes(data).map(Self::Legacy)
        }
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, or_io::Error> {
        match self {
            Self::Legacy(dict) => dict.to_bytes(),
            Self::Enhanced(dict) => dict.to_bytes(),
        }
    }

    pub(crate) fn textures(&self) -> Vec<Listed<'_>> {
        match self {
            Self::Legacy(dict) => dict
                .entries
                .iter()
                .flatten()
                .map(|t| Listed {
                    name: &t.name,
                    width: t.width,
                    height: t.height,
                    mip_count: t.mip_count,
                    cut_mip_levels: t.cut_mip_levels,
                    pixels: &**t,
                })
                .collect(),
            Self::Enhanced(dict) => dict
                .entries
                .iter()
                .flatten()
                .map(|t| Listed {
                    name: &t.name,
                    width: t.width,
                    height: t.height,
                    mip_count: t.mip_levels,
                    cut_mip_levels: 0,
                    pixels: &**t,
                })
                .collect(),
        }
    }
}

fn write_ytd(path: &Path, dict: &PgDictionary<GrcTexture>) -> Result<(), String> {
    let bytes = dict
        .to_bytes()
        .map_err(|e| format!("failed to encode YTD: {e}"))?;
    std::fs::write(path, &bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    println!("Wrote {} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

/// The first level of each texture of the dictionary `bytes`, called `name`, as a PNG in `out_dir`: what `texture export` writes with no option, for a caller that holds the bytes. Returns the bytes it wrote; a texture that does not decode is left out, as the command leaves it.
pub fn export_pngs(name: &str, bytes: &[u8], out_dir: &Path) -> Result<u64, String> {
    let listing = Listing::from_bytes(Path::new(name), bytes)
        .map_err(|e| format!("failed to parse {name}: {e}"))?;
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;
    let mut written = 0;
    for texture in listing.textures() {
        let Ok(image) = decode_level(texture.pixels, 0) else {
            continue;
        };
        let path = out_dir.join(format!("{}.png", sanitize(texture.name)));
        save_png(&path, &image).map_err(|e| format!("{}: {e}", path.display()))?;
        written += std::fs::metadata(&path).map_or(0, |m| m.len());
    }
    if written == 0 {
        return Err(format!("{name} holds no texture that decodes"));
    }
    Ok(written)
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

fn read_image(path: &Path) -> Result<RgbaImage, String> {
    texture_import::load(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn cmd_export(args: &[String]) -> Result<(), String> {
    let mut as_dds = false;
    let mut all_mips = false;
    let mut only: Option<String> = None;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" | "-f" => {
                let raw = args
                    .get(i + 1)
                    .map(String::as_str)
                    .ok_or("--format needs a value")?;
                as_dds = match raw.to_ascii_lowercase().as_str() {
                    "dds" => true,
                    "png" => false,
                    other => return Err(format!("--format expects png or dds, got '{other}'")),
                };
                i += 2;
            }
            "--name" | "-n" => {
                only = Some(args.get(i + 1).cloned().ok_or("--name needs a value")?);
                i += 2;
            }
            "--all-mips" => {
                all_mips = true;
                i += 1;
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }

    let (input, out_dir) = match positional.as_slice() {
        [input, out_dir] => (PathBuf::from(input), PathBuf::from(out_dir)),
        _ => return Err("usage: texture export <file.ytd> <outdir> [options]".into()),
    };

    let listing = Listing::read(&input)?;
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;

    let mut written = 0usize;
    for texture in listing.textures() {
        if let Some(wanted) = &only {
            if texture.name != wanted {
                continue;
            }
        }

        if as_dds {
            let path = out_dir.join(format!("{}.dds", sanitize(texture.name)));
            to_dds(texture.pixels)
                .and_then(|dds| dds.save(&path))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!(
                "{}  ({:?}, {} mips)",
                path.display(),
                texture.pixels.pixel_format(),
                texture.pixels.stored_levels()
            );
            written += 1;
            continue;
        }

        let levels = if all_mips {
            texture.pixels.stored_levels()
        } else {
            1
        };
        for level in 0..levels {
            let image = match decode_level(texture.pixels, level) {
                Ok(image) => image,
                Err(e) => {
                    error!(texture = %texture.name, level, "decode failed: {e}");
                    continue;
                }
            };
            let stem = sanitize(texture.name);
            let path = if all_mips {
                out_dir.join(format!("{stem}.mip{level}.png"))
            } else {
                out_dir.join(format!("{stem}.png"))
            };
            save_png(&path, &image).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("{}  ({}x{})", path.display(), image.width, image.height);
            written += 1;
        }
    }

    if written == 0 {
        return Err(match only {
            Some(name) => format!("no texture named '{name}' in {}", input.display()),
            None => format!("{} contains no textures", input.display()),
        });
    }
    println!("\nExported {written} file(s) to {}", out_dir.display());
    Ok(())
}

fn cmd_import(args: &[String]) -> Result<(), String> {
    let (opts, positional) = parse_flags(args)?;
    let (input, name, image_path) = match positional.as_slice() {
        [input, name, image] => (PathBuf::from(input), name.clone(), PathBuf::from(image)),
        _ => {
            return Err(
                "usage: texture import <file.ytd> <texture-name> <image> -o <out.ytd>".into(),
            )
        }
    };
    let output = opts
        .output
        .clone()
        .ok_or("texture import needs -o <out.ytd>")?;

    let mut dict = read_ytd(&input)?;

    let available: Vec<String> = dict
        .entries
        .iter()
        .flatten()
        .map(|t| t.name.clone())
        .collect();
    // ⚠ `#N` names the Nth texture: a name that is not valid text (a pack's Latin-1 byte in it) cannot be typed on a command line and read back equal
    let by_index = name.strip_prefix('#').and_then(|n| n.parse::<usize>().ok());
    let Some(existing) = dict
        .entries
        .iter_mut()
        .flatten()
        .enumerate()
        .find(|(i, t)| by_index.map_or(t.name == name, |n| n == *i))
        .map(|(_, t)| t)
    else {
        return Err(format!(
            "no texture named '{name}' in {}. Available: {}",
            input.display(),
            available.join(", ")
        ));
    };
    let previous_format = existing.pixel_format();
    let previous_srgb = existing.is_srgb();

    let source = read_image(&image_path)?;
    let format = opts.format.unwrap_or(previous_format);
    let build = opts.build_options(if opts.format.is_some() {
        source.is_srgb
    } else {
        previous_srgb
    });

    existing
        .set_pixels(format, &source, &build)
        .map_err(|e| format!("failed to encode '{name}': {e}"))?;

    println!(
        "{name}: {:?} -> {:?}, {}x{}, {} mips, {} bytes",
        previous_format,
        format,
        existing.width,
        existing.height,
        existing.mip_count,
        existing.backing_store.len()
    );

    write_ytd(&output, &dict)
}

fn cmd_reencode(args: &[String]) -> Result<(), String> {
    let (opts, positional) = parse_flags(args)?;
    let (input, output) = match (positional.as_slice(), &opts.output) {
        ([input, output], _) => (PathBuf::from(input), PathBuf::from(output)),
        ([input], Some(output)) => (PathBuf::from(input), output.clone()),
        _ => return Err("usage: texture reencode <in.ytd> <out.ytd> --format DXT5".into()),
    };
    let format = opts
        .format
        .ok_or("texture reencode needs --format <DXT1|DXT5|DXT5A|DXN|BC7|...>")?;

    let mut dict = read_ytd(&input)?;
    let mut converted = 0usize;
    let mut skipped = 0usize;

    for texture in dict.entries.iter_mut().flatten() {
        let before = texture.pixel_format();
        let before_len = texture.backing_store.len();
        let build = opts.build_options(texture.is_srgb());
        let reencoded =
            decode_level(&**texture, 0).and_then(|rgba| texture.set_pixels(format, &rgba, &build));
        match reencoded {
            Ok(()) => {
                println!(
                    "  {:<40} {:?} -> {:?}  {} -> {} bytes",
                    texture.name,
                    before,
                    format,
                    before_len,
                    texture.backing_store.len()
                );
                converted += 1;
            }
            Err(e) => {
                error!(texture = %texture.name, "re-encode failed: {e}");
                skipped += 1;
            }
        }
    }

    if converted == 0 {
        return Err(format!(
            "no texture in {} could be re-encoded",
            input.display()
        ));
    }
    println!("\nRe-encoded {converted} texture(s), skipped {skipped}");
    write_ytd(&output, &dict)
}

fn cmd_psnr(args: &[String]) -> Result<(), String> {
    let (a, b) = match args {
        [a, b] => (PathBuf::from(a), PathBuf::from(b)),
        _ => return Err("usage: texture psnr <a> <b>".into()),
    };

    let left = read_image(&a)?;
    let right = read_image(&b)?;
    let report = rgba_psnr(&left, &right).map_err(|e| e.to_string())?;

    let show = |v: f64| {
        if v.is_infinite() {
            "identical".to_string()
        } else {
            format!("{v:.2} dB")
        }
    };

    println!("{} vs {}", a.display(), b.display());
    println!("  size    {}x{}", left.width, left.height);
    println!("  overall {}", show(report.overall));
    for (label, value) in ["R", "G", "B", "A"].iter().zip(report.per_channel) {
        println!("  {label}       {}", show(value));
    }
    Ok(())
}

fn cmd_info(args: &[String]) -> Result<(), String> {
    let path = match args {
        [path] => PathBuf::from(path),
        _ => return Err("usage: texture info <file.ytd>".into()),
    };

    let listing = Listing::read(&path)?;
    let textures = listing.textures();
    println!("{}: {} texture(s)\n", path.display(), textures.len());
    println!(
        "{:<40} {:>9} {:<10} {:>4} {:>4} {:>6} {:>10}",
        "NAME", "SIZE", "FORMAT", "MIP", "CUT", "SRGB", "BYTES"
    );
    for texture in &textures {
        println!(
            "{:<40} {:>4}x{:<4} {:<10} {:>4} {:>4} {:>6} {:>10}",
            texture.name,
            texture.width,
            texture.height,
            format!("{:?}", texture.pixels.pixel_format()),
            texture.mip_count,
            texture.cut_mip_levels,
            if texture.pixels.is_srgb() {
                "yes"
            } else {
                "no"
            },
            texture.pixels.payload().len()
        );
    }
    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("Texture authoring: encode, decode and re-encode GTA V textures");
    eprintln!();
    eprintln!("Usage: {program} <subcommand> [args...]");
    eprintln!();
    eprintln!("Subcommands:");
    eprintln!("  info <file.ytd>");
    eprintln!("        List textures with format, mip and sRGB details");
    eprintln!("  export <file.ytd> <outdir> [--format png|dds] [--all-mips] [--name <texture>]");
    eprintln!("        Write textures out. DDS preserves the compressed payload exactly;");
    eprintln!("        PNG decodes to RGBA8 and carries one level per file.");
    eprintln!("  import <file.ytd> <texture-name> <image> -o <out.ytd> [encode options]");
    eprintln!(
        "        Replace one texture and write a new dictionary. Accepts {}.",
        SourceFormat::ALL.map(|f| f.label()).join(", ")
    );
    eprintln!("  reencode <in.ytd> <out.ytd> --format <fmt> [encode options]");
    eprintln!("        Re-encode every texture in a dictionary");
    eprintln!("  psnr <a> <b>");
    eprintln!("        Report peak signal-to-noise ratio, overall and per channel");
    eprintln!("  census <dir> | --game <install-root> [--json] [-v]");
    eprintln!("        Tally the pixel formats present and flag any we cannot decode");
    eprintln!();
    eprintln!("Encode options:");
    eprintln!("  --format, -f <fmt>   DXT1/BC1, DXT3/BC2, DXT5/BC3, DXT5A/BC4, DXN/BC5, BC7,");
    eprintln!("                       A8R8G8B8, A8B8G8R8, L8, A8");
    eprintln!("  --mips, -m <n>       Mip levels to generate (default: full chain)");
    eprintln!("  --filter <f>         box (default) or lanczos3");
    eprintln!("  --quality, -q <q>    fast, normal (default) or high");
    eprintln!("  --srgb / --linear    Force the colour space instead of inferring it.");
    eprintln!("                       Use --linear for normal maps and packed masks.");
    eprintln!("  --allow-npot         Permit non-power-of-two dimensions");
    eprintln!();
    eprintln!("Notes:");
    eprintln!("  BC7 encoding requires or-io's `bc7-encode` feature.");
    eprintln!("  Mips are resampled in linear space with premultiplied alpha.");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {program} export vehicle.ytd out/ --format dds");
    eprintln!("  {program} import vehicle.ytd body_diff body.png -o new.ytd -f DXT5");
    eprintln!("  {program} import bumper.ytd bumper_n normal.png -o new.ytd -f DXN --linear");
    eprintln!("  {program} reencode big.ytd small.ytd --format DXT1 --quality high");
}

pub fn run(args: &[String]) -> ExitCode {
    let program = "openrage-cli texture";

    let Some(command) = args.first() else {
        print_usage(program);
        return ExitCode::from(2);
    };
    let rest = &args[1..];

    if command == "census" {
        return match super::texture_census::run(rest) {
            Ok(code) => code,
            Err(message) => {
                error!("{message}");
                ExitCode::FAILURE
            }
        };
    }

    let result = match command.as_str() {
        "info" => cmd_info(rest),
        "export" => cmd_export(rest),
        "import" => cmd_import(rest),
        "reencode" => cmd_reencode(rest),
        "psnr" => cmd_psnr(rest),
        "-h" | "--help" | "help" => {
            print_usage(program);
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("Error: unknown subcommand '{other}'");
            eprintln!();
            print_usage(program);
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            error!("{message}");
            eprintln!("Error: {message}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_and_positionals_are_separated() {
        let (opts, positional) = parse_flags(&args(&[
            "in.ytd", "-f", "DXT5", "extra", "--mips", "4", "--filter", "lanczos3", "--linear",
            "-o", "out.ytd",
        ]))
        .unwrap();

        assert_eq!(positional, vec!["in.ytd", "extra"]);
        assert_eq!(opts.format, Some(GrcTextureFormat::DXT5));
        assert_eq!(opts.mips, Some(4));
        assert_eq!(opts.filter, MipFilter::Lanczos3);
        assert_eq!(opts.srgb, Some(false));
        assert_eq!(opts.output, Some(PathBuf::from("out.ytd")));
    }

    #[test]
    fn unknown_values_are_reported_helpfully() {
        let err = parse_flags(&args(&["-f", "DXT9"])).unwrap_err();
        assert!(err.contains("DXT9"), "should name the bad value: {err}");

        let err = parse_flags(&args(&["--filter", "bicubic"])).unwrap_err();
        assert!(err.contains("lanczos3"), "should list valid filters: {err}");

        let err = parse_flags(&args(&["--nope"])).unwrap_err();
        assert!(err.contains("--nope"));

        let err = parse_flags(&args(&["--format"])).unwrap_err();
        assert!(err.contains("needs a value"));
    }

    #[test]
    fn srgb_defaults_to_the_source_unless_overridden() {
        let (opts, _) = parse_flags(&args(&[])).unwrap();
        assert!(opts.build_options(true).srgb);
        assert!(!opts.build_options(false).srgb);

        let (forced, _) = parse_flags(&args(&["--srgb"])).unwrap();
        assert!(forced.build_options(false).srgb);
    }

    #[test]
    fn format_aliases_resolve() {
        for (name, expected) in [
            ("bc1", GrcTextureFormat::DXT1),
            ("BC3", GrcTextureFormat::DXT5),
            ("ati2", GrcTextureFormat::DXN),
            ("DXT5A", GrcTextureFormat::DXT5A),
            ("rgba8", GrcTextureFormat::A8B8G8R8),
        ] {
            assert_eq!(GrcTextureFormat::from_name(name), Some(expected), "{name}");
        }
        assert_eq!(GrcTextureFormat::from_name("nope"), None);
    }

    #[test]
    fn names_become_safe_filenames() {
        assert_eq!(sanitize("body_diff"), "body_diff");
        assert_eq!(sanitize("props/wall 01"), "props_wall_01");
        assert_eq!(sanitize(""), "unnamed");
    }
}

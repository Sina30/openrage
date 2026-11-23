//! `openrage-cli gfx`: list a Scaleform movie's tags, and carry it out to a
//! standard SWF and back.

use std::fmt::Write as _;
use std::process::ExitCode;

use or_io::gfx::GfxMovie;

use super::usage_error;

pub fn run(args: &[String]) -> ExitCode {
    let Some((verb, rest)) = args.split_first() else {
        print_usage();
        return ExitCode::from(2);
    };
    match (verb.as_str(), rest) {
        ("-h" | "--help" | "help", _) => {
            print_usage();
            ExitCode::SUCCESS
        }
        ("info", [path]) => match open(path) {
            Ok(movie) => {
                print!("{}", report(&movie));
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        ("export", [from, to]) => convert(from, to, GfxMovie::save_swf),
        ("import", [from, to]) => convert(from, to, GfxMovie::save),
        ("info", _) => usage_error("info needs <file.gfx>"),
        ("export", _) => usage_error("export needs <file.gfx> <out.swf>"),
        ("import", _) => usage_error("import needs <file.swf> <out.gfx>"),
        (other, _) => {
            eprintln!("Error: unknown subcommand: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn open(path: &str) -> Result<GfxMovie, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    GfxMovie::open(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn convert(from: &str, to: &str, save: fn(&GfxMovie) -> or_io::Result<Vec<u8>>) -> ExitCode {
    let bytes = match open(from).and_then(|m| save(&m).map_err(|e| e.to_string())) {
        Ok(bytes) => bytes,
        Err(e) => return fail(&e),
    };
    match std::fs::write(to, &bytes) {
        Ok(()) => {
            println!("wrote {to} ({} bytes)", bytes.len());
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("write {to}: {e}")),
    }
}

fn fail(why: &str) -> ExitCode {
    eprintln!("Error: {why}");
    ExitCode::FAILURE
}

/// The header, every tag with its length and the character it defines, and
/// the textures and imports the movie names.
pub(super) fn report(movie: &GfxMovie) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "SWF version {}, {}, {} tags",
        movie.version,
        if movie.compressed {
            "zlib-compressed"
        } else {
            "uncompressed"
        },
        movie.tags.len()
    );
    for (i, tag) in movie.tags.iter().enumerate() {
        let name = tag.name().unwrap_or("?");
        let _ = write!(
            out,
            "  {i:>5}  {:>4} {name:<28} {:>9} bytes",
            tag.code,
            tag.body.len()
        );
        if let Some(id) = tag.character_id() {
            let _ = write!(out, "  defines {id}");
        }
        out.push('\n');
    }
    let textures = movie.texture_refs();
    let _ = writeln!(out, "{} texture reference(s)", textures.len());
    for t in &textures {
        let _ = writeln!(
            out,
            "  tag {:>5}  id {:>5}  {}  {}",
            t.tag,
            t.id,
            t.file_name,
            t.export_name.as_deref().unwrap_or("")
        );
    }
    let imports = movie.imports();
    let _ = writeln!(out, "{} import(s)", imports.len());
    for i in &imports {
        let _ = writeln!(
            out,
            "  tag {:>5}  id {:>5}  {} from {}",
            i.tag, i.id, i.name, i.url
        );
    }
    out
}

fn print_usage() {
    eprintln!("Scaleform movies (.gfx): list tags, export to SWF, import from SWF.");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  openrage-cli gfx info <file.gfx>");
    eprintln!("  openrage-cli gfx export <file.gfx> <out.swf>");
    eprintln!("  openrage-cli gfx import <file.swf> <out.gfx>");
    eprintln!();
    eprintln!("An exported SWF is the movie under an SWF signature, Scaleform's own tags");
    eprintln!("kept, so an editor such as JPEXS opens it and an import gives the same .gfx.");
    eprintln!("An LZMA-compressed SWF (ZWS) has no .gfx form and is refused.");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame: an external image in the long form, then the end.
    fn movie() -> Vec<u8> {
        let mut image = 7u32.to_le_bytes().to_vec();
        image.extend_from_slice(&[0x0E, 0, 16, 0, 16, 0, 3]);
        image.extend_from_slice(b"hud");
        image.push(7);
        image.extend_from_slice(b"hud.dds");
        let mut body = vec![0x00, 0x00, 0x1E, 0x01, 0x00];
        body.extend_from_slice(&((1009u16 << 6) | 0x3F).to_le_bytes());
        body.extend_from_slice(&(image.len() as u32).to_le_bytes());
        body.extend_from_slice(&image);
        body.extend_from_slice(&[0x00, 0x00]);
        let mut out = b"GFX\x08".to_vec();
        out.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn info_lists_each_tag_and_the_texture_it_names() {
        let text = report(&GfxMovie::open(&movie()).unwrap());
        assert!(
            text.starts_with("SWF version 8, uncompressed, 2 tags"),
            "{text}"
        );
        assert!(text.contains("1009 DefineExternalImage2"), "{text}");
        assert!(text.contains("defines 7"), "{text}");
        assert!(text.contains("1 texture reference(s)"), "{text}");
        assert!(text.contains("hud.dds  hud"), "{text}");
        assert!(text.contains("0 import(s)"), "{text}");
    }

    #[test]
    fn export_then_import_gives_back_the_same_file() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path();
        let (gfx, swf, back) = (dir.join("a.gfx"), dir.join("a.swf"), dir.join("b.gfx"));
        std::fs::write(&gfx, movie()).unwrap();
        let arg = |p: &std::path::Path| p.to_string_lossy().into_owned();
        assert_eq!(
            run(&["export".into(), arg(&gfx), arg(&swf)]),
            ExitCode::SUCCESS
        );
        assert_eq!(&std::fs::read(&swf).unwrap()[..3], b"FWS");
        assert_eq!(
            run(&["import".into(), arg(&swf), arg(&back)]),
            ExitCode::SUCCESS
        );
        assert_eq!(std::fs::read(&back).unwrap(), movie());
    }
}

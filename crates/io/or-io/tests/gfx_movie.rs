//! `.gfx` Scaleform movies: every movie both installs ship lists its tags and
//! saves byte-identical, also by way of the SWF it exports to; and hand-built
//! movies hold what a shipped file cannot show, such as an edited compressed one.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The movies sit inside archives, some nested, so each install's own
//! executable supplies the keys and the bundled snapshot the NG ones (D18).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_io::gfx::{GfxMovie, Import, TextureRef};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

fn provider(keys: &Arc<Gta5Keys>) -> Arc<dyn RpfCryptoProvider> {
    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys)))
}

fn walk_archive(
    archive: &Arc<RpfArchive>,
    label: &str,
    depth: u32,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let path = format!("{label}/{}", node.path());
        let i = node.entry_index();
        if name.ends_with(".rpf") && depth < MAX_NEST_DEPTH {
            match archive.open_nested(i) {
                Ok(nested) => walk_archive(&Arc::new(nested), &path, depth + 1, visit),
                Err(e) => visit(path, Err(e.to_string())),
            }
        } else if name.ends_with(".gfx") {
            visit(path, archive.read_entry(i).map_err(|e| e.to_string()));
        }
    }
}

/// Every `.gfx` under `root`, loose or in an archive at any depth.
fn every_gfx(root: &Path, visit: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lower = label.to_ascii_lowercase();
            if lower.ends_with(".gfx") {
                visit(label, std::fs::read(&path).map_err(|e| e.to_string()));
            } else if lower.ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(&keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

/// Why `bytes` does not survive open, save, and the trip out to SWF and back.
fn round_trip(bytes: &[u8]) -> Result<GfxMovie, String> {
    let movie = GfxMovie::open(bytes).map_err(|e| format!("open: {e}"))?;
    let saved = movie.save().map_err(|e| format!("save: {e}"))?;
    if saved != bytes {
        return Err(format!(
            "saved {} bytes of {}, not identical",
            saved.len(),
            bytes.len()
        ));
    }
    let swf = movie.save_swf().map_err(|e| format!("export: {e}"))?;
    let back = GfxMovie::open(&swf)
        .and_then(|m| m.save())
        .map_err(|e| format!("import: {e}"))?;
    if back != bytes {
        return Err("the SWF it exports does not import to the same bytes".to_string());
    }
    Ok(movie)
}

#[test]
fn every_shipped_movie_lists_its_tags_and_saves_byte_identical() {
    let what = "every shipped .gfx byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let (mut seen, mut compressed, mut tags, mut textures, mut imports) = (0, 0, 0, 0, 0);
        let mut failed: Vec<String> = Vec::new();
        every_gfx(&root, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            seen += 1;
            match round_trip(&bytes) {
                Ok(movie) => {
                    compressed += usize::from(movie.compressed);
                    tags += movie.tags.len();
                    textures += movie.texture_refs().len();
                    imports += movie.imports().len();
                }
                Err(e) => failed.push(format!("{label}: {e}")),
            }
        });
        println!(
            "{}: {seen} movies ({compressed} compressed), {tags} tags, \
             {textures} texture references, {imports} imports",
            root.display()
        );
        assert!(seen > 0, "{} ships no .gfx", root.display());
        assert!(
            failed.is_empty(),
            "{} of {seen} under {} failed:\n{}",
            failed.len(),
            root.display(),
            failed.join("\n")
        );
    }
}

/// A short tag header: ten bits of code, six of length.
fn short(code: u16, body: &[u8]) -> Vec<u8> {
    let mut out = ((code << 6) | body.len() as u16).to_le_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

/// A long tag header, whatever the length.
fn long(code: u16, body: &[u8]) -> Vec<u8> {
    let mut out = ((code << 6) | 0x3F).to_le_bytes().to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// A one-frame body: an empty frame rectangle, 30 fps, one frame, then the
/// background colour, an external image in the long form, an import, a
/// shape, a frame and the end.
fn body() -> Vec<u8> {
    let mut image = 7u32.to_le_bytes().to_vec();
    image.extend_from_slice(&[0x0E, 0, 16, 0, 16, 0]);
    image.push(11);
    image.extend_from_slice(b"wanted_star");
    image.push(15);
    image.extend_from_slice(b"wanted_star.dds");
    image.push(0);
    let mut import = b"font_lib_efigs.swf\0".to_vec();
    import.extend_from_slice(&[1, 0, 1, 0, 5, 0]);
    import.extend_from_slice(b"button_33\0");

    let mut out = vec![0x00, 0x00, 0x1E, 0x01, 0x00];
    out.extend(short(9, &[0xFF, 0x80, 0x00]));
    out.extend(long(1009, &image));
    out.extend(long(71, &import));
    out.extend(short(2, &[3, 0, 0xAA, 0xBB]));
    out.extend(short(1, &[]));
    out.extend(short(0, &[]));
    out
}

fn file(signature: &[u8; 3], body: &[u8], stored: &[u8]) -> Vec<u8> {
    let mut out = signature.to_vec();
    out.push(8);
    out.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(stored);
    out
}

fn gfx() -> Vec<u8> {
    let body = body();
    file(b"GFX", &body, &body)
}

/// A `CFX` whose stream is zlib's fastest level, which the writer never picks.
fn cfx() -> Vec<u8> {
    let body = body();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&body).unwrap();
    file(b"CFX", &body, &encoder.finish().unwrap())
}

#[test]
fn a_hand_built_movie_lists_its_tags_and_saves_byte_identical() {
    let bytes = gfx();
    let movie = GfxMovie::open(&bytes).unwrap();
    assert_eq!(movie.version, 8);
    assert!(!movie.compressed);
    let codes: Vec<u16> = movie.tags.iter().map(|t| t.code).collect();
    assert_eq!(codes, [9, 1009, 71, 2, 1, 0]);
    let names: Vec<_> = movie.tags.iter().map(|t| t.name().unwrap()).collect();
    assert_eq!(
        names,
        [
            "SetBackgroundColor",
            "DefineExternalImage2",
            "ImportAssets2",
            "DefineShape",
            "ShowFrame",
            "End"
        ]
    );
    assert!(movie.tags[1].long && movie.tags[1].body.len() < 0x3F);
    let ids: Vec<_> = movie.tags.iter().map(|t| t.character_id()).collect();
    assert_eq!(ids, [None, Some(7), None, Some(3), None, None]);
    assert_eq!(
        movie.texture_refs(),
        [TextureRef {
            tag: 1,
            id: 7,
            export_name: Some("wanted_star".to_string()),
            file_name: "wanted_star.dds".to_string(),
        }]
    );
    assert_eq!(
        movie.imports(),
        [Import {
            tag: 2,
            url: "font_lib_efigs.swf".to_string(),
            id: 5,
            name: "button_33".to_string(),
        }]
    );
    assert_eq!(
        movie.save().unwrap(),
        bytes,
        "a long header on a short tag is kept"
    );
}

#[test]
fn a_movie_exports_to_a_standard_swf_and_imports_back_unchanged() {
    for (bytes, swf_signature) in [(gfx(), b"FWS"), (cfx(), b"CWS")] {
        let swf = GfxMovie::open(&bytes).unwrap().save_swf().unwrap();
        assert_eq!(&swf[..3], swf_signature);
        assert_eq!(swf[3..], bytes[3..], "only the signature differs");
        assert_eq!(GfxMovie::open(&swf).unwrap().save().unwrap(), bytes);
    }
}

#[test]
fn a_compressed_movie_keeps_its_stream_until_a_tag_changes() {
    let bytes = cfx();
    let mut movie = GfxMovie::open(&bytes).unwrap();
    assert!(movie.compressed);
    assert_eq!(movie.save().unwrap(), bytes);

    movie.tags[0].body = vec![0x00, 0x00, 0x00];
    let edited = movie.save().unwrap();
    assert_ne!(edited[8..], bytes[8..], "the stream is rewritten");
    let reopened = GfxMovie::open(&edited).unwrap();
    assert_eq!(reopened.tags, movie.tags);
    assert_eq!(reopened.save().unwrap(), edited);
}

#[test]
fn an_lzma_swf_is_refused_by_name() {
    let mut bytes = gfx();
    bytes[..3].copy_from_slice(b"ZWS");
    let why = GfxMovie::open(&bytes).unwrap_err().to_string();
    assert!(why.contains("LZMA") && why.contains("ZWS"), "{why}");
}

#[test]
fn a_damaged_movie_is_refused_by_what_is_wrong() {
    let body = body();
    let cases: [(&str, Vec<u8>); 4] = [
        ("no End tag", {
            let cut = &body[..body.len() - 2];
            file(b"GFX", cut, cut)
        }),
        ("past the body", {
            let cut = &body[..body.len() - 6];
            file(b"GFX", cut, cut)
        }),
        ("follow the End tag", {
            let mut more = body.clone();
            more.push(0xEE);
            file(b"GFX", &more, &more)
        }),
        ("the header states", {
            let mut bytes = gfx();
            bytes[4] ^= 1;
            bytes
        }),
    ];
    for (expected, bytes) in cases {
        let why = GfxMovie::open(&bytes).unwrap_err().to_string();
        assert!(why.contains(expected), "expected {expected:?} in {why}");
    }
}

//! What the integration suites share: a pack laid out on disk from the staged corpus, and a project that builds it.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use or_project::{Container, Module, Output, Project};
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::{Component, Garment, Ped, Prop, Slot, Variation, Wardrobe};

/// The smallest `count` staged files of `extension` that the reader for it opens.
pub fn staged(extension: &str, count: usize, what: &str) -> Option<Vec<PathBuf>> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(TARGET_GEN8)
        .find_or_skip(what)?;
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let format = or_io::gta5::formats::format(extension).expect("a declared format");
    let opens: Vec<PathBuf> = files
        .into_iter()
        .take(80)
        .filter(|p| std::fs::read(p).is_ok_and(|bytes| format.open_declared(&bytes).is_ok()))
        .take(count)
        .collect();
    if opens.len() < count {
        or_test_support::skip_corpus(
            what,
            format!("fewer than {count} small .{extension} files open"),
        );
        return None;
    }
    Some(opens)
}

/// A valid `.ydd` of one drawable that differs from the one of every other `tag`, so a drop reads it and equal bytes mean equal tags.
pub fn ydd(tag: &str) -> Vec<u8> {
    or_source::sample::drawables_of(tag)
}

/// A valid `.ytd` of one texture that differs from the one of every other `tag`.
pub fn ytd(tag: &str) -> Vec<u8> {
    or_source::sample::textures_of(tag)
}

/// Copy `from` to `folder/rel`.
pub fn put(folder: &Path, rel: &str, from: &Path) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, &to).unwrap();
}

/// Write `bytes` to `folder/rel`, making the folders on the way.
pub fn put_bytes(folder: &Path, rel: &str, bytes: &[u8]) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

pub fn variation(texture: &str) -> Variation {
    Variation {
        texture: Some(texture.to_string()),
        ..Variation::default()
    }
}

/// A garment of `ped` in `slot` whose model and textures are the files named.
pub fn garment(
    ped: Ped,
    slot: Slot,
    position: Option<u32>,
    model: &str,
    textures: &[&str],
) -> Garment {
    let mut g = Garment::new();
    g.ped = Some(ped);
    g.slot = Some(slot);
    g.position = position;
    g.model = Some(model.to_string());
    g.variations = textures.iter().map(|t| variation(t)).collect();
    g
}

pub const JBIB: Slot = Slot::Component(Component::Jbib);
pub const LOWR: Slot = Slot::Component(Component::Lowr);
pub const FEET: Slot = Slot::Component(Component::Feet);
pub const P_HEAD: Slot = Slot::Prop(Prop::Head);

/// A project in `dir` whose one clothing module is `dir` itself, with one FiveM resource build.
pub fn project(dir: &Path, wardrobe: &Wardrobe, resource: &str) -> Project {
    wardrobe.write(dir).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", ".")],
        outputs: vec![Output::fivem_resource("fivem", resource, &["wardrobe"])],
    };
    let path = dir.join("pack.openrage-proj");
    let _ = std::fs::remove_file(&path);
    Project::create(&path, &container, &[]).unwrap();
    Project::load(&path).unwrap()
}

/// Every file under `dir` by its `/` path, with its bytes.
pub fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}
/// A drawables source folder at `dir` that `describe` accepts, called `name`: its manifest and the one file a dictionary holds.
pub fn drawables_source(dir: &Path, name: &str) {
    use or_source::asset::{AssetManifest, How, Origin, Source};
    std::fs::create_dir_all(dir).unwrap();
    let origin = Origin {
        how: How::Authored,
        from_game: false,
        virtual_path: None,
        game_build: None,
        sha256: None,
        cooked_sha256: None,
        extra: BTreeMap::new(),
    };
    let manifest = AssetManifest::new("drawables", name, "gen8", origin).unwrap();
    Source {
        dir: dir.to_path_buf(),
        manifest,
    }
    .write_manifest()
    .unwrap();
    std::fs::write(dir.join("dictionary.json"), b"{}").unwrap();
}

/// A `.ytd` of textures that each store the size and the mips asked for, at any size: name, format, width, height and mips.
pub fn ytd_of(
    textures: &[(&str, or_io::codec::texture::GrcTextureFormat, u32, u32, u32)],
) -> Vec<u8> {
    use or_io::authoring::TextureBuildOptions;
    use or_io::codec::texture::RgbaImage;
    use or_io::decl::File;
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::texture_dict::gen8::PROCESSED;
    use or_io::gta5::PgDictionary;

    let mut dictionary = PgDictionary::<GrcTexture>::default();
    for (i, (name, format, width, height, mips)) in textures.iter().enumerate() {
        let mut image = RgbaImage::new(*width, *height);
        for y in 0..*height {
            for x in 0..*width {
                let c = |k: u32| ((x * (3 + k) + y * (5 + k * 2) + i as u32 * 40) % 256) as u8;
                image.set_pixel(x, y, [c(0), c(1), c(2), 255]);
            }
        }
        let mut texture = GrcTexture {
            name: name.to_string(),
            resource_type_and_conversion_flags: PROCESSED,
            ..Default::default()
        };
        let options = TextureBuildOptions {
            mip_levels: Some(*mips),
            allow_npot: true,
            ..TextureBuildOptions::default()
        };
        texture.set_pixels(*format, &image, &options).unwrap();
        dictionary.insert(texture);
    }
    dictionary.to_bytes().unwrap()
}

pub mod built;
pub mod compare;
pub mod demo;
pub mod shop;
pub mod staged;

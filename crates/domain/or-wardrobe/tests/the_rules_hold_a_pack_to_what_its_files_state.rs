//! A pack on disk is held to what its files state: each rule finds the file built to break it, a clean file is silent,
//! and what a scan counted once is not counted again.

mod common;

use std::path::Path;

use common::built::{ytd, Tex};
use common::{garment, put, JBIB};
use or_io::codec::texture::GrcTextureFormat as F;
use or_io::decl::{File, Gen8};
use or_io::gta5::gen8::{EntryData, GrcTexture};
use or_io::gta5::{GtaDrawable, PgDictionary, TextureSlot};
use or_wardrobe::{allocate, check, inspect, Depth, Finding, Ped, Wardrobe};

/// The smallest staged model that the rules find nothing wrong with, which is a model of the game's own.
fn staged_model() -> Option<std::path::PathBuf> {
    let mut files = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(or_test_support::TARGET_GEN8)
        .find_or_skip("a model is held to what it states")?;
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "tex/fine.ytd",
        &ytd(&[Tex::new("fine", 512, 512, F::DXT1)]),
    );
    for path in files.into_iter().take(60) {
        put(dir.path(), "models/a.ydd", &path);
        let w = pack(dir.path(), "models/a.ydd", &["tex/fine.ytd"]);
        if scan(dir.path(), &w).iter().all(|f| f.code == "reserved") {
            return Some(path);
        }
    }
    or_test_support::skip_corpus(
        "a model is held to what it states",
        "no staged model passes every rule at its default",
    );
    None
}

fn write(folder: &Path, rel: &str, bytes: &[u8]) {
    common::put_bytes(folder, rel, bytes);
}

/// An edit of a drawable that breaks a rule, given the hash of the name of the diffuse parameter.
type Edit = fn(&mut GtaDrawable<Gen8>, u32);

/// The pack of one garment per texture dictionary named, each in the jacket slot with `model` as its model.
fn pack(folder: &Path, model: &str, dictionaries: &[&str]) -> Wardrobe {
    let mut w = Wardrobe::new("pack");
    for (i, dictionary) in dictionaries.iter().enumerate() {
        let mut g = garment(Ped::Male, JBIB, Some(i as u32), model, &[dictionary]);
        g.label = Some(format!("garment {i}"));
        w.garments.push(g);
    }
    w.write(folder).unwrap();
    w
}

fn scan(folder: &Path, w: &Wardrobe) -> Vec<Finding> {
    check(w, &allocate(w), &inspect(folder, w, Depth::Measure))
}

fn codes_of(found: &[Finding], garment: &str, w: &Wardrobe) -> Vec<&'static str> {
    let id = &w
        .garments
        .iter()
        .find(|g| g.label.as_deref() == Some(garment))
        .unwrap()
        .id;
    found
        .iter()
        .filter(|f| f.garment.as_deref() == Some(id))
        .map(|f| f.code)
        .collect()
}

#[test]
fn each_rule_finds_the_texture_file_built_to_break_it_and_a_clean_one_is_silent() {
    let Some(model) = staged_model() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "models/a.ydd", &model);
    let broken = [
        (
            "size",
            "texture-size",
            vec![Tex::new("big", 4096, 2048, F::DXT1)],
        ),
        (
            "odd",
            "texture-not-power-of-two",
            vec![Tex::new("odd", 1000, 1000, F::DXT1)],
        ),
        (
            "flat",
            "texture-mips",
            vec![Tex::new("flat", 1024, 1024, F::DXT1).with_mips(1)],
        ),
        (
            "raw",
            "texture-format",
            vec![Tex::new("raw", 1024, 1024, F::A8R8G8B8)],
        ),
        (
            "seven",
            "texture-bc7-legacy",
            vec![Tex::new("seven", 1024, 1024, F::BC7)],
        ),
        // No texture is wide, and three of 2048 by 2048 take more than 16 MB together.
        (
            "heavy",
            "dictionary-size",
            ["h1", "h2", "h3"]
                .map(|n| Tex::new(n, 2048, 2048, F::DXT5))
                .into(),
        ),
    ];
    write(
        dir.path(),
        "tex/fine.ytd",
        &ytd(&[Tex::new("fine", 512, 512, F::DXT1)]),
    );
    for (file, _, tex) in &broken {
        write(dir.path(), &format!("tex/{file}.ytd"), &ytd(tex));
    }
    let files: Vec<String> = std::iter::once("fine")
        .chain(broken.iter().map(|(file, ..)| *file))
        .map(|file| format!("tex/{file}.ytd"))
        .collect();
    let named: Vec<&str> = files.iter().map(String::as_str).collect();
    let w = pack(dir.path(), "models/a.ydd", &named);

    let found = scan(dir.path(), &w);
    let said: Vec<_> = found.iter().map(|f| (f.code, &f.message)).collect();
    assert_eq!(
        codes_of(&found, "garment 0", &w),
        Vec::<&str>::new(),
        "a clean texture is silent: {said:?}"
    );
    for (i, (_, code, _)) in broken.iter().enumerate() {
        assert_eq!(
            codes_of(&found, &format!("garment {}", i + 1), &w),
            [*code],
            "{code}: {said:?}"
        );
    }
}

#[test]
fn a_model_of_the_corpus_is_silent_and_each_rule_finds_the_edit_built_to_break_it() {
    let Some(model) = staged_model() else {
        return;
    };
    let bytes = std::fs::read(&model).unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "tex/fine.ytd",
        &ytd(&[Tex::new("fine", 512, 512, F::DXT1)]),
    );
    write(dir.path(), "models/clean.ydd", &bytes);

    let diffuse = or_io::hash::string_hash_str("DiffuseSampler");
    let edits: [(&str, &str, Edit); 5] = [
        ("models/no_medium.ydd", "lod-missing", |d, _| {
            d.lod_group.lods[1].models.clear()
        }),
        ("models/medium.ydd", "lod-medium-heavy", |d, _| {
            d.lod_group.lods[1] = d.lod_group.lods[0].clone()
        }),
        ("models/low.ydd", "lod-low-heavy", |d, _| {
            d.lod_group.lods[2] = d.lod_group.lods[0].clone()
        }),
        ("models/no_diffuse.ydd", "diffuse-missing", |d, hash| {
            for shader in d.shader_group.iter_mut().flat_map(|g| g.shaders.iter_mut()) {
                if let Some(entry) = shader.entry_mut(hash) {
                    entry.data = EntryData::Texture(TextureSlot::Unbound);
                }
            }
        }),
        ("models/stray.ydd", "embedded-unused", |d, _| {
            if let Some(group) = d.shader_group.as_deref_mut() {
                group.embed(GrcTexture {
                    name: "stray_texture".into(),
                    width: 4,
                    height: 4,
                    depth: 1,
                    mip_count: 1,
                    ..GrcTexture::default()
                });
            }
        }),
    ];
    let mut paths = vec!["models/clean.ydd"];
    for (path, _, edit) in &edits {
        let mut dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes).unwrap();
        for drawable in dictionary.entries.iter_mut().flatten() {
            edit(drawable, diffuse);
        }
        write(
            dir.path(),
            path,
            &dictionary.to_bytes().expect("the edit saves"),
        );
        paths.push(path);
    }
    let mut w = Wardrobe::new("pack");
    for (i, path) in paths.iter().enumerate() {
        let mut g = garment(Ped::Male, JBIB, Some(i as u32), path, &["tex/fine.ytd"]);
        g.label = Some(format!("garment {i}"));
        w.garments.push(g);
    }
    w.write(dir.path()).unwrap();

    let found = scan(dir.path(), &w);
    let said: Vec<_> = found.iter().map(|f| (f.code, &f.message)).collect();
    assert_eq!(
        codes_of(&found, "garment 0", &w),
        Vec::<&str>::new(),
        "a model of the game's own is silent: {said:?}"
    );
    for (i, (_, code, _)) in edits.iter().enumerate() {
        let own = codes_of(&found, &format!("garment {}", i + 1), &w);
        assert!(
            own.contains(code),
            "{code} is not found in {own:?}: {said:?}"
        );
    }
}

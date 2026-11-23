//! `openrage-cli wardrobe fix` and `wardrobe textures` do what the editor's buttons do, say no to what they do not take, and a second run of
//! `fix` changes no byte.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_io::authoring::TextureBuildOptions;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::gta5::PgDictionary;
use or_project::{Container, Module, Output, Project};
use or_source::game_import::{decompile, Facts};
use or_wardrobe::{Component, Garment, Ped, Slot, Wardrobe};

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

/// A `.ytd` of one texture of `width` by `height` that stores one mip.
fn ytd(name: &str, width: u32, height: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.set_pixel(x, y, [(x * 3 % 256) as u8, (y * 5 % 256) as u8, 90, 255]);
        }
    }
    let mut texture = GrcTexture {
        name: name.to_string(),
        resource_type_and_conversion_flags: PROCESSED,
        ..Default::default()
    };
    let options = TextureBuildOptions {
        mip_levels: Some(1),
        allow_npot: true,
        ..TextureBuildOptions::default()
    };
    texture
        .set_pixels(GrcTextureFormat::DXT1, &image, &options)
        .unwrap();
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(texture);
    dictionary.to_bytes().unwrap()
}

/// A project whose one clothing content holds two jackets pinned at one place, the first with a texture of 100 by 60.
fn a_project(dir: &Path) -> (PathBuf, PathBuf) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    decompile(
        &ytd("odd", 100, 60),
        "odd.ytd",
        &pack.join("textures"),
        &Facts::default(),
    )
    .unwrap();
    decompile(
        &ytd("fine", 64, 64),
        "fine.ytd",
        &pack.join("textures"),
        &Facts::default(),
    )
    .unwrap();
    let mut w = Wardrobe::new("demo");
    for (model, texture) in [("a", "odd"), ("b", "fine")] {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.position = Some(3);
        g.model = Some(format!("models/{model}.ydd"));
        g.variations = vec![or_wardrobe::Variation {
            texture: Some(format!("textures/{texture}.textures.src")),
            ..Default::default()
        }];
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    let container = Container {
        modules: vec![Module::clothing("wardrobe", "clothing")],
        outputs: vec![Output::fivem_resource("fivem", "demo", &["wardrobe"])],
    };
    let file = dir.join("demo.openrage-proj");
    Project::create(&file, &container, &[]).unwrap();
    (file, pack)
}

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => walk(base, &path, out),
                false => {
                    let rel = path
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out.retain(|rel, _| !rel.starts_with(".openrage/"));
    out
}

#[test]
fn fix_mends_the_pack_and_run_again_changes_no_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (file, pack) = a_project(dir.path());
    let before = Wardrobe::read(&pack).unwrap();
    assert_eq!(before.garments[1].position, Some(3));

    assert_eq!(
        wardrobe(&["fix", file.to_str().unwrap()]),
        ExitCode::SUCCESS
    );

    let after = Wardrobe::read(&pack).unwrap();
    assert_eq!(
        after.garments[1].position, None,
        "the clashing pin is let go"
    );
    assert_eq!(
        after.garments[0].variations[0].texture.as_deref(),
        Some("textures/odd-2.textures.src"),
        "the texture of 100 by 60 is a lighter copy"
    );
    assert!(
        pack.join("textures/odd.textures.src").is_dir(),
        "the source it was is still there"
    );

    let settled = tree(&pack);
    assert_eq!(
        wardrobe(&["fix", file.to_str().unwrap(), "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(tree(&pack), settled, "a second run changes no byte");
}

#[test]
fn textures_scales_by_a_ratio_or_a_side_and_refuses_what_it_cannot_do() {
    let dir = tempfile::tempdir().unwrap();
    let (file, pack) = a_project(dir.path());
    let file = file.to_str().unwrap();
    let before = tree(&pack);

    for refused in [
        vec!["textures", file, "--ratio", "2"],
        vec!["textures", file, "--ratio", "0.5", "--side", "64"],
        vec!["textures", file, "--side", "two"],
        vec!["textures", file, "--module", "nobody"],
        vec!["textures", file, "--nonsense"],
        vec!["fix"],
    ] {
        assert_ne!(wardrobe(&refused), ExitCode::SUCCESS, "{refused:?}");
    }
    assert_eq!(tree(&pack), before, "a refusal changes nothing");

    assert_eq!(
        wardrobe(&["textures", file, "--side", "32", "--no-format"]),
        ExitCode::SUCCESS
    );
    let after = Wardrobe::read(&pack).unwrap();
    assert_eq!(
        after.garments[1].variations[0].texture.as_deref(),
        Some("textures/fine-2.textures.src"),
        "a texture of 64 by 64 that stores one mip is cut to a side of 32"
    );
}

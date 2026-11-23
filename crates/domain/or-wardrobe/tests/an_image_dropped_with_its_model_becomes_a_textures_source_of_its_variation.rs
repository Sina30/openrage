//! An image named as a texture, dropped with its model, is the texture of a variation: it becomes an authored textures
//! source of the garment, holding the picture and no DDS until it compiles. The build compiles it for the platform it
//! states, from PNG, TGA, BMP, JPEG or a DDS copied block for block, and an image that would compile to none is left out
//! by name with the reason and never guessed at.

mod common;

use std::path::{Path, PathBuf};

use common::ydd;
use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_wardrobe::edit::{add_clothes, undo};
use or_wardrobe::Wardrobe;

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, bytes).unwrap();
    at
}

/// A 32 by 32 picture in the format the extension says.
fn picture(root: &Path, rel: &str) -> PathBuf {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    let rgba = image::RgbaImage::from_fn(32, 32, |x, y| {
        image::Rgba([(x * 8) as u8, (y * 8) as u8, 128, 255])
    });
    match at.extension().and_then(|e| e.to_str()) {
        Some("jpg") => image::DynamicImage::ImageRgba8(rgba).to_rgb8().save(&at),
        Some("hdr") => {
            image::DynamicImage::ImageRgb32F(image::DynamicImage::ImageRgba8(rgba).to_rgb32f())
                .save(&at)
        }
        _ => rgba.save(&at),
    }
    .unwrap();
    at
}

/// A DDS of 32 by 32 DXT1 blocks no encoder would write, so only a copy block for block gives them back.
fn noise_dds(root: &Path, rel: &str) -> Vec<u8> {
    let size = GrcTextureFormat::DXT1.total_size(32, 32, 1) as usize;
    let mut state = 0x9E37_79B9u32;
    let blocks: Vec<u8> = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        })
        .collect();
    let texture = GrcTexture {
        name: "noise".to_string(),
        width: 32,
        height: 32,
        depth: 1,
        format: GrcTextureFormat::DXT1.to_raw_format(),
        mip_count: 1,
        backing_store: blocks.clone(),
        ..Default::default()
    };
    let dds = or_io::codec::texture::to_dds(&texture)
        .unwrap()
        .to_bytes()
        .unwrap();
    put(root, rel, &dds);
    blocks
}

fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_name() == ".openrage" {
                continue;
            }
            match e.path().is_dir() {
                true => walk(base, &e.path(), out),
                false => out.push(
                    e.path()
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                ),
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_dropped_image_becomes_an_authored_textures_source_of_its_garment_in_every_format_and_undo_takes_it_out(
) {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.png"));
    picture(&drop, &format!("{C}^jbib_diff_001_b_uni.tga"));
    picture(&drop, &format!("{C}^jbib_diff_001_c_uni.bmp"));
    picture(&drop, &format!("{C}^jbib_diff_001_d_uni.jpg"));
    let blocks = noise_dds(&drop, &format!("{C}^jbib_diff_001_e_uni.dds"));

    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop is added");

    assert_eq!(added.garments.len(), 1, "{added:?}");
    assert_eq!(added.textures, 5, "{added:?}");
    assert!(added.left_out.is_empty(), "{:?}", added.left_out);
    let pack = Wardrobe::read(&module).unwrap();
    let jacket = &pack.garments[0];
    let made: Vec<&str> = (jacket.variations.iter())
        .map(|v| v.texture.as_deref().unwrap())
        .collect();
    assert_eq!(
        made,
        ["a", "b", "c", "d", "e"]
            .map(|l| format!("textures/{C}^jbib_diff_001_{l}_uni.textures.src"))
    );
    for texture in &made {
        let source = or_source::asset::Source::open(&module.join(texture)).unwrap();
        assert_eq!(source.manifest.origin.how, or_source::asset::How::Authored);
        assert_eq!(
            source.manifest.target,
            or_source::create::AUTHORED_TARGET,
            "a source is platform-free: nothing chose Legacy or Enhanced"
        );
        // It compiles, as the build compiles it, to a dictionary of one texture.
        let compiled = or_source::textures::compile(&source).expect("the source compiles");
        let dictionary = PgDictionary::<GrcTexture>::from_bytes(&compiled.bytes).unwrap();
        let held: Vec<_> = dictionary.entries.iter().flatten().collect();
        assert_eq!(held.len(), 1, "{texture}: one texture in a dictionary");
        assert_eq!((held[0].width, held[0].height), (32, 32), "{texture}");
    }
    // Inside, a texture is named as the game names it, without the collection in front.
    let first = or_source::asset::Source::open(&module.join(made[0])).unwrap();
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(
        &or_source::textures::compile(&first).unwrap().bytes,
    )
    .unwrap();
    assert_eq!(
        dictionary.entries.iter().flatten().next().unwrap().name,
        "jbib_diff_001_a_uni"
    );
    // The DDS is exact: the dictionary holds its blocks as they were.
    let last = or_source::asset::Source::open(&module.join(made[4])).unwrap();
    let dictionary =
        PgDictionary::<GrcTexture>::from_bytes(&or_source::textures::compile(&last).unwrap().bytes)
            .unwrap();
    assert_eq!(
        dictionary
            .entries
            .iter()
            .flatten()
            .next()
            .unwrap()
            .backing_store,
        blocks
    );
    // The pictures are the inputs of their sources, and no compiled texture sits in the pack.
    assert!(
        module
            .join(made[0])
            .join("textures/jbib_diff_001_a_uni.png")
            .is_file(),
        "{:?}",
        files(&module)
    );
    assert!(
        files(&module).iter().all(|f| !f.ends_with(".ytd")),
        "no .ytd until it compiles: {:?}",
        files(&module)
    );

    undo(&module, &added).expect("the drop is taken back");
    assert_eq!(files(&module), ["wardrobe.json"]);
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());
}

#[test]
fn a_picture_whose_name_says_no_texture_is_left_out_by_name_and_is_not_the_only_models() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    picture(&drop, "cover.png");

    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 1);
    assert_eq!(added.textures, 0, "a picture of the pack is not a texture");
    assert_eq!(added.left_out.len(), 1, "{:?}", added.left_out);
    assert!(
        added.left_out[0].path.ends_with("cover.png"),
        "{:?}",
        added.left_out
    );
    assert!(
        added.left_out[0].why.contains("jbib_diff_000_a_uni"),
        "it says how to name one: {:?}",
        added.left_out
    );
}

#[test]
fn light_with_no_rule_to_bring_it_down_is_left_out_by_name_with_the_compile_s_sentence_and_no_source(
) {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.hdr"));
    picture(&drop, &format!("{C}^jbib_diff_001_b_uni.png"));

    let added = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert_eq!(added.garments.len(), 1, "the garment is still added");
    assert_eq!(
        added.textures, 1,
        "the picture that makes a texture is kept"
    );
    assert_eq!(added.left_out.len(), 1, "{:?}", added.left_out);
    assert!(
        added.left_out[0].path.ends_with("jbib_diff_001_a_uni.hdr"),
        "{:?}",
        added.left_out
    );
    assert!(
        added.left_out[0].why.contains("floating-point"),
        "{:?}",
        added.left_out
    );
}

#[test]
fn the_same_folder_of_images_dropped_twice_adds_its_garment_once() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    picture(&drop, &format!("{C}^jbib_diff_001_a_uni.png"));
    add_clothes(&module, std::slice::from_ref(&drop)).unwrap();
    let held = Wardrobe::read(&module).unwrap();

    let again = add_clothes(&module, std::slice::from_ref(&drop)).unwrap();

    assert!(again.garments.is_empty(), "{again:?}");
    assert_eq!(again.already_held.len(), 1, "{again:?}");
    assert_eq!(Wardrobe::read(&module).unwrap(), held);
}

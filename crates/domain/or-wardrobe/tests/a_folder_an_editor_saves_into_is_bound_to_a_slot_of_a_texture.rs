//! A folder an external editor saves into is bound to a slot of a garment's texture, diffuse, specular or normal, and what the editor saves
//! there becomes the picture of that slot in the texture's source, which the live recook then cooks.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, put_bytes, snapshot, ytd_of, JBIB};
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_source::asset::Source;
use or_source::game_import::{decompile, Facts};
use or_source::textures::{self, InputKind};
use or_wardrobe::slots::{bind, bindings, deliver, newest, unbind, Binding, MapSlot};
use or_wardrobe::{Ped, Wardrobe};

const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const SOURCE: &str = "textures/jbib_diff_000_a_uni.textures.src";

fn a_pack(dir: &Path) -> (PathBuf, String) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    decompile(
        &ytd_of(&[("jbib_diff_000_a_uni", DXT5, 64, 64, 1)]),
        "jbib_diff_000_a_uni.ytd",
        &pack.join("textures"),
        &Facts::default(),
    )
    .unwrap();
    put_bytes(
        &pack,
        "textures/shipped.ytd",
        &ytd_of(&[("shipped", DXT5, 64, 64, 1)]),
    );
    let mut w = Wardrobe::new("demo");
    let mut g = garment(Ped::Male, JBIB, None, "models/m.ydd", &[SOURCE]);
    g.variations.push(common::variation("textures/shipped.ytd"));
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    (pack, id)
}

fn painted(colour: [u8; 4]) -> RgbaImage {
    let mut image = RgbaImage::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            image.set_pixel(x, y, colour);
        }
    }
    image
}

fn bound(folder: &Path, garment: &str, texture: usize, slot: MapSlot) -> Binding {
    Binding {
        folder: folder.display().to_string(),
        garment: garment.to_string(),
        texture,
        slot,
    }
}

#[test]
fn what_the_editor_saves_becomes_the_picture_of_the_slot_in_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path());
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();

    // The diffuse: a picture of the texture's name is put in, and it wins over the DDS beside it.
    let diffuse = bound(&art, &id, 0, MapSlot::Diffuse);
    bind(&pack, diffuse.clone()).unwrap();
    assert_eq!(bindings(&pack), std::slice::from_ref(&diffuse));
    assert_eq!(newest(&diffuse), None, "nothing is saved yet");
    or_io::codec::texture::save_png(art.join("whatever.png"), &painted([200, 30, 30, 255]))
        .unwrap();
    let saved = newest(&diffuse).expect("the saved picture");
    let done = deliver(&pack, &diffuse, &saved).unwrap();
    assert_eq!(done.source, SOURCE);
    assert_eq!(done.texture, "jbib_diff_000_a_uni");
    let source = Source::open(&pack.join(SOURCE)).unwrap();
    let held = textures::inputs_of(&source).unwrap();
    assert_eq!(held[0].kind, InputKind::Image);
    assert_eq!(held[0].from, ["jbib_diff_000_a_uni.png"]);
    assert!(held[0]
        .unused
        .contains(&"jbib_diff_000_a_uni.dds".to_string()));
    assert!(
        art.join("whatever.png").is_file(),
        "the editor's file is left where it is"
    );

    // A second save, in another format, replaces the picture and does not leave two.
    or_io::codec::texture::save_png(art.join("again.png"), &painted([20, 200, 30, 255])).unwrap();
    bmp_of(&art.join("again.bmp"), [20, 30, 220]);
    let saved = newest(&diffuse).unwrap();
    deliver(&pack, &diffuse, &saved).unwrap();
    let pictures: Vec<String> = std::fs::read_dir(pack.join(SOURCE).join("textures"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("jbib_diff_000_a_uni.") && !n.ends_with(".dds"))
        .collect();
    assert_eq!(pictures.len(), 1, "{pictures:?}");

    // The normal slot adds a texture of its own, stored linear, beside the diffuse.
    let normal = bound(&art, &id, 0, MapSlot::Normal);
    or_io::codec::texture::save_png(art.join("n.png"), &painted([128, 128, 255, 255])).unwrap();
    deliver(&pack, &normal, &art.join("n.png")).unwrap();
    let compiled = textures::compile(&Source::open(&pack.join(SOURCE)).unwrap()).unwrap();
    let dictionary: or_io::gta5::PgDictionary<or_io::gta5::gen8::GrcTexture> =
        or_io::decl::File::from_bytes(&compiled.bytes).unwrap();
    let made = dictionary
        .get("jbib_normal_000_a_uni")
        .expect("the normal slot's texture");
    assert_eq!(made.info_bits & 1, 0, "a normal map is stored linear");
    assert!(dictionary.get("jbib_diff_000_a_uni").is_some());

    unbind(&pack, &diffuse).unwrap();
    assert!(bindings(&pack).is_empty());
}

fn bmp_of(path: &Path, rgb: [u8; 3]) {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb(rgb));
    image.save(path).unwrap();
}

#[test]
fn a_texture_that_is_no_source_of_the_pack_or_a_variation_that_is_not_there_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path());
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    or_io::codec::texture::save_png(art.join("a.png"), &painted([1, 2, 3, 255])).unwrap();
    let before = snapshot(&pack);

    // The second texture of the garment is a compiled file: a picture has no place in it.
    let compiled = bound(&art, &id, 1, MapSlot::Diffuse);
    let why = deliver(&pack, &compiled, &art.join("a.png"))
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("compiled file") && why.contains("source"),
        "{why}"
    );
    assert!(bind(&pack, compiled).is_err(), "it cannot be bound either");

    let missing = bound(&art, &id, 7, MapSlot::Diffuse);
    assert!(deliver(&pack, &missing, &art.join("a.png")).is_err());
    let nobody = bound(&art, "nobody", 0, MapSlot::Diffuse);
    assert!(deliver(&pack, &nobody, &art.join("a.png")).is_err());
    let not_a_picture = bound(&art, &id, 0, MapSlot::Diffuse);
    std::fs::write(art.join("notes.txt"), b"hello").unwrap();
    assert!(deliver(&pack, &not_a_picture, &art.join("notes.txt")).is_err());
    assert_eq!(snapshot(&pack), before, "a refusal changes no byte");
}

#[test]
fn a_copy_that_fails_leaves_the_picture_the_slot_had() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path());
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    let diffuse = bound(&art, &id, 0, MapSlot::Diffuse);
    or_io::codec::texture::save_png(art.join("v1.png"), &painted([200, 30, 30, 255])).unwrap();
    deliver(&pack, &diffuse, &art.join("v1.png")).unwrap();
    let before = snapshot(&pack);

    // The editor has renamed its next save away by the time it is copied, and it is a picture of another format.
    assert!(deliver(&pack, &diffuse, &art.join("v2.tga")).is_err());
    assert!(
        snapshot(&pack) == before,
        "a copy that failed must leave the picture that was there"
    );
}

#[test]
fn a_texture_name_that_leads_out_of_its_source_is_refused_and_nothing_is_written_outside() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path());
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    or_io::codec::texture::save_png(art.join("a.png"), &painted([1, 2, 3, 255])).unwrap();
    let rules = pack.join(SOURCE).join(textures::RULES);
    let mut stated: textures::Rules =
        serde_json::from_str(&std::fs::read_to_string(&rules).unwrap()).unwrap();
    stated.textures.push(textures::Entry {
        name: "../../../escaped".to_string(),
        source: None,
        pack: None,
        header: None,
        encode: None,
    });
    std::fs::write(&rules, or_source::json::canonical(&stated).unwrap()).unwrap();
    let before = snapshot(&pack);

    let diffuse = bound(&art, &id, 0, MapSlot::Diffuse);
    let why = deliver(&pack, &diffuse, &art.join("a.png"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("../../../escaped"), "{why}");
    assert!(bind(&pack, diffuse).is_err(), "it cannot be bound either");
    assert!(snapshot(&pack) == before, "a refusal changes no byte");
    assert!(!pack.join("escaped.png").exists());
}

/// A picture of 16 bits a channel.
fn deep(path: &Path) {
    image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        64,
        64,
        image::Rgba([40_000, 20_000, 10_000, 65_535]),
    )
    .save(path)
    .unwrap();
}

#[test]
fn a_picture_of_8_bits_put_after_one_of_16_takes_the_window_back_and_the_source_compiles() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path());
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    let diffuse = bound(&art, &id, 0, MapSlot::Diffuse);
    let rules = || std::fs::read_to_string(pack.join(SOURCE).join(textures::RULES)).unwrap();
    let compiles = || textures::compile(&Source::open(&pack.join(SOURCE)).unwrap());

    deep(&art.join("deep.png"));
    deliver(&pack, &diffuse, &art.join("deep.png")).unwrap();
    assert!(
        rules().contains("clamp"),
        "the window is stated: {}",
        rules()
    );
    compiles().expect("16 bits compile with their window");

    // The editor's bit depth is toggled, and the next save under the same name has 8: the window means nothing to it.
    or_io::codec::texture::save_png(art.join("flat.png"), &painted([20, 200, 30, 255])).unwrap();
    deliver(&pack, &diffuse, &art.join("flat.png")).unwrap();
    assert!(!rules().contains("clamp"), "{}", rules());
    compiles().expect("8 bits compile without it");

    // And a picture of 16 bits after it is given the window again.
    deep(&art.join("again.png"));
    deliver(&pack, &diffuse, &art.join("again.png")).unwrap();
    compiles().expect("16 bits again");
}

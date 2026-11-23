//! Creating a source: an empty one that validates and compiles, to be filled, and one made of pictures that holds the pictures
//! and no DDS. Neither chooses a platform: the layout a source compiles through is one constant, and a build moves the file.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{flat, save_image};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_source::asset::{How, Source, MANIFEST};
use or_source::create::{self, AUTHORED_TARGET};
use or_source::textures;

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_new_texture_dictionary_is_empty_validates_and_compiles_to_a_dictionary_of_no_textures() {
    let project = tempfile::tempdir().unwrap();

    let made = create::new_source(project.path(), textures::KIND, "shirts").expect("created");

    assert_eq!(
        made.path,
        project.path().join("sources/shirts.textures.src")
    );
    let source = Source::open(&made.path).expect("it validates as a source");
    assert_eq!(source.manifest.kind, textures::KIND);
    assert_eq!(source.manifest.name, "shirts");
    assert_eq!(source.manifest.origin.how, How::Authored);
    assert!(!source.manifest.origin.from_game);
    assert_eq!(source.manifest.target, AUTHORED_TARGET);
    assert!(made.path.join(textures::RULES).is_file());
    assert!(made.path.join(textures::DIR).is_dir());
    assert_eq!(
        names(&made.path.join(textures::DIR)),
        Vec::<String>::new(),
        "nothing in it yet"
    );

    let compiled = textures::compile(&source).expect("an empty dictionary compiles");
    assert_eq!(compiled.file_name, "shirts.ytd");
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(&compiled.bytes).expect("it reloads");
    assert_eq!(dictionary.entries.iter().flatten().count(), 0);
    assert!(made.untouched(), "the digest is the folder as created");
}

#[test]
fn a_name_that_is_taken_gets_a_number_and_a_name_no_host_can_create_is_refused() {
    let project = tempfile::tempdir().unwrap();
    create::new_source(project.path(), textures::KIND, "shirts").unwrap();

    let second = create::new_source(project.path(), textures::KIND, "shirts").unwrap();

    assert_eq!(
        second.path,
        project.path().join("sources/shirts_2.textures.src")
    );
    assert_eq!(
        Source::open(&second.path).unwrap().manifest.name,
        "shirts_2",
        "two sources of one name would cook into one file"
    );
    for bad in ["", "a/b", "CON", "x.", "a:b"] {
        let refused = create::new_source(project.path(), textures::KIND, bad).unwrap_err();
        assert!(
            refused.contains("cannot be the name") || refused.contains("device name"),
            "{bad:?}: {refused}"
        );
    }
    assert_eq!(
        names(&project.path().join("sources")),
        ["shirts.textures.src", "shirts_2.textures.src"],
        "a refusal leaves nothing behind"
    );
}

#[test]
fn a_kind_with_no_empty_form_is_refused_by_name_and_the_table_lists_what_can_be_started() {
    let project = tempfile::tempdir().unwrap();

    let refused = create::new_source(project.path(), "drawables", "x").unwrap_err();

    assert!(refused.contains("drawables"), "{refused}");
    let listed: Vec<&str> = create::KINDS.iter().map(|k| k.kind).collect();
    assert_eq!(listed, [textures::KIND]);
    assert!(
        (create::KINDS.iter()).all(|k| !k.label.is_empty()),
        "every row says what it makes"
    );
    assert!(
        create::WHY_NOT_THE_REST.contains("base"),
        "and the rest say why not"
    );
    assert!(!project.path().join("sources").exists());
}

#[test]
fn pictures_make_a_textures_source_that_holds_them_and_no_dds() {
    let project = tempfile::tempdir().unwrap();
    let pictures = tempfile::tempdir().unwrap();
    let (one, two): (PathBuf, PathBuf) = (
        pictures.path().join("jbib_diff_000_a_uni.png"),
        pictures.path().join("jbib_diff_000_b_uni.png"),
    );
    save_image(&one, &flat(16, 16, [[255, 0, 0, 255]; 4]));
    save_image(&two, &flat(16, 16, [[0, 255, 0, 255]; 4]));

    let made = create::textures_from_images(project.path(), "jacket", &[one, two]).unwrap();

    let source = Source::open(&made.path).unwrap();
    assert_eq!(source.manifest.target, AUTHORED_TARGET);
    assert_eq!(
        names(&made.path.join(textures::DIR)),
        ["jbib_diff_000_a_uni.png", "jbib_diff_000_b_uni.png"],
        "the pictures are the source, and nothing is compiled yet"
    );
    assert!(
        (names(&made.path).iter()).all(|n| !n.to_lowercase().ends_with(".dds")),
        "no DDS until it compiles"
    );
    let compiled = textures::compile(&source).expect("it compiles");
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(&compiled.bytes).unwrap();
    assert_eq!(dictionary.entries.iter().flatten().count(), 2);
    assert!(fs::read_to_string(made.path.join(MANIFEST))
        .unwrap()
        .contains("\"authored\""));
}

#[test]
fn a_picture_that_is_no_picture_or_named_like_another_is_refused_with_the_reason() {
    let project = tempfile::tempdir().unwrap();
    let pictures = tempfile::tempdir().unwrap();
    let (a, b) = (pictures.path().join("a.png"), pictures.path().join("A.tga"));
    save_image(&a, &flat(16, 16, [[1, 2, 3, 255]; 4]));
    save_image(&b, &flat(16, 16, [[1, 2, 3, 255]; 4]));
    let notes = pictures.path().join("notes.txt");
    fs::write(&notes, b"x").unwrap();

    let both = create::textures_from_images(project.path(), "t", &[a.clone(), b]).unwrap_err();
    assert!(both.contains("named like"), "{both}");
    let text = create::textures_from_images(project.path(), "t", &[a, notes]).unwrap_err();
    assert!(text.contains("is not an image"), "{text}");
    assert!(!project.path().join("sources").exists());
}

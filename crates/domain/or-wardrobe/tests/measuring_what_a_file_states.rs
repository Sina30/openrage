//! What a scan counts in a file is what the owned reader of its format says of it, and a file nobody can read has only its pages.

mod common;

use common::built::{ytd, Tex};
use or_io::codec::mesh::VertexSource;
use or_io::codec::texture::GrcTextureFormat as F;
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8};
use or_io::gta5::{DrawableTarget, GtaDrawable, PgDictionary};
use or_wardrobe::measure::{measure, Measure};

fn measured(name: &str, bytes: &[u8]) -> Measure {
    Measure::of(&measure(name, bytes).expect("a resource is measured")).expect("a count reads back")
}

#[test]
fn a_dictionary_is_counted_texture_by_texture_as_it_was_built() {
    let built = [
        Tex::new("diff_a", 1024, 512, F::DXT1),
        Tex::new("diff_b", 100, 60, F::A8R8G8B8).with_mips(1),
        Tex::new("diff_c", 256, 256, F::BC7).with_mips(4),
    ];
    let bytes = ytd(&built);
    let counted = measured("a.ytd", &bytes);

    assert!(counted.detail && !counted.enhanced);
    assert_eq!(
        counted.streaming,
        u64::from(Rsc7::peek(&bytes).unwrap().total_size()),
        "the pages are the header's, whatever the readers say"
    );
    assert_eq!(counted.textures.len(), built.len());
    for want in &built {
        let got = counted
            .textures
            .iter()
            .find(|t| t.name == want.name)
            .unwrap_or_else(|| panic!("{} is not counted", want.name));
        assert_eq!(
            (got.width, got.height, got.mips, got.format.as_str()),
            (
                u32::from(want.width),
                u32::from(want.height),
                u32::from(want.stored()),
                format!("{:?}", want.format).as_str()
            ),
            "{}",
            want.name
        );
    }
}

#[test]
fn a_file_the_owned_readers_do_not_open_is_counted_for_its_pages_alone() {
    let bytes = ytd(&[Tex::new("diff_a", 64, 64, F::DXT1)]);
    // A dictionary of textures under a model's name is a resource of another version than a model's.
    let counted = measured("a.ydd", &bytes);
    assert!(!counted.detail && counted.textures.is_empty() && counted.drawables.is_empty());
    assert_eq!(
        counted.streaming,
        u64::from(Rsc7::peek(&bytes).unwrap().total_size())
    );

    // Bytes that are no resource have no count, and neither has a file that is not one of the game's.
    assert_eq!(measure("a.ytd", b"not a resource"), None);
    assert_eq!(measure("shop.meta", b"<xml/>"), None);
}

#[test]
fn the_models_of_the_corpus_are_counted_level_by_level_as_their_reader_states_them() {
    let Some(files) = common::staged("ydd", 6, "the levels of staged models") else {
        return;
    };
    let mut with_a_lower_level = 0;
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        let counted = measured(&name, &bytes);
        let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&bytes).unwrap();
        assert!(counted.detail, "{name}");
        assert_eq!(
            counted.drawables.len(),
            dictionary.entries.iter().flatten().count()
        );
        for (got, drawable) in counted
            .drawables
            .iter()
            .zip(dictionary.entries.iter().flatten())
        {
            for (level, lod) in drawable.lod_group.lods.iter().enumerate() {
                let geometries: Vec<_> = lod
                    .models
                    .iter()
                    .flat_map(|m| m.geometries.iter())
                    .collect();
                match (&got.lods[level], lod.models.is_empty()) {
                    (None, true) => {}
                    (Some(counted), false) => {
                        // The indices are counted here as a run each, which is how the file lays a triangle list out.
                        let triangles: u64 = geometries
                            .iter()
                            .map(|g| (Gen8::indices(&g.index_buffer).len() / 3) as u64)
                            .sum();
                        let vertices: u64 = geometries
                            .iter()
                            .map(|g| g.vertex_buffer.vertex_declared() as u64)
                            .sum();
                        assert_eq!(
                            (counted.vertices, counted.triangles),
                            (vertices, triangles),
                            "{name} level {level}"
                        );
                    }
                    (got, empty) => panic!(
                        "{name} level {level}: counted {got:?} for a level that is empty: {empty}"
                    ),
                }
            }
            with_a_lower_level += usize::from(got.lods[1..].iter().any(Option::is_some));
        }
    }
    assert!(
        with_a_lower_level > 0,
        "no staged model has a lower level, so nothing here tells a level from the first"
    );
}

#[test]
fn an_enhanced_dictionary_and_model_are_counted_by_their_own_layout() {
    let corpus = |ext: &str| {
        or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
            .target(or_test_support::TARGET_GEN9)
            .find_or_skip("a staged Enhanced file is counted")
    };
    let (Some(mut dictionaries), Some(mut models)) = (corpus("ytd"), corpus("ydd")) else {
        return;
    };
    for (files, models) in [(&mut dictionaries, false), (&mut models, true)] {
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        let mut counted_any = false;
        for path in files.iter().take(12) {
            let bytes = std::fs::read(path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy();
            let counted = measured(&name, &bytes);
            if counted.detail {
                counted_any = true;
                assert!(counted.enhanced, "{name} is laid out for Enhanced");
                assert_eq!(
                    models,
                    !counted.drawables.is_empty(),
                    "{name}: a model has drawables and a dictionary has textures"
                );
                assert_eq!(!models, !counted.textures.is_empty(), "{name}");
            }
        }
        assert!(
            counted_any,
            "none of the smallest twelve staged Enhanced files is counted"
        );
    }
}
